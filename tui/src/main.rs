mod api;
mod app;
mod model;
mod rpc;
mod ui;
mod update;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
        .join("musakone")
}

fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
        })
        .join("musakone")
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = config_dir();
    let url_file = cfg.join("url");
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("-h") | Some("--help") => {
            println!(
                "usage: musakone [BACKEND_URL]   (default http://10.0.1.2:3001, remembered in {})",
                url_file.display()
            );
            println!("       musakone update        download the latest CI build over this binary");
            println!("       musakone --version");
            return Ok(());
        }
        Some("update") => return update::run().await,
        Some("--version") | Some("-V") => {
            println!(
                "musakone {} ({})",
                env!("CARGO_PKG_VERSION"),
                update::BUILT_SHA.map(|s| &s[..7]).unwrap_or("dev")
            );
            return Ok(());
        }
        _ => {}
    }
    let base = arg
        .or_else(|| std::env::var("MUSAKONE_URL").ok())
        .or_else(|| {
            std::fs::read_to_string(&url_file)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "http://10.0.1.2:3001".into())
        .trim_end_matches('/')
        .to_string();
    let _ = std::fs::create_dir_all(&cfg);
    let _ = std::fs::write(&url_file, &base);
    let ws_url = format!(
        "{}/ws",
        base.replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1)
    );
    let token_path = cfg.join("token");
    let token = std::fs::read_to_string(&token_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    let rpc = rpc::Rpc::start(ws_url, token.clone(), ev_tx);
    let api = Arc::new(api::Api::new(base, token.clone()));
    let mut app = app::App::new(
        rpc,
        api,
        msg_tx.clone(),
        token_path,
        cache_dir().join("lyrics"),
        token.is_some(),
    );

    {
        let tx = msg_tx.clone();
        tokio::spawn(async move {
            if let Some(sha) = update::available().await {
                let _ = tx.send(app::Msg::Info(format!(
                    "New build {} available: run `musakone update`",
                    &sha[..7]
                )));
            }
        });
    }
    let mut terminal = ratatui::init();
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(1000));
    let mut dirty = true;
    loop {
        if dirty {
            terminal.draw(|f| ui::draw(f, &mut app))?;
            dirty = false;
        }
        tokio::select! {
            ev = events.next() => match ev {
                Some(Ok(Event::Key(k))) if k.kind != KeyEventKind::Release => { app.key(k); dirty = true; }
                Some(Ok(Event::Resize(..))) => dirty = true,
                Some(Err(_)) | None => break,
                _ => {}
            },
            Some(m) = msg_rx.recv() => { app.on_msg(m); dirty = true; }
            Some(e) = ev_rx.recv() => {
                match e {
                    rpc::Incoming::Connected => app.on_connected(),
                    rpc::Incoming::Disconnected => app.on_disconnected(),
                    rpc::Incoming::Event(v) => app.on_event(v),
                }
                dirty = true;
            }
            _ = tick.tick() => { app.tick(); dirty = true; }
        }
        // Coalesce bursts (e.g. 100 lookup results) into one redraw.
        while let Ok(m) = msg_rx.try_recv() {
            app.on_msg(m);
        }
        if app.quit {
            break;
        }
    }
    ratatui::restore();
    Ok(())
}
