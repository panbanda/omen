# Test code

Omen has one rule that tells if a definition is test code. The rule is in
`src/index/test_code.rs`. The symbol extraction applies it in the same walk
that finds the symbols. Each analyzer reads the result from the shared index.

## The two questions

The rule answers two questions about a definition:

- Is it test code? Test code is code that only a test build or a test run uses.
- Is it a test entry? A test entry is a definition that a test tool calls: a
  test, a benchmark, a fixture, or a hook.

A symbol has one of three roles:

| Role | Meaning |
|---|---|
| `entry` | A test tool calls the definition. |
| `support` | The definition is in test code, and no test tool calls it by its syntax. |
| none | The definition is not test code. |

In the output, `is_test` is `true` for the roles `entry` and `support`.

## The rule for each language

The path of the file is the first source. A file is test code when its path
follows a test convention: a directory `test`, `tests`, `spec`, `specs`,
`__tests__`, `__mocks__`, `test_helpers`, `testdata`, or `fixtures`, or a file
name such as `test_a.py`, `a_test.go`, `a_spec.rb`, `a.test.ts`, or `a.spec.ts`.
For Rust, the directory `benches` is in the list. For Python, the file
`conftest.py` is in the list.

The syntax is the second source. It applies to each file, also to a file whose
path follows no test convention.

| Language | Test code by syntax | Test entry |
|---|---|---|
| Rust | An item with `#[cfg(test)]` or `#[cfg(all(test, ...))]`. A file or a module with `#![cfg(test)]`. A module file that `#[cfg(test)] mod name;` declares, and each module file below it. | A function with an attribute whose last path segment is `test`, such as `#[test]`, `#[tokio::test]`, and `#[gpui::test]`. A function with `#[bench]`, `#[rstest]`, `#[test_case]`, `#[test_matrix]`, `#[quickcheck]`, `#[proptest]`, `#[wasm_bindgen_test]`, or `#[parameterized]`. A function with an attribute whose arguments name a path that ends in `::test`, such as `#[testlog(tokio::test)]`. In test code, a function with an attribute macro that omen does not know. |
| Python | The body of a class whose superclass name ends in `TestCase`. | A function with `@pytest.fixture`, `@pytest_asyncio.fixture`, `@fixture`, or `@pytest.mark.*`. A class whose superclass name ends in `TestCase`, and its `test*` methods and hook methods such as `setUp`. In a test file: a `test*` function, a `Test*` class, a hook such as `setup_method`, and a `pytest_*` function. |
| JavaScript, JSX, TypeScript, TSX | A call of `describe`, `it`, `test`, `fdescribe`, `fit`, `xdescribe`, `xit`, or `xtest` that has a title and a callback. The forms `.only`, `.skip`, and `.each` are included. A call of `beforeEach`, `afterEach`, `beforeAll`, or `afterAll` that has a callback. | A callback of these calls, when it has a name. |
| Go | No syntax. | In a file whose name ends in `_test.go`: a function `Test`, `Benchmark`, `Fuzz`, or `Example`, with no lowercase letter after the prefix. |
| Java | The class of a method that is a test entry. | A method with an annotation of JUnit 4, JUnit 5, or TestNG, such as `@Test`, `@ParameterizedTest`, `@BeforeEach`, and `@DataProvider`. In a test file: a method whose name starts with `test` (JUnit 3). |
| C# | The class of a method that is a test entry. | A method with an attribute of NUnit, xUnit, or MSTest, such as `[Test]`, `[Fact]`, `[Theory]`, and `[TestMethod]`. A class with `[TestFixture]` or `[TestClass]`. |
| Ruby | The body of a class whose superclass is `Minitest::Test`, `Minitest::Spec`, or a name that ends in `TestCase` or `IntegrationTest`. A block of `describe`, `it`, `specify`, `shared_examples`, `shared_examples_for`, or `shared_context`, also with the receiver `RSpec`. | Such a class, and its `test_*`, `setup`, and `teardown` methods. In a test file: a `test_*` method. |
| PHP | The body of a class that extends a class whose name ends in `TestCase`. The class of a method that is a test entry. | Such a class, and its `test*` methods and hook methods such as `setUp`. A method with `#[Test]` or a related PHPUnit attribute. A method whose documentation comment has `@test`, `@before`, `@after`, or `@dataProvider`. |
| C, C++ | The body of a use of a test macro: `TEST`, `TEST_F`, `TEST_P`, `TYPED_TEST`, `TYPED_TEST_P`, `TEST_CASE`, `TEST_CASE_METHOD`, `SCENARIO`, `BOOST_AUTO_TEST_CASE`, `BOOST_FIXTURE_TEST_CASE`, `Test`, and `ParameterizedTest`. | The function that the macro defines, when the grammar gives a function. In a C test file: a function whose name starts with `test` (Unity). |
| Bash | No syntax. A Bats file has the extension `.bats`, and omen does not read it. | In a test file: a function whose name starts with `test` (shUnit2). |

These conditions are not test code:

- A name that starts with `test` or `Test` in a file that is not a test file.
- A Rust item with `#[cfg(not(test))]` or `#[cfg(any(test, feature = "x"))]`.
  The second condition is true in a build that is not a test build.
- A Ruby `context` block that is not in a `describe` block. Other libraries
  use the name `context`.
- An ECMAScript call such as `it.next()` or `hooks.beforeEach(run)`.

## The consumers

| Consumer | Use |
|---|---|
| `deadcode` | A test entry is not dead code, and each definition that it reaches is used. Each item has `is_test`. |
| `security` | A sink in test code, and an entry point in test code, has `in_test: true`. |
| `diff` | A caller that is test code has `is_test: true`, and the column `Tests` counts these callers. |
| `impact`, `symbol` | Each caller and each callee has `is_test`. |
| `task_context` | A symbol that is test code has the role hint `test_candidate`, with the basis `test_syntax_or_path`. |
| `tdg`, `clones` | The critical defects and the clones of Rust test code are not in the result. |

## Dead code

`deadcode` does not report these definitions, because code that omen cannot
read calls them:

- A test entry.
- A Python method with a special name such as `__eq__`, and a method with a
  name such as `_missing_`, which the `enum` module calls.
- A definition in a Python stub file (`.pyi`). A stub has no code to remove.
- A Rust function with `#[ctor]`, `#[dtor]`, `#[no_mangle]`,
  `#[unsafe(no_mangle)]`, `#[export_name]`, a `proc_macro` attribute,
  `#[panic_handler]`, `#[wasm_bindgen]`, `#[pyfunction]`, `#[napi]`, or an
  attribute whose last path segment is `main`.
- A Rust function with a foreign ABI, such as `extern "C" fn`.
- A Rust function for which the author permits the lint: `#[allow(dead_code)]`,
  `#[expect(dead_code)]`, or the lint group `unused`. The attribute can be on
  the function, on its module, on its `impl` block, or an inner attribute of
  the file. The compiler does not report such a function, and omen agrees.

Each of these definitions is a root of the reachability. Thus a function that
only a `#[ctor]` function calls is used.

Other rules for Rust:

- A private function is visible in its module and in each module below it. A
  call in a child module file can thus name it.
- A call without a receiver and without a path cannot call a method. When the
  index gives only a method for such a call, a function with the same name can
  be the callee.
- The string of `#[serde(default = "path")]` and of
  `#[schemars(schema_with = "path")]` names a function.

A Ruby symbol such as `:authenticate` names the method `authenticate`. Thus
`before_action :authenticate` and `send(:authenticate)` use the method.

The items are in the order of `confidence`, the highest first. The file and
the line break a tie. A definition in test code that nothing reaches has a
lower confidence than a definition in production code.

## The truth fixtures

Each language has a case `tests/fixtures/truth/<language>/test_code/`. Its
`labels.json` has a `tests` list. Each label gives the role of one definition
or of one line. `cargo test --test truth` compares the labels with the index.
`tests/fixtures/truth/README.md` describes the label format.

## Limits

- Omen does not run a test tool. A project that configures a different test
  directory or a different name pattern has tests that omen does not find by
  the path.
- A PHPUnit data provider that a string names, as `#[DataProvider('rows')]`
  does, is not a test entry.
- A Rust attribute macro that makes a test, and that omen does not know, is a
  test entry only in test code.
