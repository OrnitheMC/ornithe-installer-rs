use std::{env, path::PathBuf, process::Command};

extern crate embed_resource;

fn main() {
    // Include our program resources for visual styling and DPI awareness on windows
    if cfg!(windows) {
        embed_resource::compile("res/windows/program.rc", embed_resource::NONE)
            .manifest_required()
            .unwrap();

        winres::WindowsResource::new()
            .set_icon("res/windows/icon.ico")
            .set("ProductName", "Ornithe Installer")
            .set("CompanyName", "The Ornithe Project")
            .set("LegalCopyright", "Apache License Version 2.0")
            .compile()
            .expect("Failed to set windows resources");
    }

    let proj_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut server_launcher = PathBuf::from(&proj_dir);
    server_launcher.push("ServerLauncher.jar");
    if env::var("CI").is_ok() || std::fs::exists(&server_launcher).unwrap_or(false) {
        let mut out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        out_dir.push("ServerLauncher.jar");

        std::fs::copy(server_launcher, out_dir)
            .expect("Copying should be succeed, need ServerLauncher to embed!");
    } else {
        Command::new(format!(
            "{}/{}",
            &proj_dir,
            if cfg!(windows) {
                "gradlew.bat"
            } else {
                "gradlew"
            }
        ))
        .arg(":java:assemble")
        .arg("--stacktrace")
        .arg("--no-daemon")
        .status()
        .expect("Gradle build should succeed");
    }
    println!("cargo::rerun-if-changed=java/build.gradle.kts");
    println!("cargo::rerun-if-changed=java/src");
    println!("cargo::rerun-if-changed=res/windows");
    println!("cargo::rerun-if-changed=locales/");
}
