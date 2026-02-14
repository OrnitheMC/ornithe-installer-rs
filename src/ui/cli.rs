use std::{collections::HashMap, io::Write, path::PathBuf};

use clap::{ArgMatches, Command, arg, command, value_parser};
use indicatif::{ProgressBar, ProgressStyle};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

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
            add_gen_argument(Command::new("game-versions"))
            .alias("minecraft-versions")
            .long_flag("list-game-versions")
            .long_flag_alias("list-minecraft-versions")
            .long_about("List supported game versions.")
                .about("List supported game versions. Arguments: [--show-snapshots, --show-historical]")
                .arg(arg!(-s --"show-snapshots" "Include snapshot versions"))
                .arg(arg!(--"show-historical" "Include historical versions")),
        )
        .subcommand(
            Command::new("loader-versions")
            .long_flag("list-loader-versions")
            .long_about("List available loader versions.")
                .about("List available loader versions. Arguments: [--show-betas, --loader-type]")
                .arg(arg!(-b --"show-betas" "Include beta versions"))
                .arg(arg!(--"loader-type" <TYPE> "Loader type to use")
                .default_value("fabric")
                .ignore_case(true)
                .value_parser(["fabric", "quilt"])),
        )
        .subcommand(Command::new("intermediary-generations")
        .long_flag("intermediary-generations")
        .about("List the latest & stable intermediary (Calamus) generations")
    ).get_matches();

    match parse(matches).await {
        Ok(r) => {
            if r == InstallationResult::Installed {
                println!("Installation complete!");
                println!("Ornithe has been successfully installed.");
                println!(
                    "Most mods require that you also download the Ornithe Standard Libraries mod and place it in your mods folder."
                );
                println!("You can find it at {}", crate::OSL_MODRINTH_URL);
            }
        }
        Err(e) => {
            std::io::stderr()
                .write_all(
                    ("Error while running Ornithe Installer CLI: ".to_owned() + &e.0).as_bytes(),
                )
                .expect("Failed to print error!");
        }
    }
}

async fn parse(matches: ArgMatches) -> Result<InstallationResult, InstallerError> {
    if let Some(_) = matches.subcommand_matches("intermediary-generations") {
        let generations = crate::net::meta::fetch_intermediary_generations().await?;
        writeln!(
            std::io::stdout(),
            "Latest Generation: {}",
            generations.latest
        )?;
        writeln!(
            std::io::stdout(),
            "Stable Generation: {}",
            generations.stable
        )?;
        return Ok(InstallationResult::NotInstalled);
    }
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

    if let Some(matches) = matches.subcommand_matches("game-versions") {
        let mut out = String::new();
        let snapshots = matches.get_flag("show-snapshots");
        let historical = matches.get_flag("show-historical");
        let info = get_minecraft_information(matches).await?;
        for version in info.available_minecraft_versions {
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

    let (send, mut recv) = unbounded_channel();

    let fut = tokio::spawn(do_install(send, matches));
    let pb = ProgressBar::new(100).with_style(
        ProgressStyle::with_template("{spinner:.green} [{wide_bar:.cyan/blue}]")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb.set_position(0);

    while !fut.is_finished() && !recv.is_empty() {
        match recv.try_recv() {
            Ok((prog, msg)) => {
                pb.println(msg);
                pb.set_position((prog * 100.0) as u64);
            }
            Err(_) => {}
        }
    }
    pb.finish_and_clear();
    return fut.await.unwrap();
}

async fn do_install(
    send: UnboundedSender<(f32, String)>,
    matches: ArgMatches,
) -> Result<InstallationResult, InstallerError> {
    let loader_versions = crate::net::meta::fetch_loader_versions().await?;
    if let Some(matches) = matches.subcommand_matches("client") {
        let (minecraft_version, intermediary, info) =
            get_minecraft_version(matches, GameSide::Client).await?;
        let loader_type = get_loader_type(matches)?;
        let loader_versions = loader_versions.get(&loader_type).unwrap();
        let loader_version = get_loader_version(matches, loader_versions)?;
        let location = matches.get_one::<PathBuf>("dir").unwrap().clone();
        let create_profile = matches.get_flag("generate-profile");
        crate::actions::client::install(
            send,
            minecraft_version,
            intermediary,
            loader_type,
            loader_version,
            info.calamus_generation,
            location,
            create_profile,
        )
        .await?;
        return Ok(InstallationResult::Installed);
    }

    if let Some(matches) = matches.subcommand_matches("server") {
        let (minecraft_version, intermediary, info) =
            get_minecraft_version(matches, GameSide::Server).await?;
        let loader_type = get_loader_type(matches)?;
        let loader_versions = loader_versions.get(&loader_type).unwrap();
        let loader_version = get_loader_version(matches, loader_versions)?;
        let location = matches.get_one::<PathBuf>("dir").unwrap().clone();
        if let Some(matches) = matches.subcommand_matches("run") {
            let java = matches.get_one::<PathBuf>("java");
            let run_args = matches.get_one::<String>("args");
            let installed = crate::actions::server::install_and_run(
                send,
                minecraft_version,
                intermediary,
                loader_type,
                loader_version,
                info.calamus_generation,
                location,
                java,
                run_args.map(|s| s.split(" ")),
            )
            .await?;
            return Ok(match installed {
                true => InstallationResult::Installed,
                false => InstallationResult::NotInstalled,
            });
        }
        crate::actions::server::install(
            send,
            minecraft_version,
            intermediary,
            loader_type,
            loader_version,
            info.calamus_generation,
            location,
            matches.get_flag("download-minecraft"),
        )
        .await?;
        return Ok(InstallationResult::Installed);
    }

    if let Some(matches) = matches.subcommand_matches("mmc") {
        let (minecraft_version, intermediary, info) =
            get_minecraft_version(matches, GameSide::Server).await?;
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
            send,
            minecraft_version,
            intermediary,
            loader_type,
            loader_version,
            output_dir,
            copy_profile_path,
            generate_zip,
            info.calamus_generation,
        )
        .await?;
        return Ok(InstallationResult::Installed);
    }

    Ok(InstallationResult::NotInstalled)
}

async fn get_minecraft_information(
    matches: &ArgMatches,
) -> Result<MinecraftInformation, InstallerError> {
    let generation = matches.get_one::<u32>("gen").map(|u| *u);
    let minecraft_versions = crate::net::manifest::fetch_versions(&generation).await?;
    let intermediary_versions = crate::net::meta::fetch_intermediary_versions(&generation).await?;

    let mut available_minecraft_versions = Vec::new();

    for version in minecraft_versions.versions {
        if intermediary_versions.contains_key(&version.id)
            || intermediary_versions.contains_key(&(version.id.clone() + "-client"))
            || intermediary_versions.contains_key(&(version.id.clone() + "-server"))
        {
            available_minecraft_versions.push(version);
        }
    }
    Ok(MinecraftInformation {
        intermediary_versions,
        available_minecraft_versions,
        calamus_generation: generation,
    })
}

struct MinecraftInformation {
    intermediary_versions: HashMap<String, IntermediaryVersion>,
    available_minecraft_versions: Vec<MinecraftVersion>,
    calamus_generation: Option<u32>,
}

async fn get_minecraft_version(
    matches: &ArgMatches,
    side: GameSide,
) -> Result<(MinecraftVersion, IntermediaryVersion, MinecraftInformation), InstallerError> {
    let info = get_minecraft_information(matches).await?;
    let minecraft_version_arg = matches.get_one::<String>("minecraft-version").unwrap();

    let intermediary_versions = &info.intermediary_versions;
    for version in &info.available_minecraft_versions {
        if version.id == *minecraft_version_arg {
            let intermediary = intermediary_versions
                .get(&version.id)
                .or_else(|| intermediary_versions.get(&(version.id.to_owned() + "-" + side.id())));
            if let Some(int) = intermediary {
                return Ok((version.clone(), int.clone(), info));
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
    add_gen_argument(command)
        .arg(arg!(-m --"minecraft-version" <VERSION> "Minecraft version to use").required(true))
        .arg(
            arg!(--"loader-type" <TYPE> "Loader type to use")
                .default_value("fabric")
                .ignore_case(true)
                .value_parser(["fabric", "quilt"]),
        )
        .arg(arg!(--"loader-version" <VERSION> "Loader version to use").default_value("latest"))
}

fn add_gen_argument(command: Command) -> Command {
    command.arg(
        arg!(--gen <GENERATION> "The Intermediary Generation (Calamus)")
            .value_parser(value_parser!(u32))
            .alias("generation"),
    )
}
