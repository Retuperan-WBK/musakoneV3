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
    pub fn start(
        ws_url: String,
        token: Option<String>,
        events: mpsc::UnboundedSender<Incoming>,
    ) -> Arc<Rpc> {
        let (out, out_rx) = mpsc::unbounded_channel();
        let rpc = Arc::new(Rpc {
            out,
            pending: Default::default(),
            next_id: AtomicU64::new(1),
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
        self.out
            .send(Outgoing::Text(req.to_string()))
            .map_err(|_| anyhow!("rpc task gone"))?;
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
        let ws = match tokio_tungstenite::connect_async(&url).await {
            Ok((ws, _)) => ws,
            Err(_) => {
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
        fail_all(&pending);
        let _ = events.send(Incoming::Disconnected);
    }
}

fn dispatch(text: &str, pending: &Pending, events: &mpsc::UnboundedSender<Incoming>) {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
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
                None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = tx.send(res);
        }
    } else if v.get("event").is_some() || v.get("error").is_some() {
        let _ = events.send(Incoming::Event(v));
    }
}
