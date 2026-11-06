//! Mopidy JSON-RPC over the backend's /ws proxy. One socket, request ids correlated
//! to oneshot channels, events fanned out to the app. Reconnects with backoff.
use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

pub enum Incoming {
    Connected,
    Disconnected,
    Event(Value),
}

enum Outgoing {
    Text(String),
    Reconnect,
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;

pub struct Rpc {
    out: mpsc::UnboundedSender<Outgoing>,
    pending: Pending,
    next_id: AtomicU64,
    token: Arc<Mutex<Option<String>>>,
}

impl Rpc {
    /// Disconnected transport for offline previews; never starts a network task.
    pub fn offline() -> Arc<Rpc> {
        let (out, _) = mpsc::unbounded_channel();
        Arc::new(Rpc {
            out,
            pending: Default::default(),
            next_id: AtomicU64::new(1),
            token: Arc::new(Mutex::new(None)),
        })
    }

    pub fn start(
        ws_url: String,
        token: Option<String>,
        events: mpsc::UnboundedSender<Incoming>,
    ) -> Arc<Rpc> {
        let (out, out_rx) = mpsc::unbounded_channel();
        let rpc = Arc::new(Rpc {
            out,
            pending: Default::default(),
            // The backend fans every Mopidy reply out to all clients; start far away from
            // the browsers' small ids so a foreign reply never matches one of ours.
            next_id: AtomicU64::new(
                1_000_000
                    * (1 + std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64 % 4_000_000)
                        .unwrap_or(7)),
            ),
            token: Arc::new(Mutex::new(token)),
        });
        tokio::spawn(run(
            ws_url,
            rpc.token.clone(),
            rpc.pending.clone(),
            out_rx,
            events,
        ));
        rpc
    }

    pub fn set_token(&self, token: Option<String>) {
        *self.token.lock().unwrap() = token;
        let _ = self.out.send(Outgoing::Reconnect);
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let req = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if self.out.send(Outgoing::Text(req.to_string())).is_err() {
            self.pending.lock().unwrap().remove(&id);
            return Err(anyhow!("rpc task gone"));
        }
        match tokio::time::timeout(Duration::from_secs(60), rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(anyhow!("connection lost")),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(anyhow!("{method}: timed out"))
            }
        }
    }
}

fn fail_all(pending: &Pending) {
    for (_, tx) in pending.lock().unwrap().drain() {
        let _ = tx.send(Err(anyhow!("connection lost")));
    }
}

async fn run(
    ws_url: String,
    token: Arc<Mutex<Option<String>>>,
    pending: Pending,
    mut out_rx: mpsc::UnboundedReceiver<Outgoing>,
    events: mpsc::UnboundedSender<Incoming>,
) {
    let mut backoff = 1u64;
    loop {
        let url = match token.lock().unwrap().clone() {
            Some(t) => format!("{ws_url}?token={t}"),
            None => ws_url.clone(),
        };
        debug(&format!(
            "connecting {}",
            url.split('?').next().unwrap_or("")
        ));
        let ws = match tokio_tungstenite::connect_async(&url).await {
            Ok((ws, _)) => ws,
            Err(e) => {
                debug(&format!("connect failed: {e}"));
                let _ = events.send(Incoming::Disconnected);
                // Drain control messages while down so a Reconnect does not pile up.
                let _ = tokio::time::timeout(Duration::from_secs(backoff), out_rx.recv()).await;
                backoff = (backoff * 2).min(30);
                continue;
            }
        };
        backoff = 1;
        let _ = events.send(Incoming::Connected);
        let (mut sink, mut stream) = ws.split();
        loop {
            tokio::select! {
                out = out_rx.recv() => match out {
                    Some(Outgoing::Text(t)) => {
                        debug_payload("->", &t, 300);
                        if sink.send(Message::Text(t.into())).await.is_err() { break; }
                    }
                    Some(Outgoing::Reconnect) => { let _ = sink.close().await; break; }
                    None => return,
                },
                msg = stream.next() => match msg {
                    Some(Ok(Message::Text(t))) => dispatch(&t, &pending, &events),
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => {}
                },
            }
        }
        debug("socket closed");
        fail_all(&pending);
        let _ = events.send(Incoming::Disconnected);
    }
}

fn dispatch(text: &str, pending: &Pending, events: &mpsc::UnboundedSender<Incoming>) {
    debug_payload("<-", text, 200);
    let Ok(mut v) = serde_json::from_str::<Value>(text) else {
        return;
    };
    if let Some(id) = v.get("id").and_then(Value::as_u64) {
        if let Some(tx) = pending.lock().unwrap().remove(&id) {
            let res = match v.get("error") {
                Some(e) => Err(anyhow!(
                    "{}",
                    e.get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("rpc error")
                )),
                None => Ok(v.get_mut("result").map(Value::take).unwrap_or(Value::Null)),
            };
            let _ = tx.send(res);
        }
    } else if v.get("event").is_some() || v.get("error").is_some() {
        let _ = events.send(Incoming::Event(v));
    }
}

fn debug_payload(direction: &str, text: &str, limit: usize) {
    if std::env::var_os("MUSAKONE_DEBUG").is_some() {
        let mut end = text.len().min(limit);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        debug(&format!("{direction} {}", &text[..end]));
    }
}

fn debug(line: &str) {
    if let Ok(path) = std::env::var("MUSAKONE_DEBUG") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_handles_unicode_at_the_debug_preview_boundary() {
        let prefix = "{\"id\":7,\"result\":\"";
        let name = format!("{}ö", "x".repeat(199 - prefix.len()));
        let text = format!("{prefix}{name}\"}}");
        assert!(!text.is_char_boundary(200));
        let pending = Pending::default();
        let (tx, mut rx) = oneshot::channel();
        pending.lock().unwrap().insert(7, tx);
        let (events, _) = mpsc::unbounded_channel();
        dispatch(&text, &pending, &events);
        assert_eq!(rx.try_recv().unwrap().unwrap(), json!(name));
        assert!(pending.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn closed_transport_does_not_retain_pending_requests() {
        let rpc = Rpc::offline();
        assert!(rpc.call("core.library.browse", json!({})).await.is_err());
        assert!(rpc.pending.lock().unwrap().is_empty());
    }
}
