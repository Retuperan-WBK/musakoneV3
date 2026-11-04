mod api;
mod app;
mod model;
mod rpc;
mod ui;
mod update;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback)
        })
        .join("musakone")
}

/// KEY=VALUE lines from ~/.config/musakone/env; real environment variables win.
fn load_env_file(path: &Path) {
    let Ok(s) = std::fs::read_to_string(path) else {
        return;
    };
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let (k, v) = (k.trim(), v.trim().trim_matches('"'));
            if std::env::var_os(k).is_none() {
                std::env::set_var(k, v);
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = xdg("XDG_CONFIG_HOME", ".config");
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
            println!();
            println!(
                "Credentials: MUSAKONE_USER / MUSAKONE_PASSWORD in the environment or in {}",
                cfg.join("env").display()
            );
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
    load_env_file(&cfg.join("env"));
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
    let creds = match (
        std::env::var("MUSAKONE_USER"),
        std::env::var("MUSAKONE_PASSWORD"),
    ) {
        (Ok(u), Ok(p)) if !u.is_empty() => Some((u, p)),
        _ => None,
    };

    let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    let rpc = rpc::Rpc::start(ws_url, token.clone(), ev_tx);
    let api = Arc::new(api::Api::new(base, token.clone()));
    let mut app = app::App::new(
        rpc,
        api,
        msg_tx.clone(),
        token_path,
        xdg("XDG_CACHE_HOME", ".cache").join("lyrics"),
        token.is_some(),
        creds,
    );
    {
        let tx = msg_tx.clone();
        tokio::spawn(async move {
            if let Some(sha) = update::auto().await {
                let _ = tx.send(app::Msg::Info(format!(
                    "Updated to build {}: restart musakone to use it",
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
