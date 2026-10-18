//! The Flow page, for computers without the Orbit app (macOS, Windows) and for the app
//! itself: a small HTTP server on 127.0.0.1 only. `hidpilot-flow ui` opens it in the browser.
//!
//! Locked down: it listens on loopback, every API call needs the token from a file only this
//! user can read, and the Host header must be the loopback address itself, so a web page in
//! the browser can neither call it (no CORS, no token) nor reach it through DNS rebinding.

use crate::agent::{self, State};
use crate::clipboard::Clipboard;
use crate::config::{self, Side};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const UI_PORT: u16 = config::PORT + 2;
const PAGE: &str = include_str!("ui.html");

/// Where the token lives (0600); the app and `hidpilot-flow ui` read it from there.
pub fn token_path() -> Result<std::path::PathBuf> {
    Ok(config::path()?.with_file_name("flow-ui.token"))
}

pub fn token() -> Result<String> {
    Ok(std::fs::read_to_string(token_path()?)?.trim().to_string())
}

fn new_token() -> Result<String> {
    let t: String = (0..32).map(|_| format!("{:02x}", rand::random::<u8>())).collect();
    let p = token_path()?;
    std::fs::create_dir_all(p.parent().unwrap())?;
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, &t)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, &p)?;
    Ok(t)
}

pub fn start(state: State, clipboard: Clipboard) {
    tokio::spawn(async move {
        let token = match new_token() {
            Ok(t) => t,
            Err(e) => return eprintln!("flow: page disabled, no token: {e:#}"),
        };
        let listener = match TcpListener::bind(("127.0.0.1", UI_PORT)).await {
            Ok(l) => l,
            Err(e) => return eprintln!("flow: page port {UI_PORT} is busy: {e}"),
        };
        while let Ok((stream, _)) = listener.accept().await {
            let (state, clipboard, token) = (state.clone(), clipboard.clone(), token.clone());
            tokio::spawn(async move {
                let _ = tokio::time::timeout(Duration::from_secs(10), serve(stream, &state, &clipboard, &token)).await;
            });
        }
    });
}

struct Request {
    method: String,
    path: String,
    host: String,
    token: String,
    body: Vec<u8>,
}

async fn read_request(stream: &mut TcpStream) -> Result<Request> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            bail!("closed");
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > 16 * 1024 {
            bail!("headers too large");
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split(' ');
    let (method, path) = (first.next().unwrap_or_default().to_string(), first.next().unwrap_or_default().to_string());
    let (mut host, mut token, mut len) = (String::new(), String::new(), 0usize);
    for l in lines {
        let Some((k, v)) = l.split_once(':') else { continue };
        match k.trim().to_ascii_lowercase().as_str() {
            "host" => host = v.trim().to_string(),
            "x-token" => token = v.trim().to_string(),
            "content-length" => len = v.trim().parse().unwrap_or(usize::MAX),
            _ => {}
        }
    }
    if len > 8 * 1024 {
        bail!("body too large");
    }
    let mut body = buf[head_end..].to_vec();
    while body.len() < len {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            bail!("closed");
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    Ok(Request { method, path, host, token, body })
}

async fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Frame-Options: DENY\r\n\
         Content-Security-Policy: default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; frame-ancestors 'none'\r\n\
         Referrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    Ok(())
}

/// Constant time, so the token cannot be guessed byte by byte from response times.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn serve(mut stream: TcpStream, state: &State, clipboard: &Clipboard, token: &str) -> Result<()> {
    let req = read_request(&mut stream).await?;
    let local = [format!("127.0.0.1:{UI_PORT}"), format!("localhost:{UI_PORT}")];
    if !local.contains(&req.host) {
        return respond(&mut stream, "421 Misdirected Request", "text/plain", b"").await;
    }
    if req.method == "GET" && (req.path == "/" || req.path.starts_with("/?")) {
        return respond(&mut stream, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()).await;
    }
    if !req.path.starts_with("/api/") {
        return respond(&mut stream, "404 Not Found", "text/plain", b"").await;
    }
    if req.method != "POST" || !same(&req.token, token) {
        return respond(&mut stream, "403 Forbidden", "text/plain", b"").await;
    }
    let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
    match api(&req.path, &body, state, clipboard) {
        Ok(v) => respond(&mut stream, "200 OK", "application/json", v.to_string().as_bytes()).await,
        Err(e) => respond(&mut stream, "400 Bad Request", "application/json", json!({ "error": format!("{e:#}") }).to_string().as_bytes()).await,
    }
}

fn side(v: &Value) -> Result<Option<Side>> {
    match v.as_str() {
        None | Some("") => Ok(None),
        Some(s) => Ok(Some(s.parse()?)),
    }
}

fn api(path: &str, body: &Value, state: &State, clipboard: &Clipboard) -> Result<Value> {
    let str_of = |k: &str| body[k].as_str().map(str::to_string).with_context(|| format!("missing {k}"));
    match path {
        "/api/state" => {}
        "/api/pair" => match body["addr"].as_str() {
            Some(addr) if !addr.trim().is_empty() => agent::ask_pairing_addr(state, clipboard, addr, side(&body["side"])?)?,
            _ => agent::ask_pairing(state, clipboard, &str_of("key")?, side(&body["side"])?)?,
        },
        "/api/code" => agent::enter_code(state, str_of("code")?)?,
        "/api/cancel" => state.lock().unwrap().outgoing = None,
        "/api/place" => {
            let (key, side) = (str_of("key")?, side(&body["side"])?);
            let mut s = state.lock().unwrap();
            if let Some(side) = side {
                for (k, p) in s.cfg.peers.iter_mut() {
                    if *k != key && p.side == Some(side) {
                        p.side = None;
                    }
                }
            }
            s.cfg.peers.get_mut(&key).context("unknown computer")?.side = side;
            s.cfg.save()?;
        }
        "/api/forget" => {
            let key = str_of("key")?;
            let mut s = state.lock().unwrap();
            s.cfg.peers.remove(&key);
            s.cfg.save()?;
        }
        "/api/set" => {
            let on = body["value"].as_bool().context("expected true/false")?;
            let mut s = state.lock().unwrap();
            match str_of("key")?.as_str() {
                "clipboard" => s.cfg.clipboard = on,
                "keyboardFollows" => s.cfg.keyboard_follows = on,
                "audioFollows" => s.cfg.audio_follows = on,
                k => bail!("unknown setting {k}"),
            }
            s.cfg.save()?;
        }
        _ => bail!("unknown call"),
    }
    Ok(snapshot(state))
}

fn snapshot(state: &State) -> Value {
    let s = state.lock().unwrap();
    let now = Instant::now();
    let peers: Vec<Value> = s
        .cfg
        .peers
        .iter()
        .map(|(k, p)| json!({ "key": k, "name": p.name, "side": p.side, "online": agent::is_linked(&s, k) }))
        .collect();
    let nearby: Vec<Value> = s
        .nearby
        .iter()
        .filter(|(k, _)| !s.cfg.peers.contains_key(*k))
        .map(|(k, f)| json!({ "key": k, "name": f.name }))
        .collect();
    json!({
        "name": s.cfg.name,
        "id": s.cfg.my_id(),
        "addrs": crate::discovery::lan_addrs(),
        "peers": peers,
        "nearby": nearby,
        "incoming": s.incoming.as_ref().filter(|i| i.until > now).map(|i| json!({ "name": i.name, "code": i.code, "seconds": (i.until - now).as_secs() })),
        "outgoing": s.outgoing.as_ref().map(|o| json!({ "key": o.key, "name": o.name, "status": o.status, "error": o.error })),
        "clipboard": s.cfg.clipboard,
        "keyboardFollows": s.cfg.keyboard_follows,
        "audioFollows": s.cfg.audio_follows,
    })
}

/// `hidpilot-flow ui`: opens the page in the default browser. The token travels in the
/// URL fragment, which the browser never sends to the server or to anyone else.
pub fn open() -> Result<()> {
    let url = format!("http://127.0.0.1:{UI_PORT}/#{}", token().context("Flow is not running (start the hidpilot-flow service)")?);
    #[cfg(target_os = "linux")]
    let r = std::process::Command::new("xdg-open").arg(&url).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(&url).spawn();
    #[cfg(windows)]
    let r = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", &url]).spawn();
    r.context("could not open the browser")?;
    Ok(())
}
