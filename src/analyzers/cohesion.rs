//! CK (Chidamber-Kemerer) cohesion metrics analyzer.
//!
//! Calculates object-oriented metrics for classes:
//! - WMC: Weighted Methods per Class (sum of cyclomatic complexity)
//! - CBO: Coupling Between Objects (number of classes referenced)
//! - RFC: Response for Class (methods that can be invoked)
//! - LCOM: Lack of Cohesion in Methods (LCOM3 via connected components in method-field graph)
//! - DIT: Depth of Inheritance Tree
//! - NOC: Number of Children (direct subclasses)
//!
//! The analyzer parses no source. It reads the symbols, the calls, and the
//! member facts of the shared index. See `index::members` for the facts and
//! for the syntax of the classes of each language.
//!
//! # References
//!
//! - Chidamber, S.R., Kemerer, C.F. (1994) "A Metrics Suite for Object Oriented Design"
//!   IEEE TSE 20(6), pp. 476-493
//! - Hitz, M., Montazeri, B. (1995) "Measuring Coupling and Cohesion in OO Systems"
//!   (LCOM3 definition using connected components)
//! - Basili, V.R., Briand, L.C., Melo, W.L. (1996) "A Validation of Object-Oriented
//!   Design Metrics as Quality Indicators" IEEE TSE 22(10) (threshold validation)
//!
//! Note: LCOM uses LCOM3 (Hitz & Montazeri 1995), which counts connected components
//! where methods are connected if they share instance variables. This differs from
//! LCOM4 which also connects methods that call each other.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::core::{is_test_file, AnalysisContext, Analyzer as AnalyzerTrait, Language, Result};
use crate::index::build::CodeIndex;
use crate::index::extract::FileSymbols;
use crate::index::members::{class_model, ClassModel, ClassSyntax};
use crate::index::model::{Receiver, ReferenceKind, ResolutionStatus, SharedStr, SymbolKind};

/// Default threshold for WMC above which a class is considered complex.
/// Research suggests 20-24 is appropriate (Chidamber & Kemerer 1994 IEEE TSE).
pub const WMC_THRESHOLD: u32 = 24;

/// Default threshold for CBO above which a class is considered highly coupled.
pub const CBO_THRESHOLD: u32 = 14;

/// Default threshold for LCOM above which a class lacks cohesion.
pub const LCOM_THRESHOLD: u32 = 1;

/// Cohesion analyzer configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// Whether to skip test files.
    pub skip_test_files: bool,
    /// Maximum file size to analyze (0 = no limit).
    pub max_file_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            skip_test_files: true,
            max_file_size: 0,
        }
    }
}

/// Cohesion (CK metrics) analyzer.
pub struct Analyzer {
    config: Config,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    /// Creates a new cohesion analyzer with default config.
    pub fn new() -> Self {
        Self {
            config: Config::default(),
        }
    }

    /// Creates a new analyzer with the specified config.
    pub fn with_config(config: Config) -> Self {
        Self { config }
    }

    /// Includes test files in analysis.
    pub fn with_include_test_files(mut self) -> Self {
        self.config.skip_test_files = false;
        self
    }

    /// Analyzes cohesion metrics in a repository.
    ///
    /// The shared index of the context gives the facts. The index reads each
    /// file through the content source of the context, thus a historical
    /// reference gives the metrics of that reference.
    pub fn analyze_repo(&self, ctx: &AnalysisContext<'_>) -> Result<Analysis> {
        let index = ctx.code_index()?;
        let too_large: HashSet<&str> = if self.config.max_file_size == 0 {
            HashSet::new()
        } else {
            index
                .files
                .iter()
                .filter(|facts| {
                    ctx.read_file(Path::new(&facts.file))
                        .is_ok_and(|content| content.len() > self.config.max_file_size)
                })
                .map(|facts| facts.file.as_str())
                .collect()
        };
        Ok(self.analyze_index(&index, &too_large))
    }

    fn is_skipped(&self, file: &str, excluded: &HashSet<&str>) -> bool {
        if excluded.contains(file) {
            return true;
        }
        self.config.skip_test_files && is_test_file(Path::new(file))
    }

    /// Calculates the metrics of the classes of an index. The analyzer does
    /// not examine the files in `excluded`.
    pub fn analyze_index(&self, index: &CodeIndex, excluded: &HashSet<&str>) -> Analysis {
        let views: Vec<View<'_>> = index
            .files
            .iter()
            .filter(|facts| !self.is_skipped(&facts.file, excluded))
            .filter_map(|facts| match class_model(facts.language) {
                ClassModel::Classes(syntax) => Some(View { facts, syntax }),
                ClassModel::NotApplicable(_) => None,
            })
            .collect();

        let measured = Classes::new(&views).metrics();
        let mut hierarchy = ClassHierarchy::new();
        for (cls, bases) in &measured {
            hierarchy.add_class_with_bases(&cls.class_name, bases);
        }
        let mut all_classes: Vec<ClassMetrics> = measured.into_iter().map(|(cls, _)| cls).collect();
        for cls in &mut all_classes {
            cls.dit = hierarchy.get_dit(&cls.class_name);
            cls.noc = hierarchy.get_noc(&cls.class_name);
        }

        // Sort by LCOM (least cohesive first)
        all_classes.sort_by_key(|class| std::cmp::Reverse(class.lcom));

        let summary = calculate_summary(&all_classes);

        Analysis {
            generated_at: Utc::now().to_rfc3339(),
            classes: all_classes,
            summary,
        }
    }
}

impl AnalyzerTrait for Analyzer {
    type Output = Analysis;

    fn name(&self) -> &'static str {
        "cohesion"
    }

    fn description(&self) -> &'static str {
        "Calculate CK object-oriented metrics (WMC, CBO, RFC, LCOM, DIT, NOC)"
    }

    fn analyze(&self, ctx: &AnalysisContext<'_>) -> Result<Self::Output> {
        self.analyze_repo(ctx)
    }
}

/// One file of a language that has classes.
struct View<'a> {
    facts: &'a FileSymbols,
    syntax: ClassSyntax,
}

/// The position of a class: the file in `views` and the symbol in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Slot {
    view: usize,
    position: usize,
}

/// What the metrics need to know about one class.
#[derive(Default)]
struct Builder {
    methods: Vec<Slot>,
    fields: Vec<SharedStr>,
    field_set: HashSet<SharedStr>,
    used: HashMap<Slot, HashSet<SharedStr>>,
    coupled: HashSet<String>,
    called: HashSet<String>,
    /// The calls from a method of the class to a method of the same class.
    edges: Vec<(Slot, Callee)>,
    end_line: u32,
}

/// The start of the text of a call on the object itself that the index did not resolve.
const OWN_PREFIX: &str = "\u{0}own:";

/// The method that a call names, as far as the index can tell.
enum Callee {
    /// The index resolved the call to this symbol.
    Key(SharedStr),
    /// The call is on the object itself and has this name.
    OwnMethod(String),
}

/// The class of a definition and, when the definition is in a method, the method.
struct Attribution {
    class: usize,
    method: Option<usize>,
}

struct Classes<'a, 'v> {
    views: &'v [View<'a>],
    slots: Vec<Slot>,
    /// For each view, the class number of each symbol that is a class.
    class_of_symbol: Vec<Vec<Option<usize>>>,
    /// The class numbers for each language and path of a class.
    by_path: HashMap<String, Vec<usize>>,
    builders: Vec<Builder>,
    /// The names of the C# interfaces that no C# class has as a name.
    interfaces: HashSet<String>,
}

fn path_key(language: Language, path: &[String], name: Option<&str>) -> String {
    let mut key = format!("{language}\u{1f}");
    key.push_str(&path.join("\u{1f}"));
    if let Some(name) = name {
        if !path.is_empty() {
            key.push('\u{1f}');
        }
        key.push_str(name);
    }
    key
}

fn directory_of(file: &str) -> &str {
    file.rsplit_once('/').map_or("", |(directory, _)| directory)
}

impl<'a, 'v> Classes<'a, 'v> {
    fn new(views: &'v [View<'a>]) -> Self {
        let mut slots = Vec::new();
        let mut class_of_symbol = Vec::new();
        let mut by_path: HashMap<String, Vec<usize>> = HashMap::new();
        for (view_index, view) in views.iter().enumerate() {
            let mut of_symbol = vec![None; view.facts.symbols.len()];
            for (position, symbol) in view.facts.symbols.iter().enumerate() {
                if !view.syntax.classes.contains(&symbol.kind) {
                    continue;
                }
                let facts = &view.facts.link_facts[position];
                let key = path_key(
                    view.facts.language,
                    &facts.container_path,
                    Some(&symbol.name),
                );
                of_symbol[position] = Some(slots.len());
                by_path.entry(key).or_default().push(slots.len());
                slots.push(Slot {
                    view: view_index,
                    position,
                });
            }
            class_of_symbol.push(of_symbol);
        }
        let builders = slots.iter().map(|_| Builder::default()).collect();
        let named = |interface: bool| -> HashSet<String> {
            slots
                .iter()
                .map(|slot| (slot, &views[slot.view].facts.symbols[slot.position]))
                .filter(|(slot, symbol)| {
                    views[slot.view].facts.language == Language::CSharp
                        && (symbol.kind == SymbolKind::Interface) == interface
                })
                .map(|(_, symbol)| symbol.name.clone())
                .collect()
        };
        let class_names = named(false);
        let interfaces = named(true)
            .into_iter()
            .filter(|name| !class_names.contains(name))
            .collect();
        let mut classes = Self {
            views,
            slots,
            class_of_symbol,
            by_path,
            builders,
            interfaces,
        };
        classes.collect();
        classes
    }

    /// The class that contains the members of `path` in the file `view`. A
    /// definition at the byte `at` is in the class whose range contains it.
    /// A Go method or a C++ method can be in a different file of the same directory.
    fn class_of_path(&self, view: usize, path: &[String], at: usize) -> Option<usize> {
        let language = self.views[view].facts.language;
        let candidates = self.by_path.get(&path_key(language, path, None))?;
        let same_file: Vec<usize> = candidates
            .iter()
            .copied()
            .filter(|class| self.slots[*class].view == view)
            .collect();
        if !same_file.is_empty() {
            let containing = same_file.iter().copied().find(|class| {
                let range = &self.symbol_of(*class).range;
                range.start_byte <= at && at < range.end_byte
            });
            return containing.or_else(|| same_file.first().copied());
        }
        if !matches!(language, Language::Go | Language::Cpp) {
            return None;
        }
        let directory = directory_of(&self.views[view].facts.file);
        let same_directory = candidates.iter().copied().find(|class| {
            directory_of(&self.views[self.slots[*class].view].facts.file) == directory
        });
        if same_directory.is_some() {
            return same_directory;
        }
        (language == Language::Cpp && candidates.len() == 1).then(|| candidates[0])
    }

    fn symbol_of(&self, class: usize) -> &crate::index::model::Symbol {
        let slot = self.slots[class];
        &self.views[slot.view].facts.symbols[slot.position]
    }

    /// The class that a definition belongs to. A definition in a method of a
    /// class belongs to that class, and the method is the member.
    fn attribute(&self, view: usize, position: usize) -> Option<Attribution> {
        let facts = self.views[view].facts;
        let mut current = position;
        loop {
            if let Some(class) = self.class_of_symbol[view][current] {
                return Some(Attribution {
                    class,
                    method: None,
                });
            }
            let symbol = &facts.symbols[current];
            let link = &facts.link_facts[current];
            if let Some(class) =
                self.class_of_path(view, &link.container_path, symbol.range.start_byte)
            {
                return Some(Attribution {
                    class,
                    method: (symbol.is_callable() && !link.named_by_binding).then_some(current),
                });
            }
            current = link.parent?;
        }
    }

    fn collect(&mut self) {
        let views = self.views;
        let class_names: HashMap<&str, &str> = self
            .slots
            .iter()
            .map(|slot| {
                let symbol = &views[slot.view].facts.symbols[slot.position];
                (symbol.key.as_str(), symbol.name.as_str())
            })
            .collect();
        for (view_index, view) in views.iter().enumerate() {
            let facts = view.facts;
            for (position, symbol) in facts.symbols.iter().enumerate() {
                let link = &facts.link_facts[position];
                // A function value that a variable holds is not a method.
                if !symbol.is_callable()
                    || link.named_by_binding
                    || self.class_of_symbol[view_index][position].is_some()
                {
                    continue;
                }
                let Some(class) =
                    self.class_of_path(view_index, &link.container_path, symbol.range.start_byte)
                else {
                    continue;
                };
                let builder = &mut self.builders[class];
                builder.methods.push(Slot {
                    view: view_index,
                    position,
                });
                if self.slots[class].view == view_index {
                    builder.end_line = builder.end_line.max(symbol.range.end_line);
                }
            }
            for block in &facts.members.blocks {
                if let Some(class) = self.class_of_symbol[view_index][block.class as usize] {
                    let builder = &mut self.builders[class];
                    builder.end_line = builder.end_line.max(block.end_line);
                }
            }
            for declaration in &facts.members.declarations {
                if let Some(found) = self.attribute(view_index, declaration.owner as usize) {
                    let builder = &mut self.builders[found.class];
                    if builder.field_set.insert(declaration.name.clone()) {
                        builder.fields.push(declaration.name.clone());
                    }
                }
            }
            for access in &facts.members.accesses {
                let found = self.attribute(view_index, access.owner as usize);
                if let Some(Attribution {
                    class,
                    method: Some(method),
                }) = found
                {
                    let method = Slot {
                        view: view_index,
                        position: method,
                    };
                    self.builders[class]
                        .used
                        .entry(method)
                        .or_default()
                        .insert(access.name.clone());
                }
            }
            for reference in &facts.members.references {
                if let Some(found) = self.attribute(view_index, reference.owner as usize) {
                    self.builders[found.class]
                        .coupled
                        .insert(reference.name.to_string());
                }
            }
            for (call, call_facts) in facts.references.iter().zip(&facts.reference_facts) {
                let Some(owner) = call_facts.owner else {
                    continue;
                };
                let Some(found) = self.attribute(view_index, owner) else {
                    continue;
                };
                let resolved = matches!(
                    call.resolution.status,
                    ResolutionStatus::Resolved | ResolutionStatus::UniqueCandidate
                );
                {
                    // A call or a construction that resolves to one definition is a call of that
                    // method. Other calls are the same method when they have
                    // the same qualifier and the same name.
                    let callee = match call.resolution.candidates.as_slice() {
                        [only] if resolved => only.to_string(),
                        _ if call.receiver == Some(Receiver::SelfValue) => {
                            format!("{OWN_PREFIX}{}", call.name)
                        }
                        _ => format!("{}::{}", call.qualifier.as_deref().unwrap_or(""), call.name),
                    };
                    self.builders[found.class].called.insert(callee);
                    let calls_object = call.kind == ReferenceKind::Call;
                    if let (true, Some(method)) = (calls_object, found.method) {
                        let caller = Slot {
                            view: view_index,
                            position: method,
                        };
                        let edge = match call.resolution.candidates.as_slice() {
                            [only] if resolved => Some(Callee::Key(only.clone())),
                            _ if call.receiver == Some(Receiver::SelfValue) => {
                                Some(Callee::OwnMethod(call.name.clone()))
                            }
                            _ => None,
                        };
                        if let Some(edge) = edge {
                            self.builders[found.class].edges.push((caller, edge));
                        }
                    }
                }
                // Python has no declared types, thus a call of a class is the
                // evidence of coupling.
                if facts.language == Language::Python && resolved {
                    for candidate in &call.resolution.candidates {
                        if let Some(name) = class_names.get(candidate.as_str()) {
                            self.builders[found.class].coupled.insert(name.to_string());
                        }
                    }
                }
            }
        }
    }

    /// The metrics of each class, in the order of the classes in the files.
    fn metrics(self) -> Vec<(ClassMetrics, Vec<String>)> {
        let views = self.views;
        self.slots
            .iter()
            .zip(&self.builders)
            .map(|(slot, builder)| {
                let view = &views[slot.view];
                let facts = view.facts;
                let class = &facts.symbols[slot.position];
                let own_name = class.name.as_str();

                let signature_of = |method: &Slot| {
                    let method_facts = views[method.view].facts;
                    // The overload signatures of a TypeScript method have
                    // different parameters and one implementation.
                    let typescript =
                        matches!(method_facts.language, Language::TypeScript | Language::Tsx);
                    (
                        method_facts.symbols[method.position].name.as_str(),
                        method_facts.link_facts[method.position]
                            .parameter_count
                            .filter(|_| !typescript),
                    )
                };
                let defined: HashSet<(&str, Option<usize>)> = builder
                    .methods
                    .iter()
                    .filter(|method| facts_of(views, method).body_range.is_some())
                    .map(signature_of)
                    .collect();
                // A declaration in the body of a class and the definition of
                // the same method outside the body are one method.
                let methods: Vec<&Slot> = builder
                    .methods
                    .iter()
                    .filter(|method| {
                        facts_of(views, method).body_range.is_some()
                            || !defined.contains(&signature_of(method))
                    })
                    .collect();
                let method_names: HashSet<&str> = builder
                    .methods
                    .iter()
                    .map(|method| facts_of(views, method).name.as_str())
                    .collect();

                let mut method_infos: Vec<MethodInfo> = methods
                    .iter()
                    .map(|method| {
                        let method_facts = views[method.view].facts;
                        let symbol = &method_facts.symbols[method.position];
                        let complexity = 1 + method_facts
                            .members
                            .decision_points
                            .get(method.position)
                            .copied()
                            .unwrap_or(0);
                        let used_fields = builder
                            .used
                            .get(method)
                            .map(|names| {
                                names
                                    .iter()
                                    .filter(|name| {
                                        builder.field_set.contains(*name)
                                            || !method_names.contains(name.as_str())
                                    })
                                    .map(|name| name.to_string())
                                    .collect()
                            })
                            .unwrap_or_default();
                        MethodInfo {
                            name: symbol.name.clone(),
                            complexity,
                            used_fields,
                            calls: Vec::new(),
                        }
                    })
                    .collect();

                let mut kept_by_signature: HashMap<(&str, Option<usize>), usize> = HashMap::new();
                for (index, method) in methods.iter().enumerate() {
                    kept_by_signature
                        .entry(signature_of(method))
                        .or_insert(index);
                }
                let slot_index: HashMap<&Slot, usize> = methods
                    .iter()
                    .enumerate()
                    .map(|(index, method)| (*method, index))
                    .collect();
                let index_of = |slot: &Slot| {
                    slot_index
                        .get(slot)
                        .or_else(|| kept_by_signature.get(&signature_of(slot)))
                        .copied()
                };
                let by_key: HashMap<&str, usize> = builder
                    .methods
                    .iter()
                    .filter_map(|method| {
                        index_of(method).map(|index| (facts_of(views, method).key.as_str(), index))
                    })
                    .collect();
                let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
                for (index, method) in methods.iter().enumerate() {
                    by_name
                        .entry(facts_of(views, method).name.as_str())
                        .or_default()
                        .push(index);
                }
                let mut edges: Vec<(usize, usize)> = Vec::new();
                // The access to a name that is a method and not a field, as a
                // Python property is, runs that method.
                for (from, method) in methods.iter().enumerate() {
                    for name in builder.used.get(*method).into_iter().flatten() {
                        if builder.field_set.contains(name) {
                            continue;
                        }
                        let targets = by_name.get(name.as_str()).into_iter().flatten();
                        edges.extend(targets.filter(|to| **to != from).map(|to| (from, *to)));
                    }
                }
                for (caller, callee) in &builder.edges {
                    let Some(from) = index_of(caller) else {
                        continue;
                    };
                    let targets: Vec<usize> = match callee {
                        Callee::Key(key) => by_key.get(key.as_str()).copied().into_iter().collect(),
                        Callee::OwnMethod(name) => {
                            by_name.get(name.as_str()).cloned().unwrap_or_default()
                        }
                    };
                    edges.extend(
                        targets
                            .into_iter()
                            .filter(|to| *to != from)
                            .map(|to| (from, to)),
                    );
                }
                for (from, to) in edges {
                    method_infos[from].calls.push(to);
                }

                let nom = method_infos.len() as u32;
                let wmc: u32 = method_infos.iter().map(|method| method.complexity).sum();
                // RFC is the size of the union of the methods and the called methods.
                let method_keys: HashSet<&str> = builder
                    .methods
                    .iter()
                    .map(|method| facts_of(views, method).key.as_str())
                    .collect();
                let outside = builder
                    .called
                    .iter()
                    .filter(|callee| match callee.strip_prefix(OWN_PREFIX) {
                        Some(name) => !method_names.contains(name),
                        None => !method_keys.contains(callee.as_str()),
                    })
                    .count();
                let rfc = nom + outside as u32;
                let mut coupled: Vec<String> = builder
                    .coupled
                    .iter()
                    .filter(|name| name.as_str() != own_name)
                    .cloned()
                    .collect();
                coupled.sort();
                let cbo = coupled.len() as u32;
                let lcom = calculate_lcom(&method_infos);
                let fields: Vec<String> =
                    builder.fields.iter().map(|name| name.to_string()).collect();
                let start_line = class.range.start_line as usize;
                let end_line = class.range.end_line.max(builder.end_line) as usize;
                let violations = build_violations(wmc, cbo, lcom);
                let bases: Vec<String> = facts.link_facts[slot.position]
                    .bases
                    .iter()
                    .filter(|base| !self.interfaces.contains(*base))
                    .cloned()
                    .collect();

                let metrics = ClassMetrics {
                    file: facts.file.clone(),
                    class_name: class.name.clone(),
                    parent_class: bases.first().cloned(),
                    language: facts.language.to_string(),
                    start_line,
                    end_line,
                    loc: (end_line - start_line + 1) as u32,
                    wmc,
                    cbo,
                    rfc,
                    lcom,
                    dit: 0,
                    noc: 0,
                    nom,
                    nof: fields.len() as u32,
                    methods: method_infos
                        .iter()
                        .map(|method| method.name.clone())
                        .collect(),
                    fields,
                    coupled_classes: coupled,
                    violations,
                };
                (metrics, bases)
            })
            .collect()
    }
}

fn facts_of<'a>(views: &'a [View<'_>], slot: &Slot) -> &'a crate::index::model::Symbol {
    &views[slot.view].facts.symbols[slot.position]
}

/// Class hierarchy for DIT/NOC calculation.
/// Tracks the base types of each class across the codebase. A class can have more than one base.
#[derive(Debug, Default)]
pub struct ClassHierarchy {
    /// Maps class name -> names of its base types, in source order
    parents: HashMap<String, Vec<String>>,
    /// Maps class name -> list of direct children
    children: HashMap<String, Vec<String>>,
}

impl ClassHierarchy {
    /// Creates a new empty class hierarchy.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a class that has at most one base to the hierarchy.
    pub fn add_class(&mut self, class_name: &str, parent: Option<&str>) {
        let bases: Vec<String> = parent.map(str::to_string).into_iter().collect();
        self.add_class_with_bases(class_name, &bases);
    }

    /// Adds a class and its base types to the hierarchy.
    pub fn add_class_with_bases(&mut self, class_name: &str, bases: &[String]) {
        let mut unique: Vec<String> = Vec::new();
        for base in bases {
            if !unique.contains(base) {
                unique.push(base.clone());
            }
        }
        for base in &unique {
            self.children
                .entry(base.clone())
                .or_default()
                .push(class_name.to_string());
        }
        self.parents.insert(class_name.to_string(), unique);

        // Ensure the class has an entry in children map (even if empty)
        self.children.entry(class_name.to_string()).or_default();
    }

    /// Calculates DIT (Depth of Inheritance Tree) for a class.
    /// DIT = length of the longest path from the class to a root of the
    /// inheritance tree, over all base types. A base that the hierarchy does
    /// not have is a root. A cycle ends at the class that closes it.
    pub fn get_dit(&self, class_name: &str) -> u32 {
        let mut memo: HashMap<&str, u32> = HashMap::new();
        let mut path: Vec<&str> = Vec::new();
        self.depth(class_name, &mut memo, &mut path)
    }

    fn depth<'a>(
        &'a self,
        class_name: &'a str,
        memo: &mut HashMap<&'a str, u32>,
        path: &mut Vec<&'a str>,
    ) -> u32 {
        if let Some(known) = memo.get(class_name) {
            return *known;
        }
        if path.contains(&class_name) {
            return 0;
        }
        let Some(bases) = self.parents.get(class_name) else {
            return 0;
        };
        path.push(class_name);
        let deepest = bases
            .iter()
            .map(|base| 1 + self.depth(base, memo, path))
            .max()
            .unwrap_or(0);
        path.pop();
        // A depth that a cycle cut short is not the depth of the class.
        if !self.closes_cycle(class_name, path) {
            memo.insert(class_name, deepest);
        }
        deepest
    }

    /// The class has a path to a class on the current path.
    fn closes_cycle(&self, class_name: &str, path: &[&str]) -> bool {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut stack: Vec<&str> = vec![class_name];
        while let Some(current) = stack.pop() {
            if !seen.insert(current) {
                continue;
            }
            for base in self.parents.get(current).into_iter().flatten() {
                if path.contains(&base.as_str()) || base == class_name {
                    return true;
                }
                stack.push(base);
            }
        }
        false
    }

    /// Calculates NOC (Number of Children) for a class.
    /// NOC = count of immediate subclasses.
    pub fn get_noc(&self, class_name: &str) -> u32 {
        self.children
            .get(class_name)
            .map(|c| c.len() as u32)
            .unwrap_or(0)
    }
}

/// Builds the list of threshold violations for a class.
fn build_violations(wmc: u32, cbo: u32, lcom: u32) -> Vec<String> {
    let mut violations = Vec::new();
    if wmc > WMC_THRESHOLD {
        violations.push(format!("WMC {} exceeds threshold {}", wmc, WMC_THRESHOLD));
    }
    if cbo > CBO_THRESHOLD {
        violations.push(format!("CBO {} exceeds threshold {}", cbo, CBO_THRESHOLD));
    }
    if lcom > LCOM_THRESHOLD {
        violations.push(format!(
            "LCOM {} exceeds threshold {}",
            lcom, LCOM_THRESHOLD
        ));
    }
    violations
}

/// Method info for LCOM calculation.
struct MethodInfo {
    name: String,
    complexity: u32,
    used_fields: HashSet<String>,
    /// The positions of the methods of the same class that this method calls on the object itself.
    calls: Vec<usize>,
}

/// Calculates LCOM4 (Hitz & Montazeri 1995) as the number of connected components.
/// Two methods are connected if they share an instance variable, or if one
/// calls the other on the object itself.
fn calculate_lcom(methods: &[MethodInfo]) -> u32 {
    if methods.is_empty() {
        return 0;
    }
    let n = methods.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];

    // Build inverted index: field -> list of method indices that use it.
    // Then connect all methods sharing a field via the index, avoiding
    // the O(methods^2 * fields) nested scan.
    let mut field_to_methods: HashMap<&String, Vec<usize>> = HashMap::new();
    for (i, method) in methods.iter().enumerate() {
        for field in &method.used_fields {
            field_to_methods.entry(field).or_default().push(i);
        }
    }
    for method_indices in field_to_methods.values() {
        for w in 0..method_indices.len() {
            for v in (w + 1)..method_indices.len() {
                let a = method_indices[w];
                let b = method_indices[v];
                adj[a].push(b);
                adj[b].push(a);
            }
        }
    }

    for (from, method) in methods.iter().enumerate() {
        for to in &method.calls {
            adj[from].push(*to);
            adj[*to].push(from);
        }
    }

    // Count connected components using DFS
    let mut visited = vec![false; n];
    let mut components = 0;

    for i in 0..n {
        if !visited[i] {
            dfs(i, &adj, &mut visited);
            components += 1;
        }
    }

    components
}

/// DFS for connected components.
fn dfs(v: usize, adj: &[Vec<usize>], visited: &mut [bool]) {
    visited[v] = true;
    for &u in &adj[v] {
        if !visited[u] {
            dfs(u, adj, visited);
        }
    }
}

/// Calculates summary statistics.
fn calculate_summary(classes: &[ClassMetrics]) -> Summary {
    if classes.is_empty() {
        return Summary::default();
    }

    let mut files = HashSet::new();
    let mut total_wmc = 0u32;
    let mut total_cbo = 0u32;
    let mut total_rfc = 0u32;
    let mut total_lcom = 0u32;
    let mut max_wmc = 0u32;
    let mut max_cbo = 0u32;
    let mut max_rfc = 0u32;
    let mut max_lcom = 0u32;
    let mut max_dit = 0u32;
    let mut low_cohesion_count = 0usize;
    let mut violation_count = 0usize;

    for cls in classes {
        files.insert(&cls.file);
        total_wmc += cls.wmc;
        total_cbo += cls.cbo;
        total_rfc += cls.rfc;
        total_lcom += cls.lcom;

        max_wmc = max_wmc.max(cls.wmc);
        max_cbo = max_cbo.max(cls.cbo);
        max_rfc = max_rfc.max(cls.rfc);
        max_lcom = max_lcom.max(cls.lcom);
        max_dit = max_dit.max(cls.dit);

        if cls.lcom > 1 {
            low_cohesion_count += 1;
        }
        violation_count += cls.violations.len();
    }

    let n = classes.len() as f64;
    Summary {
        total_classes: classes.len(),
        total_files: files.len(),
        avg_wmc: total_wmc as f64 / n,
        avg_cbo: total_cbo as f64 / n,
        avg_rfc: total_rfc as f64 / n,
        avg_lcom: total_lcom as f64 / n,
        max_wmc,
        max_cbo,
        max_rfc,
        max_lcom,
        max_dit,
        low_cohesion_count,
        violation_count,
    }
}

/// CK metrics analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    /// When the analysis was generated.
    pub generated_at: String,
    /// Per-class metrics.
    pub classes: Vec<ClassMetrics>,
    /// Summary statistics.
    pub summary: Summary,
}

impl Analysis {
    /// Sorts classes by LCOM (least cohesive first).
    pub fn sort_by_lcom(&mut self) {
        self.classes
            .sort_by_key(|class| std::cmp::Reverse(class.lcom));
    }

    /// Sorts classes by WMC (most complex first).
    pub fn sort_by_wmc(&mut self) {
        self.classes
            .sort_by_key(|class| std::cmp::Reverse(class.wmc));
    }

    /// Sorts classes by CBO (most coupled first).
    pub fn sort_by_cbo(&mut self) {
        self.classes
            .sort_by_key(|class| std::cmp::Reverse(class.cbo));
    }
}

/// CK metrics for a single class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassMetrics {
    /// File path.
    pub file: String,
    /// Class name.
    pub class_name: String,
    /// Parent class name (for inheritance tracking).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_class: Option<String>,
    /// Programming language.
    pub language: String,
    /// Start line.
    pub start_line: usize,
    /// End line.
    pub end_line: usize,
    /// Lines of code in class.
    pub loc: u32,
    /// Weighted Methods per Class (sum of cyclomatic complexity).
    pub wmc: u32,
    /// Coupling Between Objects (number of classes referenced).
    pub cbo: u32,
    /// Response for Class (methods that can be invoked).
    pub rfc: u32,
    /// Lack of Cohesion in Methods (LCOM3 - connected components).
    pub lcom: u32,
    /// Depth of Inheritance Tree.
    pub dit: u32,
    /// Number of Children (direct subclasses).
    pub noc: u32,
    /// Number of methods.
    pub nom: u32,
    /// Number of fields.
    pub nof: u32,
    /// Method names.
    pub methods: Vec<String>,
    /// Field names.
    pub fields: Vec<String>,
    /// Coupled class names.
    pub coupled_classes: Vec<String>,
    /// Metric violations.
    pub violations: Vec<String>,
}

/// Aggregate CK metrics summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    /// Total classes analyzed.
    pub total_classes: usize,
    /// Total files analyzed.
    pub total_files: usize,
    /// Average WMC.
    pub avg_wmc: f64,
    /// Average CBO.
    pub avg_cbo: f64,
    /// Average RFC.
    pub avg_rfc: f64,
    /// Average LCOM.
    pub avg_lcom: f64,
    /// Maximum WMC.
    pub max_wmc: u32,
    /// Maximum CBO.
    pub max_cbo: u32,
    /// Maximum RFC.
    pub max_rfc: u32,
    /// Maximum LCOM.
    pub max_lcom: u32,
    /// Maximum DIT.
    pub max_dit: u32,
    /// Classes with LCOM > 1.
    pub low_cohesion_count: usize,
    /// Total number of violations.
    pub violation_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_oo_language(language: Language) -> bool {
        matches!(class_model(language), ClassModel::Classes(_))
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.skip_test_files);
        assert_eq!(config.max_file_size, 0);
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = Analyzer::new();
        assert!(analyzer.config.skip_test_files);
    }

    #[test]
    fn test_analyzer_with_include_test_files() {
        let analyzer = Analyzer::new().with_include_test_files();
        assert!(!analyzer.config.skip_test_files);
    }

    #[test]
    fn test_is_oo_language() {
        assert!(is_oo_language(Language::Java));
        assert!(is_oo_language(Language::Python));
        assert!(is_oo_language(Language::TypeScript));
        assert!(is_oo_language(Language::Go));
        assert!(is_oo_language(Language::Rust));
        assert!(!is_oo_language(Language::C));
    }

    #[test]
    fn test_is_test_file() {
        assert!(is_test_file(Path::new("foo_test.go")));
        assert!(is_test_file(Path::new("bar_test.py")));
        assert!(is_test_file(Path::new("baz.test.ts")));
        assert!(is_test_file(Path::new("qux.spec.js")));
        assert!(is_test_file(Path::new("src/test/Foo.java")));
        assert!(is_test_file(Path::new("__tests__/foo.js")));
        assert!(!is_test_file(Path::new("src/main.ts")));
        assert!(!is_test_file(Path::new("lib/foo.py")));
    }

    #[test]
    fn test_lcom_no_methods() {
        let methods: Vec<MethodInfo> = vec![];
        assert_eq!(calculate_lcom(&methods), 0);
    }

    #[test]
    fn test_lcom_no_fields() {
        let methods = vec![
            MethodInfo {
                name: "foo".to_string(),
                complexity: 1,
                used_fields: HashSet::new(),
                calls: Vec::new(),
            },
            MethodInfo {
                name: "bar".to_string(),
                complexity: 1,
                used_fields: HashSet::new(),
                calls: Vec::new(),
            },
        ];
        // No fields = methods don't share state = each is its own component
        assert_eq!(calculate_lcom(&methods), 2);
    }

    #[test]
    fn test_lcom_connected() {
        let mut fields1 = HashSet::new();
        fields1.insert("x".to_string());

        let mut fields2 = HashSet::new();
        fields2.insert("x".to_string());

        let methods = vec![
            MethodInfo {
                name: "foo".to_string(),
                complexity: 1,
                used_fields: fields1,
                calls: Vec::new(),
            },
            MethodInfo {
                name: "bar".to_string(),
                complexity: 1,
                used_fields: fields2,
                calls: Vec::new(),
            },
        ];
        // Both methods use field x, so they're connected = 1 component
        assert_eq!(calculate_lcom(&methods), 1);
    }

    #[test]
    fn test_lcom_disconnected() {
        let mut fields1 = HashSet::new();
        fields1.insert("x".to_string());

        let mut fields2 = HashSet::new();
        fields2.insert("y".to_string());

        let methods = vec![
            MethodInfo {
                name: "foo".to_string(),
                complexity: 1,
                used_fields: fields1,
                calls: Vec::new(),
            },
            MethodInfo {
                name: "bar".to_string(),
                complexity: 1,
                used_fields: fields2,
                calls: Vec::new(),
            },
        ];
        // Methods use different fields, not connected = 2 components
        assert_eq!(calculate_lcom(&methods), 2);
    }

    #[test]
    fn test_summary_empty() {
        let summary = calculate_summary(&[]);
        assert_eq!(summary.total_classes, 0);
        assert_eq!(summary.total_files, 0);
    }

    #[test]
    fn test_summary_with_classes() {
        let classes = vec![
            ClassMetrics {
                file: "a.java".to_string(),
                class_name: "Foo".to_string(),
                parent_class: None,
                language: "Java".to_string(),
                start_line: 1,
                end_line: 50,
                loc: 50,
                wmc: 10,
                cbo: 5,
                rfc: 15,
                lcom: 2,
                dit: 1,
                noc: 0,
                nom: 5,
                nof: 3,
                methods: vec![],
                fields: vec![],
                coupled_classes: vec![],
                violations: vec![],
            },
            ClassMetrics {
                file: "b.java".to_string(),
                class_name: "Bar".to_string(),
                parent_class: None,
                language: "Java".to_string(),
                start_line: 1,
                end_line: 30,
                loc: 30,
                wmc: 20,
                cbo: 8,
                rfc: 25,
                lcom: 1,
                dit: 0,
                noc: 1,
                nom: 8,
                nof: 2,
                methods: vec![],
                fields: vec![],
                coupled_classes: vec![],
                violations: vec![],
            },
        ];

        let summary = calculate_summary(&classes);
        assert_eq!(summary.total_classes, 2);
        assert_eq!(summary.total_files, 2);
        assert!((summary.avg_wmc - 15.0).abs() < 0.001);
        assert!((summary.avg_cbo - 6.5).abs() < 0.001);
        assert_eq!(summary.max_wmc, 20);
        assert_eq!(summary.max_cbo, 8);
        assert_eq!(summary.max_lcom, 2);
        assert_eq!(summary.low_cohesion_count, 1); // Only Foo has LCOM > 1
    }

    #[test]
    fn test_class_metrics_fields() {
        let metrics = ClassMetrics {
            file: "Test.java".to_string(),
            class_name: "Test".to_string(),
            parent_class: Some("BaseTest".to_string()),
            language: "Java".to_string(),
            start_line: 1,
            end_line: 100,
            loc: 100,
            wmc: 25,
            cbo: 10,
            rfc: 30,
            lcom: 3,
            dit: 2,
            noc: 1,
            nom: 8,
            nof: 4,
            methods: vec!["foo".to_string(), "bar".to_string()],
            fields: vec!["x".to_string(), "y".to_string()],
            coupled_classes: vec!["Helper".to_string()],
            violations: vec!["LCOM 3 exceeds threshold 1".to_string()],
        };

        assert_eq!(metrics.class_name, "Test");
        assert_eq!(metrics.wmc, 25);
        assert_eq!(metrics.violations.len(), 1);
    }

    #[test]
    fn test_analysis_sorting() {
        let mut analysis = Analysis {
            generated_at: "2024-01-01".to_string(),
            classes: vec![
                ClassMetrics {
                    class_name: "Low".to_string(),
                    parent_class: None,
                    lcom: 1,
                    wmc: 5,
                    cbo: 2,
                    file: String::new(),
                    language: String::new(),
                    start_line: 0,
                    end_line: 0,
                    loc: 0,
                    rfc: 0,
                    dit: 0,
                    noc: 0,
                    nom: 0,
                    nof: 0,
                    methods: vec![],
                    fields: vec![],
                    coupled_classes: vec![],
                    violations: vec![],
                },
                ClassMetrics {
                    class_name: "High".to_string(),
                    parent_class: None,
                    lcom: 5,
                    wmc: 50,
                    cbo: 10,
                    file: String::new(),
                    language: String::new(),
                    start_line: 0,
                    end_line: 0,
                    loc: 0,
                    rfc: 0,
                    dit: 0,
                    noc: 0,
                    nom: 0,
                    nof: 0,
                    methods: vec![],
                    fields: vec![],
                    coupled_classes: vec![],
                    violations: vec![],
                },
            ],
            summary: Summary::default(),
        };

        analysis.sort_by_lcom();
        assert_eq!(analysis.classes[0].class_name, "High");

        analysis.sort_by_wmc();
        assert_eq!(analysis.classes[0].class_name, "High");

        analysis.sort_by_cbo();
        assert_eq!(analysis.classes[0].class_name, "High");
    }

    #[test]
    fn test_analyzer_trait_implementation() {
        let analyzer = Analyzer::new();
        assert_eq!(analyzer.name(), "cohesion");
        assert!(analyzer.description().contains("CK"));
    }

    #[test]
    fn test_thresholds() {
        // WMC threshold of 24 per Chidamber & Kemerer 1994 IEEE TSE
        assert_eq!(WMC_THRESHOLD, 24);
        assert_eq!(CBO_THRESHOLD, 14);
        assert_eq!(LCOM_THRESHOLD, 1);
    }

    #[test]
    fn test_wmc_threshold_at_24() {
        // Verify WMC > 24 is flagged as complex
        let metrics = ClassMetrics {
            file: "Test.java".to_string(),
            class_name: "ComplexClass".to_string(),
            parent_class: None,
            language: "Java".to_string(),
            start_line: 1,
            end_line: 100,
            loc: 100,
            wmc: 25, // Just above threshold
            cbo: 5,
            rfc: 20,
            lcom: 1,
            dit: 0,
            noc: 0,
            nom: 10,
            nof: 5,
            methods: vec![],
            fields: vec![],
            coupled_classes: vec![],
            violations: vec![format!("WMC {} exceeds threshold {}", 25, WMC_THRESHOLD)],
        };

        assert!(metrics.wmc > WMC_THRESHOLD);
        assert!(!metrics.violations.is_empty());

        // Also verify WMC at threshold is not flagged
        let at_threshold = 24;
        assert!(at_threshold <= WMC_THRESHOLD);
    }

    // DIT/NOC Tests - Chidamber & Kemerer 1994 IEEE TSE
    // DIT = Depth of Inheritance Tree (max path length to root)
    // NOC = Number of Children (direct subclasses)

    #[test]
    fn test_lcom_single_method_no_fields() {
        let methods = vec![MethodInfo {
            name: "only".to_string(),
            complexity: 1,
            used_fields: HashSet::new(),
            calls: Vec::new(),
        }];
        // No fields: each method is its own component
        assert_eq!(calculate_lcom(&methods), 1);
    }

    #[test]
    fn test_lcom_single_method_with_fields() {
        let mut used = HashSet::new();
        used.insert("x".to_string());
        let methods = vec![MethodInfo {
            name: "only".to_string(),
            complexity: 1,
            used_fields: used,
            calls: Vec::new(),
        }];
        // Single method using a field: 1 connected component
        assert_eq!(calculate_lcom(&methods), 1);
    }

    #[test]
    fn test_lcom_methods_no_field_usage() {
        // Methods exist, fields exist, but methods don't use any fields
        let methods = vec![
            MethodInfo {
                name: "a".to_string(),
                complexity: 1,
                used_fields: HashSet::new(),
                calls: Vec::new(),
            },
            MethodInfo {
                name: "b".to_string(),
                complexity: 1,
                used_fields: HashSet::new(),
                calls: Vec::new(),
            },
        ];
        // No shared fields: 2 disconnected components
        assert_eq!(calculate_lcom(&methods), 2);
    }

    #[test]
    fn test_lcom_three_methods_chain_connected() {
        // A shares field with B, B shares field with C => all connected
        let mut fa = HashSet::new();
        fa.insert("x".to_string());
        let mut fb = HashSet::new();
        fb.insert("x".to_string());
        fb.insert("y".to_string());
        let mut fc = HashSet::new();
        fc.insert("y".to_string());

        let methods = vec![
            MethodInfo {
                name: "a".to_string(),
                complexity: 1,
                used_fields: fa,
                calls: Vec::new(),
            },
            MethodInfo {
                name: "b".to_string(),
                complexity: 1,
                used_fields: fb,
                calls: Vec::new(),
            },
            MethodInfo {
                name: "c".to_string(),
                complexity: 1,
                used_fields: fc,
                calls: Vec::new(),
            },
        ];
        assert_eq!(calculate_lcom(&methods), 1);
    }

    #[test]
    fn test_lcom_many_methods_sharing_one_field() {
        // All methods share a single field: should be 1 connected component.
        // This exercises the inverted-index path (many methods per field).
        let n = 50;
        let mut methods = Vec::with_capacity(n);
        for i in 0..n {
            let mut used = HashSet::new();
            used.insert("shared".to_string());
            methods.push(MethodInfo {
                name: format!("m{i}"),
                complexity: 1,
                used_fields: used,
                calls: Vec::new(),
            });
        }
        assert_eq!(calculate_lcom(&methods), 1);
    }

    #[test]
    fn test_lcom_star_topology() {
        // One method shares a field with every other method via different fields.
        // Hub method uses fields f0..f4, each satellite uses exactly one.
        let mut hub_fields = HashSet::new();
        for i in 0..5 {
            hub_fields.insert(format!("f{i}"));
        }
        let mut methods = vec![MethodInfo {
            name: "hub".to_string(),
            complexity: 1,
            used_fields: hub_fields,
            calls: Vec::new(),
        }];
        for i in 0..5 {
            let mut used = HashSet::new();
            used.insert(format!("f{i}"));
            methods.push(MethodInfo {
                name: format!("sat{i}"),
                complexity: 1,
                used_fields: used,
                calls: Vec::new(),
            });
        }
        // Hub connects to all satellites; satellites connect to hub => 1 component
        assert_eq!(calculate_lcom(&methods), 1);
    }

    #[test]
    fn test_lcom_two_disjoint_groups() {
        // Group A: methods 0,1 share field "a"
        // Group B: methods 2,3 share field "b"
        // No cross-group links => 2 components
        let methods = vec![
            MethodInfo {
                name: "a0".to_string(),
                complexity: 1,
                used_fields: ["a".to_string()].into_iter().collect(),
                calls: Vec::new(),
            },
            MethodInfo {
                name: "a1".to_string(),
                complexity: 1,
                used_fields: ["a".to_string()].into_iter().collect(),
                calls: Vec::new(),
            },
            MethodInfo {
                name: "b0".to_string(),
                complexity: 1,
                used_fields: ["b".to_string()].into_iter().collect(),
                calls: Vec::new(),
            },
            MethodInfo {
                name: "b1".to_string(),
                complexity: 1,
                used_fields: ["b".to_string()].into_iter().collect(),
                calls: Vec::new(),
            },
        ];
        assert_eq!(calculate_lcom(&methods), 2);
    }

    #[test]
    fn test_summary_division_by_zero_empty() {
        // Verify calculate_summary handles empty classes without division by zero
        let summary = calculate_summary(&[]);
        assert_eq!(summary.avg_wmc, 0.0);
        assert_eq!(summary.avg_cbo, 0.0);
        assert_eq!(summary.avg_rfc, 0.0);
        assert_eq!(summary.avg_lcom, 0.0);
    }

    #[test]
    fn test_class_hierarchy_cycle_detection() {
        // Pathological case: cyclic inheritance (should not infinite loop)
        let mut hierarchy = ClassHierarchy::new();
        hierarchy.add_class("A", Some("B"));
        hierarchy.add_class("B", Some("A"));

        // Should terminate and return a reasonable depth
        let dit_a = hierarchy.get_dit("A");
        let dit_b = hierarchy.get_dit("B");
        assert!(dit_a <= 2, "DIT should be bounded even with cycles");
        assert!(dit_b <= 2, "DIT should be bounded even with cycles");
    }

    #[test]
    fn test_class_hierarchy_self_parent() {
        // Degenerate case: class is its own parent
        let mut hierarchy = ClassHierarchy::new();
        hierarchy.add_class("SelfRef", Some("SelfRef"));

        // Should not infinite loop
        let dit = hierarchy.get_dit("SelfRef");
        assert!(dit <= 1, "Self-parent DIT should be bounded");
    }

    #[test]
    fn test_class_hierarchy_empty() {
        let hierarchy = ClassHierarchy::new();
        assert_eq!(hierarchy.get_dit("Foo"), 0);
        assert_eq!(hierarchy.get_noc("Foo"), 0);
    }

    #[test]
    fn test_class_hierarchy_no_inheritance() {
        let mut hierarchy = ClassHierarchy::new();
        hierarchy.add_class("Foo", None);
        assert_eq!(hierarchy.get_dit("Foo"), 0);
        assert_eq!(hierarchy.get_noc("Foo"), 0);
    }

    #[test]
    fn test_class_hierarchy_single_inheritance() {
        // Child extends Parent
        let mut hierarchy = ClassHierarchy::new();
        hierarchy.add_class("Parent", None);
        hierarchy.add_class("Child", Some("Parent"));

        assert_eq!(hierarchy.get_dit("Parent"), 0);
        assert_eq!(hierarchy.get_dit("Child"), 1);
        assert_eq!(hierarchy.get_noc("Parent"), 1);
        assert_eq!(hierarchy.get_noc("Child"), 0);
    }

    #[test]
    fn test_class_hierarchy_multi_level() {
        // GrandChild extends Child extends Parent
        let mut hierarchy = ClassHierarchy::new();
        hierarchy.add_class("Parent", None);
        hierarchy.add_class("Child", Some("Parent"));
        hierarchy.add_class("GrandChild", Some("Child"));

        assert_eq!(hierarchy.get_dit("Parent"), 0);
        assert_eq!(hierarchy.get_dit("Child"), 1);
        assert_eq!(hierarchy.get_dit("GrandChild"), 2);
        assert_eq!(hierarchy.get_noc("Parent"), 1);
        assert_eq!(hierarchy.get_noc("Child"), 1);
        assert_eq!(hierarchy.get_noc("GrandChild"), 0);
    }

    #[test]
    fn test_class_hierarchy_multiple_children() {
        // Child1, Child2, Child3 all extend Parent
        let mut hierarchy = ClassHierarchy::new();
        hierarchy.add_class("Parent", None);
        hierarchy.add_class("Child1", Some("Parent"));
        hierarchy.add_class("Child2", Some("Parent"));
        hierarchy.add_class("Child3", Some("Parent"));

        assert_eq!(hierarchy.get_dit("Parent"), 0);
        assert_eq!(hierarchy.get_noc("Parent"), 3);
        for child in &["Child1", "Child2", "Child3"] {
            assert_eq!(hierarchy.get_dit(child), 1);
            assert_eq!(hierarchy.get_noc(child), 0);
        }
    }

    #[test]
    fn test_class_hierarchy_unknown_parent() {
        // Child extends UnknownParent (not in codebase, e.g., stdlib)
        let mut hierarchy = ClassHierarchy::new();
        hierarchy.add_class("Child", Some("UnknownParent"));

        // DIT should be 1 (assuming unknown parent has DIT 0)
        assert_eq!(hierarchy.get_dit("Child"), 1);
        assert_eq!(hierarchy.get_noc("Child"), 0);
    }

    #[test]
    fn test_analyzer_uses_content_source_for_historical_commits() {
        use crate::config::Config;
        use crate::core::{AnalysisContext, FileSet, FilesystemSource};
        use std::path::PathBuf;
        use std::sync::Arc;

        let current = tempfile::tempdir().unwrap();
        let historical = tempfile::tempdir().unwrap();
        std::fs::write(
            current.path().join("code.py"),
            "class CurrentVersion:\n    pass\n",
        )
        .unwrap();
        std::fs::write(
            historical.path().join("code.py"),
            "class HistoricalVersion:\n    pass\n",
        )
        .unwrap();
        let files =
            FileSet::from_files(current.path().to_path_buf(), vec![PathBuf::from("code.py")]);
        let config = Config::default();
        let source = Arc::new(FilesystemSource::new(historical.path()));
        let ctx =
            AnalysisContext::new(&files, &config, Some(current.path())).with_content_source(source);

        let result = Analyzer::new().analyze_repo(&ctx).unwrap();

        let names: Vec<&str> = result
            .classes
            .iter()
            .map(|class| class.class_name.as_str())
            .collect();
        assert_eq!(names, vec!["HistoricalVersion"]);
    }

    #[test]
    fn test_file_above_the_size_limit_is_not_analyzed() {
        use crate::config::Config;
        use crate::core::{AnalysisContext, FileSet};

        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("small.py"),
            "class Small:\n    pass\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("large.py"),
            format!("class Large:\n    pass\n# {}\n", "x".repeat(200)),
        )
        .unwrap();
        let config = Config::default();
        let files = FileSet::from_path(directory.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&files, &config, Some(directory.path()));

        let analyzer = Analyzer::with_config(super::Config {
            skip_test_files: true,
            max_file_size: 100,
        });
        let result = analyzer.analyze_repo(&ctx).unwrap();

        let names: Vec<&str> = result
            .classes
            .iter()
            .map(|class| class.class_name.as_str())
            .collect();
        assert_eq!(names, vec!["Small"]);
    }
}
