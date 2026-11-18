mod standard {
    use std::process::*;

    pub fn run(program: &str) {
        Command::new(program);
    }
}

mod tokio_glob {
    use tokio::process::*;

    pub fn run(program: &str) {
        Command::new(program);
    }
}

mod other_crate {
    use remote_storage::*;

    pub fn run(program: &str) {
        std::process::Command::new(program);
        tokio::process::Command::new(program);
    }
}
