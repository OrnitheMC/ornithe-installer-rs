use std::path::PathBuf;

use base64::{Engine, prelude::BASE64_STANDARD_NO_PAD};
use chrono::Utc;
use serde_json::{Map, Value, json};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    errors::InstallerError,
    net::{
        manifest::{self, MinecraftVersion},
        maven,
        meta::{self, IntermediaryVersion, LoaderType, LoaderVersion},
    },
};

pub async fn install(
    sender: UnboundedSender<(f32, String)>,
    version: MinecraftVersion,
    intermediary: IntermediaryVersion,
    loader_type: LoaderType,
    loader_version: LoaderVersion,
    generation: Option<u32>,
    location: PathBuf,
    create_profile: bool,
    include_flap: bool,
) -> Result<(), InstallerError> {
    #[cfg(not(target_arch = "wasm32"))]
    if !location.exists() {
        return Err(InstallerError::from(t!(
            "client.error.directory_does_not_exist",
            dir = location.to_string_lossy()
        )));
    }
    let message = if cfg!(target_arch = "wasm32") {
        t!(
            "client.info.installation_start_web",
            version = version.id,
            loader = loader_type.get_localized_name(),
            loader_version = loader_version.version
        )
    } else {
        t!(
            "client.info.installation_start",
            version = version.id,
            loader = loader_type.get_localized_name(),
            loader_version = loader_version.version,
            destination = location.display()
        )
    };
    let _ = sender.send((0.2, message.into()));

    let calamus_gen = match generation {
        Some(g) => g,
        None => meta::fetch_intermediary_generations().await?.stable,
    };

    let _ = sender.send((0.4, t!("client.info.fetching_launch_jsons").into()));
    let (vanilla_profile_name, vanilla_launch_json) =
        manifest::fetch_launch_json(&version, &generation).await?;

    let (profile_name, mut ornithe_launch_json) = meta::fetch_launch_json(
        crate::net::GameSide::Client,
        &intermediary,
        &loader_type,
        &loader_version,
        &generation,
    )
    .await?;

    let _ = sender.send((0.6, t!("client.info.setting_up_destination").into()));
    #[cfg(target_arch = "wasm32")]
    let location = PathBuf::new();

    let versions_dir = location.join("versions");
    let profile_dir = versions_dir.join(&profile_name);
    let flap_jar = profile_dir.join("flap.jar");

    let flap_jar_file = if include_flap {
        Some(maven::get_latest_release_file("flap").await?)
    } else {
        None
    };

    #[cfg(not(target_arch = "wasm32"))]
    {
        let vanilla_profile_dir = versions_dir.join(&vanilla_profile_name);
        let profile_dir = versions_dir.join(&profile_name);
        if std::fs::exists(&vanilla_profile_dir).unwrap_or_default() {
            std::fs::remove_dir_all(&vanilla_profile_dir)?;
        }
        if std::fs::exists(&profile_dir).unwrap_or_default() {
            std::fs::remove_dir_all(&profile_dir)?;
        }
    }

    #[cfg(target_arch = "wasm32")]
    let mut buf = std::io::Cursor::new(Vec::new());
    #[cfg(target_arch = "wasm32")]
    let mut writer: Box<dyn super::Writer> = Box::new(zip::ZipWriter::new(&mut buf));
    #[cfg(not(target_arch = "wasm32"))]
    let mut writer: Box<dyn super::Writer> = Box::new(versions_dir);

    if include_flap {
        writer.write_file(
            &format!("{}/flap.jar", profile_name),
            &flap_jar_file.unwrap(),
        )?;

        if let Some(obj) = ornithe_launch_json.as_object_mut() {
            if !obj.contains_key("arguments") {
                let a = Value::Object(Map::new());
                obj.insert("arguments".to_string(), a);
            };
            let arguments = obj.get_mut("arguments").unwrap();
            if let Some(args) = arguments.as_object_mut() {
                if !args.contains_key("jvm") {
                    args.insert("jvm".to_string(), Value::Array(Vec::new()));
                }
                let jvm_args = args.get_mut("jvm").unwrap().as_array_mut();
                if let Some(jvm) = jvm_args {
                    jvm.insert(
                        0,
                        json!(format!("-javaagent:{}", flap_jar.to_string_lossy())),
                    );
                }
            }
        }
    }

    let _ = sender.send((0.8, t!("client.info.creating_files").into()));

    writer.create_dir(&vanilla_profile_name)?;
    writer.create_dir(&profile_name)?;

    writer.write_file(
        &format!("{}/{}.json", vanilla_profile_name, vanilla_profile_name),
        vanilla_launch_json.as_bytes(),
    )?;
    writer.write_file(
        &format!("{}/{}.json", profile_name, profile_name),
        &serde_json::to_vec(&ornithe_launch_json)?,
    )?;

    #[cfg(target_arch = "wasm32")]
    {
        drop(writer);
        let name = profile_name.clone() + ".zip";
        wasm_bindgen_futures::spawn_local(async move {
            super::download_file(name, &buf.into_inner());
        });
    }

    if create_profile && cfg!(not(target_arch = "wasm32")) {
        update_profiles(location, profile_name, version, loader_type, calamus_gen)?;
    }

    let _ = sender.send((1.0, t!("client.info.done").into()));

    Ok(())
}

fn get_launcher_profiles_json(game_dir: PathBuf) -> Result<PathBuf, InstallerError> {
    let launcher_profiles_msstore = game_dir.join("launcher_profiles_microsoft_store.json");
    if launcher_profiles_msstore.exists() {
        return Ok(launcher_profiles_msstore);
    }
    let launcher_profiles = game_dir.join("launcher_profiles.json");
    if launcher_profiles.exists() {
        return Ok(launcher_profiles);
    }
    Err(InstallerError::from(t!(
        "client.error.could_not_find_launcher_profiles_json"
    )))
}

fn update_profiles(
    game_dir: PathBuf,
    name: String,
    version: MinecraftVersion,
    loader_type: LoaderType,
    calamus_gen: u32,
) -> Result<(), InstallerError> {
    let launcher_profiles_path = get_launcher_profiles_json(game_dir)?;

    let fn_json_error = || InstallerError::from(t!("client.error.invalid_launcher_profiles_json"));

    match std::fs::read_to_string(launcher_profiles_path.clone()) {
        Ok(launcher_profiles) => match serde_json::from_str::<Value>(&launcher_profiles) {
            Ok(mut json) => {
                let raw_profiles = json
                    .as_object_mut()
                    .ok_or_else(fn_json_error)?
                    .get_mut("profiles")
                    .ok_or_else(fn_json_error)?;
                if !raw_profiles.is_object() {
                    return Err(InstallerError::from(t!(
                        "client.error.profiles_not_an_object"
                    )));
                }
                let profiles = raw_profiles.as_object_mut().ok_or_else(fn_json_error)?;

                let new_profile_name = format!(
                    "Ornithe Gen{calamus_gen} {} {}",
                    loader_type.get_localized_name(),
                    version.id
                );

                if profiles.contains_key(&new_profile_name) {
                    let raw_profile = profiles
                        .get_mut(&new_profile_name)
                        .ok_or_else(fn_json_error)?;
                    if !raw_profile.is_object() {
                        return Err(InstallerError::from(t!(
                            "client.error.cannot_update_profile",
                            name = new_profile_name
                        )));
                    }

                    raw_profile
                        .as_object_mut()
                        .ok_or_else(fn_json_error)?
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
            Err(_) => Err(InstallerError::from(t!(
                "client.error.failed_to_parse_launcher_profiles_json"
            ))),
        },
        Err(_) => Err(InstallerError::from(t!(
            "client.error.failed_to_read_launcher_profiles_json"
        ))),
    }
}

fn get_icon_string() -> String {
    let base64 = BASE64_STANDARD_NO_PAD.encode(crate::ORNITHE_ICON_BYTES);
    "data:image/png;base64,".to_string() + &base64
}
