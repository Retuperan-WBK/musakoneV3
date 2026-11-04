//! Data shapes shared by the Mopidy JSON-RPC side and the Gleam REST side.
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub uri: String,
    pub name: String,
    pub artists: String,
    pub album: String,
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
        let length = v.get("length").and_then(Value::as_u64);
        Some(Track {
            uri,
            name,
            artists,
            album,
            length,
        })
    }

    /// "Artist - Title" like ncmpcpp's default song format.
    pub fn title(&self) -> String {
        if self.artists.is_empty() {
            self.name.clone()
        } else {
            format!("{} - {}", self.artists, self.name)
        }
    }

    pub fn duration(&self) -> String {
        self.length.map(fmt_ms).unwrap_or_default()
    }
}

pub fn fmt_ms(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
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

#[derive(Clone, Debug, Deserialize)]
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
}

#[derive(Clone, Debug, Deserialize)]
pub struct Playlist {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    #[serde(default)]
    pub is_public: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PlaylistTrack {
    pub track_uri: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PlaylistWithTracks {
    pub tracks: Vec<PlaylistTrack>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Recommendation {
    pub uri: String,
    pub name: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Recommendations {
    pub items: Vec<Recommendation>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AutoplaySettings {
    pub enabled: bool,
    pub min_ahead: i64,
    pub batch_size: i64,
    pub discovery: f64,
    pub mode: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AutoplayStatus {
    pub settings: AutoplaySettings,
    #[serde(default)]
    pub added_24h: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_mopidy_track_and_formats_time() {
        let v: Value = serde_json::from_str(
            &r#"{"__model__":"TlTrack","tlid":7,"track":{"uri":"tidal:track:1","name":"Kaipaus",
                "artists":[{"name":"Retuperän WBK"}],"album":{"name":"Valioita"},"length":156000}}"#
                .replace("Kaipaus", "Kaipaus ()"),
        )
        .unwrap();
        let t = TlTrack::from_value(&v).unwrap();
        assert_eq!(t.tlid, 7);
        assert_eq!(t.track.title(), "Retuperän WBK - Kaipaus");
        assert_eq!(t.track.duration(), "2:36");
        assert_eq!(fmt_ms(3_723_000), "1:02:03");
        assert_eq!(Track::from_value(&Value::Null), None);
    }
}
