use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::{errors::InstallerError, net::meta};

use super::GameSide;

const LAUNCHER_META_URL: &str = "https://ornithemc.net/mc-versions/version_manifest.json";
const LAUNCHER_META_URL_VERSIONED: &str =
    "https://ornithemc.net/mc-versions/{}/version_manifest.json";

pub async fn fetch_versions(generation: &Option<u32>) -> Result<VersionManifest, InstallerError> {
    let url = match generation {
        Some(g) => LAUNCHER_META_URL_VERSIONED.replacen("{}", &format!("gen{}", g), 1),
        None => LAUNCHER_META_URL.to_string(),
    };
    #[cfg(target_arch = "wasm32")]
    return super::get_json_client::<VersionManifest>(&super::UNCONFIGURED_CLIENT, url).await;
    #[cfg(not(target_arch = "wasm32"))]
    return super::get_json::<VersionManifest>(url).await;
}

pub async fn vanilla_profile_name(
    version: &str,
    generation: &Option<u32>,
) -> Result<String, InstallerError> {
    let intermediary_gen = if let Some(g) = generation {
        g
    } else {
        &meta::fetch_intermediary_generations().await?.stable
    };
    Ok(format!("{}-gen{}", version, intermediary_gen))
}

pub async fn fetch_launch_json(
    version: &MinecraftVersion,
    generation: &Option<u32>,
) -> Result<(String, String), InstallerError> {
    #[cfg(target_arch = "wasm32")]
    let res = super::get_text_client(&super::UNCONFIGURED_CLIENT, &version.url).await;
    #[cfg(not(target_arch = "wasm32"))]
    let res = super::get_text(&version.url).await;
    let mut json = match res {
        Ok(j) => match serde_json::from_str::<Value>(&j) {
            Ok(v) => v,
            Err(e) => {
                return Err(InstallerError(format!("{}: {}", e, &j)));
            }
        },
        Err(e) => {
            return Err(e);
        }
    };

    if let Some(val) = json.as_object_mut() {
        let version_id = vanilla_profile_name(&version.id, generation).await?;
        val.insert("id".to_string(), Value::String(version_id.clone()));

        return Ok((version_id, serde_json::to_string(val)?));
    }
    Err(InstallerError::from(t!(
        "manifest.error.fetching_launch_json"
    )))
}

async fn fetch_version_details(
    version: &MinecraftVersion,
) -> Result<VersionDetails, InstallerError> {
    #[cfg(target_arch = "wasm32")]
    return super::get_json_client::<VersionDetails>(&super::UNCONFIGURED_CLIENT, &version.details)
        .await;
    #[cfg(not(target_arch = "wasm32"))]
    super::get_json::<VersionDetails>(&version.details).await
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<MinecraftVersion>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct LatestVersions {
    old_alpha: String,
    classic_server: String,
    alpha_server: String,
    old_beta: String,
    snapshot: String,
    release: String,
    pending: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Clone, Debug)]
pub struct MinecraftVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub _type: String,
    url: String,
    //pub time: DateTime<Utc>, // Not present for 1.2.4, 1.2.3, 1.2.2 and 1.2.1
    #[serde(rename = "releaseTime")]
    pub release_time: DateTime<Utc>,
    details: String,
}

impl MinecraftVersion {
    pub async fn get_jar_download_url(
        &self,
        side: &GameSide,
    ) -> Result<VersionDownload, InstallerError> {
        let downloads = fetch_version_details(self).await?.downloads;
        match side {
            GameSide::Client => downloads.client,
            GameSide::Server => downloads.server,
        }
        .ok_or(InstallerError::from(t!(
            "manifest.error.no_download_for_version",
            side = side.id()
        )))
    }

    pub fn is_snapshot(&self) -> bool {
        self._type == "snapshot"
    }

    pub fn is_historical(&self) -> bool {
        !self.is_release() && !self.is_snapshot() && self._type != "pending"
    }

    pub fn is_release(&self) -> bool {
        self._type == "release"
    }
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct VersionDetails {
    libraries: Option<Value>,
    #[serde(rename(deserialize = "normalizedVersion"))]
    normalized_version: String,
    downloads: VersionDownloads,
}

#[derive(Deserialize, Debug)]
struct VersionDownloads {
    client: Option<VersionDownload>,
    server: Option<VersionDownload>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct VersionDownload {
    pub sha1: String,
    pub size: u32,
    pub url: String,
}

pub async fn find_lwjgl_url_version(
    version: &MinecraftVersion,
) -> Result<(String, String), InstallerError> {
    #[cfg(target_arch = "wasm32")]
    let details =
        super::get_json_client::<Value>(&super::UNCONFIGURED_CLIENT, &version.url).await?;
    #[cfg(not(target_arch = "wasm32"))]
    let details = super::get_json::<Value>(&version.url).await?;

    if let Some(libraries) = details["libraries"].as_array() {
        for library in libraries {
            let name = &library["name"];
            if name.is_string() {
                let mut name = name.as_str().unwrap().split(":").skip(1);
                let artifact = name.next().unwrap();
                if artifact == "lwjgl" {
                    return Ok((
                        library["artifact"]["url"]
                            .as_str()
                            .unwrap_or("")
                            .to_string(),
                        name.next().unwrap().to_owned(),
                    ));
                }
            }
        }
    }

    Err(InstallerError::from(t!(
        "manifest.error.no_lwjgl",
        mc_version = &version.id
    )))
}
