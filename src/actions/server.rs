use std::{
    ffi::OsStr,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use serde_json::json;
use tokio::{sync::mpsc::UnboundedSender, task::JoinSet};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const SERVER_LAUNCHER_JAR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ServerLauncher.jar"));

use crate::{
    errors::InstallerError,
    net::{
        manifest::MinecraftVersion,
        maven,
        meta::{IntermediaryVersion, LoaderType, LoaderVersion},
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
    install_server: bool,
) -> Result<(), InstallerError> {
    install_path(
        sender.clone(),
        &version,
        &intermediary,
        &loader_type,
        &loader_version,
        &generation,
        &location,
        install_server,
    )
    .await?;

    let _ = sender.send((
        1.0,
        format!(
            "Installed Ornithe Server for Minecraft {} using {} Loader {} to {}",
            &version.id,
            &loader_type.get_localized_name(),
            &loader_version.version,
            &location.to_str().unwrap_or_default()
        ),
    ));

    Ok(())
}

async fn install_path(
    sender: UnboundedSender<(f32, String)>,
    version: &MinecraftVersion,
    intermediary: &IntermediaryVersion,
    loader_type: &LoaderType,
    loader_version: &LoaderVersion,
    generation: &Option<u32>,
    location: &PathBuf,
    install_server: bool,
) -> Result<(), InstallerError> {
    if !location.exists() {
        std::fs::create_dir_all(&location)?;
    }
    let location = location.canonicalize()?;

    let _ = sender.send((
        0.1,
        format!(
            "Installing server for {} using {} Loader {} to {}",
            version.id,
            loader_type.get_localized_name(),
            loader_version.version,
            location.display()
        ),
    ));

    let clear_paths = [location.join(".fabric"), location.join(".quilt")];
    for path in clear_paths {
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
    }

    let (_, launch_json) = crate::net::meta::fetch_launch_json(
        crate::net::GameSide::Server,
        intermediary,
        loader_type,
        loader_version,
        generation,
    )
    .await?;

    let _ = sender.send((0.2, format!("Installing libraries")));

    if !launch_json.is_object() {
        return Err(InstallerError(
            "Cannot create server installation due to server endpoint returning wrong type."
                .to_owned(),
        ));
    }

    let mut main_class = "";
    let mut launch_main_class: String;

    match loader_type {
        LoaderType::Fabric => {
            main_class = &launch_json["mainClass"]
                .as_str()
                .ok_or(InstallerError("Could not find main class entry".to_owned()))?;
            launch_main_class = "net.fabricmc.loader.launch.server.FabricServerLauncher".to_owned();
        }
        LoaderType::Quilt => {
            launch_main_class = launch_json["launcherMainClass"]
                .as_str()
                .ok_or(InstallerError("Could not find main class entry".to_owned()))?
                .to_owned();
        }
    }

    let jvm_args = launch_json["arguments"]["jvm"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .map(|v| v.as_str())
        .filter(|opt| opt.is_some())
        .map(|opt| opt.unwrap().to_string())
        .collect::<Vec<_>>();

    let libraries = launch_json["libraries"]
        .as_array()
        .ok_or(InstallerError("No libraries were specified".to_owned()))?;

    let mut library_files = JoinSet::new();

    let mut fabric_loader_artifact = None;
    let library_dir = location.join("libraries");
    let mut lib_count = libraries.len();
    for library in libraries {
        let name = library["name"]
            .as_str()
            .ok_or(InstallerError("Library had no name!".to_owned()))?
            .to_owned();
        let url = library["url"]
            .as_str()
            .ok_or(InstallerError("Library had no url!".to_owned()))?
            .to_owned();

        if name.matches("net\\.fabricmc:fabric-loader:.*").count() > 0 {
            fabric_loader_artifact = Some(name.clone());
        }

        let dir = library_dir.clone();
        library_files.spawn(async move { download_library(&dir, name, url).await });
    }

    let flap_version = maven::get_latest_version("flap").await?;
    let flap_path = library_dir.join(format!(
        "net/ornithemc/flap/flap-{}.jar",
        flap_version.version
    ));
    {
        let out_path = flap_path.clone();
        library_files.spawn(async move {
            maven::download_latest_release("flap", &out_path).await?;
            Ok(out_path)
        });
        lib_count += 1;
    }

    let mut downloaded_library_files = Vec::new();
    while let Some(done) = library_files.join_next().await {
        match done {
            Ok(res) => match res {
                Ok(file) => {
                    let name = file
                        .file_name()
                        .map(|o| o.to_string_lossy().to_string())
                        .unwrap_or("??.jar".to_string());
                    downloaded_library_files.push(file);
                    let num = downloaded_library_files.len();
                    let _ = sender.send((
                        (num as f32 / lib_count as f32) / 2.0 + 0.2,
                        format!("Downloaded {}, {}/{}", name, num, lib_count),
                    ));
                }
                Err(e) => {
                    return Err(InstallerError(
                        "Failed to download library: ".to_owned() + &e.0,
                    ));
                }
            },
            Err(e) => {
                return Err(InstallerError(
                    "Failed to download libraries: ".to_owned() + &e.to_string(),
                ));
            }
        }
    }

    let _ = sender.send((
        0.8,
        format!("Downloaded {} libraries!", downloaded_library_files.len()),
    ));

    if let Some(loader) = fabric_loader_artifact {
        let lib = location.join("libraries").join(split_artifact(&loader));
        launch_main_class = read_jar_manifest_attribute(&lib, "Main-Class")?;
    }

    if !location.exists() {
        std::fs::create_dir_all(&location)?;
    }

    create_launch_jar(
        version,
        &location,
        loader_type,
        main_class,
        &launch_main_class,
        &downloaded_library_files,
        jvm_args,
        flap_path.strip_prefix(&location.canonicalize()?)?,
    )
    .await?;

    if install_server {
        let _ = sender.send((0.9, format!("Downloading server jar")));
        let url = version
            .get_jar_download_url(&crate::net::GameSide::Server)
            .await?;
        crate::net::download_file(&url.url, &location.join("server.jar")).await?;
    }

    Ok(())
}

async fn create_launch_jar(
    version: &MinecraftVersion,
    install_location: &PathBuf,
    loader_type: &LoaderType,
    main_class: &str,
    launch_main_class: &str,
    library_files: &Vec<PathBuf>,
    jvm_args: Vec<String>,
    flap_jar_path: &Path,
) -> Result<(), InstallerError> {
    let jar_out = install_location.join(loader_type.get_name().to_owned() + "-server-launch.jar");
    if jar_out.exists() {
        std::fs::remove_file(&jar_out)?;
    }

    let file = std::fs::File::create(jar_out)?;
    let mut zip = ZipWriter::new(file);
    let mut launch_jar = ZipArchive::new(Cursor::new(SERVER_LAUNCHER_JAR))?;
    let mut manifest = Vec::new();
    for i in 0..launch_jar.len() {
        let f = launch_jar.by_index_raw(i)?;
        match f.enclosed_name() {
            Some(p) if p == Path::new("META-INF/MANIFEST.MF") => (),
            _ => zip.raw_copy_file(f)?,
        }
    }

    let mut jar_manifest = launch_jar.by_path("META-INF/MANIFEST.MF")?;
    let mut mf = String::new();
    jar_manifest.read_to_string(&mut mf)?;
    write!(manifest, "{}", mf.replace("\n\r\n", "\n"))?;
    zip.start_file("META-INF/MANIFEST.MF", SimpleFileOptions::default())?;

    let mut class_path = String::from("Class-Path: ");
    for library in library_files {
        let relative = library.strip_prefix(install_location)?.to_str();
        if let Some(p) = relative {
            class_path += &(p.replace("\\", "/") + " ");
        }
    }

    writeln!(manifest, "{}\r", wrap_manifest_line(class_path.trim_end()))?;
    writeln!(
        manifest,
        "{}\r",
        wrap_manifest_line(&format!("Minecraft-Version: {}\r", version.id))
    )?;
    zip.write_all(&manifest)?;

    zip.start_file("ornithe-args.json", SimpleFileOptions::default())?;
    zip.write_all(&serde_json::to_vec(&json!({
        "flap_jar": flap_jar_path,
        "main_class": launch_main_class,
        "jvm_args": jvm_args
    }))?)?;

    if loader_type == &LoaderType::Fabric {
        zip.start_file(
            "fabric-server-launch.properties",
            SimpleFileOptions::default(),
        )?;
        zip.write_all(("launch.mainClass=".to_owned() + main_class + "\n").as_bytes())?;
    }

    zip.finish()?;

    Ok(())
}

fn wrap_manifest_line(line: &str) -> String {
    let mut res = String::new();
    let mut count = 0;
    for char in line.chars() {
        res += &char.to_string();
        count += 1;
        // Manifest lines are at at most 72 chars long
        if count == 72 {
            res += "\r\n ";
            count = 1;
        }
    }
    res
}

fn read_jar_manifest_attribute(
    jar_file: &PathBuf,
    attribute: &str,
) -> Result<String, InstallerError> {
    let attribute = &(attribute.to_owned() + ": ");
    let file = std::fs::File::open(jar_file)?;
    let mut zip = ZipArchive::new(file)?;

    let mut manifest = zip.by_name("META-INF/MANIFEST.MF")?;
    let mf_str = std::io::read_to_string(&mut manifest)?;
    let main_class_line = mf_str.split("\n").find(|line| line.starts_with(attribute));
    if let Some(line) = main_class_line {
        let class = line.strip_prefix(attribute);
        if let Some(name) = class {
            return Ok(name.trim_ascii().to_owned());
        }
    }

    Err(InstallerError(
        "Couldn't find '".to_owned() + attribute + "' attribute in jar manifest!",
    ))
}

async fn download_library(
    libraries_dir: &PathBuf,
    name: String,
    url: String,
) -> Result<PathBuf, InstallerError> {
    let split_artifact = split_artifact(&name);
    let file = libraries_dir.join(&split_artifact);
    let raw_url = url.to_owned() + &split_artifact;
    crate::net::download_file(&raw_url, &file).await?;

    Ok(file)
}

fn split_artifact(artifact: &str) -> String {
    let parts = artifact.splitn(3, ":").collect::<Vec<&str>>();
    let group = parts.get(0).unwrap().replace(".", "/");
    let name = parts.get(1).unwrap();
    let version = parts.get(2).unwrap();

    group + "/" + name + "/" + version + "/" + name + "-" + version + ".jar"
}

pub async fn install_and_run<I, S>(
    sender: UnboundedSender<(f32, String)>,
    version: MinecraftVersion,
    intermediary: IntermediaryVersion,
    loader_type: LoaderType,
    loader_version: LoaderVersion,
    generation: Option<u32>,
    location: PathBuf,
    java: Option<&PathBuf>,
    args: Option<I>,
) -> Result<bool, InstallerError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let launch_jar = location.join(loader_type.get_name().to_owned() + "-server-launch.jar");
    let mut needs_install = false;
    let _ = sender.send((0.0, format!("Checking for present server installation...")));
    if !launch_jar.exists() {
        needs_install = true;
    }

    if !needs_install {
        needs_install = read_jar_manifest_attribute(&launch_jar, "Minecraft-Version")
            .map(|v| v.trim_ascii() != version.id)
            .unwrap_or(true);
    }

    if needs_install {
        install_path(
            sender.clone(),
            &version,
            &intermediary,
            &loader_type,
            &loader_version,
            &generation,
            &location,
            true,
        )
        .await?;
    }

    let _ = sender.send((0.95, format!("Starting server...")));

    let mut java_binary = "java".to_owned();
    if let Some(arg) = java {
        if let Some(path) = arg.to_str() {
            java_binary = path.to_owned();
        }
    }
    let jar = launch_jar.canonicalize()?;

    let mut cmd = Command::new(java_binary);
    cmd.current_dir(location)
        .stdout(Stdio::inherit())
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(args) = args {
        cmd.args(args);
    }
    cmd.arg("-jar").arg(jar).arg("nogui");
    let mut child = cmd.spawn()?;
    tokio::spawn(async move {
        thread::sleep(Duration::from_millis(100));
        child.wait().unwrap();
    });

    Ok(needs_install)
}
