use std::{fs::File, io::Write, path::PathBuf};

use arboard::Clipboard;
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{
    errors::InstallerError,
    net::{
        GameSide,
        manifest::{self, MinecraftVersion},
        maven::{self, MavenVersion},
        meta::{self, IntermediaryVersion, LoaderType, LoaderVersion},
    },
};

const INTERMEDIARY_PATCH: &str =
    include_str!("../../res/packformat/patches/net.fabricmc.intermediary.json");
const INSTANCE_CONFIG: &str = include_str!("../../res/packformat/instance.cfg");
const MMC_PACK: &str = include_str!("../../res/packformat/mmc-pack.json");

pub async fn install(
    sender: UnboundedSender<(f32, String)>,
    version: MinecraftVersion,
    intermediary_version: IntermediaryVersion,
    loader_type: LoaderType,
    loader_version: LoaderVersion,
    output_dir: PathBuf,
    copy_profile_path: bool,
    generate_zip: bool,
    generation: Option<u32>,
) -> Result<(), InstallerError> {
    let _ = sender.send((
        0.1,
        t!(
            "mmc.info.starting_installation",
            version = version.id,
            loader = loader_type.get_localized_name(),
            loader_version = loader_version.version,
            destination = output_dir.display()
        )
        .into(),
    ));
    if !output_dir.exists() {
        std::fs::create_dir_all(&output_dir)?;
    }
    let output_dir = output_dir.canonicalize()?;

    let _ = sender.send((0.2, t!("mmc.info.fetching_version_information").into()));
    let intermediary_maven = intermediary_version
        .maven
        .clone()
        .strip_suffix(&(":".to_owned() + &intermediary_version.version))
        .ok_or(InstallerError::from(t!(
            "mmc.error.failed_to_retrieve_intermediary_coordinates"
        )))?
        .to_owned();

    let (lwjgl_url, lwjgl_version) = manifest::find_lwjgl_url_version(&version).await?;

    let calamus_gen = match generation {
        Some(g) => g,
        None => meta::fetch_intermediary_generations().await?.stable,
    };

    let _ = sender.send((0.4, t!("mmc.info.transforming_templates").into()));

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

    let (_, ornithe_launch_json) = meta::fetch_launch_json(
        GameSide::Client,
        &intermediary_version,
        &loader_type,
        &loader_version,
        &generation,
    )
    .await?;
    let minecraft_patch_json =
        get_mmc_launch_json(&version, &lwjgl_version, &ornithe_launch_json).await?;

    let profile_name = format!(
        "Ornithe Gen{calamus_gen} {} {}",
        loader_type.get_localized_name(),
        version.id
    );
    let output_file = if generate_zip {
        output_dir.join(profile_name.clone() + ".zip")
    } else {
        let dir = output_dir.join(profile_name.clone());
        if std::fs::exists(&dir).unwrap_or_default() {
            return Err(InstallerError::from(t!(
                "mmc.error.instance_already_exists"
            )));
        }
        std::fs::create_dir_all(&dir)?;
        dir
    };

    let _ = sender.send((0.5, t!("mmc.info.fetching_library_information").into()));

    let MavenVersion {
        version: flap_version,
        ..
    } = maven::get_latest_version("flap").await?;

    let extra_libs = meta::fetch_profile_libraries(&generation, &version.id).await?;
    let _ = sender.send((
        0.6,
        t!(
            "mmc.info.found_library_upgrades",
            num_libraries = extra_libs.len()
        )
        .into(),
    ));

    let mut zip: Box<dyn Writer> = if generate_zip {
        let _ = sender.send((0.65, t!("mmc.info.generating_instance_zip").into()));

        if std::fs::exists(&output_file).unwrap_or_default() {
            std::fs::remove_file(&output_file)?;
        }
        let file = std::fs::File::create_new(&output_file)?;
        Box::new(ZipWriter::new(file))
    } else {
        let _ = sender.send((0.65, t!("mmc.info.generating_output_files").into()));

        Box::new(output_file.clone())
    };

    let mut instance_cfg = INSTANCE_CONFIG.replace("${profile_name}", &profile_name);

    if cfg!(all(any(unix), not(target_os = "macos"))) {
        instance_cfg += "\nOverrideCommands=true\nWrapperCommand=env __GL_THREADED_OPTIMIZATIONS=0";
    }

    zip.write_file("instance.cfg", instance_cfg.as_bytes())?;

    zip.write_file("ornithe.png", crate::ORNITHE_ICON_BYTES)?;

    zip.create_dir("patches")?;

    zip.write_file(
        "patches/net.fabricmc.intermediary.json",
        transformed_intermediary_patch.as_bytes(),
    )?;

    zip.write_file(
        "patches/net.minecraft.json",
        minecraft_patch_json.as_bytes(),
    )?;

    let pack_components = transformed_pack_json["components"].as_array_mut().unwrap();
    let _ = sender.send((0.75, t!("mmc.info.adding_library_components").into()));
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
        let version = library.name.get((colons.last().unwrap() + 1)..).unwrap();
        zip.write_file(&("patches/".to_owned() + &uid + ".json"), 
            format!(r#"{{"formatVersion": 1, "libraries": [{{"name": "{}","url": "{}"}}], "name": "{}", "type": "release", "uid": "{}", "version": "{}"}}"#,
             library.name, library.url, lib_name, uid, version).as_bytes())?;

        pack_components.push(json!({
            "cachedName": lib_name,
            "cachedVersion": version,
            "uid": uid
        }));
    }

    if !lwjgl_url.starts_with("https://libraries.minecraft.net") {
        let lwjgl_major = lwjgl_version.chars().next().unwrap();
        let uid = "org.".to_string()
            + if lwjgl_major == '3' {
                "lwjgl3"
            } else {
                "lwjgl"
            };
        zip.write_file(
            &format!("patches/{}.json", &uid),
            serde_json::to_string(&json!({
                "formatVersion": 1,
                "name": "LWJGL ".to_string()+&lwjgl_major.to_string(),
                "type": "release",
                "uid": &uid,
                "version": &lwjgl_version
            }))?
            .as_bytes(),
        )?;
    }

    zip.write_file(
        "patches/net.ornithemc.flap.json",
        serde_json::to_string(&json!({
            "formatVersion": 1,
            "name": "Flap",
            "type": "release",
            "uid": "net.ornithemc.flap",
            "version": flap_version,
            "+agents": [{
                "name": format!("net.ornithemc:flap:{}", flap_version),
                "url": maven::MAVEN_URL
            }]
        }))?
        .as_bytes(),
    )?;
    pack_components.push(json!({
        "cachedName": "Flap",
        "cachedVersion": flap_version,
        "uid": "net.ornithemc.flap"
    }));

    zip.write_file(
        "mmc-pack.json",
        &serde_json::to_vec_pretty(&transformed_pack_json)?,
    )?;

    #[cfg(all(
        any(unix, target_os = "windows"),
        not(any(target_os = "android", target_os = "emscripten"))
    ))]
    {
        if copy_profile_path {
            Clipboard::new()
                .and_then(|mut cp| cp.set().text(output_file.to_string_lossy().into_owned()))
                .map_err(|_| InstallerError::from(t!("mmc.error.failed_to_copy_path")))?;
        }
    }

    let _ = sender.send((1.0, t!("mmc.info.done").into()));

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
    ornithe_launch_json: &Value,
) -> Result<String, InstallerError> {
    let client_name = format!("com.mojang:minecraft:{}:client", version.id);
    let (_, vanilla_launch_json) = manifest::fetch_launch_json(version).await?;
    let vanilla_json = serde_json::from_str::<Value>(&vanilla_launch_json)?;

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
        !name.contains("org.ow2.asm")
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

            traits.push("FirstThreadOnMacOs");
        }
    }

    let lwjgl_major = lwjgl_version.chars().next().unwrap();
    let mut json = json!({
        "assetIndex": vanilla_json["assetIndex"],
        "compatibleJavaMajors": [25, 21, 17, 8],
        "compatibleJavaName": "java-runtime-epsilon",
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

    if let Some(jvm_arguments) = ornithe_launch_json["arguments"]["jvm"].as_array() {
        json.as_object_mut()
            .unwrap()
            .insert("+jvmArgs".to_owned(), json!(jvm_arguments));
    }

    Ok(serde_json::to_string_pretty(&json)?)
}

trait Writer {
    fn write_file(&mut self, path: &str, buf: &[u8]) -> Result<(), InstallerError>;

    fn create_dir(&mut self, path: &str) -> Result<(), InstallerError>;
}

impl Writer for PathBuf {
    fn write_file(&mut self, path: &str, buf: &[u8]) -> Result<(), InstallerError> {
        let new_file = self.join(path);
        let mut file = std::fs::File::create(new_file)?;
        file.write_all(buf)?;
        Ok(())
    }

    fn create_dir(&mut self, path: &str) -> Result<(), InstallerError> {
        let new_file = self.join(path);
        std::fs::create_dir_all(new_file)?;
        Ok(())
    }
}

impl Writer for ZipWriter<File> {
    fn write_file(&mut self, path: &str, buf: &[u8]) -> Result<(), InstallerError> {
        self.start_file(path, SimpleFileOptions::default())?;
        Ok(self.write_all(buf)?)
    }

    fn create_dir(&mut self, path: &str) -> Result<(), InstallerError> {
        Ok(self.add_directory(path, SimpleFileOptions::default())?)
    }
}
