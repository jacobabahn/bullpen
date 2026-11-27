//! The routes (docs/prd/web.md, Routes), each a few lines over the modules that do the
//! work, all behind the checks in auth.rs. The page is three files in page/, compiled in.

use crate::auth::{check, Gate};
use crate::events;
use crate::fleet::Latest;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use std::sync::Arc;

const INDEX: &str = include_str!("page/index.html");
const APP_JS: &str = include_str!("page/app.js");
const APP_CSS: &str = include_str!("page/app.css");

pub fn app(gate: Arc<Gate>, latest: Latest) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/app.css", get(app_css))
        .route("/api/fleet", get(fleet))
        .route("/events", get(stream))
        .fallback(not_found)
        .with_state(latest)
        .layer(axum::middleware::from_fn_with_state(gate, check))
}

async fn index() -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], INDEX).into_response()
}

async fn app_js() -> Response {
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], APP_JS).into_response()
}

async fn app_css() -> Response {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], APP_CSS).into_response()
}

/// The snapshot as it is now.
async fn fleet(State(latest): State<Latest>) -> Response {
    let json = latest.borrow().json.clone();
    ([(header::CONTENT_TYPE, "application/json")], json).into_response()
}

async fn stream(State(latest): State<Latest>) -> Response {
    events::stream(latest).into_response()
}

/// Nothing here: a bare 404.
async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}
