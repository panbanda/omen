use axum::{routing::get, Router};
use std::process::Command;

async fn run_command() -> String {
    prepare("x")
}

fn prepare(value: &str) -> String {
    execute(value)
}

fn execute(value: &str) -> String {
    let _ = Command::new(value).output();
    String::new()
}

fn orphan(value: &str) {
    let _ = Command::new(value).output();
}

fn routes() -> Router {
    Router::new().route("/run", get(run_command))
}
