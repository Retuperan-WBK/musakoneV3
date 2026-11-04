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

/// Replace the running binary with the published build. Ok(None) when already current.
pub async fn apply() -> Result<Option<String>> {
    let latest = latest_sha().await?;
    if BUILT_SHA == Some(latest.as_str()) {
        return Ok(None);
    }
    let asset = asset_name()?;
    let url = format!("https://github.com/{REPO}/releases/download/tui-latest/{asset}");
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
    Ok(Some(latest))
}

/// Startup hook: CI builds pull the newest build in the background; dev builds do nothing.
pub async fn auto() -> Option<String> {
    BUILT_SHA?;
    apply().await.ok().flatten()
}

pub async fn run() -> Result<()> {
    match apply().await? {
        None => println!(
            "musakone is up to date ({})",
            BUILT_SHA.map(|s| &s[..7]).unwrap_or("dev")
        ),
        Some(sha) => println!(
            "updated {} to {}",
            std::env::current_exe()?.display(),
            &sha[..7]
        ),
    }
    Ok(())
}
