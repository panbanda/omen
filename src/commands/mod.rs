//! The command handlers. Each handler takes a typed request and writes its
//! report to a writer, so a test can read the report and the CLI can send it
//! to standard output.

pub mod benchmark;
pub mod context;
pub mod eval;
pub mod impact;
pub mod outline;
pub mod scope;
pub mod symbol;

#[cfg(test)]
pub(crate) mod testing {
    use std::path::PathBuf;

    use tempfile::TempDir;

    /// A repository directory with two source files.
    pub fn repository() -> (TempDir, PathBuf) {
        let directory = TempDir::new().expect("temporary directory");
        std::fs::write(
            directory.path().join("shop.py"),
            "def helper():\n    return 1\n\n\ndef run():\n    return helper()\n",
        )
        .expect("write shop.py");
        std::fs::write(
            directory.path().join("util.py"),
            "def other():\n    return 2\n",
        )
        .expect("write util.py");
        let path = directory.path().to_path_buf();
        (directory, path)
    }
}
