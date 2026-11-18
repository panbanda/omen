use axum::{routing::post, Router};
use jobs_runtime::cleanup;
use std::process::Command;

fn audit() {
    let _ = Command::new(std::env::var("EVENT").unwrap()).output();
}

fn make_task() -> fn() {
    || ()
}

async fn create_job() -> String {
    fn load_module() {
        let _ = Command::new(std::env::var("JOB").unwrap()).output();
    }
    tokio::task::spawn_blocking(load_module);
    tokio::task::spawn_blocking(audit);
    String::new()
}

async fn shadowed() -> String {
    let audit = make_task();
    tokio::task::spawn_blocking(audit);
    String::new()
}

async fn other_file() -> String {
    tokio::task::spawn_blocking(cleanup);
    String::new()
}

async fn text_only() -> String {
    // tokio::task::spawn_blocking(audit);
    String::from("audit")
}

fn routes() -> Router {
    let jobs = Router::new().route("/jobs", post(create_job));
    let shadow = Router::new().route("/shadow", post(shadowed));
    let other = Router::new().route("/other", post(other_file));
    let text = Router::new().route("/text", post(text_only));
    jobs.merge(shadow).merge(other).merge(text)
}
