//! Tokenizer for identifiers and code text.
//!
//! The search engine does not use this module yet.
//!
//! An identifier is a sequence of letters, digits, and underscores. The
//! tokenizer divides each identifier into parts at these boundaries:
//!
//! - an underscore;
//! - a lowercase letter before an uppercase letter (`twiceLocal`);
//! - the last uppercase letter of a sequence of uppercase letters, if a
//!   lowercase letter follows it (`HTTPServer` gives `http` and `server`);
//! - a letter before a digit, and a digit before a letter (`sha256Sum`).
//!
//! The tokens of an identifier are its parts in order, and then the full
//! identifier, if it has more than one part. All tokens are lowercase. There is
//! no stemming and there are no stop words.
//!
//! The letter classes are the Unicode classes, so the rules apply to text that
//! is not ASCII. First the tokenizer changes the text to Unicode normalization
//! form NFKC. Thus a precomposed and a decomposed accent, a ligature such as
//! `ﬁ`, and full-width letters give the same tokens as the usual form. A
//! combining mark that stays after normalization continues an identifier. The
//! case and digit rules do not see a mark, so a mark is never at a boundary.
//!
//! The tokenizer changes each character to lowercase without regard to its
//! neighbours. Thus a token does not depend on the text around it, and each
//! token is a part of [`lowercase`] of the input.
//!
//! Limits: this is not full Unicode case folding. For example, `ß` and `ss`
//! give different tokens, and so do `ς` and `σ`. Normalization does not join
//! letters of different scripts that look the same, such as the Latin `e` and
//! the Cyrillic `е`.

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// Change this value when the tokens for some input change. An index that an
/// older version made is then not valid.
pub const TOKENIZER_VERSION: u32 = 3;

/// The lowercase form that the tokenizer uses: the NFKC form of the text, with
/// each character changed independently of its neighbours.
pub fn lowercase(text: &str) -> String {
    text.nfkc().flat_map(char::to_lowercase).collect()
}

/// Divide text into tokens.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut identifier = String::new();
    for character in text.nfkc() {
        let continues = !identifier.is_empty() && is_combining_mark(character);
        if character.is_alphanumeric() || character == '_' || continues {
            identifier.push(character);
        } else {
            push_identifier_tokens(&identifier, &mut tokens);
            identifier.clear();
        }
    }
    push_identifier_tokens(&identifier, &mut tokens);
    tokens
}

fn push_identifier_tokens(identifier: &str, tokens: &mut Vec<String>) {
    let identifier = identifier.trim_matches('_');
    let parts: Vec<String> = identifier
        .split('_')
        .flat_map(case_and_digit_parts)
        .collect();
    let has_more_than_one_part = parts.len() > 1;
    tokens.extend(parts);
    if has_more_than_one_part {
        let characters: Vec<char> = identifier.chars().collect();
        tokens.push(lowercase_of(&characters));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharacterClass {
    Uppercase,
    Lowercase,
    Digit,
    /// A letter of a script that has no case.
    Caseless,
}

fn class_of(character: char) -> CharacterClass {
    if character.is_numeric() {
        CharacterClass::Digit
    } else if character.is_uppercase() {
        CharacterClass::Uppercase
    } else if character.is_lowercase() {
        CharacterClass::Lowercase
    } else {
        CharacterClass::Caseless
    }
}

/// Divide a sequence of letters and digits at the case boundaries and at the
/// boundaries between letters and digits. Each part is lowercase.
fn case_and_digit_parts(segment: &str) -> Vec<String> {
    use CharacterClass::{Digit, Lowercase, Uppercase};

    let characters: Vec<char> = segment.chars().collect();
    // The rules see only the characters that are not combining marks. A mark
    // stays with the character before it, so a boundary is never before a mark.
    let bases: Vec<(usize, CharacterClass)> = characters
        .iter()
        .enumerate()
        .filter(|&(index, &character)| index == 0 || !is_combining_mark(character))
        .map(|(index, &character)| (index, class_of(character)))
        .collect();
    let mut parts = Vec::new();
    let mut start = 0;
    for position in 1..bases.len() {
        let previous = bases[position - 1].1;
        let (index, current) = bases[position];
        let next = bases.get(position + 1).map(|&(_, class)| class);
        let boundary = (previous == Digit) != (current == Digit)
            || (previous == Lowercase && current == Uppercase)
            // The last capital of an acronym starts the next word.
            || (previous == Uppercase && current == Uppercase && next == Some(Lowercase));
        if boundary {
            parts.push(lowercase_of(&characters[start..index]));
            start = index;
        }
    }
    if start < characters.len() {
        parts.push(lowercase_of(&characters[start..]));
    }
    parts
}

fn lowercase_of(characters: &[char]) -> String {
    characters
        .iter()
        .flat_map(|character| character.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn tokenize_divides_identifiers_at_each_boundary() {
        let cases: &[(&str, &[&str])] = &[
            // Empty input and input without an identifier.
            ("", &[]),
            ("   ", &[]),
            ("+-*/ () {}", &[]),
            ("___", &[]),
            // One part gives one token.
            ("twice", &["twice"]),
            ("Twice", &["twice"]),
            // Underscores.
            ("twice_local", &["twice", "local", "twice_local"]),
            ("only_here", &["only", "here", "only_here"]),
            ("a__b", &["a", "b", "a__b"]),
            // Leading and trailing underscores are not part of a token.
            ("_hidden", &["hidden"]),
            ("__init__", &["init"]),
            ("_from_side_", &["from", "side", "from_side"]),
            // Camel case and Pascal case.
            ("twiceLocal", &["twice", "local", "twicelocal"]),
            ("OnlyHere", &["only", "here", "onlyhere"]),
            ("makeSquareNow", &["make", "square", "now", "makesquarenow"]),
            // The acronym rule.
            ("HTTPServer", &["http", "server", "httpserver"]),
            (
                "parseHTTPResponse",
                &["parse", "http", "response", "parsehttpresponse"],
            ),
            ("IShape", &["i", "shape", "ishape"]),
            ("getID", &["get", "id", "getid"]),
            // All capitals.
            ("HTTP", &["http"]),
            ("MAX_SIZE", &["max", "size", "max_size"]),
            // Single letters.
            ("a", &["a"]),
            ("X", &["x"]),
            ("aB", &["a", "b", "ab"]),
            ("ABc", &["a", "bc", "abc"]),
            // Letters and digits.
            ("sha256Sum", &["sha", "256", "sum", "sha256sum"]),
            ("f64", &["f", "64", "f64"]),
            ("utf8", &["utf", "8", "utf8"]),
            ("x2y", &["x", "2", "y", "x2y"]),
            ("HTTP2Server", &["http", "2", "server", "http2server"]),
            // Digits only.
            ("123", &["123"]),
            ("1_000", &["1", "000", "1_000"]),
            // Characters that are not ASCII.
            ("größeÄndern", &["größe", "ändern", "größeändern"]),
            ("naïve_café", &["naïve", "café", "naïve_café"]),
            ("ДлинаСтроки", &["длина", "строки", "длинастроки"]),
            ("ΑΒγ", &["α", "βγ", "αβγ"]),
            // A script without case has no boundary between its letters.
            ("日本語", &["日本語"]),
            ("日本語_text", &["日本語", "text", "日本語_text"]),
            // Text with more than one identifier keeps each occurrence.
            (
                "fn area(&self) -> f64",
                &["fn", "area", "self", "f", "64", "f64"],
            ),
            ("value * value", &["value", "value"]),
            ("Circle::new(radius)", &["circle", "new", "radius"]),
            ("$this->side", &["this", "side"]),
            (
                "seen.push_back(1);",
                &["seen", "push", "back", "push_back", "1"],
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(&tokenize(input), expected, "input: {input:?}");
        }
    }

    #[test]
    fn lowercase_changes_each_character_independently() {
        // `str::to_lowercase` gives a different final sigma. A token must not
        // depend on its position, so the tokenizer does not use it.
        assert_eq!(lowercase("ΟΔΟΣ"), "οδοσ");
        assert_eq!(tokenize("ΟΔΟΣ"), ["οδοσ"]);
    }

    #[test]
    fn equivalent_forms_of_a_text_give_equal_tokens() {
        // Precomposed and decomposed accents, a compatibility ligature, and
        // full-width letters.
        let cases: &[(&str, &str)] = &[
            ("café", "cafe\u{301}"),
            ("naïve_café", "nai\u{308}ve_cafe\u{301}"),
            ("größeÄndern", "gro\u{308}ßeA\u{308}ndern"),
            ("file", "\u{fb01}le"),
            ("helper", "\u{ff48}\u{ff45}\u{ff4c}\u{ff50}\u{ff45}\u{ff52}"),
        ];

        for (plain, other) in cases {
            assert_eq!(tokenize(plain), tokenize(other), "input: {other:?}");
        }
    }

    #[test]
    fn a_combining_mark_continues_an_identifier() {
        // U+0331 has no precomposed form with `x`, so it stays a mark.
        assert_eq!(tokenize("ax\u{331}b"), ["ax\u{331}b"]);
        assert_eq!(
            tokenize("twiceLx\u{331}cal"),
            ["twice", "lx\u{331}cal", "twicelx\u{331}cal"]
        );
        // A mark without an identifier before it is not a token.
        assert_eq!(tokenize(" \u{331}"), Vec::<String>::new());
    }

    #[test]
    fn a_combining_mark_is_transparent_to_the_case_rules() {
        // Each pair gives the same division with and without the mark.
        let cases: &[(&str, &[&str])] = &[
            ("A\u{331}b", &["a\u{331}b"]),
            (
                "HTTPS\u{331}erver",
                &["http", "s\u{331}erver", "https\u{331}erver"],
            ),
            (
                "HTTP\u{331}Server",
                &["http\u{331}", "server", "http\u{331}server"],
            ),
            (
                "twiceX\u{331}ocal",
                &["twice", "x\u{331}ocal", "twicex\u{331}ocal"],
            ),
            ("x\u{331}Y", &["x\u{331}", "y", "x\u{331}y"]),
            ("a1\u{331}b", &["a", "1\u{331}", "b", "a1\u{331}b"]),
        ];

        for (input, expected) in cases {
            assert_eq!(&tokenize(input), expected, "input: {input:?}");
        }
    }

    #[test]
    fn the_version_is_set() {
        assert_eq!(TOKENIZER_VERSION, 3);
    }

    proptest! {
        #[test]
        fn tokenize_is_deterministic(input in "\\PC{0,60}") {
            prop_assert_eq!(tokenize(&input), tokenize(&input));
        }

        #[test]
        fn each_token_is_a_part_of_the_lowercase_input(input in "\\PC{0,60}") {
            let lowered = lowercase(&input);
            for token in tokenize(&input) {
                prop_assert!(!token.is_empty());
                prop_assert!(lowered.contains(&token), "{:?} is not in {:?}", token, lowered);
            }
        }

        #[test]
        fn identifier_text_gives_tokens_without_a_separator(
            input in "[A-Za-z0-9_À-ÿ]{1,24}( [A-Za-z0-9_]{1,12}){0,3}"
        ) {
            for token in tokenize(&input) {
                let middle_underscores_only = !token.starts_with('_') && !token.ends_with('_');
                prop_assert!(middle_underscores_only, "{:?}", token);
                prop_assert!(token.chars().all(|character| character.is_alphanumeric() || character == '_'));
                prop_assert_eq!(lowercase(&token), token.clone());
            }
        }
    }
}
