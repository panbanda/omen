//! The command handlers. Each handler takes a typed request and writes its
//! report to a writer, so a test can read the report and the CLI can send it
//! to standard output.
//!
//! A gate verdict and a progress message go to stderr, not to the writer.

/// Writes one line to a writer and returns the error of the write.
macro_rules! say {
    ($out:expr) => {
        writeln!($out)?
    };
    ($out:expr, $($arg:tt)*) => {
        writeln!($out, $($arg)*)?
    };
}

pub mod all;
pub mod analyzers;
pub mod benchmark;
pub mod complexity;
pub mod context;
pub mod diff;
pub mod eval;
pub mod gate;
pub mod impact;
#[cfg(feature = "mutation")]
pub mod mutation;
pub mod outline;
pub mod report;
pub mod scope;
pub mod score;
pub mod search;
pub mod stubs;
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
