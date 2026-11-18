use axum::{routing::get, Router};
use crate::routes::run;

pub fn app() -> Router {
    Router::new().route("/run", get(run::run))
}
