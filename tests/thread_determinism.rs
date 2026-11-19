//! Each analyzer whose work runs in parallel gives the same JSON for each
//! number of workers. A sum of floating-point numbers depends on the order of
//! its terms, so an order that the threads decide changes the last digits.

use std::path::Path;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

/// The number of TypeScript modules. The dependency graph must be large
/// enough that the parallel work is split between the workers.
const MODULES: usize = 600;

fn git(directory: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .env("GIT_AUTHOR_DATE", "2024-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2024-01-01T00:00:00Z")
        .output()
        .expect("git runs")
        .status;
    assert!(status.success(), "git {arguments:?}");
}

/// A pseudo-random number from a linear congruential generator.
fn next(seed: &mut u64) -> usize {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*seed >> 33) as usize
}

/// A repository of TypeScript modules that import each other, and Python,
/// Go, and Rust files, in three commits.
fn repository() -> TempDir {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    let write = |path: &str, text: &str| {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    let mut seed = 11;
    for module in 0..MODULES {
        let mut text = String::new();
        let mut body = String::new();
        for _ in 0..3 {
            let target = next(&mut seed) % MODULES;
            if target != module {
                text.push_str(&format!("import {{ f{target} }} from \"./m{target}\";\n"));
                body.push_str(&format!("  value += f{target}(value);\n"));
            }
        }
        // Statements of different shapes in a pseudo-random order, so that
        // the modules are not clones of each other.
        for _ in 0..2 + next(&mut seed) % 4 {
            body.push_str(match next(&mut seed) % 8 {
                0 => "  if (value > 3) { value -= 1; }\n",
                1 => "  for (let step = 0; step < 2; step++) { value *= 2; }\n",
                2 => "  while (value > 100) { value = value / 2; }\n",
                3 => "  const items = [value, value + 1];\n  value = items.length;\n",
                4 => "  switch (value) {\n    case 1:\n      value = 0;\n      break;\n    default:\n      value += 4;\n  }\n",
                5 => "  try {\n    value = JSON.parse(String(value));\n  } catch {\n    value = -1;\n  }\n",
                6 => "  value = value > 0 ? value : -value;\n",
                _ => "  // TODO: check the bounds\n",
            });
        }
        text.push_str(&format!(
            "\nexport function f{module}(value: number): number {{\n{body}  return value;\n}}\n"
        ));
        write(&format!("src/m{module}.ts"), &text);
    }
    write(
        "app/views.py",
        "import subprocess\n\n\ndef run(command):\n    if command:\n        return subprocess.run(command, shell=True)\n    return None\n\n\nclass Store:\n    def __init__(self):\n        self.items = []\n\n    def add(self, item):\n        self.items.append(item)\n\n    def size(self):\n        return len(self.items)\n",
    );
    write(
        "app/copy.py",
        "class Store:\n    def __init__(self):\n        self.items = []\n\n    def add(self, item):\n        self.items.append(item)\n\n    def size(self):\n        return len(self.items)\n",
    );
    write("go.mod", "module example.com/shop\n\ngo 1.22\n");
    write(
        "shop/shop.go",
        "package shop\n\nimport \"os/exec\"\n\ntype Cart struct{ items []string }\n\nfunc (c *Cart) Add(item string) {\n\tif item != \"\" {\n\t\tc.items = append(c.items, item)\n\t}\n}\n\nfunc Run(name string) error {\n\treturn exec.Command(name).Run()\n}\n",
    );
    write(
        "src/lib.rs",
        "pub struct Counter {\n    value: u32,\n}\n\nimpl Counter {\n    pub fn add(&mut self, step: u32) {\n        if step > 0 {\n            self.value += step;\n        }\n    }\n}\n\nfn unused() {}\n",
    );
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "initial"]);
    for (round, step) in [(1, 7), (2, 13)] {
        for module in (0..MODULES).step_by(step) {
            let path = root.join(format!("src/m{module}.ts"));
            let mut text = std::fs::read_to_string(&path).unwrap();
            text.push_str(&format!("// change {round}\n"));
            std::fs::write(path, text).unwrap();
        }
        git(
            root,
            &["commit", "-q", "-a", "-m", &format!("change {round}")],
        );
    }
    directory
}

/// Removes the values that change from run to run by design: times, and the
/// command line that names the number of workers.
fn without_run_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| {
                let key = key.to_ascii_lowercase();
                !(key.contains("time")
                    || key.contains("duration")
                    || key.contains("elapsed")
                    || key.contains("generated")
                    || key.ends_with("_at")
                    || key.ends_with("_ms")
                    || key == "command")
            });
            map.values_mut().for_each(without_run_fields);
        }
        Value::Array(items) => items.iter_mut().for_each(without_run_fields),
        _ => {}
    }
}

fn report(root: &Path, jobs: &str, analyzer: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_omen"))
        .args(["-p", root.to_str().unwrap(), "-f", "json", "-j", jobs])
        .args(analyzer)
        .output()
        .expect("omen runs");
    assert!(
        output.status.success(),
        "{analyzer:?} -j {jobs}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{analyzer:?} -j {jobs}: {error}"));
    without_run_fields(&mut value);
    value
}

#[test]
fn test_each_parallel_analyzer_gives_the_same_json_for_one_and_four_workers() {
    let directory = repository();
    let analyzers: [&[&str]; 12] = [
        &["graph"],
        &["smells"],
        &["repomap"],
        &["tdg"],
        &["hotspot"],
        &["defect"],
        &["score"],
        &["cohesion"],
        &["complexity"],
        &["deadcode"],
        &["security"],
        &["clones"],
    ];
    let mut different = Vec::new();
    for analyzer in analyzers {
        let one = report(directory.path(), "1", analyzer);
        let four = report(directory.path(), "4", analyzer);
        if one != four {
            different.push(analyzer[0]);
        }
    }
    assert!(different.is_empty(), "different JSON: {different:?}");
}
