use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use crate::errors::InstallerError;

use super::{GameSide, manifest::MinecraftVersion};

const META_URL: &str = "https://meta.ornithemc.net";

#[allow(dead_code)]
#[derive(Deserialize, Clone)]
pub struct LoaderVersion {
    pub version: String,
    stable: bool,
    maven: String,
    separator: String,
    build: i32,
    #[serde(rename(deserialize = "versionNoSide"))]
    version_no_side: String,
}

impl LoaderVersion {
    pub fn is_beta(&self) -> bool {
        self.version.contains("-")
    }

    pub fn is_stable(&self) -> bool {
        !self.is_beta()
    }
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub enum LoaderType {
    Fabric,
    Quilt,
}

impl LoaderType {
    pub fn get_name(&self) -> &str {
        match self {
            LoaderType::Fabric => "fabric",
            LoaderType::Quilt => "quilt",
        }
    }

    pub fn get_localized_name(&self) -> &str {
        match self {
            LoaderType::Fabric => "Fabric",
            LoaderType::Quilt => "Quilt",
        }
    }

    pub fn get_maven_uid(&self) -> &str {
        match self {
            LoaderType::Fabric => "net.fabricmc.fabric-loader",
            LoaderType::Quilt => "org.quiltmc.quilt-loader",
        }
    }

    pub fn get_maven_name_start(&self) -> &str {
        match self {
            LoaderType::Fabric => "net.fabricmc:fabric-loader",
            LoaderType::Quilt => "org.quiltmc:quilt-loader",
        }
    }
}

impl GameSide {
    fn launch_json_endpoint(&self) -> &str {
        match self {
            GameSide::Client => "/v3/versions/{}-loader/{}/{}/profile/json",
            GameSide::Server => "/v3/versions/{}-loader/{}/{}/server/json",
        }
    }
}

pub async fn fetch_launch_json(
    side: GameSide,
    version: &MinecraftVersion,
    loader_type: &LoaderType,
    loader_version: &LoaderVersion,
) -> Result<(String, Value), InstallerError> {
    let mut text = super::CLIENT
        .get(
            META_URL.to_owned()
                + &side
                    .launch_json_endpoint()
                    .replacen("{}", loader_type.get_name(), 1)
                    .replacen("{}", version.get_id(&side).await?.as_str(), 1)
                    .replacen("{}", &loader_version.version, 1),
        )
        .send()
        .await?
        .json::<Value>()
        .await?;
    let version_id = text["id"]
        .as_str()
        .ok_or(InstallerError(
            "Launch Json does not contain 'id' key!".to_string(),
        ))?
        .to_owned();
    if let Some(libraries) = text["libraries"].as_object_mut() {
        for lib in libraries {
            let lib_mut = lib.1.as_object_mut().unwrap();
            if let Some(name) = lib_mut.clone()["name"].as_str() {
                if name.starts_with("net.fabricmc:intermediary") {
                    lib_mut.insert(
                        "name".to_string(),
                        Value::String(name.replace(
                            "net.fabricmc:intermediary",
                            "net.ornithemc:calamus-intermediary",
                        )),
                    );
                    lib_mut.insert(
                        "url".to_string(),
                        Value::String("https://maven.ornithemc.net/releases".to_string()),
                    );
                }
                if name.starts_with("org.quiltmc:hashed") {
                    lib_mut.insert(
                        "name".to_string(),
                        Value::String(
                            name.replace(
                                "org.quiltmc:hashed",
                                "net.ornithemc:calamus-intermediary",
                            ),
                        ),
                    );
                    lib_mut.insert(
                        "url".to_string(),
                        Value::String("https://maven.ornithemc.net/releases".to_string()),
                    );
                }
            }
        }
    }
    Ok((version_id, text))
}

pub async fn fetch_loader_versions()
-> Result<HashMap<LoaderType, Vec<LoaderVersion>>, InstallerError> {
    let mut out = HashMap::new();
    for loader in [LoaderType::Fabric, LoaderType::Quilt] {
        let versions = fetch_loader_versions_type(&loader).await?;
        out.insert(loader, versions);
    }
    Ok(out)
}

async fn fetch_loader_versions_type(
    loader_type: &LoaderType,
) -> Result<Vec<LoaderVersion>, InstallerError> {
    let url = META_URL.to_owned()
        + "/v3/versions/"
        + match loader_type {
            LoaderType::Fabric => "fabric-loader",
            LoaderType::Quilt => "quilt-loader",
        };
    super::CLIENT
        .get(url)
        .send()
        .await?
        .json::<Vec<LoaderVersion>>()
        .await
        .map_err(|e| e.into())
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct IntermediaryVersion {
    pub version: String,
    stable: bool,
    pub maven: String,
    #[serde(rename(deserialize = "versionNoSide"))]
    pub version_no_side: String,
}

pub async fn fetch_intermediary_versions()
-> Result<HashMap<String, IntermediaryVersion>, InstallerError> {
    let versions = super::CLIENT
        .get(META_URL.to_owned() + "/v3/versions/intermediary")
        .send()
        .await?
        .json::<Vec<IntermediaryVersion>>()
        .await
        .map_err(|e| Into::<InstallerError>::into(e))?;
    let mut out = HashMap::with_capacity(versions.len());
    for ver in versions {
        out.insert(ver.version.clone(), ver);
    }
    Ok(out)
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ProfileJson {
    id: String,
    libraries: Vec<ProfileJsonLibrary>,
}

#[derive(Deserialize)]
pub struct ProfileJsonLibrary {
    pub name: String,
    pub url: String,
}

pub async fn fetch_profile_libraries(
    version: &IntermediaryVersion,
    loader_type: &LoaderType,
    loader_version: &LoaderVersion,
) -> Result<Vec<ProfileJsonLibrary>, InstallerError> {
    let profile = super::CLIENT
        .get(
            META_URL.to_owned()
                + &format!(
                    "/v3/versions/{}-loader/{}/{}/profile/json",
                    loader_type.get_name(),
                    version.version,
                    loader_version.version
                ),
        )
        .send()
        .await?
        .json::<ProfileJson>()
        .await?;

    let mut out = Vec::new();
    let mut loader_found = false;

    for lib in profile.libraries {
        if loader_found {
            out.push(lib);
            continue;
        }

        if lib.name.starts_with(loader_type.get_maven_name_start()) {
            loader_found = true;
        }
    }

    Ok(out)
}
