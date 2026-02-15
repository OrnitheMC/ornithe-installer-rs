use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::InstallerError;

use super::GameSide;

const META_URL: &str = "https://meta.ornithemc.net";

#[allow(dead_code)]
#[derive(Deserialize, Clone)]
pub struct LoaderVersion {
    pub version: String,
    stable: bool,
    maven: String,
    separator: String,
    build: i32,
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
}

impl GameSide {
    fn launch_json_endpoint(&self) -> &str {
        match self {
            GameSide::Client => "/v3/versions/{}-loader/{}/{}/profile/json",
            GameSide::Server => "/v3/versions/{}-loader/{}/{}/server/json",
        }
    }
    fn launch_json_endpoint_versioned(&self) -> &str {
        match self {
            GameSide::Client => "/v3/versions/{}/{}-loader/{}/{}/profile/json",
            GameSide::Server => "/v3/versions/{}/{}-loader/{}/{}/server/json",
        }
    }
}

pub async fn fetch_launch_json(
    side: GameSide,
    intermediary: &IntermediaryVersion,
    loader_type: &LoaderType,
    loader_version: &LoaderVersion,
    generation: &Option<u32>,
) -> Result<(String, Value), InstallerError> {
    let endpoint = match generation {
        Some(g) => &side
            .launch_json_endpoint_versioned()
            .replacen("{}", &format!("gen{}", g), 1),
        None => &side.launch_json_endpoint().to_string(),
    };
    let mut text = super::CLIENT
        .get(
            META_URL.to_owned()
                + &endpoint
                    .replacen("{}", loader_type.get_name(), 1)
                    .replacen("{}", &intermediary.version, 1)
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

    let library_upgrades = super::CLIENT
        .get(META_URL.to_owned() + &format!("/v3/versions/libraries/{}", &intermediary.version))
        .send()
        .await?
        .json::<Vec<ProfileJsonLibrary>>()
        .await?;

    if let Some(libraries) = text["libraries"].as_array_mut() {
        for lib in &mut *libraries {
            let lib_mut = lib.as_object_mut().unwrap();
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
        for upgrade in library_upgrades {
            libraries.push(serde_json::to_value(upgrade)?);
        }
    }
    Ok((version_id, text))
}

pub async fn fetch_loader_versions(
    generation: &Option<u32>,
) -> Result<HashMap<LoaderType, Vec<LoaderVersion>>, InstallerError> {
    let mut out = HashMap::new();
    for loader in [LoaderType::Fabric, LoaderType::Quilt] {
        let versions = fetch_loader_versions_type(generation, &loader).await?;
        out.insert(loader, versions);
    }
    Ok(out)
}

async fn fetch_loader_versions_type(
    generation: &Option<u32>,
    loader_type: &LoaderType,
) -> Result<Vec<LoaderVersion>, InstallerError> {
    let url = match generation {
        Some(g) => format!("/v3/versions/gen{}/", g),
        None => "/v3/versions/".to_owned(),
    } + match loader_type {
        LoaderType::Fabric => "fabric-loader",
        LoaderType::Quilt => "quilt-loader",
    };
    super::CLIENT
        .get(META_URL.to_owned() + &url)
        .send()
        .await?
        .json::<Vec<LoaderVersion>>()
        .await
        .map_err(|e| e.into())
}

#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct IntermediaryVersion {
    pub version: String,
    stable: bool,
    pub maven: String,
}

pub async fn fetch_intermediary_versions(
    generation: &Option<u32>,
) -> Result<HashMap<String, IntermediaryVersion>, InstallerError> {
    let url = match generation {
        Some(g) => format!("/v3/versions/gen{}/intermediary", g),
        None => "/v3/versions/intermediary".to_owned(),
    };
    let versions = super::CLIENT
        .get(META_URL.to_owned() + &url)
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

#[derive(Deserialize, Serialize)]
pub struct ProfileJsonLibrary {
    pub name: String,
    pub url: String,
}

pub async fn fetch_profile_libraries(
    generation: &Option<u32>,
    version: &str,
) -> Result<Vec<ProfileJsonLibrary>, InstallerError> {
    let url = match generation {
        Some(g) => format!("/v3/versions/gen{}/libraries/{}", g, version),
        None => format!("/v3/versions/libraries/{}", version),
    };
    let library_upgrades = super::CLIENT
        .get(META_URL.to_owned() + &url)
        .send()
        .await?
        .json::<Vec<ProfileJsonLibrary>>()
        .await?;

    Ok(library_upgrades)
}

#[derive(Deserialize)]
pub struct IntermediaryGenerations {
    #[serde(rename(deserialize = "latestIntermediaryGeneration"))]
    pub latest: u32,
    #[serde(rename(deserialize = "stableIntermediaryGeneration"))]
    pub stable: u32,
}

pub async fn fetch_intermediary_generations() -> Result<IntermediaryGenerations, InstallerError> {
    let generations = super::CLIENT
        .get(META_URL.to_owned() + "/v3/versions/intermediary_generations")
        .send()
        .await?
        .json::<IntermediaryGenerations>()
        .await?;
    Ok(generations)
}
