//! Self-update from the rolling `tui-latest` GitHub release built by CI.
use anyhow::{anyhow, bail, Result};
use serde_json::Value;

pub const REPO: &str = "Retuperan-WBK/musakoneV3";
pub const BUILT_SHA: Option<&str> = option_env!("MUSAKONE_SHA");

fn asset_name() -> Result<String> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "macos",
        o => bail!("no prebuilt binary for {o}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        a => bail!("no prebuilt binary for {a}"),
    };
    Ok(format!("musakone-{os}-{arch}"))
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("musakone-tui")
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("http client")
}

/// Sha of the latest published build (first word of the release notes).
pub async fn latest_sha() -> Result<String> {
    let v: Value = client()
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/tags/tui-latest"
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    v.get("body")
        .and_then(Value::as_str)
        .and_then(|b| b.split_whitespace().nth(1))
        .map(String::from)
        .ok_or_else(|| anyhow!("release notes carry no sha"))
}

/// Some(sha) when a newer build than this binary is published.
pub async fn available() -> Option<String> {
    let built = BUILT_SHA?;
    let latest = latest_sha().await.ok()?;
    (latest != built).then_some(latest)
}

pub async fn run() -> Result<()> {
    let latest = latest_sha().await?;
    if BUILT_SHA == Some(latest.as_str()) {
        println!("musakone is up to date ({})", &latest[..7]);
        return Ok(());
    }
    let asset = asset_name()?;
    let url = format!("https://github.com/{REPO}/releases/download/tui-latest/{asset}");
    println!("downloading {url}");
    let bytes = client()
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let exe = std::env::current_exe()?;
    let tmp = exe.with_extension("new");
    std::fs::write(&tmp, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, &exe)?;
    println!("updated {} to {}", exe.display(), &latest[..7]);
    Ok(())
}
