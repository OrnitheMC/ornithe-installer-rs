use std::path::PathBuf;

use serde::de::DeserializeOwned;

use crate::errors::InstallerError;

pub mod manifest;
pub mod maven;
pub mod meta;

static CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .build()
        .unwrap()
});

pub async fn download_file(url: &str, output: &PathBuf) -> Result<(), InstallerError> {
    let bytes = get_bytes(url).await?;
    if let Some(parent) = output.parent() {
        if !std::fs::exists(parent)? {
            std::fs::create_dir_all(parent)?;
        }
    }
    if std::fs::exists(output).unwrap_or(false) {
        std::fs::remove_file(output)?;
    }
    std::fs::write(output, bytes)?;

    Ok(())
}

pub async fn get_json<T>(url: impl Into<String>) -> Result<T, InstallerError>
where
    T: DeserializeOwned,
{
    Ok(CLIENT.get(url.into()).send().await?.json::<T>().await?)
}

pub async fn get_text(url: impl Into<String>) -> Result<String, InstallerError> {
    Ok(CLIENT.get(url.into()).send().await?.text().await?)
}

pub async fn get_bytes(url: impl Into<String>) -> Result<Vec<u8>, InstallerError> {
    Ok(CLIENT.get(url.into()).send().await?.bytes().await?.to_vec())
}

pub enum GameSide {
    Client,
    Server,
}

impl GameSide {
    pub fn id(&self) -> &str {
        match self {
            GameSide::Client => "client",
            GameSide::Server => "server",
        }
    }

    pub fn other_side(&self) -> GameSide {
        match self {
            GameSide::Client => GameSide::Server,
            GameSide::Server => GameSide::Client,
        }
    }
}
