//! Gleam backend REST: auth and the shared playlists. Plus LRCLIB for lyrics.
use crate::model::*;
use anyhow::{anyhow, bail, Result};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::sync::Mutex;

pub struct Api {
    base: String,
    token: Mutex<Option<String>>,
    http: reqwest::Client,
}

impl Api {
    pub fn new(base: String, token: Option<String>) -> Api {
        Api {
            base,
            token: Mutex::new(token),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .user_agent("musakone-tui/0.1 (https://github.com/Retuperan-WBK/musakoneV3)")
                .build()
                .expect("http client"),
        }
    }

    pub fn set_token(&self, t: Option<String>) {
        *self.token.lock().unwrap() = t;
    }

    async fn req<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T> {
        let mut r = self.http.request(method, format!("{}{}", self.base, path));
        if let Some(t) = self.token.lock().unwrap().clone() {
            r = r.bearer_auth(t);
        }
        if let Some(b) = body {
            r = r.json(&b);
        }
        let resp = r.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("error").and_then(Value::as_str).map(String::from))
                .unwrap_or_else(|| format!("HTTP {status}"));
            if status.as_u16() == 401 {
                bail!("unauthorized: {msg}");
            }
            bail!("{msg}");
        }
        let text = if text.is_empty() {
            "null".to_string()
        } else {
            text
        };
        serde_json::from_str(&text).map_err(|e| anyhow!("bad response for {path}: {e}"))
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.req(reqwest::Method::GET, path, None).await
    }
    async fn post<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T> {
        self.req(reqwest::Method::POST, path, Some(body)).await
    }
    async fn put<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T> {
        self.req(reqwest::Method::PUT, path, Some(body)).await
    }

    pub async fn login(&self, user: &str, pass: &str) -> Result<String> {
        let v: Value = self
            .post(
                "/api/auth/login",
                json!({"username": user, "password": pass}),
            )
            .await?;
        v.get("token")
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| anyhow!("no token in login response"))
    }

    /// Current user id, or Err("unauthorized…") when the token is dead.
    pub async fn me(&self) -> Result<i64> {
        let v: Value = self.get("/api/auth/me").await?;
        v.get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow!("no id in /me"))
    }

    /// Every playlist. The current backend returns them all from /api/playlists; the
    /// previous release returned only the caller's and had the rest under /public.
    pub async fn playlists(&self) -> Result<Vec<Playlist>> {
        let mut all: Vec<Playlist> = self.get("/api/playlists").await?;
        if let Ok(public) = self.get::<Vec<Playlist>>("/api/playlists/public").await {
            for p in public {
                if !all.iter().any(|a| a.id == p.id) {
                    all.push(p);
                }
            }
        }
        Ok(all)
    }
    pub async fn playlist_tracks(&self, id: i64) -> Result<Vec<String>> {
        let p: PlaylistWithTracks = self.get(&format!("/api/playlists/{id}")).await?;
        Ok(p.tracks.into_iter().map(|t| t.track_uri).collect())
    }
    pub async fn create_playlist(&self, name: &str) -> Result<Playlist> {
        self.post(
            "/api/playlists",
            json!({"name": name, "description": null, "is_public": true}),
        )
        .await
    }
    pub async fn rename_playlist(&self, id: i64, name: &str) -> Result<Playlist> {
        self.put(
            &format!("/api/playlists/{id}"),
            json!({"name": name, "description": null, "is_public": true}),
        )
        .await
    }
    pub async fn delete_playlist(&self, id: i64) -> Result<()> {
        let _: Value = self
            .req(
                reqwest::Method::DELETE,
                &format!("/api/playlists/{id}"),
                None,
            )
            .await?;
        Ok(())
    }
    pub async fn add_track(&self, id: i64, uri: &str) -> Result<()> {
        let _: Value = self
            .post(
                &format!("/api/playlists/{id}/tracks"),
                json!({"track_uri": uri}),
            )
            .await?;
        Ok(())
    }
    pub async fn remove_track(&self, id: i64, uri: &str) -> Result<()> {
        let _: Value = self
            .post(
                &format!("/api/playlists/{id}/tracks/remove"),
                json!({"track_uri": uri}),
            )
            .await?;
        Ok(())
    }
    pub async fn reorder_track(&self, id: i64, uri: &str, pos: usize) -> Result<()> {
        let _: Value = self
            .put(
                &format!("/api/playlists/{id}/tracks/reorder"),
                json!({"track_uri": uri, "new_position": pos}),
            )
            .await?;
        Ok(())
    }

    /// Lyrics from LRCLIB. Disk-cached per track uri.
    /// Raw bytes of a URL (album art). Not a backend call; uses the shared client.
    pub async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self.http.get(url).send().await?.error_for_status()?;
        Ok(resp.bytes().await?.to_vec())
    }

    pub async fn lyrics(&self, track: &Track, cache_dir: &std::path::Path) -> Result<Lyrics> {
        let path = cache_dir.join(format!("{}.json", lyrics_key(&track.uri)));
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(l) = serde_json::from_str::<Lyrics>(&s) {
                return Ok(l);
            }
        }
        let artist = track.artists.split(", ").next().unwrap_or("");
        let mut url = format!(
            "https://lrclib.net/api/get?artist_name={}&track_name={}",
            urlenc(artist),
            urlenc(&track.name)
        );
        if !track.album.is_empty() {
            url += &format!("&album_name={}", urlenc(&track.album));
        }
        if let Some(ms) = track.length {
            url += &format!("&duration={}", ms / 1000);
        }
        let resp = self.http.get(&url).send().await?;
        let v: Value = if resp.status().as_u16() == 404 {
            // Exact match failed: fall back to search by artist + title, take the first hit.
            let s = self
                .http
                .get(format!(
                    "https://lrclib.net/api/search?artist_name={}&track_name={}",
                    urlenc(artist),
                    urlenc(&track.name)
                ))
                .send()
                .await?
                .json::<Value>()
                .await?;
            s.as_array()
                .and_then(|a| a.first().cloned())
                .unwrap_or(Value::Null)
        } else {
            resp.json().await?
        };
        let lyrics = Lyrics {
            synced: v
                .get("syncedLyrics")
                .and_then(Value::as_str)
                .map(String::from),
            plain: v
                .get("plainLyrics")
                .and_then(Value::as_str)
                .map(String::from),
        };
        let _ = std::fs::create_dir_all(cache_dir);
        let _ = std::fs::write(&path, serde_json::to_string(&lyrics)?);
        Ok(lyrics)
    }
}

pub fn lyrics_key(uri: &str) -> String {
    uri.replace(['/', ':'], "_")
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Lyrics {
    pub synced: Option<String>,
    pub plain: Option<String>,
}

impl Lyrics {
    /// Lines as (timestamp ms or None, text). Synced preferred.
    pub fn lines(&self) -> Vec<(Option<u64>, String)> {
        if let Some(s) = &self.synced {
            let v: Vec<_> = s.lines().filter_map(parse_lrc_line).collect();
            if !v.is_empty() {
                return v;
            }
        }
        self.plain
            .as_deref()
            .unwrap_or("")
            .lines()
            .map(|l| (None, l.to_string()))
            .collect()
    }
}

/// "[mm:ss.xx] text" -> (ms, text)
pub fn parse_lrc_line(line: &str) -> Option<(Option<u64>, String)> {
    let line = line.trim_end();
    if !line.starts_with('[') {
        return None;
    }
    let end = line.find(']')?;
    let (m, rest) = line[1..end].split_once(':')?;
    let m: u64 = m.parse().ok()?;
    let secs: f64 = rest.parse().ok()?;
    Some((
        Some(m * 60_000 + (secs * 1000.0) as u64),
        line[end + 1..].trim().to_string(),
    ))
}

fn urlenc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lrc_and_urlenc() {
        assert_eq!(
            parse_lrc_line("[01:02.50] hello"),
            Some((Some(62_500), "hello".into()))
        );
        assert_eq!(parse_lrc_line("no stamp"), None);
        assert_eq!(urlenc("Retuperän WBK"), "Retuper%C3%A4n+WBK");
    }
}
