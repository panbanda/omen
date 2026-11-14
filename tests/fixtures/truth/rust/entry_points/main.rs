use axum::{routing::get, Router};

async fn list() {}

pub fn app() -> Router {
    Router::new().route("/items/{id}", get(list))
}

#[get("/ping")]
async fn ping() {}

#[no_mangle]
pub extern "C" fn exported() {}

fn main() {}
