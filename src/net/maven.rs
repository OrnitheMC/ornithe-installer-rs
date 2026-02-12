use std::path::PathBuf;

use serde::Deserialize;

use crate::{
    errors::InstallerError,
    net::{CLIENT, download_file},
};

pub const MAVEN_URL: &str = "https://maven.ornithemc.net/releases/";
const MAVEN_LATEST_VERSION_API_URL: &str =
    "https://maven.ornithemc.net/api/maven/latest/version/releases/net/ornithemc/";
const MAVEN_LATEST_RELEASE_API_URL: &str =
    "https://maven.ornithemc.net/api/maven/latest/file/releases/net/ornithemc/";

#[derive(Deserialize)]
pub struct MavenVersion {
    #[serde(rename(deserialize = "isSnapshot"))]
    pub is_snapshot: bool,
    pub version: String,
}

pub async fn get_latest_version(artifact: &str) -> Result<MavenVersion, InstallerError> {
    Ok(CLIENT
        .get(format!("{}{}", MAVEN_LATEST_VERSION_API_URL, artifact))
        .send()
        .await?
        .json::<MavenVersion>()
        .await?)
}

pub async fn download_latest_release(
    artifact: &str,
    output: &PathBuf,
) -> Result<(), InstallerError> {
    download_file(
        &format!("{}{}", MAVEN_LATEST_RELEASE_API_URL, artifact),
        output,
    )
    .await
}
