use std::io::Cursor;
use std::path::Path;

use serde::Deserialize;

use crate::error::{AppError, AppResult};

pub const ZAPRET_REPO: &str = "Flowseal/zapret-discord-youtube";

pub const ZAPRET_MIRROR_TAG: &str = "1.9.9d";

const ZAPRET_MIRROR_URL: &str = "https://sourceforge.net/projects/flowseal.mirror/files/1.9.9d/zapret-discord-youtube-1.9.9d.zip/download";

const UA: &str = concat!("FN/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(UA)
        .build()
        .map_err(Into::into)
}

/// Latest release tag for a repo (used to detect whether an update exists).
pub async fn latest_tag(repo: &str) -> AppResult<String> {
    let rel = fetch_latest(repo).await?;
    Ok(rel.tag_name)
}

/// (tag, download url) of the first `.zip` asset on the latest release.
pub async fn latest_zip(repo: &str) -> AppResult<(String, String)> {
    let rel = fetch_latest(repo).await?;
    let asset = rel
        .assets
        .into_iter()
        .find(|a| a.name.to_lowercase().ends_with(".zip"))
        .ok_or_else(|| AppError::Msg(format!("в релизе {repo} нет .zip-архива")))?;
    Ok((rel.tag_name, asset.browser_download_url))
}

pub async fn download_latest_zapret() -> AppResult<(String, Vec<u8>)> {
    if let Ok((tag, url)) = latest_zip(ZAPRET_REPO).await {
        if let Ok(bytes) = download_bytes(&url).await {
            return Ok((tag, bytes));
        }
    }

    Ok((
        ZAPRET_MIRROR_TAG.into(),
        download_bytes(ZAPRET_MIRROR_URL).await?,
    ))
}

async fn fetch_latest(repo: &str) -> AppResult<Release> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let http = client()?;
    let resp = http
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if resp.status().is_success() {
        return Ok(resp.json::<Release>().await?);
    }

    // `releases/latest` ignores pre-releases and can also be unavailable while
    // the chronological list still works. Try the list before using mirrors.
    let list_url = format!("https://api.github.com/repos/{repo}/releases?per_page=20");
    let list_resp = http
        .get(list_url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if list_resp.status().is_success() {
        let releases = list_resp.json::<Vec<Release>>().await?;
        if let Some(release) = releases.into_iter().find(|r| !r.assets.is_empty()) {
            return Ok(release);
        }
    }

    Err(AppError::Http(format!(
        "GitHub API недоступен для {repo}; резервное зеркало также будет проверено"
    )))
}

/// Download an arbitrary URL into memory.
pub async fn download_bytes(url: &str) -> AppResult<Vec<u8>> {
    let resp = client()?.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(AppError::Http(format!(
            "загрузка вернула {}",
            resp.status()
        )));
    }
    Ok(resp.bytes().await?.to_vec())
}

/// Extract a zip archive (in memory) into `dest`, flattening a single
/// top-level folder if the archive wraps everything in one (common for
/// GitHub-generated archives).
pub fn extract_zip(bytes: &[u8], dest: &Path) -> AppResult<()> {
    let reader = Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| AppError::Msg(format!("zip: {e}")))?;

    let strip = single_root_prefix(&mut archive);

    std::fs::create_dir_all(dest)?;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| AppError::Msg(format!("zip: {e}")))?;
        // `enclosed_name` returns &Path or PathBuf depending on the zip
        // version; normalise to an owned PathBuf either way.
        let enclosed: std::path::PathBuf = match file.enclosed_name() {
            Some(n) => n.to_path_buf(),
            None => continue, // skip unsafe / absolute paths
        };
        let rel = match &strip {
            Some(prefix) => enclosed
                .strip_prefix(prefix)
                .map(|p| p.to_path_buf())
                .unwrap_or(enclosed),
            None => enclosed,
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(rel);
        if file.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut writer = std::fs::File::create(&out)?;
            std::io::copy(&mut file, &mut writer)?;
        }
    }
    Ok(())
}

/// If every entry lives under the same first path segment, return it so we can
/// strip it during extraction.
fn single_root_prefix(archive: &mut zip::ZipArchive<Cursor<&[u8]>>) -> Option<std::path::PathBuf> {
    let mut root: Option<std::path::PathBuf> = None;
    for i in 0..archive.len() {
        let file = archive.by_index(i).ok()?;
        let name = file.enclosed_name()?.to_path_buf();
        let first = name.components().next()?;
        let first = std::path::PathBuf::from(first.as_os_str());
        match &root {
            None => root = Some(first),
            Some(r) if *r == first => {}
            Some(_) => return None, // more than one top-level entry
        }
    }
    root
}
