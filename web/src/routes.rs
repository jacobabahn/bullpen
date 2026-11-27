//! The routes (docs/prd/web.md, Routes), each a few lines over the modules that do the
//! work, all behind the checks in auth.rs. The page is three files in page/, compiled in.

use crate::auth::{check, Gate};
use crate::chat::{self, Missing, Target};
use crate::events;
use crate::fleet::Latest;
use crate::transcripts::Transcripts;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

const INDEX: &str = include_str!("page/index.html");
const APP_JS: &str = include_str!("page/app.js");
const APP_CSS: &str = include_str!("page/app.css");

/// What the routes share: the snapshot, and the transcripts read.
#[derive(Clone)]
pub struct App {
    pub latest: Latest,
    pub transcripts: Arc<Transcripts>,
}

impl FromRef<App> for Latest {
    fn from_ref(app: &App) -> Latest {
        app.latest.clone()
    }
}

pub fn app(gate: Arc<Gate>, latest: Latest, transcripts: Arc<Transcripts>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/app.css", get(app_css))
        .route("/api/fleet", get(fleet))
        .route("/api/chat/{repo}/{id}", get(chat_page))
        .route("/events", get(stream))
        .fallback(not_found)
        .with_state(App { latest, transcripts })
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

/// A page of a chat: the last items, or those before `before`.
async fn chat_page(
    State(app): State<App>,
    Path((repo, id)): Path<(String, String)>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let target = match Target::new(&repo, &id, q.get("who").map(String::as_str)) {
        Ok(t) => t,
        Err(e) => return refused(StatusCode::BAD_REQUEST, e),
    };
    let snapshot: Value = serde_json::from_str(&app.latest.borrow().json).unwrap_or_default();
    let place = match chat::locate(&snapshot, &target) {
        Ok(p) => p,
        Err(Missing::Unknown(e)) => return refused(StatusCode::NOT_FOUND, e),
        Err(Missing::Wrong(e)) => return refused(StatusCode::BAD_REQUEST, e),
    };
    let before = q.get("before").cloned();
    let read = tokio::task::spawn_blocking(move || {
        let Some((path, source)) = place.find() else {
            return before.is_none().then(|| chat::answer(&place, None, None));
        };
        let page = app.transcripts.read(&path, source).page(before.as_deref())?;
        Some(chat::answer(&place, Some(source), Some(page)))
    });
    match read.await {
        Ok(Some(answer)) => Json(answer).into_response(),
        Ok(None) => refused(StatusCode::NOT_FOUND, format!("no item {} in this chat", q["before"])),
        Err(e) => refused(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// `{error}`, for a request that can't be answered.
fn refused(status: StatusCode, error: String) -> Response {
    (status, Json(json!({ "error": error }))).into_response()
}

/// The event stream, with the chat `watch` names (`{repo}/{id}[/{who}]`), from the
/// offset `cursor` a page of it was read to.
async fn stream(State(app): State<App>, Query(q): Query<HashMap<String, String>>) -> Response {
    let watch = q
        .get("watch")
        .and_then(|w| Target::watched(w))
        .map(|target| events::Watch {
            target,
            cursor: q.get("cursor").and_then(|c| c.parse().ok()),
        });
    events::stream(app.latest, app.transcripts, watch).into_response()
}

/// Nothing here: a bare 404.
async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}
