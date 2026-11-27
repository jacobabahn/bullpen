//! Who may use the page, and from where (docs/prd/web.md, Security). Every request must
//! carry a `Tailscale-User-Login` that `web_users` names (unset: the login this machine
//! is logged in to Tailscale as) and a `Host` that is the machine's tailnet name
//! (`web_host`, else Tailscale's), and anything but a GET must come from the page itself
//! (JSON, `X-Bullpen: 1`, same origin). Anything else is a bare 403, logged in
//! `<state>/web.log`. Every answer gets the page's headers (its CSP among them).

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use bullpen_core::json::Obj;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The header `tailscale serve` adds to each request, in place of any the client sent.
pub const LOGIN: &str = "tailscale-user-login";

/// The page's own: script, style and data from it alone, and never in a frame.
const CSP: &str = "default-src 'self'; frame-ancestors 'none'";

/// Who may connect, to which host, and where refusals are logged.
pub struct Gate {
    users: Vec<String>,
    host: String,
    log: PathBuf,
}

impl Gate {
    /// From the global settings, asking Tailscale for what they leave unset.
    pub fn new(state: &Path) -> Result<Gate, String> {
        let cfg = bullpen_core::config::inherited_config().map_err(|e| e.0)?;
        let users = list(text(&cfg, "web_users"));
        let host = text(&cfg, "web_host").trim_end_matches('.').to_lowercase();
        let mut gate = Gate {
            users,
            host,
            log: state.join("web.log"),
        };
        if gate.users.is_empty() || gate.host.is_empty() {
            let me = tailscale_self().map_err(|e| {
                format!(
                    "couldn't ask Tailscale who this machine is ({e}): log in to Tailscale, or set who may \
                     connect and the page's host name with `bullpen config --global --web-users LOGIN --web-host NAME`"
                )
            })?;
            if gate.users.is_empty() {
                gate.users = vec![me.login];
            }
            if gate.host.is_empty() {
                gate.host = me.host;
            }
        }
        Ok(gate)
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn users(&self) -> &[String] {
        &self.users
    }

    /// Why this request is refused, if it is.
    pub fn refusal(&self, method: &Method, headers: &HeaderMap) -> Option<String> {
        let login = value(headers, LOGIN);
        if login.is_empty() {
            return Some("no Tailscale login (not through `tailscale serve`?)".into());
        }
        if !self.users.iter().any(|u| u.eq_ignore_ascii_case(login)) {
            return Some(format!("{login} isn't one of web_users"));
        }
        let host = value(headers, header::HOST.as_str());
        if !self.is_host(host) {
            return Some(format!("the host {host:?} isn't {}", self.host));
        }
        if method != Method::GET && method != Method::HEAD {
            return self.cross_site(headers);
        }
        None
    }

    /// The machine's tailnet name, with or without the HTTPS port.
    fn is_host(&self, host: &str) -> bool {
        let host = host.strip_suffix(":443").unwrap_or(host).trim_end_matches('.');
        host.eq_ignore_ascii_case(&self.host)
    }

    /// Why a request that changes something may not be the page's own: a form on
    /// another site can send neither JSON nor a header of its own without a preflight
    /// (which nothing here answers), and the browser says where it comes from.
    fn cross_site(&self, headers: &HeaderMap) -> Option<String> {
        let kind = value(headers, header::CONTENT_TYPE.as_str());
        if kind.split(';').next().map(str::trim) != Some("application/json") {
            return Some(format!("a {kind:?} body, not JSON"));
        }
        if value(headers, "x-bullpen") != "1" {
            return Some("no X-Bullpen: 1".into());
        }
        let site = value(headers, "sec-fetch-site");
        let origin = value(headers, header::ORIGIN.as_str());
        if site == "same-origin" || origin.eq_ignore_ascii_case(&format!("https://{}", self.host)) {
            return None;
        }
        Some(format!(
            "from another site (Sec-Fetch-Site {site:?}, Origin {origin:?})"
        ))
    }

    /// A line in web.log for a refusal: when, what was asked, why, and who and which host asked.
    fn log(&self, method: &Method, path: &str, headers: &HeaderMap, why: &str) {
        let login = value(headers, LOGIN);
        let who = if login.is_empty() {
            "no login".into()
        } else {
            format!("login {login}")
        };
        let host = value(headers, header::HOST.as_str());
        let line = format!(
            "{} refused {method} {path}: {why} ({who}, host {host:?})\n",
            bullpen_core::timeutil::iso_now()
        );
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&self.log);
        if let Ok(mut f) = file {
            let _ = f.write_all(printable(&line).as_bytes());
        }
    }
}

/// A header's value as text ("" when it's missing or not text).
fn value<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
}

/// A line with no control characters but its newline: what a client sent can't forge
/// lines of the log.
fn printable(line: &str) -> String {
    let body = line.trim_end_matches('\n');
    let clean: String = body.chars().map(|c| if c.is_control() { '?' } else { c }).collect();
    clean + "\n"
}

fn text<'a>(cfg: &'a Obj, key: &str) -> &'a str {
    cfg.get(key).and_then(Value::as_str).unwrap_or("").trim()
}

/// "ann@x.com, bob@y.com" as its logins.
fn list(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// This machine as Tailscale knows it: the login it's logged in as, and its tailnet name.
struct Me {
    login: String,
    host: String,
}

fn tailscale_self() -> Result<Me, String> {
    let out = std::process::Command::new("tailscale")
        .args(["status", "--json"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("`tailscale status --json`: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("`tailscale status --json`: {}", err.trim()));
    }
    let status: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("`tailscale status --json`: {e}"))?;
    me_in(&status).ok_or("`tailscale status --json` names no login or host for this machine".into())
}

/// Self's login (its UserID's entry in User) and its DNSName, from `tailscale status --json`.
fn me_in(status: &Value) -> Option<Me> {
    let me = status.get("Self")?;
    let uid = me.get("UserID")?;
    let user = status.get("User")?.get(uid.to_string())?;
    let login = user.get("LoginName")?.as_str()?.to_string();
    let host = me.get("DNSName")?.as_str()?.trim_end_matches('.').to_lowercase();
    (!login.is_empty() && !host.is_empty()).then_some(Me { login, host })
}

/// The middleware every route goes through: a bare 403 (logged) for a request the gate
/// refuses, and the page's headers on every answer.
pub async fn check(State(gate): State<Arc<Gate>>, req: Request, next: Next) -> Response {
    let mut res = match gate.refusal(req.method(), req.headers()) {
        Some(why) => {
            gate.log(req.method(), req.uri().path(), req.headers(), &why);
            let mut r = Response::new(Body::empty());
            *r.status_mut() = StatusCode::FORBIDDEN;
            r
        }
        None => next.run(req).await,
    };
    let h = res.headers_mut();
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

#[cfg(test)]
#[path = "tests/test_auth.rs"]
mod tests;
