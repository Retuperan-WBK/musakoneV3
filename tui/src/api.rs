//! Gleam backend REST: auth, playlists, mix/autoplay. Plus LRCLIB for lyrics.
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
        if text.is_empty() {
            // DELETE and friends: synthesize null for `()`-like targets.
            return serde_json::from_str("null").map_err(|e| anyhow!(e));
        }
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

    pub async fn me(&self) -> Result<Value> {
        self.get("/api/auth/me").await
    }

    pub async fn playlists(&self) -> Result<Vec<Playlist>> {
        let mut mine: Vec<Playlist> = self.get("/api/playlists").await?;
        let public: Vec<Playlist> = self.get("/api/playlists/public").await.unwrap_or_default();
        for p in public {
            if !mine.iter().any(|m| m.id == p.id) {
                mine.push(p);
            }
        }
        Ok(mine)
    }
    pub async fn playlist_tracks(&self, id: i64) -> Result<Vec<String>> {
        let p: PlaylistWithTracks = self.get(&format!("/api/playlists/{id}")).await?;
        Ok(p.tracks.into_iter().map(|t| t.track_uri).collect())
    }
    pub async fn create_playlist(&self, name: &str) -> Result<Playlist> {
        self.post(
            "/api/playlists",
            json!({"name": name, "description": null, "is_public": false}),
        )
        .await
    }
    pub async fn rename_playlist(&self, p: &Playlist, name: &str) -> Result<Playlist> {
        self.put(
            &format!("/api/playlists/{}", p.id),
            json!({"name": name, "description": null, "is_public": p.is_public}),
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

    pub async fn autoplay(&self) -> Result<AutoplayStatus> {
        self.get("/api/autoplay").await
    }
    pub async fn set_autoplay(&self, patch: Value) -> Result<AutoplayStatus> {
        self.put("/api/autoplay", patch).await
    }
    pub async fn autoplay_fill(&self) -> Result<i64> {
        let v: Value = self.post("/api/autoplay/fill", json!({})).await?;
        Ok(v.get("added").and_then(Value::as_i64).unwrap_or(0))
    }
    pub async fn recommendations(&self, scope: &str) -> Result<Vec<Recommendation>> {
        let r: Recommendations = self
            .get(&format!("/api/recommendations?scope={scope}&limit=30"))
            .await?;
        Ok(r.items)
    }

    /// Lyrics from LRCLIB. Returns (synced lrc, plain). Disk-cached per track uri.
    pub async fn lyrics(&self, track: &Track, cache_dir: &std::path::Path) -> Result<Lyrics> {
        let key = track.uri.replace(['/', ':'], "_");
        let path = cache_dir.join(format!("{key}.json"));
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
        let resp = self
            .http
            .get(&url)
            .header(
                "User-Agent",
                "musakone-tui/0.1 (https://github.com/retuperan-wbk/musakoneV3)",
            )
            .send()
            .await?;
        let v: Value = if resp.status().as_u16() == 404 {
            // Exact match failed: fall back to search by artist + title, take first hit.
            let s = self
                .http
                .get(format!(
                    "https://lrclib.net/api/search?artist_name={}&track_name={}",
                    urlenc(artist),
                    urlenc(&track.name)
                ))
                .header("User-Agent", "musakone-tui/0.1")
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

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Lyrics {
    pub synced: Option<String>,
    pub plain: Option<String>,
}

impl Lyrics {
    /// Lines as (timestamp ms or None, text). Synced preferred.
    pub fn lines(&self) -> Vec<(Option<u64>, String)> {
        if let Some(s) = &self.synced {
            return s.lines().filter_map(parse_lrc_line).collect();
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
    let stamp = &line[1..end];
    let (m, rest) = stamp.split_once(':')?;
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
