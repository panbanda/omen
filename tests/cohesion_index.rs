//! The cohesion metrics come from the facts of the shared index. Each test
//! gives source text to the index and reads the metrics of one class.
//!
//! The truth harness has no cohesion dimension. Each language has a class
//! `Counter` with three fields and three methods. `increment` and `reset`
//! share the field `count`. `describe` uses only `label`. Thus LCOM is 2.
//! `increment` has one decision point, thus WMC is 4. `increment` and
//! `describe` each call one function, thus RFC is 5.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use omen::analyzers::cohesion::{Analysis, Analyzer, ClassMetrics};
use omen::core::Language;
use omen::index::build::CodeIndex;
use omen::index::members::{class_model, ClassModel};

fn analyze(sources: &[(&str, &str)]) -> Analysis {
    let read = |path: &Path| {
        sources
            .iter()
            .find(|(name, _)| Path::new(name) == path)
            .map(|(_, text)| text.as_bytes().to_vec())
            .ok_or_else(|| omen::core::Error::git("no such file".to_string()))
    };
    let list = |_: &Path| -> Vec<PathBuf> { Vec::new() };
    let files: Vec<PathBuf> = sources
        .iter()
        .map(|(name, _)| PathBuf::from(name))
        .collect();
    let index = CodeIndex::build(Path::new("."), &files, &read, &list);
    Analyzer::new().analyze_index(&index, &HashSet::new())
}

fn class<'a>(analysis: &'a Analysis, name: &str) -> &'a ClassMetrics {
    let found: Vec<&ClassMetrics> = analysis
        .classes
        .iter()
        .filter(|class| class.class_name == name)
        .collect();
    assert_eq!(found.len(), 1, "{name}: {:?}", analysis.classes);
    found[0]
}

struct Expected {
    parent: Option<&'static str>,
}

fn check_counter(file: &str, source: &str, expected: Expected) {
    let analysis = analyze(&[(file, source)]);
    let counter = class(&analysis, "Counter");
    assert_eq!(counter.nom, 3, "{file}: methods {:?}", counter.methods);
    assert_eq!(counter.nof, 3, "{file}: fields {:?}", counter.fields);
    assert_eq!(counter.wmc, 4, "{file}");
    assert_eq!(counter.lcom, 2, "{file}");
    assert_eq!(counter.rfc, 5, "{file}");
    assert_eq!(counter.parent_class.as_deref(), expected.parent, "{file}");
}

#[test]
fn test_rust_struct_with_impl_block() {
    let source = "pub struct Counter {\n    count: u32,\n    step: u32,\n    label: String,\n}\n\nimpl Counter {\n    pub fn increment(&mut self) {\n        if self.step > 0 {\n            self.count += self.step;\n        }\n        audit();\n    }\n\n    pub fn reset(&mut self) {\n        self.count = 0;\n    }\n\n    pub fn describe(&self) -> String {\n        render(&self.label)\n    }\n}\n";
    check_counter("src/counter.rs", source, Expected { parent: None });
}

#[test]
fn test_rust_methods_of_two_impl_blocks_belong_to_one_struct() {
    let source = "pub struct Widget {\n    width: u32,\n    height: u32,\n}\n\nimpl Widget {\n    pub fn area(&self) -> u32 {\n        self.width * self.height\n    }\n}\n\nimpl Default for Widget {\n    fn default() -> Self {\n        Widget { width: 1, height: 1 }\n    }\n}\n";
    let analysis = analyze(&[("src/widget.rs", source)]);
    let widget = class(&analysis, "Widget");
    assert_eq!(widget.nom, 2);
    assert_eq!(widget.nof, 2);
    assert_eq!(widget.start_line, 1);
    assert_eq!(widget.end_line, 16);
}

#[test]
fn test_go_struct_with_receiver_methods() {
    let source = "package main\n\ntype Counter struct {\n\tcount int\n\tstep  int\n\tlabel string\n}\n\nfunc (c *Counter) Increment() {\n\tif c.step > 0 {\n\t\tc.count += c.step\n\t}\n\taudit()\n}\n\nfunc (c *Counter) Reset() {\n\tc.count = 0\n}\n\nfunc (c Counter) Describe() string {\n\treturn render(c.label)\n}\n";
    check_counter("counter.go", source, Expected { parent: None });
}

#[test]
fn test_go_receiver_name_is_not_guessed_from_its_length() {
    let source = "package main\n\ntype Counter struct {\n\tcount int\n\tlabel string\n}\n\nfunc (counter *Counter) Reset(other Counter) {\n\tcounter.count = 0\n\tother.label = \"\"\n}\n\nfunc (counter *Counter) Describe(w Counter) string {\n\treturn w.label\n}\n";
    let analysis = analyze(&[("counter.go", source)]);
    let counter = class(&analysis, "Counter");
    assert_eq!(counter.nom, 2);
    // Only the receiver reaches a field of the object. `other.label` and
    // `w.label` do not.
    assert_eq!(counter.lcom, 2);
}

#[test]
fn test_go_unexported_type_couples() {
    let source = "package shop\n\ntype worker struct{}\n\ntype Shop struct {\n\tw      worker\n\tname   string\n\tfailed error\n\tcode   rune\n}\n";
    let analysis = analyze(&[("shop.go", source)]);
    let shop = class(&analysis, "Shop");
    assert_eq!(shop.coupled_classes, ["worker"]);
    assert_eq!(shop.cbo, 1);
}

#[test]
fn test_go_type_with_a_one_character_name_couples() {
    let source = "package shop\n\ntype w struct{}\n\ntype Shop struct {\n\titem w\n}\n";
    let analysis = analyze(&[("shop.go", source)]);
    assert_eq!(class(&analysis, "Shop").coupled_classes, ["w"]);
    assert_eq!(class(&analysis, "Shop").cbo, 1);
}

#[test]
fn test_go_type_parameter_does_not_couple() {
    let source = "package box\n\ntype Box[T any] struct {\n\tvalue T\n}\n\nfunc (b *Box[T]) Get() T {\n\treturn b.value\n}\n";
    let analysis = analyze(&[("box.go", source)]);
    assert_eq!(class(&analysis, "Box").cbo, 0);
}

#[test]
fn test_go_methods_in_another_file_of_the_package_belong_to_the_struct() {
    let declaration = "package shop\n\ntype Cart struct {\n\titems int\n}\n";
    let methods = "package shop\n\nfunc (c *Cart) Add() {\n\tc.items++\n}\n\nfunc (c *Cart) Count() int {\n\treturn c.items\n}\n";
    let analysis = analyze(&[
        ("shop/cart.go", declaration),
        ("shop/cart_methods.go", methods),
    ]);
    let cart = class(&analysis, "Cart");
    assert_eq!(cart.nom, 2);
    assert_eq!(cart.lcom, 1);
}

#[test]
fn test_python_class() {
    let source = "class Counter(Base):\n    def increment(self):\n        if self.step > 0:\n            self.count = self.count + self.step\n        audit()\n\n    def reset(self):\n        self.count = 0\n        self.step = 1\n\n    def describe(self):\n        self.label = 'x'\n        return render(self.label)\n";
    check_counter(
        "counter.py",
        source,
        Expected {
            parent: Some("Base"),
        },
    );
}

#[test]
fn test_python_coupling_does_not_count_each_identifier() {
    let source = "class Order:\n    def total(self, price, quantity):\n        subtotal = price * quantity\n        return subtotal\n\n    def customer(self, who: Customer) -> Invoice:\n        return who\n";
    let analysis = analyze(&[("order.py", source)]);
    let order = class(&analysis, "Order");
    assert_eq!(order.coupled_classes, ["Customer", "Invoice"]);
    assert_eq!(order.cbo, 2);
}

#[test]
fn test_python_call_of_a_class_in_the_repository_couples() {
    let sources = [
        ("model.py", "class Invoice:\n    pass\n"),
        (
            "order.py",
            "from model import Invoice\n\nclass Order:\n    def bill(self):\n        return Invoice()\n",
        ),
    ];
    let analysis = analyze(&sources);
    assert_eq!(class(&analysis, "Order").coupled_classes, ["Invoice"]);
}

#[test]
fn test_ruby_class() {
    let source = "class Counter < Base\n  def increment\n    if @step > 0\n      @count = @count + @step\n    end\n    audit()\n  end\n\n  def reset\n    @count = 0\n  end\n\n  def describe\n    render(@label)\n  end\nend\n";
    check_counter(
        "counter.rb",
        source,
        Expected {
            parent: Some("Base"),
        },
    );
}

#[test]
fn test_ruby_module_is_a_class() {
    let source =
        "module Helpers\n  def one\n    @value\n  end\n\n  def two\n    @value\n  end\nend\n";
    let analysis = analyze(&[("helpers.rb", source)]);
    let helpers = class(&analysis, "Helpers");
    assert_eq!(helpers.nom, 2);
    assert_eq!(helpers.lcom, 1);
}

#[test]
fn test_java_class_with_fields_used_without_this() {
    let source = "class Counter extends Base {\n    private int count;\n    private int step;\n    private String label;\n\n    void increment() {\n        if (step > 0) {\n            count = count + step;\n        }\n        audit();\n    }\n\n    void reset() {\n        count = 0;\n    }\n\n    String describe() {\n        return render(label);\n    }\n}\n";
    check_counter(
        "Counter.java",
        source,
        Expected {
            parent: Some("Base"),
        },
    );
}

#[test]
fn test_java_this_access_and_local_that_hides_a_field() {
    let source = "class Box {\n    private int width;\n    private int height;\n\n    int area() {\n        return this.width * this.height;\n    }\n\n    int shadow(int height) {\n        int width = height;\n        return width;\n    }\n}\n";
    let analysis = analyze(&[("Box.java", source)]);
    let found = class(&analysis, "Box");
    // `shadow` names a parameter and a local variable, not the fields.
    assert_eq!(found.lcom, 2);
}

#[test]
fn test_java_interface_counts_its_methods() {
    let source = "interface Shape {\n    double area();\n    double perimeter();\n}\n";
    let analysis = analyze(&[("Shape.java", source)]);
    assert_eq!(class(&analysis, "Shape").nom, 2);
}

#[test]
fn test_csharp_class() {
    let source = "class Counter : Base\n{\n    private int count;\n    private int step;\n    private string label;\n\n    void Increment()\n    {\n        if (step > 0)\n        {\n            count = count + step;\n        }\n        Audit();\n    }\n\n    void Reset()\n    {\n        this.count = 0;\n    }\n\n    string Describe()\n    {\n        return Render(label);\n    }\n}\n";
    check_counter(
        "Counter.cs",
        source,
        Expected {
            parent: Some("Base"),
        },
    );
}

#[test]
fn test_csharp_record_positional_parameter_is_a_property() {
    let source = "public record Pair(int X)\n{\n    public int First() { return X; }\n    public int Second() { return X; }\n}\n";
    let analysis = analyze(&[("Pair.cs", source)]);
    let pair = class(&analysis, "Pair");
    assert_eq!(pair.nom, 2);
    assert_eq!(pair.fields, ["X"]);
    assert_eq!(pair.lcom, 1);
    let method = "public record Pair(int X)\n{\n    public int Add(int X) { return X; }\n    public int Second() { return X; }\n}\n";
    let analysis = analyze(&[("Pair.cs", method)]);
    assert_eq!(class(&analysis, "Pair").lcom, 2);
}

#[test]
fn test_csharp_property_is_a_field() {
    let source = "class Person\n{\n    public string Name { get; set; }\n    public int Age { get; set; }\n}\n";
    let analysis = analyze(&[("Person.cs", source)]);
    assert_eq!(class(&analysis, "Person").nof, 2);
}

#[test]
fn test_cpp_class_in_one_file() {
    let source = "class Counter : public Base {\n    int count;\n    int step;\n    std::string label;\n\npublic:\n    void increment() {\n        if (step > 0) {\n            count = count + step;\n        }\n        audit();\n    }\n\n    void reset() {\n        this->count = 0;\n    }\n\n    std::string describe() {\n        return render(label);\n    }\n};\n";
    check_counter(
        "counter.cpp",
        source,
        Expected {
            parent: Some("Base"),
        },
    );
}

#[test]
fn test_cpp_out_of_line_methods_belong_to_the_class_of_the_header() {
    let header = "class Counter {\n    int count;\n    int label;\n\npublic:\n    void increment();\n    void reset();\n    int describe();\n};\n";
    let source = "#include \"counter.h\"\n\nvoid Counter::increment() {\n    if (count > 0) {\n        count = count + 1;\n    }\n}\n\nvoid Counter::reset() {\n    count = 0;\n}\n\nint Counter::describe() {\n    return label;\n}\n";
    let analysis = analyze(&[("counter.h", header), ("counter.cpp", source)]);
    let counter = class(&analysis, "Counter");
    assert_eq!(counter.nom, 3, "methods {:?}", counter.methods);
    assert_eq!(counter.wmc, 4);
    assert_eq!(counter.lcom, 2);
}

fn typescript_counter() -> &'static str {
    "class Counter extends Base {\n  count = 0;\n  step = 1;\n  label = 'x';\n\n  increment() {\n    if (this.step > 0) {\n      this.count = this.count + this.step;\n    }\n    audit();\n  }\n\n  reset() {\n    this.count = 0;\n  }\n\n  describe() {\n    return render(this.label);\n  }\n}\n"
}

#[test]
fn test_typescript_class() {
    check_counter(
        "counter.ts",
        typescript_counter(),
        Expected {
            parent: Some("Base"),
        },
    );
}

#[test]
fn test_tsx_class() {
    check_counter(
        "counter.tsx",
        typescript_counter(),
        Expected {
            parent: Some("Base"),
        },
    );
}

#[test]
fn test_javascript_class() {
    check_counter(
        "counter.js",
        typescript_counter(),
        Expected {
            parent: Some("Base"),
        },
    );
}

#[test]
fn test_jsx_class() {
    check_counter(
        "counter.jsx",
        typescript_counter(),
        Expected {
            parent: Some("Base"),
        },
    );
}

#[test]
fn test_typescript_field_is_not_a_method() {
    let source = "class Box {\n  width = 1;\n  height = 2;\n  area() {\n    return this.width * this.height;\n  }\n}\n";
    let analysis = analyze(&[("box.ts", source)]);
    let found = class(&analysis, "Box");
    assert_eq!(found.nom, 1);
    assert_eq!(found.lcom, 1);
}

#[test]
fn test_javascript_new_expression_couples() {
    let source = "class Shop {\n  open() {\n    return new Door();\n  }\n}\n";
    let analysis = analyze(&[("shop.js", source)]);
    assert_eq!(class(&analysis, "Shop").coupled_classes, ["Door"]);
}

#[test]
fn test_php_class() {
    let source = "<?php\nclass Counter extends Base {\n    private $count = 0;\n    private $step = 1;\n    private $label = 'x';\n\n    function increment() {\n        if ($this->step > 0) {\n            $this->count = $this->count + $this->step;\n        }\n        audit();\n    }\n\n    function reset() {\n        $this->count = 0;\n    }\n\n    function describe() {\n        return render($this->label);\n    }\n}\n";
    check_counter(
        "Counter.php",
        source,
        Expected {
            parent: Some("Base"),
        },
    );
}

#[test]
fn test_php_promoted_constructor_parameter_is_a_field() {
    let source = "<?php\nclass Point {\n    public function __construct(private int $x, private int $y) {}\n\n    public function sum() {\n        return $this->x + $this->y;\n    }\n}\n";
    let analysis = analyze(&[("Point.php", source)]);
    let point = class(&analysis, "Point");
    assert_eq!(point.nof, 2);
    assert_eq!(point.lcom, 2);
}

#[test]
fn test_c_and_bash_have_no_class() {
    for language in [Language::C, Language::Bash] {
        assert!(
            matches!(class_model(language), ClassModel::NotApplicable(reason) if !reason.is_empty()),
            "{language}"
        );
    }
    let analysis = analyze(&[
        (
            "main.c",
            "struct Shop { int doors; };\nint open(struct Shop *shop) { return shop->doors; }\n",
        ),
        ("run.sh", "open() {\n  echo open\n}\n"),
    ]);
    assert!(analysis.classes.is_empty());
}

#[test]
fn test_nested_class_does_not_add_its_methods_to_the_outer_class() {
    let source = "class Outer:\n    def one(self):\n        pass\n\n    class Inner:\n        def two(self):\n            pass\n\n        def three(self):\n            pass\n";
    let analysis = analyze(&[("nested.py", source)]);
    assert_eq!(class(&analysis, "Outer").nom, 1);
    assert_eq!(class(&analysis, "Inner").nom, 2);
}

#[test]
fn test_function_in_a_method_belongs_to_the_method() {
    let source = "class Job:\n    def run(self):\n        def inner():\n            return self.state\n        if self.state:\n            return inner()\n\n    def stop(self):\n        self.state = 0\n";
    let analysis = analyze(&[("job.py", source)]);
    let job = class(&analysis, "Job");
    assert_eq!(job.nom, 2);
    assert_eq!(job.wmc, 3);
    assert_eq!(job.lcom, 1);
}

#[test]
fn test_inheritance_depth_and_children() {
    let sources = [
        ("a.py", "class A:\n    pass\n"),
        ("b.py", "class B(A):\n    pass\n"),
        ("c.py", "class C(B):\n    pass\n"),
    ];
    let analysis = analyze(&sources);
    assert_eq!(class(&analysis, "A").noc, 1);
    assert_eq!(class(&analysis, "B").dit, 1);
    assert_eq!(class(&analysis, "C").dit, 2);
}

#[test]
fn test_dit_is_the_longest_path_over_all_bases() {
    let python = "class Root: pass\nclass Deep(Root): pass\nclass Shallow: pass\nclass Leaf(Shallow, Deep): pass\n";
    let analysis = analyze(&[("a.py", python)]);
    assert_eq!(class(&analysis, "Leaf").dit, 2);
    assert_eq!(
        class(&analysis, "Leaf").parent_class.as_deref(),
        Some("Shallow")
    );
    assert_eq!(class(&analysis, "Deep").noc, 1);
    assert_eq!(class(&analysis, "Shallow").noc, 1);
    assert_eq!(class(&analysis, "Root").noc, 1);

    let cpp = "class Root {};\nclass Deep : public Root {};\nclass Shallow {};\nclass Leaf : public Shallow, public Deep {};\n";
    let analysis = analyze(&[("a.cpp", cpp)]);
    assert_eq!(class(&analysis, "Leaf").dit, 2);

    let java = "interface Root {}\ninterface Deep extends Root {}\ninterface Shallow {}\ninterface Leaf extends Shallow, Deep {}\n";
    let analysis = analyze(&[("A.java", java)]);
    assert_eq!(class(&analysis, "Leaf").dit, 2);
}

#[test]
fn test_dit_reads_generic_and_qualified_bases() {
    let python = "from typing import Generic\n\nclass Root: pass\nclass Deep(Root, Generic[T]): pass\nclass Shallow: pass\nclass Leaf(Shallow, Deep[int]): pass\nclass Other(pkg.Leaf): pass\n";
    let analysis = analyze(&[("a.py", python)]);
    assert_eq!(class(&analysis, "Leaf").dit, 2);
    assert_eq!(class(&analysis, "Other").dit, 3);

    let java =
        "class Root<T> {}\nclass Deep extends Root<String> {}\nclass Leaf extends shop.Deep {}\n";
    let analysis = analyze(&[("A.java", java)]);
    assert_eq!(class(&analysis, "Deep").dit, 1);
    assert_eq!(class(&analysis, "Leaf").dit, 2);

    let csharp = "class Root<T> {}\nclass Deep : Root<int> {}\nclass Leaf : Shop.Deep {}\n";
    let analysis = analyze(&[("A.cs", csharp)]);
    assert_eq!(class(&analysis, "Deep").dit, 1);
    assert_eq!(class(&analysis, "Leaf").dit, 2);

    let cpp = "template <typename T> class Root {};\nclass Deep : public Root<int> {};\nclass Leaf : public shop::Deep {};\n";
    let analysis = analyze(&[("a.cpp", cpp)]);
    assert_eq!(class(&analysis, "Deep").dit, 1);
    assert_eq!(class(&analysis, "Leaf").dit, 2);

    let typescript =
        "class Root<T> {}\nclass Deep extends Root<string> {}\nclass Leaf extends shop.Deep {}\n";
    let analysis = analyze(&[("a.ts", typescript)]);
    assert_eq!(class(&analysis, "Deep").dit, 1);
    assert_eq!(class(&analysis, "Leaf").dit, 2);

    let php = "<?php\nclass Root {}\nclass Deep extends \\Shop\\Root {}\n";
    let analysis = analyze(&[("a.php", php)]);
    assert_eq!(class(&analysis, "Deep").dit, 1);

    let ruby = "class Root\nend\n\nclass Deep < Shop::Root\nend\n";
    let analysis = analyze(&[("a.rb", ruby)]);
    assert_eq!(class(&analysis, "Deep").dit, 1);
}

#[test]
fn test_dit_ends_when_the_bases_have_a_cycle() {
    let sources = [
        ("a.py", "class A(B, C): pass\n"),
        ("b.py", "class B(A): pass\n"),
        ("c.py", "class C: pass\n"),
    ];
    let analysis = analyze(&sources);
    assert!(class(&analysis, "A").dit <= 3);
}

#[test]
fn test_test_files_are_skipped_unless_asked() {
    let source = "class Hidden:\n    pass\n";
    let analysis = analyze(&[("tests/hidden_test.py", source)]);
    assert!(analysis.classes.is_empty());
}

#[test]
fn test_loop_keyword_adds_one_decision_point() {
    let cases: &[(&str, &str)] = &[
        ("a.go", "package a\n\ntype A struct{ n int }\n\nfunc (a *A) Run() {\n\tfor i := 0; i < a.n; i++ {\n\t}\n}\n"),
        ("a.rs", "struct A { n: u32 }\nimpl A {\n    fn run(&self) {\n        for _ in 0..self.n {}\n        while false {}\n    }\n}\n"),
        ("a.py", "class A:\n    def run(self):\n        for _ in range(3):\n            pass\n        while False:\n            pass\n"),
        ("A.java", "class A {\n    void run() {\n        for (int i = 0; i < 3; i++) {}\n        while (false) {}\n    }\n}\n"),
        ("a.ts", "class A {\n  run() {\n    for (let i = 0; i < 3; i++) {}\n    while (false) {}\n  }\n}\n"),
        ("a.rb", "class A\n  def run\n    while false\n    end\n    for i in 1..3\n    end\n  end\nend\n"),
    ];
    for (file, source) in cases {
        let analysis = analyze(&[(file, source)]);
        let found = class(&analysis, "A");
        let loops = if file.ends_with(".go") { 1 } else { 2 };
        assert_eq!(found.wmc, 1 + loops, "{file}");
    }
}

#[test]
fn test_calling_a_method_is_not_a_field_use() {
    let source = "class Shop:\n    def open(self):\n        self.log()\n\n    def log(self):\n        pass\n\n    def close(self):\n        self.window = 1\n";
    let analysis = analyze(&[("shop.py", source)]);
    let shop = class(&analysis, "Shop");
    assert_eq!(shop.nom, 3);
    assert_eq!(shop.nof, 1);
    // `open` and `log` are connected by the call. `close` has the field.
    assert_eq!(shop.lcom, 2);
}

#[test]
fn test_field_that_the_class_does_not_declare_still_connects_methods() {
    let source = "class Entity(Base):\n    def first(self):\n        return self.identifier\n\n    def second(self):\n        return self.identifier\n";
    let analysis = analyze(&[("entity.py", source)]);
    assert_eq!(class(&analysis, "Entity").lcom, 1);
}

#[test]
fn test_rfc_counts_a_called_method_one_time() {
    let source = "class Shop:\n    def open(self, door, window):\n        door.lock()\n        window.lock()\n        door.lock()\n";
    let analysis = analyze(&[("shop.py", source)]);
    let shop = class(&analysis, "Shop");
    assert_eq!(shop.nom, 1);
    assert_eq!(shop.rfc, 2);
}

#[test]
fn test_rfc_is_the_union_of_the_methods_and_the_called_methods() {
    let cases: &[(&str, &str)] = &[
        (
            "Door.java",
            "class Door {\n    void open() { close(); }\n    void close() {}\n}\n",
        ),
        (
            "door.py",
            "class Door:\n    def open(self):\n        self.close()\n\n    def close(self):\n        pass\n",
        ),
        (
            "door.rs",
            "struct Door;\nimpl Door {\n    fn open(&self) {\n        self.close();\n    }\n\n    fn close(&self) {}\n}\n",
        ),
        (
            "door.ts",
            "class Door {\n  open() {\n    this.close();\n  }\n\n  close() {}\n}\n",
        ),
    ];
    for (file, source) in cases {
        let analysis = analyze(&[(file, source)]);
        let door = class(&analysis, "Door");
        assert_eq!(door.nom, 2, "{file}");
        assert_eq!(door.rfc, 2, "{file}");
    }
}

#[test]
fn test_rfc_counts_a_constructor_call() {
    let java = "class Part {\n    Part() {}\n}\n\nclass Factory {\n    Part make() { return new Part(); }\n}\n";
    let analysis = analyze(&[("A.java", java)]);
    assert_eq!(class(&analysis, "Factory").rfc, 2);

    let no_constructor =
        "class Part {\n}\n\nclass Factory {\n    Part make() { return new Part(); }\n}\n";
    let analysis = analyze(&[("A.java", no_constructor)]);
    assert_eq!(class(&analysis, "Factory").rfc, 2);

    let csharp = "class Part {\n    public Part() {}\n}\n\nclass Factory {\n    Part Make() { return new Part(); }\n}\n";
    let analysis = analyze(&[("A.cs", csharp)]);
    assert_eq!(class(&analysis, "Factory").rfc, 2);

    let typescript = "class Part {\n  constructor() {}\n}\n\nclass Factory {\n  make() {\n    return new Part();\n  }\n}\n";
    let analysis = analyze(&[("a.ts", typescript)]);
    assert_eq!(class(&analysis, "Factory").rfc, 2);
}

#[test]
fn test_rfc_counts_a_method_that_the_class_does_not_have() {
    let source = "class Door {\n    void open() { close(); lock(); }\n    void close() {}\n}\n";
    let analysis = analyze(&[("Door.java", source)]);
    assert_eq!(class(&analysis, "Door").rfc, 3);
}

#[test]
fn test_rfc_counts_two_resolved_methods_with_one_name_two_times() {
    let source = "package shop\n\ntype Door struct{}\n\nfunc (d Door) Lock() {}\n\ntype Window struct{}\n\nfunc (w Window) Lock() {}\n\ntype Shop struct{}\n\nfunc (s Shop) Close(door Door, window Window) {\n\tdoor.Lock()\n\twindow.Lock()\n}\n";
    let analysis = analyze(&[("shop.go", source)]);
    let shop = class(&analysis, "Shop");
    assert_eq!(shop.rfc, 3);
}

#[test]
fn test_csharp_interface_is_not_a_parent_class() {
    let source = "interface IShape\n{\n    double Area();\n}\n\nclass Circle : IShape\n{\n    public double Area() { return 1; }\n}\n\nclass Ring : Circle\n{\n}\n";
    let analysis = analyze(&[("Shapes.cs", source)]);
    assert_eq!(class(&analysis, "Circle").parent_class, None);
    assert_eq!(class(&analysis, "Circle").dit, 0);
    assert_eq!(
        class(&analysis, "Ring").parent_class.as_deref(),
        Some("Circle")
    );
    assert_eq!(class(&analysis, "Ring").dit, 1);
    assert_eq!(class(&analysis, "Circle").noc, 1);
}

#[test]
fn test_record_is_a_class() {
    let java = analyze(&[(
        "Point.java",
        "record Point(int x, int y) {\n    int sum() { return x + y; }\n}\n",
    )]);
    assert_eq!(class(&java, "Point").nom, 1);
    let csharp = analyze(&[("Size.cs", "record Size(double Width, double Height)\n{\n    public double Total() { return Width * Height; }\n}\n")]);
    assert_eq!(class(&csharp, "Size").nom, 1);
}

#[test]
fn test_csharp_struct_is_measured() {
    let source =
        "public struct Meter\n{\n    public int Twice(int value) { return value * 2; }\n}\n";
    let analysis = analyze(&[("Meter.cs", source)]);
    let meter = class(&analysis, "Meter");
    assert_eq!(meter.nom, 1);
    assert_eq!(meter.wmc, 1);
}

#[test]
fn test_java_enum_with_methods_is_measured() {
    let source = "enum Level {\n    LOW, HIGH;\n\n    private int weight;\n\n    int weight() { return weight; }\n\n    boolean isHigh() { return this == HIGH; }\n}\n";
    let analysis = analyze(&[("Level.java", source)]);
    let level = class(&analysis, "Level");
    assert_eq!(level.nom, 2);
    assert_eq!(level.nof, 1);
}

#[test]
fn test_rust_enum_and_trait_are_measured() {
    let source = "enum Mode {\n    Fast,\n    Slow,\n}\n\nimpl Mode {\n    fn rank(&self) -> u32 {\n        1\n    }\n}\n\ntrait Shape {\n    fn area(&self) -> u32;\n\n    fn double(&self) -> u32 {\n        self.area() * 2\n    }\n}\n\nstruct Square;\n\nimpl Shape for Square {\n    fn area(&self) -> u32 {\n        4\n    }\n}\n";
    let analysis = analyze(&[("lib.rs", source)]);
    assert_eq!(class(&analysis, "Mode").nom, 1);
    assert_eq!(class(&analysis, "Shape").nom, 2);
    assert_eq!(class(&analysis, "Square").nom, 1);
}

#[test]
fn test_php_enum_with_methods_is_measured() {
    let source = "<?php\nenum Signal {\n    case Stop;\n\n    public function code(): int {\n        return 0;\n    }\n}\n";
    let analysis = analyze(&[("Signal.php", source)]);
    let signal = class(&analysis, "Signal");
    assert_eq!(signal.nom, 1);
    assert_eq!(signal.wmc, 1);
}

#[test]
fn test_php_trait_is_measured() {
    let source = "<?php\ntrait Logs {\n    private $lines = [];\n\n    function add($line) { $this->lines[] = $line; }\n\n    function all() { return $this->lines; }\n}\n";
    let analysis = analyze(&[("Logs.php", source)]);
    let logs = class(&analysis, "Logs");
    assert_eq!(logs.nom, 2);
    assert_eq!(logs.lcom, 1);
}

#[test]
fn test_abstract_typescript_class_is_a_class() {
    let source = "abstract class Base {\n  abstract area(): number;\n  describe() {\n    return 'x';\n  }\n}\n";
    let analysis = analyze(&[("base.ts", source)]);
    assert_eq!(class(&analysis, "Base").nom, 2);
}

#[test]
fn test_class_does_not_couple_to_itself() {
    let java = "class Node {\n    Node next;\n    Node link(Node other) {\n        return other;\n    }\n}\n";
    let analysis = analyze(&[("Node.java", java)]);
    assert!(class(&analysis, "Node").coupled_classes.is_empty());
}

#[test]
fn test_ruby_superclass_has_no_operator_text() {
    let analysis = analyze(&[("a.rb", "class Base\nend\n\nclass Child < Base\nend\n")]);
    assert_eq!(
        class(&analysis, "Child").parent_class.as_deref(),
        Some("Base")
    );
    assert_eq!(class(&analysis, "Child").dit, 1);
    assert_eq!(class(&analysis, "Base").noc, 1);
}

#[test]
fn test_cpp_declaration_and_definition_are_one_method() {
    let header = "class Counter {\npublic:\n    void tick(int n);\n    void tick();\n};\n";
    let source = "#include \"counter.h\"\n\nvoid Counter::tick(int n) {\n    if (n > 0) {}\n}\n";
    let analysis = analyze(&[("counter.h", header), ("counter.cpp", source)]);
    let counter = class(&analysis, "Counter");
    assert_eq!(counter.nom, 2, "{:?}", counter.methods);
    assert_eq!(counter.wmc, 3);
}

#[test]
fn test_rust_impl_in_a_module_belongs_to_the_struct_of_that_module() {
    let source = "mod inner {\n    pub struct Part {\n        weight: u32,\n    }\n\n    impl Part {\n        pub fn weigh(&self) -> u32 {\n            self.weight\n        }\n    }\n}\n";
    let analysis = analyze(&[("lib.rs", source)]);
    let part = class(&analysis, "Part");
    assert_eq!(part.nom, 1);
    assert_eq!(part.nof, 1);
}

#[test]
fn test_methods_that_call_each_other_are_one_component() {
    let cases: &[(&str, &str)] = &[
        (
            "shop.py",
            "class Shop:\n    def open(self):\n        self.prepare()\n\n    def prepare(self):\n        self.door = 1\n\n    def close(self):\n        self.window = 1\n",
        ),
        (
            "Shop.java",
            "class Shop {\n    int door;\n    int window;\n\n    void open() {\n        prepare();\n    }\n\n    void prepare() {\n        door = 1;\n    }\n\n    void close() {\n        window = 1;\n    }\n}\n",
        ),
        (
            "shop.rs",
            "struct Shop {\n    door: u32,\n    window: u32,\n}\n\nimpl Shop {\n    fn open(&self) {\n        self.prepare();\n    }\n\n    fn prepare(&self) -> u32 {\n        self.door\n    }\n\n    fn close(&self) -> u32 {\n        self.window\n    }\n}\n",
        ),
        (
            "shop.ts",
            "class Shop {\n  door = 0;\n  window = 0;\n\n  open() {\n    this.prepare();\n  }\n\n  prepare() {\n    return this.door;\n  }\n\n  close() {\n    return this.window;\n  }\n}\n",
        ),
        (
            "shop.go",
            "package shop\n\ntype Shop struct {\n\tdoor   int\n\twindow int\n}\n\nfunc (s *Shop) Open() {\n\ts.Prepare()\n}\n\nfunc (s *Shop) Prepare() int {\n\treturn s.door\n}\n\nfunc (s *Shop) Close() int {\n\treturn s.window\n}\n",
        ),
        (
            "shop.rb",
            "class Shop\n  def open\n    self.prepare\n  end\n\n  def prepare\n    @door\n  end\n\n  def close\n    @window\n  end\nend\n",
        ),
    ];
    for (file, source) in cases {
        let analysis = analyze(&[(file, source)]);
        let shop = class(&analysis, "Shop");
        assert_eq!(shop.nom, 3, "{file}");
        assert_eq!(shop.lcom, 2, "{file}");
    }
}

#[test]
fn test_function_value_in_a_class_body_is_not_a_method() {
    let python = analyze(&[(
        "a.py",
        "class A:\n    key = lambda value: value\n\n    def run(self):\n        pass\n",
    )]);
    assert_eq!(class(&python, "A").nom, 1);
    let typescript = analyze(&[(
        "a.ts",
        "class A {\n  handler = () => 1;\n\n  run() {\n    return 2;\n  }\n}\n",
    )]);
    let found = class(&typescript, "A");
    assert_eq!(found.nom, 1);
    assert_eq!(found.fields, ["handler"]);
    let ruby = analyze(&[(
        "a.rb",
        "class A\n  parse = lambda do |value|\n    value\n  end\n\n  def run\n  end\nend\n",
    )]);
    assert_eq!(class(&ruby, "A").nom, 1);
}

#[test]
fn test_typescript_overload_signatures_are_one_method() {
    let source = "class Maker {\n  static create(a: string): Maker;\n  static create(a: number, b: number): Maker;\n  static create(a: any, b?: any): Maker {\n    return new Maker();\n  }\n}\n";
    let analysis = analyze(&[("maker.ts", source)]);
    assert_eq!(class(&analysis, "Maker").nom, 1);
}

#[test]
fn test_tuple_struct_field_connects_methods() {
    let source = "struct Wrapper(u32);\n\nimpl Wrapper {\n    fn get(&self) -> u32 {\n        self.0\n    }\n\n    fn double(&self) -> u32 {\n        self.0 * 2\n    }\n}\n";
    let analysis = analyze(&[("wrapper.rs", source)]);
    let wrapper = class(&analysis, "Wrapper");
    assert_eq!(wrapper.nom, 2);
    assert_eq!(wrapper.lcom, 1);
}

#[test]
fn test_analyzer_does_not_parse_source() {
    let source = include_str!("../src/analyzers/cohesion.rs");
    let code = source.split("#[cfg(test)]").next().unwrap();
    for forbidden in ["tree_sitter", "Parser", "extract_classes"] {
        assert!(
            !code.contains(forbidden),
            "cohesion.rs mentions {forbidden}"
        );
    }
}

#[test]
fn test_csharp_record_primary_base_is_the_parent() {
    let source = "record Root(int X);\nrecord Child(int X) : Root(X);\n";
    let analysis = analyze(&[("Records.cs", source)]);
    let child = class(&analysis, "Child");
    assert_eq!(child.parent_class.as_deref(), Some("Root"));
    assert_eq!(child.dit, 1);
    assert_eq!(class(&analysis, "Root").noc, 1);
}

#[test]
fn test_go_type_parameter_that_shadows_a_package_type_is_not_coupling() {
    let source = "package p\n\ntype T struct{}\n\ntype Box[T any] struct {\n\tvalue T\n}\n";
    let analysis = analyze(&[("box.go", source)]);
    assert_eq!(class(&analysis, "Box").cbo, 0);
}

#[test]
fn test_go_receiver_type_parameters_are_in_scope_in_the_method() {
    let source = "package p\n\ntype T struct{}\n\ntype Box[T any] struct {\n\tvalue T\n}\n\nfunc (b Box[T]) Get() T {\n\treturn b.value\n}\n\nfunc (b *Box[T]) Put(next T) {\n\tb.value = next\n}\n";
    let analysis = analyze(&[("box.go", source)]);
    assert_eq!(class(&analysis, "Box").cbo, 0);
}

#[test]
fn test_type_parameter_that_shadows_a_class_is_not_coupling() {
    let cases = [
        (
            "Box.java",
            "class Item {}\nclass Box<Item> {\n    Item value;\n}\n",
        ),
        (
            "Box.cs",
            "class Item {}\nclass Box<Item> {\n    Item value;\n}\n",
        ),
        (
            "box.ts",
            "class Item {}\nclass Box<Item> {\n    value: Item;\n}\n",
        ),
        (
            "box.rs",
            "struct Item;\nstruct Box<Item> {\n    value: Item,\n}\n",
        ),
        (
            "box.cpp",
            "class Item {};\ntemplate <typename Item>\nclass Box {\n    Item value;\n};\n",
        ),
    ];
    for (file, source) in cases {
        let analysis = analyze(&[(file, source)]);
        assert_eq!(class(&analysis, "Box").cbo, 0, "{file}");
    }
}

#[test]
fn test_csharp_target_typed_new_counts_as_construction() {
    let source = "class Part {}\n\nclass Factory {\n    Part Make() { return new(); }\n}\n";
    let analysis = analyze(&[("Factory.cs", source)]);
    assert_eq!(class(&analysis, "Factory").rfc, 2);
}

#[test]
fn test_csharp_target_typed_new_reads_the_declared_type() {
    let declared = [
        "Part Make() => new();",
        "void Make() { Part part = new(); }",
        "Part? Make() { return new(); }",
    ];
    for member in declared {
        let source = format!("class Part {{}}\n\nclass Factory {{\n    {member}\n}}\n");
        let analysis = analyze(&[("Factory.cs", &source)]);
        assert_eq!(class(&analysis, "Factory").rfc, 2, "{member}");
    }
    let abstained = [
        "async Task<Part> Make() { return new(); }",
        "Part Make(Part other) { return other ?? new(); }",
    ];
    for member in abstained {
        let source = format!("class Part {{}}\n\nclass Factory {{\n    {member}\n}}\n");
        let analysis = analyze(&[("Factory.cs", &source)]);
        assert_eq!(class(&analysis, "Factory").rfc, 1, "{member}");
    }
}
