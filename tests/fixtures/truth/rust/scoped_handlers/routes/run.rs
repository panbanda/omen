use std::process::Command;

pub async fn run() -> String {
    execute("x")
}

fn execute(value: &str) -> String {
    let _ = Command::new(value).output();
    String::new()
}
