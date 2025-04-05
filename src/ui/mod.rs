use std::path::PathBuf;

pub mod cli;
pub mod gui;

fn home_dir() -> Option<PathBuf> {
    #[allow(deprecated)]
    std::env::home_dir()
}

fn location(minecraft_path: Option<PathBuf>, default: &str) -> String {
    use std::env::current_dir;

    let path = if let Some(path) = minecraft_path {
        path
    } else {
        current_dir().ok().unwrap_or(PathBuf::from(default))
    };

    path.to_str().unwrap_or(default).to_owned()
}

#[cfg(any(unix))]
pub fn dot_minecraft_location() -> String {
    location(home_dir().map(|p| p.join(".minecraft")), "/")
}

#[cfg(target_os = "windows")]
pub fn dot_minecraft_location() -> String {
    let appdata = std::env::var("APPDATA").ok();
    location(appdata.map(|p| PathBuf::from(p).join(".minecraft")), r"C:\")
}

#[cfg(target_os = "macos")]
pub fn dot_minecraft_location() -> String {
    location(
        home_dir().map(|p| p.join("Libary/Application Support/minecraft")),
        default,
    )
}
