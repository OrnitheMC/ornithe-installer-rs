use std::{path::PathBuf, sync::LazyLock};

use reqwest::Client;

use crate::errors::InstallerError;

pub mod manifest;
pub mod maven;
pub mod meta;

static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .user_agent(crate::USER_AGENT)
        .build()
        .unwrap()
});

pub async fn download_file(url: &str, output: &PathBuf) -> Result<(), InstallerError> {
    let bytes = CLIENT.get(url).send().await?.bytes().await?;
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
