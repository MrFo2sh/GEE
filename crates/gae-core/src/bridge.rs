//! Worker threads and the one channel that connects them to the app. No Bevy here.
//!
//! - one thread per `Source`, polling at the source's own interval
//! - an HTTP thread on 127.0.0.1: `POST /events` (see `ingest`), `GET /health`, `GET /state`
//!   (a JSON summary the app publishes, so scripts and agents can check what the user sees)
//!
//! The channel is bounded. Sources block when the app falls behind (backpressure); HTTP answers
//! 503 instead of blocking a client.

use crate::ingest::parse_events;
use crate::model::Event;
use crate::source::Source;
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::thread;

pub const QUEUE: usize = 4_096;
pub const MAX_BODY: u64 = 4 * 1024 * 1024;

pub enum ToApp {
    Events(Vec<Event>),
    Status(String),
    Error(String),
}

pub struct BridgeConfig {
    /// `None` turns the HTTP endpoint off.
    pub http_port: Option<u16>,
    /// Shown by `GET /health`.
    pub banner: String,
}

pub struct Bridge {
    pub rx: Receiver<ToApp>,
    state: Arc<Mutex<String>>,
}

impl Bridge {
    /// What `GET /state` answers from now on.
    pub fn publish_state(&self, json: String) {
        if let Ok(mut s) = self.state.lock() {
            *s = json;
        }
    }

    /// A bridge fed by hand, for tests: the sender plays the worker side.
    pub fn manual() -> (Bridge, Sender<ToApp>) {
        let (tx, rx) = bounded(QUEUE);
        (
            Bridge {
                rx,
                state: Arc::new(Mutex::new("{}".into())),
            },
            tx,
        )
    }
}

pub fn start(cfg: BridgeConfig, sources: Vec<Box<dyn Source>>) -> Bridge {
    let (tx, rx) = bounded::<ToApp>(QUEUE);
    let state = Arc::new(Mutex::new("{}".to_string()));
    for s in sources {
        spawn_source(s, tx.clone());
    }
    if let Some(port) = cfg.http_port {
        spawn_http(port, cfg.banner, tx, state.clone());
    }
    Bridge { rx, state }
}

fn spawn_source(mut source: Box<dyn Source>, tx: Sender<ToApp>) {
    let name = source.name();
    thread::Builder::new()
        .name(format!("source:{name}"))
        .spawn(move || {
            let _ = tx.send(ToApp::Status(format!("{name}: running")));
            let mut failing = false;
            loop {
                match source.poll() {
                    Ok(events) => {
                        failing = false;
                        if !events.is_empty() && tx.send(ToApp::Events(events)).is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        // Say it once per failure streak, not every poll.
                        if !failing && tx.send(ToApp::Error(format!("{name}: {e}"))).is_err() {
                            return;
                        }
                        failing = true;
                    }
                }
                thread::sleep(source.interval());
            }
        })
        .expect("spawn source thread");
}

fn spawn_http(port: u16, banner: String, tx: Sender<ToApp>, state: Arc<Mutex<String>>) {
    thread::Builder::new()
        .name("http".into())
        .spawn(move || {
            let server = match tiny_http::Server::http(("127.0.0.1", port)) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(ToApp::Error(format!("ingest http off: {e}")));
                    return;
                }
            };
            let _ = tx.send(ToApp::Status(format!("ingest on 127.0.0.1:{port}")));
            for req in server.incoming_requests() {
                handle(req, &tx, &state, &banner);
            }
        })
        .expect("spawn http thread");
}

fn header<'a>(req: &'a tiny_http::Request, name: &str) -> Option<&'a str> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

/// Browsers send `Origin` on cross-site requests; local tools (curl, hooks, scripts) do not.
/// Refusing it keeps any web page from writing into the world through the user's browser.
pub fn browser_request(origin: Option<&str>) -> bool {
    origin.is_some_and(|o| !o.is_empty())
}

fn handle(mut req: tiny_http::Request, tx: &Sender<ToApp>, state: &Arc<Mutex<String>>, banner: &str) {
    let url = req.url().split('?').next().unwrap_or("").to_string();
    let method = req.method().as_str().to_string();
    let (status, body, json) = if browser_request(header(&req, "Origin")) {
        (403, "browser requests are refused".to_string(), false)
    } else {
        match (method.as_str(), url.as_str()) {
            ("GET", "/health") => (200, format!("ok {banner}"), false),
            ("GET", "/state") => (200, state.lock().map(|s| s.clone()).unwrap_or_default(), true),
            ("POST", "/events") => {
                let mut text = String::new();
                let read = req.as_reader().take(MAX_BODY + 1).read_to_string(&mut text);
                if read.is_err() {
                    (400, "body is not UTF-8".to_string(), false)
                } else if text.len() as u64 > MAX_BODY {
                    (413, format!("body over {MAX_BODY} bytes"), false)
                } else {
                    match parse_events(&text) {
                        Ok(events) => {
                            let n = events.len();
                            match tx.try_send(ToApp::Events(events)) {
                                Ok(()) => (200, format!("accepted {n}"), false),
                                Err(TrySendError::Full(_)) => (503, "busy, retry".to_string(), false),
                                Err(TrySendError::Disconnected(_)) => (503, "shutting down".to_string(), false),
                            }
                        }
                        Err(e) => (400, e, false),
                    }
                }
            }
            _ => (404, "not found".to_string(), false),
        }
    };
    let mut resp = tiny_http::Response::from_string(body).with_status_code(status);
    if json && let Ok(h) = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]) {
        resp = resp.with_header(h);
    }
    let _ = req.respond(resp);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpStream;
    use std::time::Duration;

    fn request(port: u16, raw: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn http_accepts_events_serves_state_and_refuses_browsers() {
        let port = 47_000 + (std::process::id() % 1000) as u16;
        let bridge = start(
            BridgeConfig {
                http_port: Some(port),
                banner: "test".into(),
            },
            Vec::new(),
        );
        // Wait for the listener.
        let mut up = false;
        for _ in 0..50 {
            if let Ok(ToApp::Status(s)) = bridge.rx.recv_timeout(Duration::from_millis(100))
                && s.contains("ingest on")
            {
                up = true;
                break;
            }
        }
        assert!(up, "listener started");
        let body = r#"{"type":"unit_down","unit":"a"}"#;
        let r = request(
            port,
            &format!(
                "POST /events HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(r.contains("accepted 1"), "{r}");
        assert!(matches!(bridge.rx.recv_timeout(Duration::from_secs(2)), Ok(ToApp::Events(e)) if e.len() == 1));
        let r = request(
            port,
            &format!(
                "POST /events HTTP/1.1\r\nHost: x\r\nOrigin: https://evil.example\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(r.starts_with("HTTP/1.1 403"), "{r}");
        bridge.publish_state(r#"{"units":3}"#.into());
        let r = request(port, "GET /state HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
        assert!(r.contains(r#"{"units":3}"#), "{r}");
        let r = request(port, "GET /nope HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
        assert!(r.starts_with("HTTP/1.1 404"));
    }

    #[test]
    fn origin_rule() {
        assert!(browser_request(Some("http://localhost:3000")));
        assert!(!browser_request(None));
        assert!(!browser_request(Some("")));
    }
}
