use serde::Deserialize;

use crate::{
    errors::InstallerError,
    net::{self, get_json},
};

pub const MAVEN_URL: &str = "https://maven.ornithemc.net/releases/";
const MAVEN_LATEST_VERSION_API_URL: &str =
    "https://maven.ornithemc.net/api/maven/latest/version/releases/net/ornithemc/";
const MAVEN_LATEST_RELEASE_API_URL: &str =
    "https://maven.ornithemc.net/api/maven/latest/file/releases/net/ornithemc/";

#[derive(Deserialize, Debug)]
pub struct MavenVersion {
    #[serde(rename(deserialize = "isSnapshot"))]
    #[allow(unused)]
    pub is_snapshot: bool,
    pub version: String,
}

pub async fn get_latest_version(artifact: &str) -> Result<MavenVersion, InstallerError> {
    get_json::<MavenVersion>(format!("{}{}", MAVEN_LATEST_VERSION_API_URL, artifact)).await
}

pub async fn get_latest_release_file(artifact: &str) -> Result<Vec<u8>, InstallerError> {
    net::get_bytes(&format!("{}{}", MAVEN_LATEST_RELEASE_API_URL, artifact)).await
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn download_latest_release(
    artifact: &str,
    output: &std::path::PathBuf,
) -> Result<(), InstallerError> {
    crate::net::download_file(
        &format!("{}{}", MAVEN_LATEST_RELEASE_API_URL, artifact),
        output,
    )
    .await
}
