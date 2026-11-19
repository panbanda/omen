//! Tests of the uses of a function as a value. A use is not a call, and it is
//! not in the call references.

use super::testing::{function_reference_rows as rows, rows as call_rows};
use crate::index::timing::assert_scales_linearly;

#[test]
fn test_python_callback_by_name_is_a_function_reference() {
    let source = "def worker():\n    pass\n\n\ndef run(pool):\n    pool.submit(worker)\n";
    assert_eq!(
        rows(&[("a.py", source)], "a.py"),
        ["6 run>worker UniqueCandidate SameFile [a.py#worker]"]
    );
    let calls = call_rows(&[("a.py", source)], "a.py");
    assert!(calls.iter().all(|row| !row.contains("worker")), "{calls:?}");
}

#[test]
fn test_python_nested_function_as_a_callback() {
    let source = "def handler(request):\n    def load_module():\n        exec(request)\n\n    return call_later(load_module, 5, label=\"x\")\n";
    assert_eq!(
        rows(&[("a.py", source)], "a.py"),
        ["5 handler>load_module UniqueCandidate LexicalScope [a.py#handler.load_module]"]
    );
}

#[test]
fn test_python_value_positions() {
    let source = "def a():\n    pass\n\n\ndef b():\n    pass\n\n\ndef c():\n    pass\n\n\ndef d():\n    pass\n\n\ndef e():\n    pass\n\n\ndef f():\n    pass\n\n\ndef run(register, default=f):\n    chosen = a\n    register(key=b)\n    table = {\"c\": c}\n    items = [d]\n    return e\n";
    assert_eq!(
        rows(&[("a.py", source)], "a.py"),
        [
            "25 run>f UniqueCandidate SameFile [a.py#f]",
            "26 run>a UniqueCandidate SameFile [a.py#a]",
            "27 run>b UniqueCandidate SameFile [a.py#b]",
            "28 run>c UniqueCandidate SameFile [a.py#c]",
            "29 run>d UniqueCandidate SameFile [a.py#d]",
            "30 run>e UniqueCandidate SameFile [a.py#e]",
        ]
    );
}

#[test]
fn test_python_local_value_hides_the_function() {
    let parameter =
        "def worker():\n    pass\n\n\ndef run(pool, worker):\n    pool.submit(worker)\n";
    assert!(rows(&[("a.py", parameter)], "a.py").is_empty());
    let local = "def worker():\n    pass\n\n\ndef run(pool):\n    worker = make()\n    pool.submit(worker)\n";
    assert!(rows(&[("a.py", local)], "a.py").is_empty());
}

/// The name alone is no evidence for a value: a call of the name keeps the
/// function as a candidate, and a use of the name as a value does not.
#[test]
fn test_same_name_in_a_file_without_an_import_is_no_function_reference() {
    let python = [
        (
            "a.py",
            "def run(pool, item):\n    pool.submit(worker)\n    pool.submit(item.step)\n    worker()\n",
        ),
        (
            "b.py",
            "def worker():\n    pass\n\n\nclass Job:\n    def step(self):\n        pass\n",
        ),
    ];
    assert!(rows(&python, "a.py").is_empty());
    assert_eq!(
        call_rows(&python, "a.py"),
        [
            "2 run>pool.submit Unresolved - []",
            "3 run>pool.submit Unresolved - []",
            "4 run>worker Ambiguous NameOnly [b.py#worker]",
        ]
    );
    // A file that loads a different file can name the functions of that file.
    let bash = [
        ("a.sh", "main() {\n  trap cleanup EXIT\n}\n"),
        ("b.sh", "cleanup() {\n  :\n}\n"),
        ("c.sh", "source ./b.sh\nmain() {\n  trap cleanup EXIT\n}\n"),
    ];
    assert!(rows(&bash, "a.sh").is_empty());
    assert_eq!(
        rows(&bash, "c.sh"),
        ["3 main>cleanup UniqueCandidate ImportedModule [b.sh#cleanup]"]
    );
}

/// A member of a parameter that has no type can be a field or a method of
/// each type. A member of a value with a known type is a method of that type.
#[test]
fn test_member_of_a_value_needs_a_known_type() {
    let source = "class Job:\n    def step(self):\n        pass\n\n\ndef run(pool, item, typed: Job):\n    made = Job()\n    pool.submit(item.step)\n    pool.submit(typed.step)\n    pool.submit(made.step)\n";
    assert_eq!(
        one("a.py", source),
        [
            "9 run>typed.step UniqueCandidate ReceiverDeclaredType [a.py#Job.step]",
            "10 run>made.step UniqueCandidate ReceiverDeclaredType [a.py#Job.step]",
        ]
    );
}

#[test]
fn test_python_imported_function_and_bound_method() {
    let files = [
        (
            "a.py",
            "from b import worker\n\n\nclass Job:\n    def step(self):\n        pass\n\n    def run(self, pool):\n        pool.submit(worker)\n        pool.submit(self.step)\n",
        ),
        ("b.py", "def worker():\n    pass\n"),
    ];
    assert_eq!(
        rows(&files, "a.py"),
        [
            "9 Job.run>worker UniqueCandidate ImportedName [b.py#worker]",
            "10 Job.run>self.step UniqueCandidate EnclosingType [a.py#Job.step]",
        ]
    );
}

/// The local name of an import can be different from the name of the function.
#[test]
fn test_import_with_a_different_local_name_is_a_function_reference() {
    let python = [
        (
            "a.py",
            "from b import worker as job\n\n\ndef run(pool):\n    pool.submit(job)\n",
        ),
        ("b.py", "def worker():\n    pass\n"),
    ];
    assert_eq!(
        rows(&python, "a.py"),
        ["5 run>job UniqueCandidate ImportedAlias [b.py#worker]"]
    );
    let typescript = [
        (
            "a.ts",
            "import { worker as job } from './b';\n\nexport function run(pool: Pool): void {\n  pool.submit(job);\n}\n",
        ),
        ("b.ts", "export function worker(): void {}\n"),
    ];
    assert_eq!(
        rows(&typescript, "a.ts"),
        ["4 run>job UniqueCandidate ImportedAlias [b.ts#worker]"]
    );
}

/// A chain of imports can give a function two new names. The use of the last
/// name as a value has the function that a call of that name has.
#[test]
fn test_function_with_a_new_name_from_a_chain_of_imports() {
    let files = [
        (
            "a.py",
            "from pkg import task\n\n\ndef run(pool):\n    pool.submit(task)\n    task()\n",
        ),
        ("pkg/__init__.py", "from .mod import worker as task\n"),
        ("pkg/mod.py", "def worker():\n    pass\n"),
    ];
    let candidates = |row: &String| row.split_once('[').map(|(_, keys)| keys.to_string());
    let calls = call_rows(&files, "a.py");
    let call = calls.iter().find(|row| row.contains("run>task")).unwrap();
    let values = rows(&files, "a.py");
    assert_eq!(values.len(), 1, "{values:?}");
    assert_eq!(candidates(&values[0]), candidates(call));
    assert_eq!(candidates(call).as_deref(), Some("pkg/mod.py#worker]"));
}

/// The candidates in the row of `name`, for the calls or the values of a file.
fn candidates_of(found: &[String], name: &str) -> Vec<String> {
    found
        .iter()
        .filter(|row| row.contains(&format!(">{name} ")))
        .filter_map(|row| row.split_once('['))
        .map(|(_, keys)| keys.trim_end_matches(']').to_string())
        .collect()
}

/// A local name hides a function only in the block that declares the name.
#[test]
fn test_local_name_of_a_block_that_ended_does_not_hide_the_function() {
    let files = [
        (
            "a.go",
            "package a\n\nfunc entry() {\n\t{\n\t\tcleanup := 1\n\t\t_ = cleanup\n\t\tregister(cleanup)\n\t}\n\tregister(cleanup)\n\tcleanup()\n}\n",
        ),
        ("b.go", "package a\n\nfunc cleanup() {}\n"),
    ];
    assert_eq!(
        rows(&files, "a.go"),
        ["9 entry>cleanup UniqueCandidate SamePackage [b.go#cleanup]"]
    );
    assert_eq!(
        candidates_of(&call_rows(&files, "a.go"), "cleanup"),
        ["b.go#cleanup"]
    );
}

/// `Type::<T>name` has type arguments before the name of the method.
#[test]
fn test_java_method_reference_with_type_arguments() {
    let source = "class A {\n    static <T> void step(T item) {}\n\n    void run(java.util.List<String> list) {\n        list.forEach(A::<String>step);\n        Runnable make = A::<String>new;\n    }\n}\n";
    assert_eq!(
        one("A.java", source),
        ["5 A.run>A.step UniqueCandidate QualifiedPath [A.java#A.step]"]
    );
}

/// A module can export the default export of a different module with a name.
/// The use of that name as a value has the function that a call of it has.
#[test]
fn test_default_export_that_a_module_exports_again_with_a_name() {
    let files = [
        (
            "a.js",
            "import { work } from './index.js';\n\nexport function run(pool) {\n  pool.submit(work);\n  work();\n}\n",
        ),
        ("index.js", "export { default as work } from './task.js';\n"),
        ("task.js", "export default function task() {}\n"),
    ];
    let calls = candidates_of(&call_rows(&files, "a.js"), "work");
    assert_eq!(calls, ["task.js#task"]);
    assert_eq!(candidates_of(&rows(&files, "a.js"), "work"), calls);
}

/// A CommonJS `require` in a function binds a local name. The index gives a
/// call of that name no definition, thus the use of the name as a value has
/// none. The two agree also for a `require` at the top level of the file.
#[test]
fn test_required_module_as_a_value_agrees_with_a_call_of_it() {
    for source in [
        "function run(pool) {\n  const handler = require('./handler');\n  pool.submit(handler);\n  handler();\n}\n",
        "const handler = require('./handler');\n\nfunction run(pool) {\n  pool.submit(handler);\n  handler();\n}\n",
    ] {
        let files = [
            ("a.js", source),
            ("handler.js", "module.exports = function handler() {};\n"),
        ];
        let calls = candidates_of(&call_rows(&files, "a.js"), "handler");
        let values = candidates_of(&rows(&files, "a.js"), "handler");
        let edges = |found: &[String]| -> Vec<String> {
            found.iter().filter(|keys| !keys.is_empty()).cloned().collect()
        };
        assert_eq!(edges(&values), edges(&calls), "{source}");
    }
}

/// Each function has a parameter with the same name. The search for the
/// local name of a call must not read the parameters of the other functions.
fn one_parameter_name_in_many_functions_at(size: usize) -> std::time::Duration {
    let mut source = String::new();
    for position in 0..size {
        source.push_str(&format!(
            "def step_{position}(pool):\n    pool.submit(1)\n\n\n"
        ));
    }
    let started = crate::index::timing::Stopwatch::start();
    let found = call_rows(&[("a.py", &source)], "a.py");
    let elapsed = started.elapsed();
    assert_eq!(found.len(), size);
    elapsed
}

#[test]
fn test_one_parameter_name_in_many_functions() {
    one_parameter_name_in_many_functions_at(200);
}

#[test]
#[ignore = "timing: run in release"]
fn timing_one_parameter_name_in_many_functions_scales_linearly() {
    assert_scales_linearly(
        "one parameter name",
        20_000,
        one_parameter_name_in_many_functions_at,
    );
}

/// One file has a chain of imports: each import gives a new name to the name
/// of the import after it, and the last import names a function.
fn chain_of_import_names_at(size: usize) -> std::time::Duration {
    let mut source = String::new();
    for position in 0..size {
        source.push_str(&format!(
            "from b import name_{} as name_{position}\n",
            position + 1
        ));
    }
    source.push_str("\n\ndef run(pool):\n    pool.submit(name_0)\n");
    let other = format!("def name_{size}():\n    pass\n");
    let files = [("a.py", source.as_str()), ("b.py", other.as_str())];
    let mut facts = crate::index::imports::testing::index(&files);
    let started = crate::index::timing::Stopwatch::start();
    super::drop_values_that_name_no_function(&mut facts);
    let elapsed = started.elapsed();
    assert_eq!(facts[0].value_sites.len(), 1);
    elapsed
}

#[test]
fn test_chain_of_import_names_keeps_the_last_name() {
    chain_of_import_names_at(200);
}

#[test]
#[ignore = "timing: run in release"]
fn timing_chain_of_import_names_scales_linearly() {
    assert_scales_linearly("chain of import names", 320_000, chain_of_import_names_at);
}

/// One function uses `size` functions as values. With `one_name`, each use
/// names the same function, and the function has `size` local names also.
fn many_function_references_at(size: usize, one_name: bool) -> std::time::Duration {
    let mut source = String::new();
    for position in 0..size {
        source.push_str(&format!("def task_{position}():\n    pass\n\n\n"));
    }
    source.push_str("def run(pool):\n");
    for position in 0..size {
        let name = if one_name { 0 } else { position };
        source.push_str(&format!(
            "    local_{position} = pool.submit(task_{name})\n"
        ));
    }
    let started = crate::index::timing::Stopwatch::start();
    let found = rows(&[("a.py", &source)], "a.py");
    let elapsed = started.elapsed();
    assert_eq!(found.len(), size);
    let last = if one_name { 0 } else { size - 1 };
    assert_eq!(
        found[size - 1],
        format!(
            "{} run>task_{last} UniqueCandidate SameFile [a.py#task_{last}]",
            size * 5 + 1
        )
    );
    elapsed
}

#[test]
fn test_many_function_references_in_one_file() {
    many_function_references_at(200, false);
    many_function_references_at(200, true);
}

#[test]
#[ignore = "timing: run in release"]
fn timing_many_function_references_in_one_file_scale_linearly() {
    for one_name in [false, true] {
        assert_scales_linearly(&format!("one name: {one_name}"), 20_000, |size| {
            many_function_references_at(size, one_name)
        });
    }
}

/// A handler gives the first function of a chain as a callback. Each function
/// of the chain gives the next function as a callback, and the last function
/// holds a sink.
fn chain_of_callbacks_at(size: usize) -> std::time::Duration {
    let mut source = String::from(
        "from flask import Flask\n\napp = Flask(__name__)\n\n\n@app.route(\"/run\")\ndef handler(pool):\n    pool.submit(step_0)\n\n\n",
    );
    for position in 0..size {
        source.push_str(&format!(
            "def step_{position}(pool):\n    pool.submit(step_{})\n\n\n",
            position + 1
        ));
    }
    source.push_str(&format!("def step_{size}(pool):\n    eval(pool.code)\n"));
    let started = crate::index::timing::Stopwatch::start();
    let analysis = crate::index::security::analysis::analyze_sources(
        &[("app.py", &source)],
        crate::index::view::DEFAULT_MIN_BASIS,
    );
    let elapsed = started.elapsed();
    let paths = &analysis.reachability.paths;
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].steps.len(), size + 2);
    assert_eq!(paths[0].function_references, size + 1);
    assert_eq!(paths[0].path_count, 1);
    assert!(analysis.reachability.no_known_entry.is_empty());
    elapsed
}

#[test]
fn test_chain_of_callbacks_has_one_path() {
    chain_of_callbacks_at(200);
}

#[test]
#[ignore = "timing: run in release"]
fn timing_chain_of_callbacks_scales_linearly() {
    assert_scales_linearly("chain of callbacks", 20_000, chain_of_callbacks_at);
}

#[test]
fn test_python_name_in_a_string_or_a_comment_and_a_class_are_not_references() {
    let source = "class Shape:\n    pass\n\n\ndef worker():\n    pass\n\n\ndef run(pool):\n    # pool.submit(worker)\n    pool.submit(\"worker\")\n    pool.submit(Shape)\n";
    assert!(rows(&[("a.py", source)], "a.py").is_empty());
}

fn one(file: &str, source: &str) -> Vec<String> {
    rows(&[(file, source)], file)
}

/// A function value that has the name of a local variable is a definition.
/// The use of the variable as a value is a reference to that definition.
#[test]
fn test_closure_that_a_local_name_holds_is_a_function_reference() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "a.py",
            "def run(pool):\n    load = lambda: 1\n    pool.submit(load)\n",
            "3 run>load UniqueCandidate LexicalScope [a.py#run.load]",
        ),
        (
            "a.js",
            "function run(pool) {\n  const load = () => 1;\n  pool.submit(load);\n}\n",
            "3 run>load UniqueCandidate LexicalScope [a.js#run.load]",
        ),
        (
            "a.ts",
            "function run(pool: Pool): void {\n  const load = function (): number { return 1; };\n  pool.submit(load);\n}\n",
            "3 run>load UniqueCandidate LexicalScope [a.ts#run.load]",
        ),
        (
            "a.go",
            "package a\n\nfunc run() {\n\tload := func() {}\n\tsubmit(load)\n}\n",
            "5 run>load UniqueCandidate LexicalScope [a.go#run.load]",
        ),
        (
            "a.rs",
            "fn run() {\n    let load = || 1;\n    spawn(load);\n}\n",
            "3 run>load UniqueCandidate LexicalScope [a.rs#run.load]",
        ),
        (
            "A.java",
            "class A {\n    void run(java.util.concurrent.ExecutorService pool) {\n        Runnable load = () -> {};\n        pool.submit(load);\n    }\n}\n",
            "4 A.run>load UniqueCandidate LexicalScope [A.java#A.run.load]",
        ),
        (
            "a.cs",
            "class A\n{\n    void Run()\n    {\n        System.Action load = () => {};\n        System.Threading.Tasks.Task.Run(load);\n    }\n}\n",
            "6 A.Run>load UniqueCandidate LexicalScope [a.cs#A.Run.load]",
        ),
        (
            "a.cpp",
            "void run() {\n    auto load = []() {};\n    submit(load);\n}\n",
            "3 run>load UniqueCandidate LexicalScope [a.cpp#run.load]",
        ),
        (
            "a.rb",
            "def run(pool)\n  load = lambda do\n    1\n  end\n  pool.submit(load)\nend\n",
            "5 run>load UniqueCandidate LexicalScope [a.rb#run.load]",
        ),
        (
            "a.php",
            "<?php\nfunction run($pool) {\n    $load = function () { return 1; };\n    $pool->submit($load);\n}\n",
            "4 run>$load UniqueCandidate LexicalScope [a.php#run.load]",
        ),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .map(|(file, source, expected)| (file, one(file, source), expected))
        .filter(|(_, found, expected)| found.as_slice() != [expected.to_string()])
        .map(|(file, found, _)| format!("{file}: {found:?}"))
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn test_javascript_callback_and_value_positions() {
    let source = "function worker() {}\nfunction other() {}\n\nfunction run(pool) {\n  pool.submit(worker);\n  const table = { a: worker, other };\n  const list = [worker];\n  let chosen = worker;\n  chosen = other;\n  return worker;\n}\n";
    assert_eq!(
        one("a.js", source),
        [
            "5 run>worker UniqueCandidate SameFile [a.js#worker]",
            "6 run>worker UniqueCandidate SameFile [a.js#worker]",
            "6 run>other UniqueCandidate SameFile [a.js#other]",
            "7 run>worker UniqueCandidate SameFile [a.js#worker]",
            "8 run>worker UniqueCandidate SameFile [a.js#worker]",
            "9 run>other UniqueCandidate SameFile [a.js#other]",
            "10 run>worker UniqueCandidate SameFile [a.js#worker]",
        ]
    );
}

#[test]
fn test_javascript_local_value_hides_the_function() {
    let parameter =
        "function worker() {}\n\nfunction run(pool, worker) {\n  pool.submit(worker);\n}\n";
    assert!(one("a.js", parameter).is_empty());
    let local = "function worker() {}\n\nfunction run(pool) {\n  const worker = make();\n  pool.submit(worker);\n}\n";
    assert!(one("a.js", local).is_empty());
    let text = "function worker() {}\n\nfunction run(pool) {\n  // pool.submit(worker)\n  pool.submit(\"worker\");\n}\n";
    assert!(one("a.js", text).is_empty());
}

#[test]
fn test_jsx_attribute_value_is_a_function_reference() {
    let source = "function App() {\n  function handleClick() {}\n  const handleKey = () => {};\n  return <Button onClick={handleClick} onKey={handleKey}>{handleClick}</Button>;\n}\n";
    for file in ["a.jsx", "a.tsx"] {
        assert_eq!(
            one(file, source),
            [
                format!("4 App>handleClick UniqueCandidate LexicalScope [{file}#App.handleClick]"),
                format!("4 App>handleKey UniqueCandidate LexicalScope [{file}#App.handleKey]"),
            ]
        );
    }
}

#[test]
fn test_typescript_method_of_the_object_as_a_value() {
    let source = "class Job {\n  step(): void {}\n  handler = this.step;\n  run(pool: Pool): void {\n    pool.submit(this.step);\n    pool.submit(this.step as Fn);\n  }\n}\n";
    assert_eq!(
        one("a.ts", source),
        [
            "3 Job>self.step UniqueCandidate EnclosingType [a.ts#Job.step]",
            "5 Job.run>self.step UniqueCandidate EnclosingType [a.ts#Job.step]",
        ]
    );
}

#[test]
fn test_go_function_and_method_value() {
    let source = "package a\n\ntype S struct{}\n\nfunc worker() {}\n\nfunc (s *S) Step() {}\n\nfunc (s *S) Run() {\n\tsubmit(worker)\n\tsubmit(s.Step)\n\th := worker\n\tvar g = worker\n\tg = worker\n\tuse(h, g)\n}\n";
    assert_eq!(
        one("a.go", source),
        [
            "10 S.Run>worker UniqueCandidate SameFile [a.go#worker]",
            "11 S.Run>s.Step UniqueCandidate ReceiverDeclaredType [a.go#S.Step]",
            "12 S.Run>worker UniqueCandidate SameFile [a.go#worker]",
            "13 S.Run>worker UniqueCandidate SameFile [a.go#worker]",
            "14 S.Run>worker UniqueCandidate SameFile [a.go#worker]",
        ]
    );
}

#[test]
fn test_go_key_of_a_literal_and_a_new_local_name_are_not_values() {
    let source = "package a\n\ntype T struct{ Handler func() }\n\nfunc Handler() {}\n\nfunc other() {}\n\nfunc build() T {\n\treturn T{Handler: other}\n}\n\nfunc list() []func() {\n\treturn []func(){other}\n}\n\nfunc shadow() {\n\tother := make()\n\tsubmit(other)\n}\n";
    assert_eq!(
        one("a.go", source),
        [
            "10 build>other UniqueCandidate SameFile [a.go#other]",
            "14 list>other UniqueCandidate SameFile [a.go#other]",
        ]
    );
}

#[test]
fn test_go_function_of_the_package_in_a_different_file() {
    let files = [
        ("a.go", "package a\n\nfunc run() {\n\tsubmit(worker)\n}\n"),
        ("b.go", "package a\n\nfunc worker() {}\n"),
        ("c/c.go", "package c\n\nfunc run() {\n\tsubmit(worker)\n}\n"),
    ];
    assert_eq!(
        rows(&files, "a.go"),
        ["4 run>worker UniqueCandidate SamePackage [b.go#worker]"]
    );
    // A different package cannot name a function that is not exported.
    assert!(rows(&files, "c/c.go").is_empty());
}

#[test]
fn test_rust_function_item_as_a_value() {
    let source = "mod jobs {\n    pub fn worker() {}\n}\n\nfn parse(text: &str) -> usize {\n    text.len()\n}\n\nfn run(items: Vec<&str>) -> Vec<usize> {\n    spawn(jobs::worker);\n    let chosen = parse;\n    items.into_iter().map(parse).collect()\n}\n\nfn pick() -> fn(&str) -> usize {\n    parse\n}\n";
    assert_eq!(
        one("a.rs", source),
        [
            "10 run>jobs::worker UniqueCandidate QualifiedPath [a.rs#jobs.worker]",
            "11 run>parse UniqueCandidate SameFile [a.rs#parse]",
            "12 run>parse UniqueCandidate SameFile [a.rs#parse]",
            "16 pick>parse UniqueCandidate SameFile [a.rs#parse]",
        ]
    );
}

#[test]
fn test_rust_function_value_with_a_module_path_resolves_through_the_module_files() {
    let files = [
        ("Cargo.toml", "[package]\nname = \"app\"\n"),
        ("src/lib.rs", "mod routes;\nmod server;\n"),
        ("src/routes/mod.rs", "pub mod health;\n"),
        ("src/routes/health.rs", "pub fn check() {}\n"),
        ("src/other.rs", "pub fn check() {}\n"),
        (
            "src/server.rs",
            "fn app() {\n    spawn(crate::routes::health::check);\n    spawn(super::routes::health::check);\n}\n",
        ),
    ];
    assert_eq!(
        rows(&files, "src/server.rs"),
        [
            "2 app>crate::routes::health::check UniqueCandidate QualifiedPath [src/routes/health.rs#check]",
            "3 app>super::routes::health::check UniqueCandidate QualifiedPath [src/routes/health.rs#check]",
        ]
    );
}

#[test]
fn test_rust_local_value_and_a_type_are_not_function_references() {
    let source = "struct Config;\n\nfn parse(text: &str) -> usize {\n    text.len()\n}\n\nfn run(parse: usize) {\n    spawn(parse);\n    spawn(Config);\n}\n\nfn other() {\n    let parse = 1;\n    spawn(parse);\n}\n";
    assert!(one("a.rs", source).is_empty());
}

#[test]
fn test_java_method_reference() {
    let source = "class A {\n    void step(String text) {}\n\n    static void bar(String text) {}\n\n    void twice(String text) {}\n\n    void twice(int number) {}\n\n    void run(java.util.List<String> list) {\n        list.forEach(this::step);\n        list.forEach(A::bar);\n        list.forEach(this::twice);\n        Runnable make = A::new;\n    }\n}\n";
    assert_eq!(
        one("A.java", source),
        [
            "11 A.run>self.step UniqueCandidate EnclosingType [A.java#A.step]",
            "12 A.run>A.bar UniqueCandidate QualifiedPath [A.java#A.bar]",
            "13 A.run>self.twice Ambiguous EnclosingType [A.java#A.twice, A.java#A.twice~2]",
        ]
    );
}

#[test]
fn test_csharp_method_group() {
    let source = "class A\n{\n    void Step(string text) {}\n\n    void Run(System.Collections.Generic.List<string> list, string Step2)\n    {\n        list.ForEach(Step);\n        System.Action<string> a = this.Step;\n        Click += Step;\n        list.ForEach(Step2);\n    }\n}\n";
    assert_eq!(
        one("a.cs", source),
        [
            "7 A.Run>Step UniqueCandidate EnclosingType [a.cs#A.Step]",
            "8 A.Run>self.Step UniqueCandidate EnclosingType [a.cs#A.Step]",
            "9 A.Run>Step UniqueCandidate EnclosingType [a.cs#A.Step]",
        ]
    );
}

#[test]
fn test_c_function_pointer_by_name() {
    let source = "static void handler(int number) {}\n\nstatic int compare(const void *left, const void *right) { return 0; }\n\nstruct ops { void (*open)(int); };\n\nvoid run(int *items, int handler2) {\n    signal(2, handler);\n    qsort(items, 1, 4, &compare);\n    struct ops table = { .open = handler };\n    void (*chosen)(int) = handler;\n    signal(2, handler2);\n    puts(\"handler\");\n}\n";
    assert_eq!(
        one("a.c", source),
        [
            "8 run>handler UniqueCandidate SameFile [a.c#handler]",
            "9 run>compare UniqueCandidate SameFile [a.c#compare]",
            "10 run>handler UniqueCandidate SameFile [a.c#handler]",
            "11 run>handler UniqueCandidate SameFile [a.c#handler]",
        ]
    );
}

#[test]
fn test_cpp_function_and_member_function_pointer() {
    let source = "#include <thread>\n\nnamespace jobs {\nvoid helper() {}\n}\n\nvoid worker() {}\n\nstruct Job {\n    void step() {}\n    void run() {\n        std::thread first(&Job::step, this);\n        std::thread second{worker};\n        call(jobs::helper);\n    }\n};\n";
    assert_eq!(
        one("a.cpp", source),
        [
            "12 Job.run>Job::step UniqueCandidate QualifiedPath [a.cpp#Job.step]",
            "13 Job.run>worker UniqueCandidate SameFile [a.cpp#worker]",
            "14 Job.run>jobs::helper UniqueCandidate QualifiedPath [a.cpp#jobs.helper]",
        ]
    );
}

#[test]
fn test_ruby_method_object() {
    let source = "class Job\n  def step(item)\n  end\n\n  def run(list)\n    list.each(&method(:step))\n    list.map(&:step)\n    list.each(&self.method(:step))\n    puts \"step\"\n  end\nend\n";
    assert_eq!(
        one("a.rb", source),
        [
            "6 Job.run>step UniqueCandidate EnclosingType [a.rb#Job.step]",
            "8 Job.run>self.step UniqueCandidate EnclosingType [a.rb#Job.step]",
        ]
    );
}

#[test]
fn test_php_callable_array() {
    let source = "<?php\nclass Job {\n    function step($item) {}\n\n    static function order($left, $right) {}\n\n    function run($list) {\n        array_map([$this, 'step'], $list);\n        usort($list, [Job::class, 'order']);\n        usort($list, [self::class, \"order\"]);\n        array_map('step', $list);\n        $pair = [$this, 'step', 1];\n    }\n}\n";
    assert_eq!(
        one("a.php", source),
        [
            "8 Job.run>self.step UniqueCandidate EnclosingType [a.php#Job.step]",
            "9 Job.run>Job::order UniqueCandidate QualifiedPath [a.php#Job.order]",
            "10 Job.run>self.order UniqueCandidate EnclosingType [a.php#Job.order]",
        ]
    );
}

/// The first-class callable syntax `name(...)` is a call in the index.
#[test]
fn test_php_first_class_callable_is_a_call() {
    let source = "<?php\nfunction worker($item) {}\n\nfunction run($list) {\n    return array_map(worker(...), $list);\n}\n";
    assert!(one("a.php", source).is_empty());
    let calls = call_rows(&[("a.php", source)], "a.php");
    assert!(
        calls.contains(&"5 run>worker UniqueCandidate SameFile [a.php#worker]".to_string()),
        "{calls:?}"
    );
}

#[test]
fn test_bash_trap_names_a_function() {
    let source = "cleanup() {\n  rm -f x\n}\n\nmain() {\n  trap cleanup EXIT\n  trap 'cleanup' INT\n  trap \"cleanup\" TERM\n  trap -- cleanup HUP\n  trap 'cleanup; exit 1' QUIT\n  trap - EXIT\n  echo cleanup | xargs -n1 echo\n  echo \"cleanup\"\n}\n";
    assert_eq!(
        one("a.sh", source),
        [
            "6 main>cleanup UniqueCandidate SameFile [a.sh#cleanup]",
            "7 main>cleanup UniqueCandidate SameFile [a.sh#cleanup]",
            "8 main>cleanup UniqueCandidate SameFile [a.sh#cleanup]",
            "9 main>cleanup UniqueCandidate SameFile [a.sh#cleanup]",
        ]
    );
}
