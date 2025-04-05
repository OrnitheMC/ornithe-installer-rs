use std::{io::Write, path::PathBuf};

use serde_json::{Value, json};
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{
    errors::InstallerError,
    net::{
        manifest::{self, MinecraftVersion},
        meta::{self, LoaderType, LoaderVersion},
    },
};

const INTERMEDIARY_PATCH: &str =
    include_str!("../../res/packformat/patches/net.fabricmc.intermediary.json");
const INSTANCE_CONFIG: &str = include_str!("../../res/packformat/instance.cfg");
const MMC_PACK: &str = include_str!("../../res/packformat/mmc-pack.json");

pub async fn install(
    version: MinecraftVersion,
    loader_type: LoaderType,
    loader_version: LoaderVersion,
    output_dir: PathBuf,
    copy_profile_path: bool,
) -> Result<(), InstallerError> {
    let version_id = version.get_id(&crate::net::GameSide::Client).await?;
    let intermediary_versions = meta::fetch_intermediary_versions().await?;
    let intermediary_version = intermediary_versions
        .get(&version_id)
        .ok_or(InstallerError(
            "Could not find matching intermediary version".to_owned(),
        ))?;

    let intermediary_maven = intermediary_version
        .maven
        .clone()
        .strip_suffix(&(":".to_owned() + &intermediary_version.version))
        .ok_or(InstallerError(
            "Failed to retrieve intermediary maven coordinates".to_owned(),
        ))?
        .to_owned();

    let lwjgl_version = manifest::find_lwjgl_version(&version).await?;

    let mut transformed_pack_json = serde_json::from_str::<Value>(
        &transform_pack_json(
            &version,
            &loader_type,
            &loader_version,
            &lwjgl_version,
            &intermediary_version.version,
        )
        .await?,
    )?;

    let transformed_intermediary_patch =
        transform_intermediary_patch(&version, &intermediary_version.version, &intermediary_maven)
            .await?;

    let minecraft_patch_json = get_mmc_launch_json(&version, &lwjgl_version).await?;
    let output_file = output_dir.join("Ornithe-".to_owned() + &version.id + ".zip");
    if std::fs::exists(&output_file).unwrap_or_default() {
        std::fs::remove_file(&output_file)?;
    }
    let file = std::fs::File::create_new(&output_file)?;
    let mut zip = ZipWriter::new(file);

    zip.start_file("instance.cfg", SimpleFileOptions::default())?;
    let mut instance_cfg = INSTANCE_CONFIG.replace("${mc_version}", &version.id);

    #[cfg(any(unix))]
    {
        instance_cfg += "\nOverrideCommands=true\nWrapperCommand=env __GL_THREADED_OPTIMIZATIONS=0";
    }

    zip.write_all(instance_cfg.as_bytes())?;

    zip.start_file("ornithe.png", SimpleFileOptions::default())?;
    zip.write_all(crate::ORNITHE_ICON_BYTES)?;

    zip.add_directory("patches", SimpleFileOptions::default())?;
    zip.start_file(
        "patches/net.fabricmc.intermediary.json",
        SimpleFileOptions::default(),
    )?;
    zip.write_all(transformed_intermediary_patch.as_bytes())?;

    zip.start_file("patches/net.minecraft.json", SimpleFileOptions::default())?;
    zip.write_all(minecraft_patch_json.as_bytes())?;

    let extra_libs = meta::fetch_profile_libraries(&version, &loader_type, &loader_version).await?;

    let pack_components = transformed_pack_json["components"].as_array_mut().unwrap();
    for library in extra_libs {
        let colons = library
            .name
            .char_indices()
            .filter(|c| c.1 == ':')
            .map(|c| c.0);
        let index = colons.clone().last().unwrap();
        let uid = library.name.get(0..index).unwrap().replace(":", ".");
        let lib_name = library
            .name
            .get((colons.clone().next().unwrap() + 1)..colons.clone().last().unwrap())
            .unwrap();
        let version = library.name.get(0..(colons.last().unwrap() + 1)).unwrap();
        zip.start_file(
            "patches/".to_owned() + &uid + ".json",
            SimpleFileOptions::default(),
        )?;
        zip.write_all(
            format!(r#"{{"formatVersion": 1, "libraries": [{{"name": "{}","url": "{}"}}], "name": "{}", "type": "release", "uid": "{}", "version": "{}"}}"#,
             library.name, library.url, lib_name, uid, version).as_bytes())?;

        pack_components.push(json!({
            "cachedName": lib_name,
            "cachedVersion": version,
            "uid": uid
        }));
    }

    zip.start_file("mmc-pack.json", SimpleFileOptions::default())?;
    zip.write_all(&serde_json::to_vec_pretty(&transformed_pack_json)?)?;

    zip.finish()?;

    if copy_profile_path {
        cli_clipboard::set_contents(output_file.to_string_lossy().into_owned())
            .map_err(|_| InstallerError("Failed to copy profile path".to_owned()))?;
    }

    Ok(())
}

async fn transform_intermediary_patch(
    version: &MinecraftVersion,
    intermediary_version: &String,
    intermediary_maven: &String,
) -> Result<String, InstallerError> {
    Ok(INTERMEDIARY_PATCH
        .replace("${mc_version}", &version.id)
        .replace("${intermediary_ver}", &intermediary_version)
        .replace("${intermediary_maven}", &intermediary_maven))
}

async fn transform_pack_json(
    version: &MinecraftVersion,
    loader_type: &LoaderType,
    loader_version: &LoaderVersion,
    lwjgl_version: &String,
    intermediary_version: &String,
) -> Result<String, InstallerError> {
    let lwjgl_major = lwjgl_version.chars().next().unwrap();
    Ok(MMC_PACK
        .replace("${mc_version}", &version.id)
        .replace("${intermediary_ver}", &intermediary_version)
        .replace("${loader_version}", &loader_version.version)
        .replace(
            "${loader_name}",
            &(loader_type.get_localized_name().to_owned() + " Loader"),
        )
        .replace("${loader_uid}", loader_type.get_maven_uid())
        .replace("${lwjgl_version}", &lwjgl_version)
        .replace("${lwjgl_major_ver}", &lwjgl_major.to_string())
        .replace(
            "${lwjgl_uid}",
            if lwjgl_major == '3' {
                "org.lwjgl3"
            } else {
                "org.lwjgl"
            },
        ))
}

async fn get_mmc_launch_json(
    version: &MinecraftVersion,
    lwjgl_version: &String,
) -> Result<String, InstallerError> {
    let client_name = format!("com.mojang:minecraft:{}:client", version.id);
    let vanilla_json = serde_json::from_str::<Value>(&manifest::fetch_launch_json(version).await?)?;

    let client = vanilla_json["downloads"]["client"].as_object().unwrap();

    let main_jar = json!({
        "downloads": {
            "artifact": client
        },
        "name": client_name
    });

    let mut libraries = vanilla_json["libraries"].clone();
    let vanilla_libraries = libraries.as_array_mut().unwrap();
    vanilla_libraries.retain(|lib| {
        let name = lib["name"].as_str().unwrap_or_default();
        !name.contains("org.ow2.asm") && !name.contains("org.lwjgl")
    });

    let mut traits = Vec::new();

    if vanilla_json["mainClass"]
        .as_str()
        .unwrap_or_default()
        .contains("launchwrapper")
    {
        traits.push("texturepacks");
    }

    let mut minecraft_arguments = vanilla_json["minecraftArguments"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    if let Some(game_arguments) = vanilla_json["arguments"]["game"].as_array() {
        if !game_arguments.is_empty() {
            let mut combined = String::new();
            for arg in game_arguments {
                if arg.is_string() {
                    combined += &(arg.as_str().unwrap().to_owned() + " ");
                }
            }
            minecraft_arguments = combined.trim().to_owned();

            traits.push("FirstThreadOnMaxOs");
        }
    }

    let lwjgl_major = lwjgl_version.chars().next().unwrap();
    let mut json = json!({
        "assetIndex": vanilla_json["assetIndex"],
        "compatibleJavaMajors": [8, 17, 21],
        "formatVersion":1,
        "libraries": vanilla_libraries,
        "mainClass": vanilla_json["mainClass"],
        "mainJar": main_jar,
        "minecraftArguments": minecraft_arguments,
        "name":"Minecraft",
        "releaseTime": vanilla_json["releaseTime"],
        "requires": [{
            "suggests": lwjgl_version,
            "uid": if lwjgl_major == '3' {
                "org.lwjgl3"
            } else {
                "org.lwjgl"
            }
        }],
        "type":vanilla_json["type"],
        "uid":"net.minecraft",
        "version": &version.id
    });

    if !traits.is_empty() {
        json.as_object_mut()
            .unwrap()
            .insert("+traits".to_owned(), json!(traits));
    }

    Ok(serde_json::to_string_pretty(&json)?)
}
