//! `--ref` on a local repository: each command that reads source reads the
//! files of that commit, and each command that cannot do so gives an error.

use assert_cmd::Command;
use tempfile::TempDir;

fn omen() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("omen").expect("binary exists")
}

fn isolated_git(repository: &std::path::Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("git starts");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

const OLD_SOURCE: &str = "\
# TODO: remove the old report
import cycle_b


class OldReport:
    def __init__(self):
        self.total = 0

    def add(self, value):
        if value > 0:
            self.total += value
        return self.total

    def reset(self):
        self.total = 0


def old_only(value):
    if value > 1:
        return cycle_b.cycle_b_entry(value)
    return not_ready()


def not_ready():
    raise NotImplementedError


def never_called():
    return 1
";

const CYCLE_B: &str =
    "import old_only\n\n\ndef cycle_b_entry(value):\n    return old_only.old_only(value)\n";

struct Repository {
    directory: TempDir,
    first: String,
}

impl Repository {
    fn root(&self) -> &str {
        self.directory.path().to_str().expect("utf-8 path")
    }

    /// The first commit holds `old_only.py` and `cycle_b.py`. The second
    /// commit removes both files and adds `new_only.py`.
    fn new() -> Self {
        let directory = TempDir::new().expect("temporary directory");
        let git = |arguments: &[&str]| isolated_git(directory.path(), arguments);
        git(&["init", "-q"]);
        std::fs::write(directory.path().join("old_only.py"), OLD_SOURCE).unwrap();
        std::fs::write(directory.path().join("cycle_b.py"), CYCLE_B).unwrap();
        std::fs::write(
            directory.path().join("old_only.js"),
            "function oldScript() {\n  return 1;\n}\n",
        )
        .unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "first"]);
        let first = git(&["rev-parse", "HEAD"]);
        git(&["rm", "-q", "old_only.py", "cycle_b.py", "old_only.js"]);
        std::fs::write(
            directory.path().join("new_only.py"),
            "def new_only():\n    return 2\n",
        )
        .unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "second"]);
        Self { directory, first }
    }

    fn run(&self, arguments: &[&str]) -> std::process::Output {
        omen()
            .args(["-p", self.root(), "--ref", &self.first, "-f", "json"])
            .args(arguments)
            .output()
            .expect("omen starts")
    }
}

fn stdout_of(output: &std::process::Output) -> String {
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn test_repomap_reads_the_commit_of_ref() {
    let repository = Repository::new();
    let text = stdout_of(&repository.run(&["repomap"]));
    assert!(text.contains("old_only.py#old_only"), "{text}");
    assert!(!text.contains("new_only"), "{text}");
}

#[test]
fn test_context_reads_the_commit_of_ref() {
    let repository = Repository::new();
    let text = stdout_of(&repository.run(&["context"]));
    assert!(text.contains("old_only.py"), "{text}");
    assert!(!text.contains("new_only"), "{text}");
}

#[test]
fn test_graph_reads_the_commit_of_ref() {
    let repository = Repository::new();
    let text = stdout_of(&repository.run(&["graph"]));
    assert!(text.contains("old_only.py"), "{text}");
    assert!(text.contains("cycle_b.py"), "{text}");
    assert!(!text.contains("new_only"), "{text}");
}

#[test]
fn test_smells_reads_the_commit_of_ref() {
    let repository = Repository::new();
    let text = stdout_of(&repository.run(&["smells"]));
    assert!(text.contains("old_only.py"), "{text}");
    assert!(text.contains("cycle_b.py"), "{text}");
}

#[test]
fn test_each_file_reading_command_reads_the_commit_of_ref() {
    let repository = Repository::new();
    for command in ["outline", "cohesion", "complexity", "satd", "stubs"] {
        let text = stdout_of(&repository.run(&[command]));
        assert!(text.contains("old_only.py"), "{command}: {text}");
        assert!(!text.contains("new_only"), "{command}: {text}");
    }
}

#[test]
fn test_deadcode_reads_the_commit_of_ref() {
    let repository = Repository::new();
    let text = stdout_of(&repository.run(&["deadcode"]));
    assert!(text.contains("old_only.js"), "{text}");
    assert!(!text.contains("new_only"), "{text}");
}

#[test]
fn test_outline_of_one_file_reads_the_commit_of_ref() {
    let repository = Repository::new();
    let text = stdout_of(&repository.run(&["outline", "--file", "old_only.py"]));
    assert!(text.contains("old_only"), "{text}");
    let output = repository.run(&["outline", "--file", "new_only.py"]);
    assert!(!output.status.success(), "{output:?}");
}

#[test]
fn test_clones_reads_the_commit_of_ref() {
    let directory = TempDir::new().unwrap();
    let git = |arguments: &[&str]| isolated_git(directory.path(), arguments);
    git(&["init", "-q"]);
    let body: String = (0..30)
        .map(|index| format!("    value_{index} = compute(value, {index}) + other({index})\n"))
        .collect();
    for name in ["twin_a.py", "twin_b.py"] {
        std::fs::write(
            directory.path().join(name),
            format!("def run(value):\n{body}    return value\n"),
        )
        .unwrap();
    }
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "first"]);
    let first = git(&["rev-parse", "HEAD"]);
    git(&["rm", "-q", "twin_a.py", "twin_b.py"]);
    std::fs::write(directory.path().join("other.py"), "x = 1\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "second"]);
    let output = omen()
        .args(["-p", directory.path().to_str().unwrap(), "--ref", &first])
        .args(["-f", "json", "clones"])
        .output()
        .unwrap();
    let text = stdout_of(&output);
    assert!(text.contains("twin_a.py"), "{text}");
}

#[test]
fn test_flags_reads_the_commit_of_ref() {
    let directory = TempDir::new().unwrap();
    let git = |arguments: &[&str]| isolated_git(directory.path(), arguments);
    git(&["init", "-q"]);
    std::fs::write(
        directory.path().join("old_flags.js"),
        "function run(client, user) {\n  return client.variation(\"old-flag\", user, false);\n}\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "first"]);
    let first = git(&["rev-parse", "HEAD"]);
    git(&["rm", "-q", "old_flags.js"]);
    std::fs::write(directory.path().join("other.py"), "x = 1\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "second"]);
    let output = omen()
        .args(["-p", directory.path().to_str().unwrap(), "--ref", &first])
        .args(["-f", "json", "flags", "--provider", "launchdarkly"])
        .output()
        .unwrap();
    let text = stdout_of(&output);
    assert!(text.contains("old-flag"), "{text}");
}

#[test]
fn test_a_command_that_cannot_read_a_commit_rejects_ref() {
    let repository = Repository::new();
    for arguments in [
        vec!["churn"],
        vec!["hotspot"],
        vec!["defect"],
        vec!["temporal"],
        vec!["ownership"],
        vec!["changes"],
        vec!["tdg"],
        vec!["score"],
        vec!["all"],
        vec!["diff"],
        vec!["search", "index"],
        vec!["search", "query", "report"],
    ] {
        let output = repository.run(&arguments);
        assert!(!output.status.success(), "{arguments:?}: {output:?}");
        let message = String::from_utf8_lossy(&output.stderr);
        assert!(message.contains("--ref"), "{arguments:?}: {message}");
    }
}

#[test]
fn test_a_reference_that_does_not_exist_is_an_error() {
    let repository = Repository::new();
    let output = omen()
        .args([
            "-p",
            repository.root(),
            "--ref",
            "no-such-ref",
            "-f",
            "json",
        ])
        .arg("repomap")
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
}

#[test]
fn test_security_reads_the_commit_of_ref() {
    let directory = TempDir::new().unwrap();
    let git = |arguments: &[&str]| isolated_git(directory.path(), arguments);
    git(&["init", "-q"]);
    std::fs::write(
        directory.path().join("old_sink.py"),
        "import os\n\n\ndef run(command):\n    os.system(command)\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "first"]);
    let first = git(&["rev-parse", "HEAD"]);
    git(&["rm", "-q", "old_sink.py"]);
    std::fs::write(
        directory.path().join("new_only.py"),
        "def new_only():\n    return 2\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "second"]);
    let run = |reference: Option<&str>| {
        let mut command = omen();
        command.args(["-p", directory.path().to_str().unwrap(), "-f", "json"]);
        if let Some(reference) = reference {
            command.args(["--ref", reference]);
        }
        command.args(["security", "sinks"]).output().unwrap()
    };
    let old = stdout_of(&run(Some(&first)));
    assert!(old.contains("old_sink.py"), "{old}");
    let current = stdout_of(&run(None));
    assert!(!current.contains("old_sink.py"), "{current}");
}
