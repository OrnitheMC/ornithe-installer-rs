use std::{collections::HashMap, io::Write, path::PathBuf};

use clap::{ArgMatches, Command, arg, command, value_parser};
use log::info;

use crate::{
    errors::InstallerError,
    net::{
        GameSide,
        manifest::MinecraftVersion,
        meta::{IntermediaryVersion, LoaderType, LoaderVersion},
    },
};

#[derive(PartialEq, Eq)]
enum InstallationResult {
    Installed,
    NotInstalled,
}

pub async fn run() {
    let matches = command!()
        .arg_required_else_help(true)
        .name("Ornithe Installer")
        .subcommand(
            add_arguments(Command::new("client")
                .about("Client installation for the official launcher")
                .long_flag("client")
                .arg(
                    arg!(-d --dir <DIR> "Installation directory")
                        .default_value(super::dot_minecraft_location())
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    arg!(-p --"generate-profile" <VALUE> "Whether to generate a launch profile")
                    .default_value("true")
                        .value_parser(value_parser!(bool)),
                )),
        )
        .subcommand(
            add_arguments(Command::new("mmc")
                .visible_alias("prism")
                .long_flag("mmc")
                .visible_long_flag_alias("prism")
                .about("Generate an instance for MultiMC/PrismLauncher")
                .arg(
                    arg!(-d --dir <DIR> "Output directory")
                        .default_value(super::current_location())
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(arg!(-z --"generate-zip" <VALUE> "Whether to generate an instance zip instead of installing an instance into the directory")
                    .default_value("true").value_parser(value_parser!(bool)))
                .arg(arg!(-c --"copy-profile-path" <VALUE> "Whether to copy the path of the generated profile to the clipboard")
                    .default_value("false").value_parser(value_parser!(bool))
            .value_parser(value_parser!(bool)))),
        )
        .subcommand(
            add_arguments(Command::new("server")
                .about("Server installation")
                .long_flag("server")
                .arg(
                    arg!(-d --dir <DIR> "Installation directory")
                        .default_value(super::server_location())
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(arg!(--"download-minecraft" "Whether to download the minecraft server jar"))
                .subcommand(Command::new("run").about("Install and run the server")
                    .arg(arg!(--args <ARGS> "Java arguments to pass to the server (before the server jar)"))
                    .arg(arg!(--java <PATH> "The java binary to use to run the server").value_parser(value_parser!(PathBuf))
                )),
        ))
        .subcommand(
            Command::new("game-versions")
            .alias("minecraft-versions")
            .long_flag("list-game-versions")
            .long_flag_alias("list-minecraft-versions")
                .about("List supported game versions")
                .arg(arg!(-s --"show-snapshots" "Include snapshot versions"))
                .arg(arg!(--"show-historical" "Include historical versions")),
        )
        .subcommand(
            Command::new("loader-versions")
            .long_flag("list-loader-versions")
                .about("List available loader versions")
                .arg(arg!(-b --"show-betas" "Include beta versions"))
                .arg(arg!(--"loader-type" <TYPE> "Loader type to use")
                .default_value("fabric")
                .ignore_case(true)
                .value_parser(["fabric", "quilt"])),
        )
        .get_matches();

    match parse(matches).await {
        Ok(r) => {
            if r == InstallationResult::Installed {
                info!("Installation complete!");
                info!("Ornithe has been successfully installed.");
                info!(
                    "Most mods require that you also download the Ornithe Standard Libraries mod and place it in your mods folder."
                );
                info!("You can find it at {}", crate::OSL_MODRINTH_URL);
            }
        }
        Err(e) => {
            std::io::stderr()
                .write_all(("Failed to load Ornithe Installer CLI: ".to_owned() + &e.0).as_bytes())
                .expect("Failed to print error!");
        }
    }
}

async fn parse(matches: ArgMatches) -> Result<InstallationResult, InstallerError> {
    if let Some(matches) = matches.subcommand_matches("loader-versions") {
        let versions = crate::net::meta::fetch_loader_versions().await?;
        let loader_type = get_loader_type(matches)?;
        let betas = matches.get_flag("show-betas");

        let mut out = String::new();
        for version in versions.get(&loader_type).unwrap() {
            if betas || version.is_stable() {
                out += &(version.version.clone() + " ");
            }
        }
        writeln!(
            std::io::stdout(),
            "Latest {} Loader version: {}",
            loader_type.get_localized_name(),
            versions
                .get(&loader_type)
                .and_then(|list| list.get(0))
                .map(|v| v.version.clone())
                .unwrap_or("<not available>".to_owned())
        )?;
        writeln!(
            std::io::stdout(),
            "Available {} Loader versions:",
            loader_type.get_localized_name()
        )?;
        writeln!(std::io::stdout(), "{}", out)?;

        return Ok(InstallationResult::NotInstalled);
    }

    let minecraft_versions = crate::net::manifest::fetch_versions().await?;
    let intermediary_versions = crate::net::meta::fetch_intermediary_versions().await?;

    let mut available_minecraft_versions = Vec::new();

    for version in minecraft_versions.versions {
        if intermediary_versions.contains_key(&version.id)
            || intermediary_versions.contains_key(&(version.id.clone() + "-client"))
            || intermediary_versions.contains_key(&(version.id.clone() + "-server"))
        {
            available_minecraft_versions.push(version);
        }
    }

    if let Some(matches) = matches.subcommand_matches("game-versions") {
        let mut out = String::new();
        let snapshots = matches.get_flag("show-snapshots");
        let historical = matches.get_flag("show-historical");
        for version in available_minecraft_versions {
            let mut displayed = if snapshots && historical {
                true
            } else {
                version.is_release()
            };
            if !displayed && snapshots {
                displayed |= version.is_snapshot();
            }
            if !displayed && historical {
                displayed |= version.is_historical();
            }
            if displayed {
                out += &(version.id.clone() + " ");
            }
        }
        writeln!(std::io::stdout(), "Available Minecraft versions:\n")?;
        writeln!(std::io::stdout(), "{}", out)?;
        return Ok(InstallationResult::NotInstalled);
    }

    let loader_versions = crate::net::meta::fetch_loader_versions().await?;

    if let Some(matches) = matches.subcommand_matches("client") {
        let minecraft_version = get_minecraft_version(
            matches,
            available_minecraft_versions,
            intermediary_versions,
            GameSide::Client,
        )?;
        let loader_type = get_loader_type(matches)?;
        let loader_versions = loader_versions.get(&loader_type).unwrap();
        let loader_version = get_loader_version(matches, loader_versions)?;
        let location = matches.get_one::<PathBuf>("dir").unwrap().clone();
        let create_profile = matches.get_flag("generate-profile");
        crate::actions::client::install(
            minecraft_version,
            loader_type,
            loader_version,
            location,
            create_profile,
        )
        .await?;
        return Ok(InstallationResult::Installed);
    }

    if let Some(matches) = matches.subcommand_matches("server") {
        let minecraft_version = get_minecraft_version(
            matches,
            available_minecraft_versions,
            intermediary_versions,
            GameSide::Server,
        )?;
        let loader_type = get_loader_type(matches)?;
        let loader_versions = loader_versions.get(&loader_type).unwrap();
        let loader_version = get_loader_version(matches, loader_versions)?;
        let location = matches.get_one::<PathBuf>("dir").unwrap().clone();
        if let Some(matches) = matches.subcommand_matches("run") {
            let java = matches.get_one::<PathBuf>("java");
            let run_args = matches.get_one::<String>("args");
            crate::actions::server::install_and_run(
                minecraft_version,
                loader_type,
                loader_version,
                location,
                java,
                run_args.map(|s| s.split(" ")),
            )
            .await?;
            return Ok(InstallationResult::Installed);
        }
        crate::actions::server::install(
            minecraft_version,
            loader_type,
            loader_version,
            location,
            matches.get_flag("download-minecraft"),
        )
        .await?;
        return Ok(InstallationResult::Installed);
    }

    if let Some(matches) = matches.subcommand_matches("mmc") {
        let minecraft_version = get_minecraft_version(
            matches,
            available_minecraft_versions,
            intermediary_versions,
            GameSide::Client,
        )?;
        let loader_type = get_loader_type(matches)?;
        let loader_versions = loader_versions.get(&loader_type).unwrap();
        let loader_version = get_loader_version(matches, loader_versions)?;
        let output_dir = matches.get_one::<PathBuf>("dir").unwrap().clone();
        let copy_profile_path = matches
            .get_one::<bool>("copy-profile-path")
            .unwrap()
            .clone();
        let generate_zip = matches.get_one::<bool>("generate-zip").unwrap().clone();
        crate::actions::mmc_pack::install(
            minecraft_version,
            loader_type,
            loader_version,
            output_dir,
            copy_profile_path,
            generate_zip,
        )
        .await?;
        return Ok(InstallationResult::Installed);
    }

    Ok(InstallationResult::NotInstalled)
}

fn get_minecraft_version(
    matches: &ArgMatches,
    versions: Vec<MinecraftVersion>,
    intermediary_versions: HashMap<String, IntermediaryVersion>,
    side: GameSide,
) -> Result<MinecraftVersion, InstallerError> {
    let minecraft_version_arg = matches.get_one::<String>("minecraft-version").unwrap();

    for version in versions {
        if version.id == *minecraft_version_arg {
            if intermediary_versions.contains_key(&version.id)
                || intermediary_versions.contains_key(&(version.id.to_owned() + "-" + side.id()))
            {
                return Ok(version);
            } else if !intermediary_versions.contains_key(&version.id)
                && intermediary_versions
                    .contains_key(&(version.id.to_owned() + "-" + side.other_side().id()))
            {
                return Err(InstallerError(
                    "Cannot install ".to_owned()
                        + minecraft_version_arg
                        + " for the "
                        + side.id()
                        + "! This version is "
                        + side.other_side().id()
                        + "-only!",
                ));
            }
        }
    }
    Err(InstallerError(
        "Could not find Minecraft version ".to_owned()
            + minecraft_version_arg
            + " among supported versions!",
    ))
}

fn get_loader_type(matches: &ArgMatches) -> Result<LoaderType, InstallerError> {
    Ok(
        match matches.get_one::<String>("loader-type").unwrap().as_str() {
            "quilt" => crate::net::meta::LoaderType::Quilt,
            "fabric" => crate::net::meta::LoaderType::Fabric,
            &_ => {
                return Err(InstallerError("Unsupported loader type!".to_owned()));
            }
        },
    )
}

fn get_loader_version(
    matches: &ArgMatches,
    versions: &Vec<LoaderVersion>,
) -> Result<LoaderVersion, InstallerError> {
    let arg = matches.get_one::<String>("loader-version").unwrap();

    if *arg == "latest" {
        return versions.get(0).map(|v| v.clone()).ok_or(InstallerError(
            "Failed to find loader version in list".to_owned(),
        ));
    }

    for version in versions {
        if version.version == *arg {
            return Ok(version.clone());
        }
    }

    Err(InstallerError(
        "Could not find loader version: ".to_owned() + arg,
    ))
}

fn add_arguments(command: Command) -> Command {
    command
        .arg(arg!(-m --"minecraft-version" <VERSION> "Minecraft version to use").required(true))
        .arg(
            arg!(--"loader-type" <TYPE> "Loader type to use")
                .default_value("fabric")
                .ignore_case(true)
                .value_parser(["fabric", "quilt"]),
        )
        .arg(arg!(--"loader-version" <VERSION> "Loader version to use").default_value("latest"))
}
