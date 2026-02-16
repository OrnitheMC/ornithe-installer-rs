use std::{
    collections::HashMap,
    fmt::Display,
    path::Path,
    sync::mpsc::{Receiver, Sender},
};

use egui::{
    Button, Checkbox, Color32, ComboBox, FontId, Frame, Layout, ProgressBar, RichText, Sense,
    Theme, UiBuilder, Vec2, Vec2b,
};
use log::{error, info};
use rfd::{AsyncFileDialog, AsyncMessageDialog, MessageButtons, MessageDialogResult};
use tokio::{
    sync::mpsc::{UnboundedReceiver, unbounded_channel},
    task::JoinHandle,
};

use crate::{
    errors::InstallerError,
    net::{
        self, GameSide,
        manifest::MinecraftVersion,
        meta::{IntermediaryVersion, LoaderType, LoaderVersion},
    },
};

use egui::{
    Id, Response, ScrollArea, TextEdit, Ui, Widget, WidgetText,
    text::{CCursor, CCursorRange},
};
use std::hash::Hash;

#[derive(PartialEq, Clone, Copy, Debug)]
enum Mode {
    Client,
    Server,
    MMC,
}

pub async fn run() -> Result<(), InstallerError> {
    info!("Starting GUI installer...");

    let res = create_window().await;
    if let Err(e) = res {
        error!("{}", e.0);
        display_dialog(t!("gui.error.generic"), &e.0);
        return Err(e);
    }

    Ok(())
}

async fn create_window() -> Result<(), InstallerError> {
    let data = eframe::icon_data::from_png_bytes(crate::ORNITHE_ICON_BYTES)
        .expect("The Ornithe Icon is a valid PNG file");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([630.0, 490.0])
            .with_resizable(false)
            .with_icon(data),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    let res = App::create().await;
    if let Err(e) = res {
        error!("{}", e.0);
        display_dialog(t!("gui.error.generic"), &e.0);
        return Ok(());
    }
    let app = res.unwrap();

    eframe::run_native(
        &("Ornithe Installer ".to_owned() + crate::VERSION),
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    )?;
    Ok(())
}

fn display_dialog<T: Into<String> + Display, M: Into<String> + Display>(title: T, message: M) {
    display_dialog_ext(title, message, MessageButtons::Ok, |_| {});
}

fn display_dialog_ext<F, T: Into<String> + Display, M: Into<String> + Display>(
    title: T,
    message: M,
    buttons: MessageButtons,
    after: F,
) where
    F: FnOnce(MessageDialogResult) -> (),
    F: Send,
    F: 'static,
{
    info!("Displaying dialog: {}: {}", title, message);
    let dialog = AsyncMessageDialog::new()
        .set_title(title)
        .set_level(rfd::MessageLevel::Info)
        .set_description(message)
        .set_buttons(buttons);
    let fut = dialog.show();
    tokio::spawn(async move {
        after(fut.await);
    });
}

struct App {
    mode: Mode,
    selected_minecraft_version: String,
    available_minecraft_versions: Vec<MinecraftVersion>,
    intermediary_versions: HashMap<String, IntermediaryVersion>,
    available_intermediary_versions: Vec<String>,
    filtered_minecraft_versions: Vec<String>,
    show_snapshots: bool,
    show_historical: bool,
    selected_loader_type: LoaderType,
    selected_loader_version: String,
    available_loader_versions: HashMap<LoaderType, Vec<LoaderVersion>>,
    show_betas: bool,
    create_profile: bool,
    client_install_location: String,
    mmc_output_location: String,
    server_install_location: String,
    copy_generated_location: bool,
    generate_zip: bool,
    download_minecraft_server: bool,
    installation_task: Option<InstallationProgress>,
    file_picker_channel: (
        Sender<Option<FilePickResult>>,
        Receiver<Option<FilePickResult>>,
    ),
    file_picker_open: bool,
    minecraft_version_dropdown_open: bool,
    detonation_easter_egg: bool,
}

pub struct InstallationProgress {
    last_progress: f32,
    status: Vec<String>,
    task: Option<(
        UnboundedReceiver<(f32, String)>,
        JoinHandle<Result<(), InstallerError>>,
    )>,
}

impl InstallationProgress {
    pub fn new(
        task: (
            UnboundedReceiver<(f32, String)>,
            JoinHandle<Result<(), InstallerError>>,
        ),
    ) -> Self {
        Self {
            last_progress: 0.0,
            status: Vec::new(),
            task: Some(task),
        }
    }

    pub fn poll(&mut self) {
        if let Some((ref mut rec, _)) = self.task {
            match rec.try_recv() {
                Ok((progress, message)) => {
                    info!("{}% - {}", (progress * 100.0) as i32, &message);
                    self.last_progress = progress;
                    self.status.push(message);
                }
                Err(_) => {}
            }
        }
    }
}

struct FilePickResult {
    mode: Mode,
    path: String,
}

impl App {
    async fn create() -> Result<App, InstallerError> {
        let mut available_minecraft_versions = Vec::new();
        let mut available_intermediary_versions = Vec::new();
        let mut available_loader_versions = HashMap::new();
        let mut intermediary_versions = HashMap::new();
        let manifest_future = net::manifest::fetch_versions(&None);
        let intermediary_future = net::meta::fetch_intermediary_versions(&None);
        let loader_future = net::meta::fetch_loader_versions(&None);

        info!("Loading versions...");
        if let Ok(versions) = manifest_future.await {
            for ele in versions.versions {
                available_minecraft_versions.push(ele);
            }
        }

        if let Ok(versions) = intermediary_future.await {
            for v in versions {
                available_intermediary_versions.push(v.0.clone());
                intermediary_versions.insert(v.0, v.1);
            }
        }
        if available_minecraft_versions.len() == 0 {
            return Err(InstallerError::from(t!(
                "error.no_available_minecraft_versions"
            )));
        }
        info!(
            "Loaded {} Minecraft versions",
            available_minecraft_versions.len()
        );
        info!(
            "Loaded {} Intermediary versions",
            available_intermediary_versions.len()
        );

        if let Ok(versions) = loader_future.await {
            available_loader_versions = versions;
        }
        info!(
            "Loaded versions for {} loaders",
            available_loader_versions.len()
        );

        let mut app = App {
            mode: Mode::Client,
            selected_minecraft_version: String::new(),
            available_minecraft_versions,
            intermediary_versions,
            available_intermediary_versions,
            filtered_minecraft_versions: Vec::new(),
            show_snapshots: false,
            show_historical: false,
            selected_loader_type: LoaderType::Fabric,
            selected_loader_version: available_loader_versions
                .get(&LoaderType::Fabric)
                .map(|v| v.get(0).unwrap().version.clone())
                .unwrap_or(String::new()),
            available_loader_versions,
            show_betas: false,
            create_profile: true,
            client_install_location: super::dot_minecraft_location(),
            mmc_output_location: super::current_location(),
            server_install_location: super::server_location(),
            copy_generated_location: false,
            generate_zip: true,
            download_minecraft_server: true,
            file_picker_channel: std::sync::mpsc::channel(),
            file_picker_open: false,
            installation_task: None,
            minecraft_version_dropdown_open: false,
            detonation_easter_egg: rand::random_bool(0.001),
        };
        app.filter_minecraft_versions();
        Ok(app)
    }

    fn add_location_picker(&mut self, frame: &mut eframe::Frame, ui: &mut egui::Ui) {
        let mut line_rect = ui.available_rect_before_wrap();
        line_rect.max.y = ui.cursor().min.y + 25.0;
        line_rect.min.y -= 6.0;
        let mut child = ui.new_child(UiBuilder::new().max_rect(line_rect));
        child.horizontal_centered(|ui| {
            ui.text_edit_singleline(match self.mode {
                Mode::Client => &mut self.client_install_location,
                Mode::Server => &mut self.server_install_location,
                Mode::MMC => &mut self.mmc_output_location,
            });
            if ui.button(t!("gui.ui.button.pick_location")).clicked() {
                let picked = AsyncFileDialog::new()
                    .set_directory(Path::new(match self.mode {
                        Mode::Client => &self.client_install_location,
                        Mode::Server => &self.server_install_location,
                        Mode::MMC => &self.mmc_output_location,
                    }))
                    .set_parent(&frame)
                    .pick_folder();
                self.file_picker_open = true;
                let sender = self.file_picker_channel.0.clone();
                let mode = self.mode.clone();
                let ctx = ui.ctx().clone();
                tokio::spawn(async move {
                    let opt = picked.await;
                    let mut send = None;
                    if let Some(path) = opt {
                        if let Some(path) = path.path().to_str() {
                            send = Some(FilePickResult {
                                mode,
                                path: path.to_owned(),
                            });
                        }
                    }
                    let _ = sender.send(send);
                    ctx.request_repaint();
                });
            }
        });
        ui.add_space(20.0);
    }

    fn add_environment_options(&mut self, ui: &mut egui::Ui) {
        ui.label(t!("gui.ui.environment"));
        let mut line_rect = ui.available_rect_before_wrap();
        line_rect.max.y = ui.cursor().min.y + 25.0;
        line_rect.min.y -= 6.0;
        let mut child = ui.new_child(UiBuilder::new().max_rect(line_rect));
        ui.add_space(20.0);
        child.horizontal_centered(|ui| {
            let mut clicked = false;
            let width = line_rect.width() / 2.0;
            ui.scope(|ui| {
                ui.set_max_width(width);
                clicked |= ui
                    .radio_value(&mut self.mode, Mode::Client, t!("gui.mode.client"))
                    .clicked();
            });
            ui.scope(|ui| {
                ui.set_max_width(width);
                clicked |= ui
                    .radio_value(&mut self.mode, Mode::MMC, t!("gui.mode.mmc"))
                    .clicked();
            });
            ui.scope(|ui| {
                ui.set_max_width(width);
                clicked |= ui
                    .radio_value(&mut self.mode, Mode::Server, t!("gui.mode.server"))
                    .clicked();
            });
            if clicked {
                self.filter_minecraft_versions();
            }
        });
    }

    fn add_minecraft_version(&mut self, ui: &mut egui::Ui) {
        ui.label(t!("gui.ui.minecraft_version"));
        let mut line_rect = ui.available_rect_before_wrap();
        line_rect.max.y = ui.cursor().min.y + 25.0;
        line_rect.min.y -= 6.0;
        let mut child = ui.new_child(UiBuilder::new().max_rect(line_rect));

        child.horizontal_centered(|ui| {
            ui.add(
                DropDownBox::from_iter(
                    &self.filtered_minecraft_versions,
                    "minecraft_version",
                    &mut self.selected_minecraft_version,
                    |ui, text| {
                        Button::selectable(false, text)
                            .min_size(Vec2::new(ui.available_width(), 0.0))
                            .ui(ui)
                    },
                    &mut self.minecraft_version_dropdown_open,
                )
                .max_height(130.0)
                .desired_width(170.0)
                .hint_text(t!("gui.ui.search_available_versions")),
            );
            if ui
                .checkbox(&mut self.show_snapshots, t!("gui.ui.checkbox.snapshots"))
                .clicked()
                || ui
                    .checkbox(&mut self.show_historical, t!("gui.ui.checkbox.historical"))
                    .clicked()
            {
                self.filter_minecraft_versions();
            }
        });
        ui.add_space(20.0);
    }

    fn filter_minecraft_versions(&mut self) {
        self.filtered_minecraft_versions = self
            .available_minecraft_versions
            .iter()
            .filter(|v| {
                self.available_intermediary_versions.contains(&v.id)
                    || self.available_intermediary_versions.contains(
                        &(v.id.clone()
                            + "-"
                            + match self.mode {
                                Mode::Server => "server",
                                _ => "client",
                            }),
                    )
            })
            .filter(|v| {
                if self.show_snapshots && self.show_historical {
                    return true;
                }
                let mut displayed = v.is_release();
                if !displayed && self.show_snapshots {
                    displayed = v.is_snapshot();
                }
                if !displayed && self.show_historical {
                    displayed = v.is_historical();
                }
                displayed
            })
            .map(|v| v.id.clone())
            .collect::<Vec<String>>();
        info!(
            "Filtered {} valid minecraft versions to display out of {} total",
            self.filtered_minecraft_versions.len(),
            self.available_minecraft_versions.len()
        );
    }

    fn add_loader(&mut self, ui: &mut egui::Ui) {
        ui.label(t!("gui.ui.loader"));
        let mut line_rect = ui.available_rect_before_wrap();
        line_rect.max.y = ui.cursor().min.y + 25.0;
        line_rect.min.y -= 6.0;
        let mut child = ui.new_child(UiBuilder::new().max_rect(line_rect));
        ui.add_space(20.0);
        child.horizontal_centered(|ui| {
            let loader_type_response = ComboBox::from_id_salt("loader_type")
                .selected_text(t!(
                    "gui.ui.selection.loader.name",
                    name = &self.selected_loader_type.get_localized_name()
                ))
                .show_ui(ui, |ui| {
                    let mut changed = false;
                    changed |= ui
                        .selectable_value(
                            &mut self.selected_loader_type,
                            LoaderType::Fabric,
                            t!(
                                "gui.ui.selection.loader.name",
                                name = LoaderType::Fabric.get_localized_name()
                            ),
                        )
                        .changed();
                    changed |= ui
                        .selectable_value(
                            &mut self.selected_loader_type,
                            LoaderType::Quilt,
                            t!(
                                "gui.ui.selection.loader.name",
                                name = LoaderType::Quilt.get_localized_name()
                            ),
                        )
                        .changed();
                    changed
                });

            ui.label(t!("gui.ui.loader_version"));
            ComboBox::from_id_salt("loader_version")
                .selected_text(format!("{}", &self.selected_loader_version))
                .show_ui(ui, |ui| {
                    for ele in self
                        .available_loader_versions
                        .get(&self.selected_loader_type)
                        .unwrap()
                    {
                        if self.show_betas || ele.is_stable() {
                            ui.selectable_value(
                                &mut self.selected_loader_version,
                                ele.version.clone(),
                                ele.version.clone(),
                            );
                        }
                    }
                });
            let checkbox_response =
                ui.checkbox(&mut self.show_betas, t!("gui.ui.show_loader_betas"));
            if !self
                .available_loader_versions
                .get(&self.selected_loader_type)
                .unwrap()
                .iter()
                .find(|v| v.version == self.selected_loader_version)
                .is_some()
                || checkbox_response.clicked()
                || loader_type_response.inner.is_some_and(|t| t)
            {
                self.selected_loader_version = self
                    .available_loader_versions
                    .get(&self.selected_loader_type)
                    .unwrap()
                    .iter()
                    .filter(|v| self.show_betas || v.is_stable())
                    .map(|v| v.version.clone())
                    .next()
                    .unwrap()
                    .clone();
            }
        });
    }

    fn run_installation(&mut self) {
        if let Some(version) = self
            .available_minecraft_versions
            .iter()
            .find(|v| v.id == self.selected_minecraft_version)
        {
            let selected_version = version.clone();
            let loader_version = self
                .available_loader_versions
                .get(&self.selected_loader_type)
                .unwrap()
                .iter()
                .find(|v| v.version == self.selected_loader_version)
                .unwrap()
                .clone();
            let (sender, receiver) = unbounded_channel();
            match self.mode {
                Mode::Client => {
                    let loader_type = self.selected_loader_type.clone();
                    let location = Path::new(&self.client_install_location).to_path_buf();
                    let create_profile = self.create_profile;
                    let intermediary_version =
                        match self.get_intermediary_version(&selected_version, GameSide::Client) {
                            Ok(v) => v,
                            Err(e) => {
                                display_dialog(t!("gui.error.installation_failed"), &e.0);
                                return;
                            }
                        };
                    let handle = tokio::spawn(crate::actions::client::install(
                        sender,
                        selected_version,
                        intermediary_version,
                        loader_type,
                        loader_version,
                        None,
                        location,
                        create_profile,
                    ));
                    self.installation_task = Some(InstallationProgress::new((receiver, handle)));
                }
                Mode::Server => {
                    let loader_type = self.selected_loader_type.clone();
                    let location = Path::new(&self.server_install_location).to_path_buf();
                    let download_server = self.download_minecraft_server;
                    let intermediary_version =
                        match self.get_intermediary_version(&selected_version, GameSide::Server) {
                            Ok(v) => v,
                            Err(e) => {
                                display_dialog(t!("gui.error.installation_failed"), e.0);
                                return;
                            }
                        };
                    self.installation_task = Some(InstallationProgress::new((
                        receiver,
                        tokio::spawn(crate::actions::server::install(
                            sender,
                            selected_version,
                            intermediary_version,
                            loader_type,
                            loader_version,
                            None,
                            location,
                            download_server,
                        )),
                    )));
                }
                Mode::MMC => {
                    let loader_type = self.selected_loader_type.clone();
                    let location = Path::new(&self.mmc_output_location).to_path_buf();
                    let copy_profile_path = self.copy_generated_location;
                    let generate_zip = self.generate_zip;
                    let intermediary_version =
                        match self.get_intermediary_version(&selected_version, GameSide::Client) {
                            Ok(v) => v,
                            Err(e) => {
                                display_dialog(t!("gui.error.installation_failed"), e.0);
                                return;
                            }
                        };
                    let handle = tokio::spawn(crate::actions::mmc_pack::install(
                        sender,
                        selected_version,
                        intermediary_version,
                        loader_type,
                        loader_version,
                        location,
                        copy_profile_path,
                        generate_zip,
                        None,
                    ));
                    self.installation_task = Some(InstallationProgress::new((receiver, handle)));
                }
            }
        } else {
            display_dialog(
                t!("gui.error.installation_failed"),
                t!("gui.error.no_supported_minecraft_version_selected"),
            );
        }
    }

    fn monitor_installation(&mut self) {
        if let Some(InstallationProgress { task: install, .. }) = &self.installation_task {
            if install.as_ref().map(|t| t.1.is_finished()).unwrap_or(false) {
                let prog = self.installation_task.as_mut().unwrap();
                while !prog.task.as_ref().map(|t| t.0.is_empty()).unwrap_or(false) {
                    prog.poll();
                }
                let (_, handle) = prog.task.take().unwrap();
                tokio::spawn(async move {
                    match handle.await.unwrap() {
                        Err(e) => {
                            error!("{}", e.0);
                            display_dialog(
                                t!("gui.error.installation_failed"),
                                t!("gui.error.failed_to_install", error = e.0),
                            )
                        }
                        Ok(_) => display_dialog_ext(
                            t!("gui.dialog.installation_successful"),
                            t!("gui.dialog.installation_successful.message"),
                            MessageButtons::YesNo,
                            |res| {
                                if res == MessageDialogResult::Yes {
                                    if webbrowser::open(crate::OSL_MODRINTH_URL).is_err() {
                                        display_dialog(
                                            t!("gui.error.failed_to_open_modrinth"),
                                            t!(
                                                "error.failed_to_open_modrinth.message",
                                                osl_url = crate::OSL_MODRINTH_URL
                                            ),
                                        );
                                    }
                                }
                            },
                        ),
                    }
                });
            }
        }
    }

    fn add_additional_options(&mut self, ui: &mut egui::Ui) {
        let mut line_rect = ui.available_rect_before_wrap();
        line_rect.max.y = ui.cursor().min.y + 25.0;
        line_rect.min.y -= 6.0;
        let mut child = ui.new_child(UiBuilder::new().max_rect(line_rect));
        ui.add_space(20.0);
        child.horizontal_centered(|ui| match self.mode {
            Mode::Client => {
                ui.checkbox(
                    &mut self.create_profile,
                    t!("gui.checkbox.generate_profile"),
                );
            }
            Mode::Server => {
                ui.checkbox(
                    &mut self.download_minecraft_server,
                    t!("gui.checkbox.download_minecraft_server"),
                );
            }
            Mode::MMC => {
                let copy_profile_path = Checkbox::new(
                    &mut self.copy_generated_location,
                    t!("gui.checkbox.copy_profile_path"),
                );
                ui.add_sized([ui.max_rect().width() / 2.0, 20.0], copy_profile_path);
                ui.checkbox(
                    &mut self.generate_zip,
                    t!("gui.checkbox.generate_instance_zip"),
                );
            }
        });
    }

    fn get_intermediary_version(
        &self,
        selected_version: &MinecraftVersion,
        side: GameSide,
    ) -> Result<IntermediaryVersion, InstallerError> {
        let ver = self.intermediary_versions.get(&selected_version.id);
        match side {
            GameSide::Client => ver.or_else(|| {
                self.intermediary_versions
                    .get(&(selected_version.id.to_owned() + "-client"))
            }),
            GameSide::Server => ver.or_else(|| {
                self.intermediary_versions
                    .get(&(selected_version.id.to_owned() + "-server"))
            }),
        }
        .map(|v| v.clone())
        .ok_or(InstallerError::from(t!(
            "error.no_matching_intermediary_version",
            version = selected_version.id
        )))
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        ctx.set_zoom_factor(1.5);
        ctx.options_mut(|opt| opt.fallback_theme = Theme::Light);

        if let Ok(result) = self.file_picker_channel.1.try_recv() {
            self.file_picker_open = false;
            if let Some(result) = result {
                match result.mode {
                    Mode::Client => self.client_install_location = result.path,
                    Mode::Server => self.server_install_location = result.path,
                    Mode::MMC => self.mmc_output_location = result.path,
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.style_mut().interaction.selectable_labels = false;
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.add_enabled_ui(!self.file_picker_open, |ui| {
                let mut child =
                    ui.new_child(UiBuilder::new().layout(Layout::right_to_left(egui::Align::TOP)));
                ui.vertical_centered(|ui| {
                    ui.heading(t!("gui.ui.title"));
                });
                child.horizontal(|ui| {
                    ComboBox::from_id_salt("language")
                        .width(20.0)
                        .selected_text(&*rust_i18n::locale())
                        .show_ui(ui, |ui| {
                            for ele in rust_i18n::available_locales!() {
                                if ui
                                    .selectable_label(*ele == *rust_i18n::locale(), ele)
                                    .clicked()
                                {
                                    rust_i18n::set_locale(ele);
                                }
                            }
                        });
                    ui.label(t!("gui.ui.language"));
                });

                if self.installation_task.is_some() {
                    ui.vertical(|ui| {
                        let progress = self.installation_task.as_mut().unwrap();
                        ui.add_space(15.0);
                        progress.poll();

                        ui.label(t!("gui.ui.output"));
                        let mut rect = ui.cursor();
                        let mut output_height = ui.available_height() - 44.0;
                        rect.set_width(ui.available_width());
                        rect.set_height(output_height);
                        ui.painter().rect_filled(rect, 8.0, Color32::LIGHT_GRAY);
                        ui.add_space(4.0);
                        output_height -= 10.0;
                        ScrollArea::vertical()
                            .auto_shrink(Vec2b::FALSE)
                            .min_scrolled_height(output_height)
                            .min_scrolled_width(ui.available_width())
                            .max_height(output_height)
                            .max_width(ui.available_width())
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                let text = progress.status.join("\n");
                                TextEdit::multiline(&mut text.as_str())
                                    .desired_width(ui.available_width())
                                    .font(FontId::monospace(10.0))
                                    .return_key(None)
                                    .cursor_at_end(true)
                                    .show(ui);
                            });
                        ui.add_space(4.0);
                        ProgressBar::new(progress.last_progress)
                            .desired_width(ui.available_width())
                            .animate(true)
                            .text(
                                RichText::new(format!(
                                    "{}%",
                                    (progress.last_progress * 100.0) as i32
                                ))
                                .background_color(Color32::LIGHT_BLUE),
                            )
                            .fill(Color32::LIGHT_BLUE)
                            .ui(ui);
                    });
                    ui.vertical_centered(|ui| {
                        let mut back = Button::new(RichText::new(t!("gui.button.back")).heading());
                        if self
                            .installation_task
                            .as_ref()
                            .and_then(|t| t.task.as_ref())
                            .map(|t| t.1.is_finished())
                            .unwrap_or(false)
                        {
                            back = back.sense(Sense::empty());
                        }
                        if back.ui(ui).clicked() {
                            self.installation_task = None;
                        }
                    });
                    return;
                }
                ui.vertical(|ui| {
                    ui.add_space(15.0);

                    self.add_environment_options(ui);

                    ui.add_space(15.0);
                    self.add_minecraft_version(ui);
                    ui.add_space(15.0);
                    self.add_loader(ui);

                    ui.add_space(15.0);
                    ui.label(if self.mode == Mode::MMC && self.generate_zip {
                        if *rust_i18n::locale() == *"fr" && self.detonation_easter_egg {
                            std::borrow::Cow::Borrowed("Emplacement de detonation")
                        } else {
                            t!("gui.ui.output_location")
                        }
                    } else {
                        t!("gui.ui.install_location")
                    });
                    self.add_location_picker(frame, ui);
                });

                ui.add_space(15.0);
                self.add_additional_options(ui);

                ui.add_space(15.0);
                ui.vertical_centered(|ui| {
                    if Button::new(RichText::new(t!("gui.button.install")).heading())
                        .min_size(Vec2::new(100.0, 0.0))
                        .ui(ui)
                        .clicked()
                    {
                        self.run_installation();
                    }
                });
            });
        });

        self.monitor_installation();
    }
}

/// Dropdown widget (https://github.com/ItsEthra/egui-dropdown/pull/21, with slight changes)
pub struct DropDownBox<
    'a,
    F: FnMut(&mut Ui, &str) -> Response,
    V: AsRef<str>,
    I: Iterator<Item = V>,
> {
    buf: &'a mut String,
    popup_id: Id,
    display: F,
    it: I,
    hint_text: WidgetText,
    filter_by_input: bool,
    select_on_focus: bool,
    desired_width: Option<f32>,
    max_height: Option<f32>,
    open: &'a mut bool,
}

impl<'a, F: FnMut(&mut Ui, &str) -> Response, V: AsRef<str>, I: Iterator<Item = V>>
    DropDownBox<'a, F, V, I>
{
    /// Creates new dropdown box.
    pub fn from_iter(
        it: impl IntoIterator<IntoIter = I>,
        id_source: impl Hash,
        buf: &'a mut String,
        display: F,
        open_state: &'a mut bool,
    ) -> Self {
        Self {
            popup_id: Id::new(id_source),
            it: it.into_iter(),
            display,
            buf,
            hint_text: WidgetText::default(),
            filter_by_input: true,
            select_on_focus: true,
            desired_width: None,
            max_height: None,
            open: open_state,
        }
    }

    /// Add a hint text to the Text Edit
    pub fn hint_text(mut self, hint_text: impl Into<WidgetText>) -> Self {
        self.hint_text = hint_text.into();
        self
    }

    /// Passes through the desired width value to the underlying Text Edit
    pub fn desired_width(mut self, desired_width: f32) -> Self {
        self.desired_width = desired_width.into();
        self
    }

    /// Set a maximum height limit for the opened popup
    pub fn max_height(mut self, height: f32) -> Self {
        self.max_height = height.into();
        self
    }
}

impl<F: FnMut(&mut Ui, &str) -> Response, V: AsRef<str>, I: Iterator<Item = V>> Widget
    for DropDownBox<'_, F, V, I>
{
    fn ui(self, ui: &mut Ui) -> Response {
        let Self {
            popup_id,
            buf,
            it,
            mut display,
            hint_text,
            filter_by_input,
            select_on_focus,
            desired_width,
            max_height,
            open,
        } = self;

        let mut edit = TextEdit::singleline(buf).hint_text(hint_text);
        if let Some(dw) = desired_width {
            edit = edit.desired_width(dw);
        }
        let mut edit_output = edit.show(ui);
        let mut r = edit_output.response;
        if r.gained_focus() {
            if select_on_focus {
                edit_output
                    .state
                    .cursor
                    .set_char_range(Some(CCursorRange::two(
                        CCursor::new(0),
                        CCursor::new(buf.len()),
                    )));
                edit_output.state.store(ui.ctx(), r.id);
            }
            *open = true;
        }

        let mut changed = false;
        let frame = Frame::popup(ui.style());
        let padding = frame.inner_margin.rightf();
        if let Some(popup_response) = egui::Popup::new(
            popup_id,
            ui.ctx().clone(),
            r.rect.left_bottom(),
            ui.layer_id(),
        )
        .frame(frame)
        .open(*open)
        .show(|ui| {
            if let Some(max) = max_height {
                ui.set_max_height(max);
            }
            ScrollArea::vertical()
                .max_height(f32::INFINITY)
                .show(ui, |ui| {
                    if let Some(width) = desired_width {
                        ui.set_width(width - padding);
                    }
                    for var in it {
                        let text = var.as_ref();
                        if filter_by_input
                            && !buf.is_empty()
                            && !text.to_lowercase().contains(&buf.to_lowercase())
                        {
                            continue;
                        }

                        if display(ui, text).clicked() {
                            *buf = text.to_owned();
                            changed = true;

                            *open = false;
                        }
                    }
                });
        }) {
            if r.lost_focus() && !popup_response.response.has_focus() {
                *open = false;
            }
        } else {
            if r.lost_focus() {
                *open = false;
            }
        }

        if changed {
            r.mark_changed();
        }

        r
    }
}
