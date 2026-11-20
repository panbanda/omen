//! Vue single-file components.
//!
//! A component file has top-level blocks: `<template>`, `<script>`, `<style>`,
//! and custom blocks. `translate` gives a text with the length of the file.
//! The script blocks keep their bytes at their positions. Each other byte is a
//! space, and each line break stays. Thus a position in the text is the
//! position in the file.
//!
//! The template becomes one array expression. Each expression of the
//! template, each component tag, and each event handler is an element of the
//! array, at the position of its text in the file. The grammar of the script
//! language parses the text. Thus the index reads the uses of the template as
//! uses of top-level code, which runs when the component renders.
//!
//! A name that a `v-for` alias or the props of a slot bind is not a name of
//! the script. Its first character becomes `$` in the text, thus it names
//! nothing of the script.
//!
//! A custom directive `v-focus` uses the binding `vFocus` of `<script setup>`,
//! and `<component :is="Card">` uses `Card`. These names are uses only when
//! the script binds them, thus the parser checks them against the tree of the
//! script (see `ScriptUse`).
//!
//! When the template has a construct that this translation does not show
//! completely, the translation abstains: it gives the reason, and a consumer
//! must treat each definition of the script as used.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;

use crate::core::Language;

/// The file is a Vue single-file component.
pub fn is_component(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("vue"))
}

/// The facts of a component that the syntax tree does not show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The BLAKE3 hash of the bytes of the file.
    pub content_hash: [u8; 32],
    /// The reason why the uses of the template are not complete. When it is
    /// present, each definition of the script can have uses that the index
    /// does not show.
    pub abstention: Option<String>,
}

/// The text that the grammar of the script parses.
#[derive(Debug, Clone)]
pub struct Translation {
    pub language: Language,
    pub text: Vec<u8>,
    /// The range of `text` that the template gives.
    pub template: Option<Range<usize>>,
    pub abstention: Option<String>,
    /// The names in the template that are uses only when the script binds
    /// them. The parser checks them against the tree of the script.
    pub uses: Vec<ScriptUse>,
}

/// A name in the template that is a use only when the script binds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptUse {
    pub name: Vec<u8>,
    /// The range of the translation that the name and its `,` take.
    pub range: Range<usize>,
    pub kind: UseKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind {
    /// `v-focus` uses the binding `vFocus` of `<script setup>`. Without the
    /// binding the directive is global: it is not a use and not an abstention.
    Directive,
    /// `<component :is="Card">` uses `Card` when the script imports it. A
    /// different value can name each component, thus the template abstains.
    Component,
}

impl Translation {
    /// The text without the template: the script blocks only.
    pub fn script_text(&self) -> Vec<u8> {
        let mut text = self.text.clone();
        if let Some(range) = &self.template {
            blank(&mut text[range.clone()]);
        }
        text
    }
}

/// The language of the script blocks. A component without a script block is
/// JavaScript, as Vue gives. `None` when a block has a language that is not
/// supported, or when two blocks have different languages.
pub fn script_language(content: &[u8]) -> Option<Language> {
    let blocks = blocks(content).ok()?;
    language_of(content, &blocks)
}

/// The text of the file with only the bytes of the script blocks. The other
/// bytes are spaces, and each line break stays.
pub fn script_text(content: &[u8]) -> Option<Vec<u8>> {
    translate(content).map(|translation| translation.script_text())
}

/// Translates a component. `None` when the script language is not supported.
pub fn translate(content: &[u8]) -> Option<Translation> {
    let mut text = content.to_vec();
    blank(&mut text);
    let (blocks, abstention) = match blocks(content) {
        Ok(blocks) => (blocks, None),
        Err(reason) => (Vec::new(), Some(reason)),
    };
    let language = if abstention.is_some() {
        Language::JavaScript
    } else {
        language_of(content, &blocks)?
    };
    for block in blocks.iter().filter(|block| block.name == "script") {
        text[block.content.clone()].copy_from_slice(&content[block.content.clone()]);
    }
    let template = blocks.iter().find(|block| block.name == "template");
    let mut translation = Translation {
        language,
        text,
        template: None,
        abstention,
        uses: Vec::new(),
    };
    if translation.abstention.is_some() {
        return Some(translation);
    }
    let Some(template) = template else {
        translation.abstention = Some(
            "the component has no template; a render function can use each definition".to_string(),
        );
        return Some(translation);
    };
    if let Some(reason) = unsupported_template(content, template) {
        translation.abstention = Some(reason);
        return Some(translation);
    }
    let setup = blocks.iter().any(|block| {
        block.name == "script"
            && block
                .attributes
                .iter()
                .any(|attribute| content[attribute.name.clone()].eq_ignore_ascii_case(b"setup"))
    });
    let mut writer = Writer {
        content,
        text: &mut translation.text,
        setup,
        uses: Vec::new(),
    };
    writer.put(template.start, b";[");
    writer.put(template.close, b"];");
    let result = Template::new(content, &mut writer).run(template.content.clone());
    let uses = std::mem::take(&mut writer.uses);
    translation.template = Some(template.start..template.close + 2);
    if let Err(reason) = result {
        let range = template.start..template.close + 2;
        blank(&mut translation.text[range]);
        translation.template = None;
        translation.abstention = Some(reason);
    } else {
        translation.uses = uses;
    }
    Some(translation)
}

fn unsupported_template(content: &[u8], template: &Block) -> Option<String> {
    for attribute in &template.attributes {
        let name = &content[attribute.name.clone()];
        let value = attribute
            .value
            .as_ref()
            .map(|value| &content[value.range.clone()]);
        if name == b"lang" && value.is_some_and(|value| value != b"html") {
            return Some(format!(
                "the template has the language `{}`",
                String::from_utf8_lossy(value.unwrap_or_default())
            ));
        }
        if name == b"src" {
            return Some("the template is in a different file".to_string());
        }
        if name == b"functional" {
            return Some("the template is functional".to_string());
        }
    }
    None
}

fn language_of(content: &[u8], blocks: &[Block]) -> Option<Language> {
    let mut found: Option<Language> = None;
    for block in blocks.iter().filter(|block| block.name == "script") {
        let lang = block
            .attributes
            .iter()
            .find(|attribute| &content[attribute.name.clone()] == b"lang")
            .and_then(|attribute| attribute.value.as_ref())
            .map(|value| &content[value.range.clone()]);
        let language = match lang {
            None | Some(b"js") => Language::JavaScript,
            Some(b"ts") => Language::TypeScript,
            Some(b"tsx") => Language::Tsx,
            Some(b"jsx") => Language::Jsx,
            Some(_) => return None,
        };
        if found.is_some_and(|found| found != language) {
            return None;
        }
        found = Some(language);
    }
    Some(found.unwrap_or(Language::JavaScript))
}

/// Each byte becomes a space, and each line break stays.
fn blank(text: &mut [u8]) {
    for byte in text {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

#[derive(Debug, Clone)]
struct Value {
    /// The text of the value, without the quotes.
    range: Range<usize>,
    quoted: bool,
}

#[derive(Debug, Clone)]
struct Attribute {
    name: Range<usize>,
    value: Option<Value>,
}

#[derive(Debug, Clone)]
struct Tag {
    name: Range<usize>,
    attributes: Vec<Attribute>,
    /// The position after `>`.
    end: usize,
    self_closing: bool,
}

/// A top-level block of a component.
#[derive(Debug, Clone)]
struct Block {
    name: String,
    attributes: Vec<Attribute>,
    /// The position of `<` of the start tag.
    start: usize,
    content: Range<usize>,
    /// The position of `<` of the end tag.
    close: usize,
}

fn is_space(byte: u8) -> bool {
    byte.is_ascii_whitespace()
}

fn starts_with_at(content: &[u8], position: usize, prefix: &[u8]) -> bool {
    content
        .get(position..position + prefix.len())
        .is_some_and(|slice| slice.eq_ignore_ascii_case(prefix))
}

fn find(content: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    content
        .get(from..)?
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
        .map(|offset| from + offset)
}

/// Reads the start tag at `start`, where `content[start]` is `<`. An
/// attribute value in quotes can contain `>`.
fn start_tag(content: &[u8], start: usize) -> Option<Tag> {
    let mut position = start + 1;
    let name_start = position;
    while position < content.len()
        && !is_space(content[position])
        && content[position] != b'>'
        && content[position] != b'/'
    {
        position += 1;
    }
    let name = name_start..position;
    if name.is_empty() {
        return None;
    }
    let mut attributes = Vec::new();
    loop {
        while position < content.len() && is_space(content[position]) {
            position += 1;
        }
        match content.get(position)? {
            b'>' => {
                return Some(Tag {
                    name,
                    attributes,
                    end: position + 1,
                    self_closing: false,
                })
            }
            b'/' if content.get(position + 1) == Some(&b'>') => {
                return Some(Tag {
                    name,
                    attributes,
                    end: position + 2,
                    self_closing: true,
                })
            }
            _ => {}
        }
        let attribute_start = position;
        while position < content.len()
            && !is_space(content[position])
            && content[position] != b'='
            && content[position] != b'>'
            && !(content[position] == b'/' && content.get(position + 1) == Some(&b'>'))
        {
            position += 1;
        }
        if position == attribute_start {
            // A lone `/` or `=`.
            position += 1;
            continue;
        }
        let attribute_name = attribute_start..position;
        let mut after_name = position;
        while after_name < content.len() && is_space(content[after_name]) {
            after_name += 1;
        }
        if content.get(after_name) != Some(&b'=') {
            attributes.push(Attribute {
                name: attribute_name,
                value: None,
            });
            continue;
        }
        position = after_name + 1;
        while position < content.len() && is_space(content[position]) {
            position += 1;
        }
        let value = match content.get(position)? {
            quote @ (b'"' | b'\'') => {
                let value_start = position + 1;
                let length = content[value_start..]
                    .iter()
                    .position(|byte| byte == quote)?;
                position = value_start + length + 1;
                Value {
                    range: value_start..value_start + length,
                    quoted: true,
                }
            }
            _ => {
                let value_start = position;
                while position < content.len()
                    && !is_space(content[position])
                    && content[position] != b'>'
                {
                    position += 1;
                }
                Value {
                    range: value_start..position,
                    quoted: false,
                }
            }
        };
        attributes.push(Attribute {
            name: attribute_name,
            value: Some(value),
        });
    }
}

/// Reads the top-level blocks of a component.
fn blocks(content: &[u8]) -> Result<Vec<Block>, String> {
    let mut blocks = Vec::new();
    let mut position = 0;
    while let Some(offset) = content[position..].iter().position(|byte| *byte == b'<') {
        let start = position + offset;
        if starts_with_at(content, start, b"<!--") {
            position = find(content, start + 4, b"-->")
                .map(|end| end + 3)
                .ok_or("a comment of the component has no end")?;
            continue;
        }
        if !content
            .get(start + 1)
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        {
            position = start + 1;
            continue;
        }
        let tag = start_tag(content, start).ok_or("a block of the component has no end")?;
        let name = String::from_utf8_lossy(&content[tag.name.clone()]).to_ascii_lowercase();
        if tag.self_closing {
            position = tag.end;
            continue;
        }
        let close = if name == "template" {
            template_end(content, tag.end)?
        } else {
            let mut end_tag = b"</".to_vec();
            end_tag.extend_from_slice(name.as_bytes());
            find(content, tag.end, &end_tag)
                .ok_or_else(|| format!("the `{name}` block has no end tag"))?
        };
        position = find(content, close, b">").map_or(content.len(), |end| end + 1);
        blocks.push(Block {
            name,
            attributes: tag.attributes,
            start,
            content: tag.end..close,
            close,
        });
    }
    Ok(blocks)
}

fn is_tag_end(byte: Option<&u8>) -> bool {
    byte.is_none_or(|byte| is_space(*byte) || *byte == b'>' || *byte == b'/')
}

/// The position of `<` of the end tag of the top-level template. A
/// `<template>` element in the template can contain other ones.
fn template_end(content: &[u8], from: usize) -> Result<usize, String> {
    let mut depth = 0usize;
    let mut position = from;
    while let Some(offset) = content[position..].iter().position(|byte| *byte == b'<') {
        let start = position + offset;
        if starts_with_at(content, start, b"<!--") {
            position = find(content, start + 4, b"-->")
                .map(|end| end + 3)
                .ok_or("a comment of the template has no end")?;
        } else if starts_with_at(content, start, b"</template")
            && is_tag_end(content.get(start + 10))
        {
            if depth == 0 {
                return Ok(start);
            }
            depth -= 1;
            position = start + 10;
        } else if content
            .get(start + 1)
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        {
            let tag = start_tag(content, start).ok_or("a tag of the template has no end")?;
            if &content[tag.name.clone()] == b"template" && !tag.self_closing {
                depth += 1;
            }
            position = tag.end;
        } else {
            position = start + 1;
        }
    }
    Err("the template has no end tag".to_string())
}

/// The names that the template binds at a place: `v-for` aliases and the
/// props of slots. A name can be bound more than one time.
#[derive(Default)]
struct Scope {
    names: Vec<Vec<u8>>,
    counts: HashMap<Vec<u8>, usize>,
}

impl Scope {
    fn len(&self) -> usize {
        self.names.len()
    }

    fn contains(&self, name: &[u8]) -> bool {
        self.counts.contains_key(name)
    }

    fn push(&mut self, name: Vec<u8>) {
        *self.counts.entry(name.clone()).or_default() += 1;
        self.names.push(name);
    }

    fn truncate(&mut self, length: usize) {
        while self.names.len() > length {
            let Some(name) = self.names.pop() else {
                break;
            };
            if let Some(count) = self.counts.get_mut(&name) {
                *count -= 1;
                if *count == 0 {
                    self.counts.remove(&name);
                }
            }
        }
    }
}

/// Writes the translation of the template into the text.
struct Writer<'a> {
    content: &'a [u8],
    text: &'a mut Vec<u8>,
    /// The component has `<script setup>`, whose bindings the template can use.
    setup: bool,
    uses: Vec<ScriptUse>,
}

impl Writer<'_> {
    fn put(&mut self, position: usize, bytes: &[u8]) {
        self.text[position..position + bytes.len()].copy_from_slice(bytes);
    }

    /// The expression at `range` becomes an element of the array. `glue` is
    /// the position of the byte before it, which becomes `,`.
    fn expression(
        &mut self,
        glue: usize,
        range: Range<usize>,
        bound: &Scope,
    ) -> Result<(), String> {
        let expression = &self.content[range.clone()];
        if expression.iter().all(|byte| is_space(*byte)) {
            return Ok(());
        }
        check_entities(expression)?;
        self.text[glue] = b',';
        self.copy(range, bound);
        Ok(())
    }

    /// Copies the text at `range` and renames each name that `bound` has.
    fn copy(&mut self, range: Range<usize>, bound: &Scope) {
        self.text[range.clone()].copy_from_slice(&self.content[range.clone()]);
        if bound.len() == 0 {
            return;
        }
        for token in tokens(&self.content[range.clone()]) {
            if token.property {
                continue;
            }
            let name =
                &self.content[range.start + token.range.start..range.start + token.range.end];
            if bound.contains(name) {
                self.text[range.start + token.range.start] = b'$';
            }
        }
    }
}

/// Vue decodes character references in the template, and the text of the
/// file does not show the decoded expression.
fn check_entities(expression: &[u8]) -> Result<(), String> {
    for (position, byte) in expression.iter().enumerate() {
        if *byte != b'&' {
            continue;
        }
        let rest = &expression[position + 1..];
        let length = rest
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'#')
            .count();
        if length > 0 && rest.get(length) == Some(&b';') {
            return Err("an expression of the template has a character reference".to_string());
        }
    }
    Ok(())
}

/// An identifier of an expression.
struct Token {
    range: Range<usize>,
    /// A `.` is before the identifier: it is the name of a property.
    property: bool,
    /// A `:` is after the identifier.
    before_colon: bool,
}

fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' || byte >= 0x80
}

fn is_identifier_part(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}

/// The identifiers of an expression. Strings, template strings, comments,
/// and numbers are not identifiers. The text in `${}` of a template string is
/// an expression.
fn tokens(text: &[u8]) -> Vec<Token> {
    let mut found = Vec::new();
    let mut position = 0;
    // The last byte that is not a space, outside strings and comments.
    let mut previous: Option<u8> = None;
    // For each open `${`, the depth of the braces in it.
    let mut substitutions: Vec<usize> = Vec::new();
    while position < text.len() {
        let byte = text[position];
        match byte {
            b'"' | b'\'' => {
                position = skip_string(text, position);
                previous = Some(byte);
            }
            b'`' => match skip_template(text, position + 1) {
                (end, true) => {
                    substitutions.push(0);
                    position = end;
                    previous = Some(b'{');
                }
                (end, false) => {
                    position = end;
                    previous = Some(b'`');
                }
            },
            b'{' => {
                if let Some(depth) = substitutions.last_mut() {
                    *depth += 1;
                }
                previous = Some(byte);
                position += 1;
            }
            b'}' => {
                match substitutions.last_mut() {
                    Some(0) => {
                        substitutions.pop();
                        match skip_template(text, position + 1) {
                            (end, true) => {
                                substitutions.push(0);
                                position = end;
                                previous = Some(b'{');
                            }
                            (end, false) => {
                                position = end;
                                previous = Some(b'`');
                            }
                        }
                        continue;
                    }
                    Some(depth) => *depth -= 1,
                    None => {}
                }
                previous = Some(byte);
                position += 1;
            }
            b'/' if text.get(position + 1) == Some(&b'/') => {
                position = text[position..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(text.len(), |offset| position + offset);
            }
            b'/' if text.get(position + 1) == Some(&b'*') => {
                position = find(text, position + 2, b"*/").map_or(text.len(), |end| end + 2);
            }
            b'/' if previous.is_none_or(|previous| {
                !(is_identifier_part(previous) || previous == b')' || previous == b']')
            }) =>
            {
                position = skip_regex(text, position);
                previous = Some(b'/');
            }
            _ if byte.is_ascii_digit() => {
                while position < text.len()
                    && (is_identifier_part(text[position]) || text[position] == b'.')
                {
                    position += 1;
                }
                previous = Some(b'0');
            }
            _ if is_identifier_start(byte) => {
                let start = position;
                while position < text.len() && is_identifier_part(text[position]) {
                    position += 1;
                }
                let after = text[position..]
                    .iter()
                    .find(|byte| !is_space(**byte))
                    .copied();
                found.push(Token {
                    range: start..position,
                    property: previous == Some(b'.'),
                    before_colon: after == Some(b':'),
                });
                previous = Some(b'a');
            }
            _ if is_space(byte) => position += 1,
            _ => {
                previous = Some(byte);
                position += 1;
            }
        }
    }
    found
}

fn skip_string(text: &[u8], start: usize) -> usize {
    let quote = text[start];
    let mut position = start + 1;
    while position < text.len() {
        match text[position] {
            b'\\' => position += 2,
            byte if byte == quote => return position + 1,
            _ => position += 1,
        }
    }
    text.len()
}

/// Skips the text of a template string from `start`. The flag tells if the
/// text stops at `${`.
fn skip_template(text: &[u8], start: usize) -> (usize, bool) {
    let mut position = start;
    while position < text.len() {
        match text[position] {
            b'\\' => position += 2,
            b'`' => return (position + 1, false),
            b'$' if text.get(position + 1) == Some(&b'{') => return (position + 2, true),
            _ => position += 1,
        }
    }
    (text.len(), false)
}

fn skip_regex(text: &[u8], start: usize) -> usize {
    let mut position = start + 1;
    let mut class = false;
    while position < text.len() {
        match text[position] {
            b'\\' => position += 2,
            b'[' => {
                class = true;
                position += 1;
            }
            b']' => {
                class = false;
                position += 1;
            }
            b'/' if !class => {
                position += 1;
                while position < text.len() && text[position].is_ascii_alphabetic() {
                    position += 1;
                }
                return position;
            }
            b'\n' => return position,
            _ => position += 1,
        }
    }
    text.len()
}

/// The names that a pattern binds: a `v-for` alias or the props of a slot.
/// A default value in the pattern is an expression; the translation does not
/// read it.
fn bound_names(pattern: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    if pattern.contains(&b'=') {
        return Err("a pattern of the template has a default value".to_string());
    }
    Ok(tokens(pattern)
        .into_iter()
        .filter(|token| !token.property && !token.before_colon)
        .map(|token| pattern[token.range].to_vec())
        .collect())
}

const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// The built-in components of Vue. They are not names of the script.
const BUILT_IN_COMPONENTS: &[&str] = &[
    "template",
    "slot",
    "transition",
    "transition-group",
    "keep-alive",
    "teleport",
    "suspense",
    "Transition",
    "TransitionGroup",
    "KeepAlive",
    "Teleport",
    "Suspense",
];

/// The directives of Vue whose value is an expression.
const EXPRESSION_DIRECTIVES: &[&str] = &[
    "bind", "model", "if", "else-if", "show", "html", "text", "memo",
];

/// The directives of Vue without a value.
const PLAIN_DIRECTIVES: &[&str] = &["else", "once", "cloak", "pre"];

/// The tag is a component: it has an upper-case letter, a `-`, or a `.`.
fn is_component_tag(name: &[u8]) -> bool {
    name.iter()
        .any(|byte| byte.is_ascii_uppercase() || *byte == b'-' || *byte == b'.')
}

/// `my-card` names the component `MyCard`.
fn pascal_case(name: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(name.len());
    let mut upper = true;
    for byte in name {
        if *byte == b'-' {
            upper = true;
            continue;
        }
        result.push(if upper {
            byte.to_ascii_uppercase()
        } else {
            *byte
        });
        upper = false;
    }
    result
}

/// A Vue event handler is a value when it is a path of names, or a function.
/// Other text is a list of statements.
fn is_handler_value(text: &[u8]) -> bool {
    let text = text.trim_ascii();
    let path = !text.is_empty()
        && is_identifier_start(text[0])
        && text
            .iter()
            .all(|byte| is_identifier_part(*byte) || *byte == b'.');
    path || text.starts_with(b"function") || arrow_function(text)
}

fn arrow_function(text: &[u8]) -> bool {
    let Some(arrow) = text.windows(2).position(|window| window == b"=>") else {
        return false;
    };
    let head = text[..arrow].trim_ascii();
    let head = head.strip_prefix(b"async").map_or(head, <[u8]>::trim_ascii);
    let simple = !head.is_empty() && head.iter().all(|byte| is_identifier_part(*byte));
    simple || head.starts_with(b"(") && head.ends_with(b")")
}

/// The meaning of an attribute of an element of the template.
enum Directive {
    /// A static attribute.
    Static,
    /// The directive has no value: `v-else`, `v-once`, `v-cloak`.
    Plain,
    Pre,
    Expression,
    Handler,
    For,
    Slot,
    BindAll,
    Custom(String),
}

fn directive_of(name: &[u8]) -> (Directive, Option<Range<usize>>) {
    let argument = |from: usize| -> Option<Range<usize>> {
        let open = name[from..].iter().position(|byte| *byte == b'[')? + from;
        let close = name[open..].iter().position(|byte| *byte == b']')? + open;
        Some(open + 1..close)
    };
    match name.first() {
        Some(b':' | b'.') => return (Directive::Expression, argument(1)),
        Some(b'@') => return (Directive::Handler, argument(1)),
        Some(b'#') => return (Directive::Slot, argument(1)),
        _ => {}
    }
    let Some(rest) = name.strip_prefix(b"v-") else {
        return (Directive::Static, None);
    };
    let end = rest
        .iter()
        .position(|byte| *byte == b':' || *byte == b'.')
        .unwrap_or(rest.len());
    let directive = String::from_utf8_lossy(&rest[..end]).into_owned();
    let has_argument = rest.get(end) == Some(&b':');
    let argument = argument(2 + end);
    let kind = match directive.as_str() {
        "bind" if !has_argument => Directive::BindAll,
        "on" => Directive::Handler,
        "for" => Directive::For,
        "slot" => Directive::Slot,
        "pre" => Directive::Pre,
        name if EXPRESSION_DIRECTIVES.contains(&name) => Directive::Expression,
        name if PLAIN_DIRECTIVES.contains(&name) => Directive::Plain,
        _ => Directive::Custom(directive),
    };
    (kind, argument)
}

struct Element {
    name: Vec<u8>,
    /// The length of the list of bound names before the element.
    bound: usize,
    pre: bool,
}

struct Template<'a, 'w> {
    content: &'a [u8],
    writer: &'w mut Writer<'a>,
    elements: Vec<Element>,
    bound: Scope,
}

impl<'a, 'w> Template<'a, 'w> {
    fn new(content: &'a [u8], writer: &'w mut Writer<'a>) -> Self {
        Self {
            content,
            writer,
            elements: Vec::new(),
            bound: Scope::default(),
        }
    }

    fn in_pre(&self) -> bool {
        self.elements.last().is_some_and(|element| element.pre)
    }

    fn run(mut self, range: Range<usize>) -> Result<(), String> {
        let content = self.content;
        let mut position = range.start;
        while position < range.end {
            let Some(offset) = content[position..range.end]
                .iter()
                .position(|byte| *byte == b'<')
            else {
                self.text(position..range.end)?;
                break;
            };
            let start = position + offset;
            self.text(position..start)?;
            if starts_with_at(content, start, b"<!--") {
                position = find(content, start + 4, b"-->")
                    .filter(|end| *end < range.end)
                    .map(|end| end + 3)
                    .ok_or("a comment of the template has no end")?;
            } else if content.get(start + 1) == Some(&b'/') {
                let end = find(content, start, b">")
                    .filter(|end| *end < range.end)
                    .ok_or("an end tag of the template has no end")?;
                let name = content[start + 2..end].trim_ascii();
                self.close(name)?;
                position = end + 1;
            } else if content
                .get(start + 1)
                .is_some_and(|byte| byte.is_ascii_alphabetic())
            {
                let tag = start_tag(content, start)
                    .filter(|tag| tag.end <= range.end)
                    .ok_or("a tag of the template has no end")?;
                position = self.open(start, &tag)?;
            } else {
                position = start + 1;
            }
        }
        if let Some(element) = self.elements.last() {
            return Err(format!(
                "the element `{}` of the template has no end tag",
                String::from_utf8_lossy(&element.name)
            ));
        }
        Ok(())
    }

    fn close(&mut self, name: &[u8]) -> Result<(), String> {
        match self.elements.pop() {
            Some(element) if element.name == name => {
                self.bound.truncate(element.bound);
                Ok(())
            }
            _ => Err(format!(
                "the end tag `{}` of the template has no start tag",
                String::from_utf8_lossy(name)
            )),
        }
    }

    /// Reads the mustaches of a text.
    fn text(&mut self, range: Range<usize>) -> Result<(), String> {
        if self.in_pre() {
            return Ok(());
        }
        // Each search stops at the end of the text.
        let text = &self.content[..range.end];
        let mut position = range.start;
        while let Some(open) = find(text, position, b"{{") {
            let close =
                find(text, open + 2, b"}}").ok_or("a mustache of the template has no end")?;
            self.writer.expression(open, open + 2..close, &self.bound)?;
            position = close + 2;
        }
        Ok(())
    }

    /// Reads a start tag, and gives the position after it.
    fn open(&mut self, start: usize, tag: &Tag) -> Result<usize, String> {
        let content = self.content;
        let name = &content[tag.name.clone()];
        if name.eq_ignore_ascii_case(b"script") || name.eq_ignore_ascii_case(b"style") {
            return Err(format!(
                "the template has a `{}` element",
                String::from_utf8_lossy(name)
            ));
        }
        let outer = self.bound.len();
        let in_pre = self.in_pre();
        let pre = in_pre
            || tag
                .attributes
                .iter()
                .any(|attribute| &content[attribute.name.clone()] == b"v-pre");
        let mut slot_names: Vec<Vec<u8>> = Vec::new();
        if !pre {
            if name == b"component" || name == b"Component" {
                self.dynamic_component(tag)?;
            }
            if is_component_tag(name)
                && !BUILT_IN_COMPONENTS.contains(&&*String::from_utf8_lossy(name))
            {
                self.writer.put(start, b",");
                if name.contains(&b'-') {
                    let pascal = pascal_case(name);
                    blank(&mut self.writer.text[tag.name.clone()]);
                    self.writer.put(tag.name.start, &pascal);
                } else {
                    self.writer.copy(tag.name.clone(), &Scope::default());
                }
            }
            // The alias of `v-for` is in scope for the other attributes of the
            // element, but not for `v-if`, which Vue reads first.
            let mut aliases: Vec<Vec<u8>> = Vec::new();
            for attribute in &tag.attributes {
                let (directive, _) = directive_of(&content[attribute.name.clone()]);
                if let (Directive::For, Some(value)) = (directive, &attribute.value) {
                    let (alias, _) = split_for(&content[value.range.clone()])?;
                    aliases = bound_names(alias)?;
                }
            }
            let reads_first = |attribute: &&Attribute| {
                let name = &content[attribute.name.clone()];
                name == b"v-if" || name == b"v-else-if" || name == b"v-for"
            };
            for attribute in tag.attributes.iter().filter(reads_first) {
                self.attribute(attribute, &mut slot_names)?;
            }
            for alias in aliases {
                self.bound.push(alias);
            }
            for attribute in tag
                .attributes
                .iter()
                .filter(|attribute| !reads_first(attribute))
            {
                self.attribute(attribute, &mut slot_names)?;
            }
        }
        let void = VOID_ELEMENTS.contains(&&*String::from_utf8_lossy(name));
        if tag.self_closing || void {
            self.bound.truncate(outer);
            return Ok(tag.end);
        }
        for name in slot_names {
            self.bound.push(name);
        }
        self.elements.push(Element {
            name: name.to_vec(),
            bound: outer,
            pre,
        });
        Ok(tag.end)
    }

    /// `<component :is="Card">` is a use of `Card` when `Card` can be an
    /// import of `<script setup>`. Each other value abstains.
    fn dynamic_component(&mut self, tag: &Tag) -> Result<(), String> {
        let dynamic = || "the template has a dynamic component".to_string();
        if !self.writer.setup {
            return Err(dynamic());
        }
        let content = self.content;
        let value = tag
            .attributes
            .iter()
            .find(|attribute| {
                let name = &content[attribute.name.clone()];
                name == b":is" || name == b"v-bind:is"
            })
            .and_then(|attribute| attribute.value.as_ref())
            .ok_or_else(dynamic)?;
        let text = &content[value.range.clone()];
        let start = text.iter().position(|byte| !is_space(*byte)).unwrap_or(0);
        let end = text
            .iter()
            .rposition(|byte| !is_space(*byte))
            .map_or(0, |end| end + 1);
        let name = &text[start..end.max(start)];
        let identifier = !name.is_empty()
            && is_identifier_start(name[0])
            && name.iter().all(|byte| is_identifier_part(*byte));
        if !identifier || self.bound.contains(name) {
            return Err(dynamic());
        }
        self.writer.uses.push(ScriptUse {
            name: name.to_vec(),
            range: value.range.start - 1..value.range.end,
            kind: UseKind::Component,
        });
        Ok(())
    }

    /// Reads an attribute in the scope of `self.bound`.
    fn attribute(
        &mut self,
        attribute: &Attribute,
        slot_names: &mut Vec<Vec<u8>>,
    ) -> Result<(), String> {
        let content = self.content;
        let name = &content[attribute.name.clone()];
        let (directive, argument) = directive_of(name);
        let scope = &self.bound;
        if let Some(argument) = argument {
            let argument =
                attribute.name.start + argument.start..attribute.name.start + argument.end;
            self.writer
                .expression(argument.start - 1, argument, scope)?;
        }
        let value = attribute.value.as_ref();
        let glue = |value: &Value| value.range.start - 1;
        match directive {
            Directive::Static => {
                let Some(value) = value.filter(|_| name == b"ref") else {
                    return Ok(());
                };
                let text = &content[value.range.clone()];
                let identifier = !text.is_empty()
                    && is_identifier_start(text[0])
                    && text.iter().all(|byte| is_identifier_part(*byte));
                if identifier {
                    self.writer
                        .expression(glue(value), value.range.clone(), scope)?;
                }
            }
            Directive::Plain | Directive::Pre => {}
            Directive::Custom(directive) => {
                if self.writer.setup {
                    let start = attribute.name.start;
                    if start == 0 || !is_space(content[start - 1]) {
                        return Err(format!(
                            "the custom directive `v-{directive}` has no space before it"
                        ));
                    }
                    let mut binding = b"v".to_vec();
                    binding.extend(pascal_case(directive.as_bytes()));
                    self.writer.text[start - 1] = b',';
                    self.writer.put(start, &binding);
                    self.writer.uses.push(ScriptUse {
                        range: start - 1..start + binding.len(),
                        name: binding,
                        kind: UseKind::Directive,
                    });
                }
                if let Some(value) = value {
                    self.writer
                        .expression(glue(value), value.range.clone(), &self.bound)?;
                }
            }
            Directive::Expression | Directive::BindAll => {
                if let Some(value) = value {
                    self.writer
                        .expression(glue(value), value.range.clone(), scope)?;
                }
            }
            Directive::Handler => {
                let Some(value) = value else {
                    return Ok(());
                };
                let text = &content[value.range.clone()];
                if text.trim_ascii().is_empty() || is_handler_value(text) {
                    self.writer
                        .expression(glue(value), value.range.clone(), scope)?;
                    return Ok(());
                }
                // The statements become the body of a function: `$=>{` before
                // the value, and `}` at the end quote. The attribute name and
                // the space before it give the bytes.
                let prefix = b",$=>{";
                if !value.quoted || value.range.start < attribute.name.start + prefix.len() - 1 {
                    return Err(
                        "an event handler of the template has no space for its function"
                            .to_string(),
                    );
                }
                check_entities(text)?;
                self.writer.put(value.range.start - prefix.len(), prefix);
                self.writer.copy(value.range.clone(), scope);
                self.writer.put(value.range.end, b"}");
            }
            Directive::For => {
                let Some(value) = value else {
                    return Ok(());
                };
                let text = &content[value.range.clone()];
                let (_, source) = split_for(text)?;
                let source = value.range.start + source.start..value.range.start + source.end;
                self.writer.expression(source.start - 1, source, scope)?;
            }
            Directive::Slot => {
                if let Some(value) = value {
                    slot_names.extend(bound_names(&content[value.range.clone()])?);
                }
            }
        }
        Ok(())
    }
}

/// Splits the value of `v-for` into the alias and the range of the source,
/// as Vue does: `alias in source` or `alias of source`.
fn split_for(text: &[u8]) -> Result<(&[u8], Range<usize>), String> {
    let mut position = 0;
    while position < text.len() {
        if is_space(text[position]) {
            let mut word = position;
            while word < text.len() && is_space(text[word]) {
                word += 1;
            }
            let keyword = text.get(word..word + 2);
            if matches!(keyword, Some(b"in" | b"of"))
                && text.get(word + 2).is_some_and(|byte| is_space(*byte))
            {
                let mut alias = text[..position].trim_ascii();
                if let Some(inner) = alias
                    .strip_prefix(b"(")
                    .and_then(|alias| alias.strip_suffix(b")"))
                {
                    alias = inner;
                }
                let mut source = word + 2;
                while source < text.len() && is_space(text[source]) {
                    source += 1;
                }
                return Ok((alias, source..text.len()));
            }
            position = word;
        } else {
            position += 1;
        }
    }
    Err("a `v-for` of the template has no `in` or `of`".to_string())
}

/// The names that the top level of the script binds as values.
#[derive(Debug, Default)]
pub struct Bindings {
    /// The names of value imports.
    pub imports: HashSet<Vec<u8>>,
    /// The names of value imports and of declarations.
    pub values: HashSet<Vec<u8>>,
}

/// The value bindings of the top level of the tree of a translation:
/// imports without `type`, and declarations of one name.
pub fn top_level_bindings(root: tree_sitter::Node<'_>, source: &[u8]) -> Bindings {
    let text = |node: tree_sitter::Node<'_>| source[node.byte_range()].to_vec();
    let mut bindings = Bindings::default();
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor) {
        match statement.kind() {
            "import_statement" => {
                let mut cursor = statement.walk();
                let type_only = statement
                    .children(&mut cursor)
                    .any(|child| child.kind() == "type");
                let clause = statement
                    .named_children(&mut statement.walk())
                    .find(|child| child.kind() == "import_clause");
                let Some(clause) = clause.filter(|_| !type_only) else {
                    continue;
                };
                for part in clause.named_children(&mut clause.walk()) {
                    match part.kind() {
                        "identifier" => {
                            bindings.imports.insert(text(part));
                        }
                        "namespace_import" => {
                            let name = part
                                .named_children(&mut part.walk())
                                .find(|child| child.kind() == "identifier");
                            bindings.imports.extend(name.map(text));
                        }
                        "named_imports" => {
                            for specifier in part.named_children(&mut part.walk()) {
                                let is_type = specifier
                                    .children(&mut specifier.walk())
                                    .any(|child| child.kind() == "type");
                                let name = specifier
                                    .child_by_field_name("alias")
                                    .or_else(|| specifier.child_by_field_name("name"));
                                if let Some(name) = name
                                    .filter(|_| specifier.kind() == "import_specifier" && !is_type)
                                {
                                    bindings.imports.insert(text(name));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                for declarator in statement.named_children(&mut statement.walk()) {
                    let name = declarator
                        .child_by_field_name("name")
                        .filter(|name| name.kind() == "identifier");
                    bindings.values.extend(name.map(text));
                }
            }
            "function_declaration" | "generator_function_declaration" | "class_declaration" => {
                bindings
                    .values
                    .extend(statement.child_by_field_name("name").map(text));
            }
            _ => {}
        }
    }
    bindings.values.extend(bindings.imports.iter().cloned());
    bindings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn translation(content: &str) -> Translation {
        translate(content.as_bytes()).expect("a supported language")
    }

    /// The text of the translation without spaces.
    fn code(content: &str) -> String {
        let translation = translation(content);
        assert_eq!(translation.text.len(), content.len());
        String::from_utf8(translation.text)
            .unwrap()
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect()
    }

    /// The text that the template gives, without spaces.
    fn template_code(template: &str) -> String {
        let content =
            format!("<template>{template}</template>\n<script setup lang=\"ts\"></script>\n");
        let translation = translation(&content);
        assert_eq!(translation.abstention, None, "{content}");
        code(&content)
    }

    fn abstention(template: &str) -> String {
        let content =
            format!("<template>{template}</template>\n<script setup lang=\"ts\"></script>\n");
        translation(&content)
            .abstention
            .unwrap_or_else(|| panic!("no abstention for {template}"))
    }

    #[test]
    fn test_a_component_path_has_the_vue_extension() {
        assert!(is_component(Path::new("src/App.vue")));
        assert!(is_component(Path::new("src/App.VUE")));
        assert!(!is_component(Path::new("src/app.ts")));
    }

    #[test]
    fn test_the_script_keeps_its_bytes_at_their_positions() {
        let content = "<template><div/></template>\n<script setup lang=\"ts\">\nconst a = 1;\n</script>\n<style>.a { color: red }</style>\n";
        let translation = translation(content);
        assert_eq!(translation.language, Language::TypeScript);
        assert_eq!(translation.text.len(), content.len());
        let start = content.find("const").unwrap();
        assert_eq!(&translation.text[start..start + 12], b"const a = 1;");
        assert_eq!(
            translation
                .text
                .iter()
                .filter(|byte| **byte == b'\n')
                .count(),
            content.matches('\n').count()
        );
        assert_eq!(code(content), ";[];consta=1;");
    }

    #[test]
    fn test_a_generic_attribute_can_contain_a_greater_than_sign() {
        let content = "<script setup lang=\"ts\" generic=\"T extends Record<string, number>\">\nlet x: T;\n</script>\n<template><p/></template>\n";
        assert_eq!(code(content), "letx:T;;[];");
    }

    #[test]
    fn test_each_script_block_is_kept() {
        let content = "<script lang=\"ts\">\nexport default {}\n</script>\n<script setup lang=\"ts\">\nconst b = 2\n</script>\n<template><p/></template>\n";
        assert_eq!(code(content), "exportdefault{}constb=2;[];");
    }

    #[test]
    fn test_the_script_language_comes_from_the_lang_attribute() {
        let language = |content: &str| script_language(content.as_bytes());
        assert_eq!(language("<script>\n</script>"), Some(Language::JavaScript));
        assert_eq!(
            language("<script lang=\"ts\">\n</script>"),
            Some(Language::TypeScript)
        );
        assert_eq!(
            language("<script lang='tsx'>\n</script>"),
            Some(Language::Tsx)
        );
        assert_eq!(
            language("<script lang=\"jsx\">\n</script>"),
            Some(Language::Jsx)
        );
        assert_eq!(
            language("<template><p/></template>"),
            Some(Language::JavaScript)
        );
        assert_eq!(language("<script lang=\"coffee\">\n</script>"), None);
        assert_eq!(
            language("<script lang=\"ts\">\n</script><script setup>\n</script>"),
            None
        );
    }

    #[test]
    fn test_the_script_text_has_only_the_script() {
        let content = "<template><p>{{ a }}</p></template>\n<script>\nconst a = 1\n</script>\n";
        let text = script_text(content.as_bytes()).unwrap();
        let code: String = String::from_utf8(text)
            .unwrap()
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        assert_eq!(code, "consta=1");
    }

    #[test]
    fn test_a_mustache_is_an_element() {
        assert_eq!(
            template_code("<p>{{ count }} and {{ total + 1 }}</p>"),
            ";[,count,total+1];"
        );
    }

    #[test]
    fn test_a_component_tag_is_an_element() {
        assert_eq!(
            template_code("<MyButton/><my-card></my-card><Foo.Bar/><div><span></span></div>"),
            ";[,MyButton,MyCard,Foo.Bar];"
        );
    }

    #[test]
    fn test_a_built_in_component_is_not_an_element() {
        assert_eq!(
            template_code("<Transition><keep-alive><slot name=\"a\" :item=\"row\"></slot></keep-alive></Transition>"),
            ";[,row];"
        );
    }

    #[test]
    fn test_a_bound_attribute_is_an_element() {
        assert_eq!(
            template_code("<div :title=\"label\" v-if=\"shown\" v-show='ok' v-bind:id=\"key\" .prop=\"value\" class=\"x\"></div>"),
            ";[,label,shown,ok,key,value];"
        );
    }

    #[test]
    fn test_an_unquoted_value_is_an_element() {
        assert_eq!(template_code("<div :title=label></div>"), ";[,label];");
    }

    #[test]
    fn test_a_handler_value_is_an_element() {
        assert_eq!(
            template_code("<button @click=\"save\" v-on:focus=\"form.focus\" @blur=\"() => leave()\"></button>"),
            ";[,save,form.focus,()=>leave()];"
        );
    }

    #[test]
    fn test_handler_statements_are_the_body_of_a_function() {
        assert_eq!(
            template_code("<button @click=\"count++; save($event)\"></button>"),
            ";[,$=>{count++;save($event)}];"
        );
    }

    #[test]
    fn test_a_for_alias_is_not_a_name_of_the_script() {
        assert_eq!(
            template_code("<li v-for=\"(item, index) in items\" :key=\"item.id\">{{ item.name }} {{ index }} {{ other.item }}</li><p>{{ item }}</p>"),
            ";[,items,$tem.id,$tem.name,$ndex,other.item,item];"
        );
    }

    #[test]
    fn test_v_if_is_outside_the_scope_of_the_for_alias() {
        assert_eq!(
            template_code("<li v-for=\"item of items\" v-if=\"item\">{{ item }}</li>"),
            ";[,items,item,$tem];"
        );
    }

    #[test]
    fn test_slot_props_are_not_names_of_the_script() {
        assert_eq!(
            template_code("<MyList><template #row=\"{ row, index: position }\">{{ row.id }} {{ position }} {{ index }}</template></MyList>"),
            ";[,MyList,$ow.id,$osition,index];"
        );
    }

    #[test]
    fn test_v_slot_on_a_component_binds_its_children() {
        assert_eq!(
            template_code(
                "<MyList v-slot=\"slotProps\" :a=\"slotProps\">{{ slotProps.item }}</MyList>"
            ),
            ";[,MyList,slotProps,$lotProps.item];"
        );
    }

    #[test]
    fn test_model_dynamic_arguments_and_template_refs_are_elements() {
        assert_eq!(
            template_code("<input v-model=\"query\" ref=\"field\"><div :[name]=\"value\"></div>"),
            ";[,query,field,name,value];"
        );
    }

    #[test]
    fn test_v_pre_and_comments_have_no_elements() {
        assert_eq!(
            template_code("<div v-pre>{{ raw }}<Foo/></div><!-- {{ x }} <Bar/> --><p v-else v-once v-cloak></p>"),
            ";[];"
        );
    }

    #[test]
    fn test_strings_and_template_strings_do_not_bind() {
        assert_eq!(
            template_code("<li v-for=\"item in items\">{{ 'item' + `${item}-item` }}</li>"),
            ";[,items,'item'+`${$tem}-item`];"
        );
    }

    #[test]
    fn test_nested_templates_end_at_their_own_tags() {
        assert_eq!(
            template_code(
                "<template v-if=\"a\"><template v-if=\"b\">{{ c }}</template></template>"
            ),
            ";[,a,b,c];"
        );
    }

    #[test]
    fn test_v_bind_of_an_object_is_an_expression() {
        assert_eq!(
            template_code("<div v-bind=\"attrs\"></div><p v-bind=\"{ ...rest, id }\"></p>"),
            ";[,attrs,{...rest,id}];"
        );
    }

    #[test]
    fn test_a_custom_directive_uses_its_script_binding() {
        let translation = translation(
            "<template><input v-focus=\"value\"><p v-n8n-html.raw:[arg]=\"html\"></p><i v-tip></i></template>\n<script setup lang=\"ts\"></script>\n",
        );
        assert_eq!(translation.abstention, None);
        let names: Vec<(&[u8], UseKind)> = translation
            .uses
            .iter()
            .map(|used| (used.name.as_slice(), used.kind))
            .collect();
        assert_eq!(
            names,
            [
                (&b"vFocus"[..], UseKind::Directive),
                (&b"vN8nHtml"[..], UseKind::Directive),
                (&b"vTip"[..], UseKind::Directive),
            ]
        );
        assert_eq!(
            template_code(
                "<input v-focus=\"value\"><p v-n8n-html.raw:[arg]=\"html\"></p><i v-tip></i>"
            ),
            ";[,vFocus,value,vN8nHtml,arg,html,vTip];"
        );
    }

    #[test]
    fn test_a_custom_directive_without_script_setup_is_global() {
        let content = "<template><input v-focus=\"value\"></template>\n<script lang=\"ts\">export default {}</script>\n";
        let translation = translation(content);
        assert_eq!(translation.abstention, None);
        assert!(translation.uses.is_empty());
        assert_eq!(code(content), ";[,value];exportdefault{}");
    }

    #[test]
    fn test_a_dynamic_component_of_one_name_is_a_use() {
        let translation = translation(
            "<template><component :is=\"Current\"/><component v-bind:is=\"Other\"></component></template>\n<script setup lang=\"ts\"></script>\n",
        );
        assert_eq!(translation.abstention, None);
        let names: Vec<(&[u8], UseKind)> = translation
            .uses
            .iter()
            .map(|used| (used.name.as_slice(), used.kind))
            .collect();
        assert_eq!(
            names,
            [
                (&b"Current"[..], UseKind::Component),
                (&b"Other"[..], UseKind::Component),
            ]
        );
        assert_eq!(
            template_code("<component :is=\"Current\"/>"),
            ";[,Current];"
        );
    }

    #[test]
    fn test_a_dynamic_component_without_script_setup_abstains() {
        let content = "<template><component :is=\"Current\"/></template>\n<script lang=\"ts\">export default {}</script>\n";
        assert!(translation(content)
            .abstention
            .unwrap()
            .contains("dynamic component"));
    }

    #[test]
    fn test_the_template_abstains_on_a_construct_it_cannot_show() {
        assert!(abstention("<component/>").contains("dynamic component"));
        assert!(abstention("<component :is=\"'card'\"/>").contains("dynamic component"));
        assert!(abstention("<component :is=\"a ? B : C\"/>").contains("dynamic component"));
        assert!(abstention("<component is=\"card\"/>").contains("dynamic component"));
        assert!(
            abstention("<li v-for=\"c in list\"><component :is=\"c\"/></li>")
                .contains("dynamic component")
        );
        assert!(abstention("<div></span>").contains("`span`"));
        assert!(abstention("<div>").contains("`div`"));
        assert!(abstention("<p>{{ a </p>").contains("mustache"));
        assert!(abstention("<p :a=\"x &amp;&amp; y\"></p>").contains("character reference"));
        assert!(abstention("<li v-for=\"items\"></li>").contains("v-for"));
        assert!(abstention("<p @click=a++></p>").contains("event handler"));
    }

    #[test]
    fn test_a_component_without_a_template_abstains() {
        let content = "<script>\nexport default { render() {} }\n</script>\n";
        assert!(translation(content)
            .abstention
            .unwrap()
            .contains("no template"));
    }

    #[test]
    fn test_a_template_in_a_different_language_abstains() {
        let content = "<template lang=\"pug\">\np {{ a }}\n</template>\n<script>\n</script>\n";
        let translation = translation(content);
        assert!(translation.abstention.unwrap().contains("pug"));
        assert_eq!(translation.template, None);
    }

    fn translate_in(content: &str) -> std::time::Duration {
        let watch = crate::index::timing::Stopwatch::start();
        let translation = translate(content.as_bytes()).unwrap();
        assert_eq!(translation.abstention, None);
        watch.elapsed()
    }

    /// Each level binds a name, and each level uses each name of the levels
    /// around it by one expression: the lookup of a bound name does not
    /// read the list of the bound names.
    #[test]
    #[ignore = "timing: run in release"]
    fn timing_deeply_nested_for_aliases_are_read_in_linear_time() {
        crate::index::timing::assert_scales_linearly("levels", 2_000, |size| {
            let mut template = String::new();
            for level in 0..size {
                template.push_str(&format!(
                    "<div v-for=\"item{level} in items\" :key=\"item{level}.id\">{{{{ other }}}}"
                ));
            }
            template.push_str(&"</div>".repeat(size));
            translate_in(&format!(
                "<template>{template}</template>\n<script setup>\nconst items = [];\n</script>\n"
            ))
        });
    }

    #[test]
    #[ignore = "timing: run in release"]
    fn timing_many_elements_are_read_in_linear_time() {
        crate::index::timing::assert_scales_linearly("elements", 5_000, |size| {
            let template = "<MyCard :title=\"label\" @click=\"count++; save($event)\">{{ format(value) }}</MyCard>\n"
                .repeat(size);
            translate_in(&format!(
                "<template>{template}</template>\n<script setup>\nconst label = 1;\n</script>\n"
            ))
        });
    }

    /// The search for a mustache in a text stops at the end of the text.
    #[test]
    #[ignore = "timing: run in release"]
    fn timing_texts_without_mustaches_are_read_in_linear_time() {
        crate::index::timing::assert_scales_linearly("elements", 20_000, |size| {
            let template = "<div>a</div>".repeat(size);
            translate_in(&format!(
                "<template>{template}</template>\n<script setup>\nconst a = 1;\n</script>\n"
            ))
        });
    }

    #[test]
    fn test_an_abstention_keeps_the_script() {
        let content =
            "<template><div></template>\n<script setup lang=\"ts\">\nconst a = 1\n</script>\n";
        let translation = translation(content);
        assert!(translation.abstention.is_some());
        assert_eq!(code(content), "consta=1");
    }
}
