use std::path::PathBuf;

use base64::{Engine, prelude::BASE64_STANDARD_NO_PAD};
use chrono::Utc;
use log::info;
use serde_json::{Value, json};

use crate::{
    errors::InstallerError,
    net::{
        manifest::{self, MinecraftVersion},
        meta::{self, LoaderType, LoaderVersion},
    },
};

pub async fn install(
    version: MinecraftVersion,
    loader_type: LoaderType,
    loader_version: LoaderVersion,
    location: PathBuf,
    create_profile: bool,
) -> Result<(), InstallerError> {
    if !location.exists() {
        std::fs::create_dir_all(&location)?;
    }
    let location = location.canonicalize()?;
    info!(
        "Installing Minecraft client at {}",
        location.to_str().unwrap_or("<not representable>")
    );

    info!("Fetching launch jsons..");
    let vanilla_launch_json = manifest::fetch_launch_json(&version).await?;

    let (profile_name, ornithe_launch_json) = meta::fetch_launch_json(
        crate::net::GameSide::Client,
        &version,
        &loader_type,
        &loader_version,
    )
    .await?;

    info!("Setting up destination..");

    let vanilla_profile_name = version.id.to_string() + "-vanilla";

    let versions_dir = location.join("versions");
    let vanilla_profile_dir = versions_dir.join(&vanilla_profile_name);
    let vanilla_profile_json = vanilla_profile_dir.join(vanilla_profile_name.clone() + ".json");
    let profile_dir = versions_dir.join(&profile_name);
    let profile_json = profile_dir.join(profile_name.clone() + ".json");

    if std::fs::exists(&vanilla_profile_dir).unwrap_or_default() {
        std::fs::remove_dir_all(&vanilla_profile_dir)?;
    }
    if std::fs::exists(&profile_dir).unwrap_or_default() {
        std::fs::remove_dir_all(&profile_dir)?;
    }

    info!("Creating files..");

    create_empty_jar(&vanilla_profile_dir, &vanilla_profile_name)?;
    create_empty_jar(&profile_dir, &profile_name)?;

    std::fs::write(vanilla_profile_json, vanilla_launch_json)?;
    std::fs::write(
        profile_json,
        serde_json::to_string_pretty(&ornithe_launch_json)?,
    )?;

    if create_profile {
        update_profiles(location, profile_name, version, loader_type)?;
    }

    Ok(())
}

fn create_empty_jar(dir: &PathBuf, name: &String) -> Result<(), InstallerError> {
    std::fs::create_dir_all(dir)?;
    std::fs::File::create(dir.join(name.clone() + ".jar"))?;
    Ok(())
}

fn update_profiles(
    game_dir: PathBuf,
    name: String,
    version: MinecraftVersion,
    loader_type: LoaderType,
) -> Result<(), InstallerError> {
    let launcher_profiles_path = game_dir.join("launcher_profiles.json");

    match std::fs::read_to_string(launcher_profiles_path.clone()) {
        Ok(launcher_profiles) => match serde_json::from_str::<Value>(&launcher_profiles) {
            Ok(mut json) => {
                let raw_profiles = json.as_object_mut().unwrap().get_mut("profiles").unwrap();
                if !raw_profiles.is_object() {
                    return Err(InstallerError(
                        "\"profiles\" field must be an object".to_string(),
                    ));
                }
                let profiles = raw_profiles.as_object_mut().unwrap();

                let new_profile_name =
                    "Ornithe (".to_owned() + loader_type.get_localized_name() + ") " + &version.id;

                if profiles.contains_key(&new_profile_name) {
                    let raw_profile = profiles.get_mut(&new_profile_name).unwrap();
                    if !raw_profile.is_object() {
                        return Err(InstallerError(format!(
                            "Cannot update profile of name {new_profile_name} because it is not an object!"
                        )));
                    }

                    raw_profile
                        .as_object_mut()
                        .unwrap()
                        .insert("lastVersionId".to_string(), Value::String(name));
                } else {
                    let profile = json!({
                        "name": new_profile_name,
                        "type":"custom",
                        "created": Utc::now(),
                        "lastUsed": Utc::now(),
                        "icon": get_icon_string(),
                        "lastVersionId": name
                    });
                    profiles.insert(new_profile_name, profile);
                }

                std::fs::write(&launcher_profiles_path, serde_json::to_string(&json)?)?;

                Ok(())
            }
            Err(_) => Err(InstallerError(
                "Failed to parse launcher_profiles.json json".to_string(),
            )),
        },
        Err(_) => Err(InstallerError(
            "Failed to read launcher_profiles.json".to_string(),
        )),
    }
}

fn get_icon_string() -> String {
    let base64 = BASE64_STANDARD_NO_PAD.encode(crate::ORNITHE_ICON_BYTES);
    "data:image/png;base64,".to_string() + &base64
}
