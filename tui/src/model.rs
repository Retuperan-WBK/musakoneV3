//! Data shapes shared by the Mopidy JSON-RPC side and the Gleam REST side.
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub uri: String,
    pub name: String,
    pub artists: String,
    pub album: String,
    pub date: String,
    pub track_no: Option<u64>,
    pub length: Option<u64>,
}

impl Track {
    pub fn from_value(v: &Value) -> Option<Track> {
        let uri = v.get("uri")?.as_str()?.to_string();
        // Mopidy-Tidal appends " (version)" and leaves " ()" when there is none.
        let name = v
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim_end_matches(" ()")
            .to_string();
        let artists = v
            .get("artists")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.get("name").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let album = v
            .pointer("/album/name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let date = v
            .get("date")
            .and_then(Value::as_str)
            .or_else(|| v.pointer("/album/date").and_then(Value::as_str))
            .unwrap_or("")
            .chars()
            .take(4)
            .collect();
        Some(Track {
            uri,
            name,
            artists,
            album,
            date,
            track_no: v.get("track_no").and_then(Value::as_u64),
            length: v.get("length").and_then(Value::as_u64),
        })
    }

    /// `%f`: the last path segment of the uri, what ncmpcpp shows when tags are missing.
    pub fn filename(&self) -> String {
        let tail = self.uri.rsplit(['/', ':']).next().unwrap_or(&self.uri);
        urldecode(tail)
    }

    /// `{%a - }{%t}|{%f}` (song_list_format without colours).
    pub fn classic(&self) -> String {
        if self.name.is_empty() {
            return self.filename();
        }
        if self.artists.is_empty() {
            self.name.clone()
        } else {
            format!("{} - {}", self.artists, self.name)
        }
    }

    /// `{%n - }{%t}|{%f}` (song_library_format).
    pub fn library(&self) -> String {
        if self.name.is_empty() {
            return self.filename();
        }
        match self.track_no {
            Some(n) => format!("{n} - {}", self.name),
            None => self.name.clone(),
        }
    }

    pub fn duration(&self) -> String {
        self.length.map(fmt_ms).unwrap_or_default()
    }
}

pub fn urldecode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// MPD::Song::ShowTime: m:ss or h:mm:ss.
pub fn fmt_ms(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// helpers.h ShowTime(long names): "1 day, 2 hours, 3 minutes, 4 seconds".
pub fn fmt_long(ms: u64) -> String {
    let mut s = ms / 1000;
    let mut parts = vec![];
    for (unit, one, many) in [
        (86400, "day", "days"),
        (3600, "hour", "hours"),
        (60, "minute", "minutes"),
        (1, "second", "seconds"),
    ] {
        let n = s / unit;
        if n > 0 {
            parts.push(format!("{n} {}", if n == 1 { one } else { many }));
            s -= n * unit;
        }
    }
    parts.join(", ")
}

#[derive(Clone, Debug)]
pub struct TlTrack {
    pub tlid: i64,
    pub track: Track,
}

impl TlTrack {
    pub fn from_value(v: &Value) -> Option<TlTrack> {
        Some(TlTrack {
            tlid: v.get("tlid")?.as_i64()?,
            track: Track::from_value(v.get("track")?)?,
        })
    }
    pub fn list(v: &Value) -> Vec<TlTrack> {
        v.as_array()
            .map(|a| a.iter().filter_map(TlTrack::from_value).collect())
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Ref {
    #[serde(rename = "type")]
    pub kind: String,
    pub uri: String,
    pub name: String,
}

impl Ref {
    pub fn is_track(&self) -> bool {
        self.kind == "track"
    }
    pub fn is_playlist(&self) -> bool {
        self.kind == "playlist"
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Playlist {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    #[serde(default)]
    pub username: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PlaylistTrack {
    pub track_uri: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PlaylistWithTracks {
    pub tracks: Vec<PlaylistTrack>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_mopidy_track_and_formats() {
        let v: Value = serde_json::from_str(
            r#"{"__model__":"TlTrack","tlid":7,"track":{"uri":"tidal:track:1","name":"Kaipaus ()",
                "artists":[{"name":"Retuperän WBK"}],"album":{"name":"Valioita","date":"1999-01-01"},"track_no":3,"length":156000}}"#,
        )
        .unwrap();
        let t = TlTrack::from_value(&v).unwrap();
        assert_eq!(t.tlid, 7);
        assert_eq!(t.track.classic(), "Retuperän WBK - Kaipaus");
        assert_eq!(t.track.library(), "3 - Kaipaus");
        assert_eq!(t.track.duration(), "2:36");
        assert_eq!(fmt_ms(3_723_000), "1:02:03");
        assert_eq!(fmt_long(90_061_000), "1 day, 1 hour, 1 minute, 1 second");
        assert_eq!(fmt_long(120_000), "2 minutes");
        assert_eq!(Track::from_value(&Value::Null), None);
        assert_eq!(urldecode("Royal%20Chill.m3u8"), "Royal Chill.m3u8");
    }
}
