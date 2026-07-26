use std::{collections::HashMap, path::PathBuf};

use crate::{
    errors::InstallerError,
    net::{GameSide, manifest::MinecraftVersion, meta::IntermediaryVersion},
};

pub mod cli;

#[cfg(all(feature = "gui", not(target_arch = "wasm32")))]
pub mod gui;
#[cfg(all(feature = "gui", target_arch = "wasm32"))]
pub mod gui_web;

#[cfg(feature = "gui")]
mod font_loader;

#[allow(unused)]
fn home_dir() -> Option<PathBuf> {
    #[allow(deprecated)]
    std::env::home_dir()
}

#[allow(unused)]
fn location(minecraft_path: Option<PathBuf>, default: &str) -> String {
    use std::env::current_dir;

    let path = if let Some(path) = minecraft_path {
        path
    } else {
        current_dir().ok().unwrap_or(PathBuf::from(default))
    };

    path.to_str().unwrap_or(default).to_owned()
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn dot_minecraft_location() -> String {
    let mc_dir = home_dir().map(|p| {
        let dot_mc = p.join(".minecraft");
        let flatpak_dot_mc = p.join(".var/app/com.mojang.Minecraft/.minecraft");
        if flatpak_dot_mc.exists() && !dot_mc.exists() {
            return flatpak_dot_mc;
        }
        dot_mc
    });
    location(mc_dir, "/")
}

#[cfg(windows)]
pub fn dot_minecraft_location() -> String {
    let appdata = std::env::var("APPDATA").ok();
    location(appdata.map(|p| PathBuf::from(p).join(".minecraft")), r"C:\")
}

#[cfg(target_os = "macos")]
pub fn dot_minecraft_location() -> String {
    location(
        home_dir().map(|p| p.join("Library/Application Support/minecraft")),
        "/",
    )
}

#[cfg(target_arch = "wasm32")]
pub fn dot_minecraft_location() -> String {
    ".".to_owned()
}

#[allow(unused)]
fn current_dir(default: &str) -> String {
    let fallback = home_dir().unwrap_or(PathBuf::from(default));
    std::env::current_dir()
        .ok()
        .unwrap_or(fallback)
        .to_str()
        .unwrap_or(default)
        .to_owned()
}

#[cfg(unix)]
pub fn current_location() -> String {
    current_dir("/")
}

#[cfg(windows)]
pub fn current_location() -> String {
    current_dir(r"C:\")
}

#[cfg(target_arch = "wasm32")]
pub fn current_location() -> String {
    ".".to_owned()
}

#[allow(unused)]
fn server_dir(default: &str) -> String {
    let fallback = home_dir().unwrap_or(PathBuf::from(default));
    std::env::current_dir()
        .ok()
        .unwrap_or(fallback)
        .join("server")
        .to_str()
        .unwrap_or(default)
        .to_owned()
}

#[cfg(unix)]
pub fn server_location() -> String {
    server_dir("/")
}

#[cfg(windows)]
pub fn server_location() -> String {
    server_dir(r"C:\")
}

#[cfg(target_arch = "wasm32")]
pub fn server_location() -> String {
    ".".to_owned()
}

fn get_intermediary_version(
    intermediary_versions: HashMap<String, IntermediaryVersion>,
    selected_version: &MinecraftVersion,
    side: GameSide,
) -> Result<IntermediaryVersion, InstallerError> {
    let ver = intermediary_versions.get(&selected_version.id);
    match side {
        GameSide::Client => {
            ver.or_else(|| intermediary_versions.get(&(selected_version.id.to_owned() + "-client")))
        }
        GameSide::Server => {
            ver.or_else(|| intermediary_versions.get(&(selected_version.id.to_owned() + "-server")))
        }
    }
    .cloned()
    .ok_or(InstallerError::from(t!(
        "gui.error.no_matching_intermediary_version",
        version = selected_version.id
    )))
}
