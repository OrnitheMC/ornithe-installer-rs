use std::{
    collections::HashMap,
    fmt::Display,
    hash::Hash,
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, Sender},
};

use egui::{
    Align, Button, Checkbox, Color32, ComboBox, FontId, Frame, Id, Layout, Margin, Modal,
    ProgressBar, Response, RichText, ScrollArea, Sense, TextEdit, Theme, Tooltip, Ui, UiBuilder,
    Vec2, Vec2b, Widget, WidgetText,
    text::{CCursor, CCursorRange},
};
use log::{error, info};
use rfd::{AsyncMessageDialog, MessageButtons, MessageDialogResult};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use crate::{
    errors::InstallerError,
    net::{
        self, GameSide,
        manifest::MinecraftVersion,
        meta::{IntermediaryVersion, LoaderType, LoaderVersion},
    },
    ui::font_loader::load_system_font_to_egui,
};

#[derive(PartialEq, Clone, Copy, Debug)]
enum Mode {
    Client,
    Server,
    PrismLauncher,
}

pub async fn run() -> Result<(), InstallerError> {
    info!("Starting GUI installer...");
    if let Ok(locale) = current_locale::current_locale() {
        rust_i18n::set_locale(&locale);
        // It is possible that we do not support the preferred language,
        // in which case rust_i18n falls back to [`en`]. rust_i18n::locale() will still return
        // whatever was just set.
        info!("Trying to adjust language to user locale: {}", locale);
    }

    let res = create_window().await;
    if let Err(e) = res {
        error!("{}", e.0);
        display_dialog(t!("gui.error.generic"), &e.0);
        return Err(e);
    }

    Ok(())
}

async fn create_window() -> Result<(), InstallerError> {
    let res = App::create().await;
    if let Err(e) = res {
        error!("{}", e.0);
        display_dialog(t!("gui.error.generic"), &e.0);
        return Ok(());
    }
    let app = res.unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    {
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

        eframe::run_native(
            &("Ornithe Installer ".to_owned() + crate::VERSION),
            options,
            Box::new(|cc| {
                // load needed system fonts
                load_system_font_to_egui(&cc.egui_ctx);

                Ok(Box::new(app))
            }),
        )?;
    }
    #[cfg(target_arch = "wasm32")]
    {
        use eframe::wasm_bindgen::JsCast as _;

        let web_options = eframe::WebOptions::default();

        let window = web_sys::window().expect("No window");
        let document = window.document().expect("No document");

        let canvas = document
            .get_element_by_id("main_canvas")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("main_canvas was not a HtmlCanvasElement");

        let start_result = eframe::WebRunner::new()
            .start(
                canvas.clone(),
                web_options,
                Box::new(|cc| {
                    // load needed system fonts
                    load_system_font_to_egui(&cc.egui_ctx);

                    Ok(Box::new(app))
                }),
            )
            .await;

        // Remove the loading text and spinner:
        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            match start_result {
                Ok(_) => {
                    loading_text.remove();
                }
                Err(e) => {
                    loading_text.set_inner_html(
                        "<p> The app has crashed. See the developer console for details. </p>",
                    );
                    panic!("Failed to start eframe: {e:?}");
                }
            }
        }
    }
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
    let fut = async move {
        after(dialog.show().await);
    };
    #[cfg(not(target_arch = "wasm32"))]
    tokio::spawn(fut);
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(fut);
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
    #[cfg(not(target_arch = "wasm32"))]
    detonation_easter_egg: bool,
    include_flap: bool,
    modals: Vec<ModalPopup>,
    modal_channel: (Sender<ModalPopup>, Receiver<ModalPopup>),
}

struct ModalPopup {
    title: String,
    message: String,
    buttons: MessageButtons,
    after: Box<dyn FnOnce(MessageDialogResult) -> () + Send + 'static>,
}

impl ModalPopup {
    pub fn yesno(
        title: String,
        message: String,
        after: Box<dyn FnOnce(MessageDialogResult) -> () + Send + 'static>,
    ) -> Self {
        Self {
            title,
            message,
            buttons: MessageButtons::YesNo,
            after,
        }
    }

    pub fn ok_ext(
        title: String,
        message: String,
        after: Box<dyn FnOnce(MessageDialogResult) -> () + Send + 'static>,
    ) -> Self {
        Self {
            title,
            message,
            buttons: MessageButtons::Ok,
            after,
        }
    }

    pub fn ok<A: Into<String>, B: Into<String>>(title: A, message: B) -> ModalPopup {
        ModalPopup::ok_ext(title.into(), message.into(), Box::new(|_| {}))
    }
}

pub struct InstallationProgress {
    last_progress: f32,
    status: Vec<String>,
    #[cfg(not(target_arch = "wasm32"))]
    task: Option<(
        UnboundedReceiver<(f32, String)>,
        tokio::task::JoinHandle<Result<(), InstallerError>>,
    )>,
    #[cfg(target_arch = "wasm32")]
    task: Option<UnboundedReceiver<(f32, String)>>,
}

#[cfg(target_arch = "wasm32")]
impl InstallationProgress {
    pub fn new(task: UnboundedReceiver<(f32, String)>) -> Self {
        Self {
            last_progress: 0.0,
            status: Vec::new(),
            task: Some(task),
        }
    }

    pub fn rec(&mut self) -> Option<&mut UnboundedReceiver<(f32, String)>> {
        self.task.as_mut()
    }

    pub fn is_finished(&self) -> bool {
        self.last_progress >= 1.0 && self.task.is_some()
    }

    pub fn is_running(&self) -> bool {
        if self.last_progress < 1.0 {
            self.task.is_some()
        } else {
            false
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl InstallationProgress {
    pub fn new(
        task: (
            UnboundedReceiver<(f32, String)>,
            tokio::task::JoinHandle<Result<(), InstallerError>>,
        ),
    ) -> Self {
        Self {
            last_progress: 0.0,
            status: Vec::new(),
            task: Some(task),
        }
    }

    pub fn rec(&mut self) -> Option<&mut UnboundedReceiver<(f32, String)>> {
        if let Some((ref mut rec, _)) = self.task {
            return Some(rec);
        }
        None
    }

    pub fn is_finished(&self) -> bool {
        if let Some((_, task)) = &self.task {
            return task.is_finished();
        }
        false
    }

    pub fn is_running(&self) -> bool {
        if let Some((_, task)) = &self.task {
            return !task.is_finished();
        }
        false
    }
}
impl InstallationProgress {
    pub fn poll(&mut self) {
        if let Some(ref mut rec) = self.rec() {
            match rec.try_recv() {
                Ok((progress, message)) => {
                    if progress <= 1.0 {
                        info!("{}% - {}", (progress * 100.0) as i32, &message);
                    }
                    self.last_progress = progress;
                    if !message.is_empty() {
                        self.status.push(message);
                    }
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
                "gui.error.no_available_minecraft_versions"
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
            #[cfg(not(target_arch = "wasm32"))]
            detonation_easter_egg: rand::random_bool(0.001),
            include_flap: true,
            modals: Vec::new(),
            modal_channel: std::sync::mpsc::channel(),
        };
        app.filter_minecraft_versions();
        Ok(app)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn add_location_picker(&mut self, frame: &mut eframe::Frame, ui: &mut egui::Ui) {
        ui.label(if self.mode == Mode::PrismLauncher && self.generate_zip {
            if *rust_i18n::locale() == *"fr" && self.detonation_easter_egg {
                std::borrow::Cow::Borrowed("Emplacement de detonation")
            } else {
                t!("gui.ui.output_location")
            }
        } else {
            t!("gui.ui.install_location")
        });
        let mut line_rect = ui.available_rect_before_wrap();
        line_rect.max.y = ui.cursor().min.y + 25.0;
        line_rect.min.y -= 6.0;
        let mut child = ui.new_child(UiBuilder::new().max_rect(line_rect));
        child.horizontal_centered(|ui| {
            let res = ui.text_edit_singleline(match self.mode {
                Mode::Client => &mut self.client_install_location,
                Mode::Server => &mut self.server_install_location,
                Mode::PrismLauncher => &mut self.mmc_output_location,
            });
            if !res.hovered() && !res.has_focus() {
                ui.painter().rect_stroke(
                    res.rect.expand(ui.visuals().widgets.hovered.expansion),
                    ui.visuals().widgets.hovered.corner_radius,
                    ui.visuals().widgets.hovered.bg_stroke,
                    egui::StrokeKind::Inside,
                );
            }
            if ui.button(t!("gui.ui.button.pick_location")).clicked() {
                let picked = rfd::AsyncFileDialog::new()
                    .set_directory(Path::new(match self.mode {
                        Mode::Client => &self.client_install_location,
                        Mode::Server => &self.server_install_location,
                        Mode::PrismLauncher => &self.mmc_output_location,
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
            ui.spacing_mut().icon_width -= 4.0;
            let mut clicked = false;
            let width = line_rect.width() / 2.0;
            let prev_mode = self.mode;
            ui.scope(|ui| {
                ui.set_max_width(width);
                clicked |= ui
                    .radio_value(&mut self.mode, Mode::Client, t!("gui.mode.client"))
                    .clicked();
            });
            ui.scope(|ui| {
                ui.set_max_width(width);
                clicked |= ui
                    .radio_value(&mut self.mode, Mode::PrismLauncher, t!("gui.mode.prism"))
                    .clicked();
            });
            ui.scope(|ui| {
                ui.set_max_width(width);
                clicked |= ui
                    .radio_value(&mut self.mode, Mode::Server, t!("gui.mode.server"))
                    .clicked();
            });
            if clicked && prev_mode != self.mode {
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
            let res = DropDownBox::from_iter(
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
            .hint_text(RichText::from(t!("gui.ui.search_available_versions")))
            .ui(ui);

            if !res.hovered() && !res.has_focus() {
                ui.painter().rect_stroke(
                    res.rect.expand(ui.visuals().widgets.hovered.expansion),
                    ui.visuals().widgets.hovered.corner_radius,
                    ui.visuals().widgets.hovered.bg_stroke,
                    egui::StrokeKind::Inside,
                );
            }

            let snapshots_clicked = ui
                .checkbox(&mut self.show_snapshots, t!("gui.ui.checkbox.snapshots"))
                .clicked();
            let historical_clicked = ui
                .checkbox(&mut self.show_historical, t!("gui.ui.checkbox.historical"))
                .clicked();

            if snapshots_clicked || historical_clicked {
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
                .height(130.0)
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
                .height(130.0)
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
            let include_flap = self.include_flap;
            let (sender, receiver) = unbounded_channel();
            #[cfg(target_arch = "wasm32")]
            let sender2 = sender.clone();
            let loader_type = self.selected_loader_type.clone();
            let intermediary_version =
                match self.get_intermediary_version(&selected_version, GameSide::Server) {
                    Ok(v) => v,
                    Err(e) => {
                        self.modals
                            .push(ModalPopup::ok(t!("gui.error.installation_failed"), e.0));
                        return;
                    }
                };
            if !include_flap {
                let _ = sender.send((0.0, t!("gui.message.excluding_flap").into()));
            }
            match self.mode {
                Mode::Client => {
                    let location = PathBuf::from(&self.client_install_location);
                    let create_profile = self.create_profile;
                    if !create_profile {
                        let _ = sender.send((0.0, t!("gui.message.not_creating_profile").into()));
                    }
                    let fut = crate::actions::client::install(
                        sender,
                        selected_version,
                        intermediary_version,
                        loader_type,
                        loader_version,
                        None,
                        location,
                        create_profile,
                        include_flap,
                    );

                    #[cfg(target_arch = "wasm32")]
                    {
                        self.installation_task = Some(InstallationProgress::new(receiver));
                        let dialog_sender = self.modal_channel.0.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let res = fut.await;
                            sender2
                                .send((1.1, String::new()))
                                .expect("failed to finish");
                            sender2.closed().await;
                            App::post_installation(res, dialog_sender);
                        });
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        self.installation_task =
                            Some(InstallationProgress::new((receiver, tokio::spawn(fut))));
                    }
                }
                Mode::Server => {
                    let location = Path::new(&self.server_install_location).to_path_buf();
                    let download_server = self.download_minecraft_server;
                    let fut = crate::actions::server::install(
                        sender,
                        selected_version,
                        intermediary_version,
                        loader_type,
                        loader_version,
                        None,
                        location,
                        download_server,
                        include_flap,
                    );
                    #[cfg(target_arch = "wasm32")]
                    {
                        self.installation_task = Some(InstallationProgress::new(receiver));
                        let dialog_sender = self.modal_channel.0.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let res = fut.await;
                            sender2
                                .send((1.1, String::new()))
                                .expect("failed to finish");
                            sender2.closed().await;
                            App::post_installation(res, dialog_sender);
                        })
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        self.installation_task =
                            Some(InstallationProgress::new((receiver, tokio::spawn(fut))));
                    }
                }
                Mode::PrismLauncher => {
                    let location = Path::new(&self.mmc_output_location).to_path_buf();
                    let copy_profile_path = self.copy_generated_location;
                    let generate_zip = self.generate_zip;
                    let fut = crate::actions::prism_pack::install(
                        sender,
                        selected_version,
                        intermediary_version,
                        loader_type,
                        loader_version,
                        location,
                        copy_profile_path,
                        generate_zip,
                        None,
                        include_flap,
                    );
                    #[cfg(target_arch = "wasm32")]
                    {
                        self.installation_task = Some(InstallationProgress::new(receiver));
                        let dialog_sender = self.modal_channel.0.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let res = fut.await;
                            sender2
                                .send((1.1, String::new()))
                                .expect("failed to finish");
                            sender2.closed().await;
                            App::post_installation(res, dialog_sender);
                        })
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        self.installation_task =
                            Some(InstallationProgress::new((receiver, tokio::spawn(fut))));
                    }
                }
            }
        } else {
            self.modals.push(ModalPopup::ok(
                t!("gui.error.installation_failed"),
                t!("gui.error.no_supported_minecraft_version_selected"),
            ));
        }
    }

    fn monitor_installation(&mut self) {
        if let Some(progress) = &self.installation_task {
            if progress.is_finished() {
                let prog = self.installation_task.as_mut().unwrap();
                while !prog.rec().map(|rec| rec.is_empty()).unwrap_or(false) {
                    prog.poll();
                }
                #[cfg(target_arch = "wasm32")]
                let _ = prog.task.take();
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let (_, handle) = prog.task.take().unwrap();
                    let dialog_sender = self.modal_channel.0.clone();
                    tokio::spawn(async move {
                        App::post_installation(handle.await.unwrap(), dialog_sender);
                    });
                }
            }
        }
    }

    fn add_additional_options(&mut self, ui: &mut egui::Ui) {
        let mut line_rect = ui.available_rect_before_wrap();
        line_rect.max.y = ui.cursor().min.y + 25.0;
        line_rect.min.y -= 6.0;
        let mut child = ui.new_child(UiBuilder::new().max_rect(line_rect));
        ui.add_space(20.0);
        child.horizontal_centered(|ui| {
            let flap_checkbox =
                Checkbox::new(&mut self.include_flap, t!("gui.checkbox.include_flap"));
            let flap_box_response = if self.mode == Mode::PrismLauncher {
                ui.add_sized([ui.available_width() / 5.0, 20.0], flap_checkbox)
            } else {
                ui.add(flap_checkbox)
            };
            if flap_box_response.has_focus() || flap_box_response.hovered() {
                Tooltip::for_widget(&flap_box_response)
                    .show(|ui| ui.label(t!("gui.flap.description")));
            }
            match self.mode {
                Mode::Client => {
                    #[cfg(not(target_arch = "wasm32"))]
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
                Mode::PrismLauncher => {
                    ui.add_sized(
                        [ui.available_width() * 2.0 / 3.0, 20.0],
                        Checkbox::new(
                            &mut self.copy_generated_location,
                            t!("gui.checkbox.copy_profile_path"),
                        ),
                    );
                    #[cfg(not(target_arch = "wasm32"))]
                    ui.checkbox(
                        &mut self.generate_zip,
                        t!("gui.checkbox.generate_instance_zip"),
                    );
                }
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

    fn add_output(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            let progress = self.installation_task.as_mut().unwrap();
            progress.poll();

            ui.label(t!("gui.ui.output"));
            let output_height = ui.available_height() - 60.0;
            Frame::new()
                .inner_margin(Margin {
                    left: 0,
                    right: 2,
                    top: 6,
                    bottom: 6,
                })
                .corner_radius(8.0)
                .fill(ui.visuals().widgets.inactive.bg_fill)
                .show(ui, |ui| {
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
                });
            let bar_color = if ui.visuals().dark_mode {
                Color32::DARK_BLUE
            } else {
                Color32::LIGHT_BLUE
            };
            ProgressBar::new(progress.last_progress.min(1.0))
                .desired_width(ui.available_width())
                .animate(true)
                .text(
                    RichText::new(format!(
                        "{}%",
                        (progress.last_progress * 100.0).min(100.0) as i32
                    ))
                    .background_color(bar_color),
                )
                .fill(bar_color)
                .ui(ui);
        });
        ui.vertical_centered(|ui| {
            let mut back = Button::new(RichText::new(t!("gui.button.back")).heading());
            if self
                .installation_task
                .as_ref()
                .map(|t| t.is_running())
                .unwrap_or(false)
            {
                back = back.sense(Sense::empty());
            }
            if back.ui(ui).clicked() {
                self.installation_task = None;
            }
        });
    }

    fn add_language_selector(&mut self, ui: &mut egui::Ui) {
        let mut child =
            ui.new_child(UiBuilder::new().layout(Layout::right_to_left(egui::Align::TOP)));
        let current = &*rust_i18n::locale();
        child.horizontal(|ui| {
            ComboBox::from_id_salt("language")
                .height(130.0)
                .width(20.0)
                .selected_text(t!(format!("language_name"), locale = current))
                .show_ui(ui, |ui| {
                    for ele in rust_i18n::available_locales!() {
                        let mut name = t!(format!("language_name"), locale = ele);
                        if name == t!(format!("language_name"), locale = "en") && ele != "en" {
                            name = std::borrow::Cow::Borrowed(ele);
                        }
                        if ui.selectable_label(ele == current, name).clicked() {
                            rust_i18n::set_locale(ele);
                        }
                    }
                });
            ui.label(t!("gui.ui.language"));
        });
    }

    fn post_installation(result: Result<(), InstallerError>, dialog_sender: Sender<ModalPopup>) {
        match result {
            Err(e) => {
                error!("{}", e.0);
                let _ = dialog_sender.send(ModalPopup::ok(
                    t!("gui.error.installation_failed"),
                    t!("gui.error.failed_to_install", error = e.0),
                ));
            }
            Ok(_) => {
                let s = dialog_sender.clone();
                let _ = dialog_sender.send(ModalPopup::yesno(
                    t!("gui.dialog.installation_successful").to_string(),
                    t!("gui.dialog.installation_successful.message").to_string(),
                    Box::new(move |res| {
                        if res == MessageDialogResult::Yes || res == MessageDialogResult::Ok {
                            if webbrowser::open(crate::OSL_MODRINTH_URL).is_err() {
                                let _ = s.send(ModalPopup::ok(
                                    t!("gui.error.failed_to_open_modrinth"),
                                    t!(
                                        "gui.error.failed_to_open_modrinth.message",
                                        osl_url = crate::OSL_MODRINTH_URL
                                    ),
                                ));
                            }
                        }
                    }),
                ));
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_zoom_factor(1.5);
        ctx.options_mut(|opt| opt.fallback_theme = Theme::Light);

        if let Ok(result) = self.file_picker_channel.1.try_recv() {
            self.file_picker_open = false;
            if let Some(result) = result {
                match result.mode {
                    Mode::Client => self.client_install_location = result.path,
                    Mode::Server => self.server_install_location = result.path,
                    Mode::PrismLauncher => self.mmc_output_location = result.path,
                }
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.style_mut().interaction.selectable_labels = false;
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.add_enabled_ui(!self.file_picker_open, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading(t!("gui.ui.title"));
                });
                ui.add_space(15.0);
                if self.installation_task.is_some() {
                    self.add_output(ui);
                    return;
                }
                self.add_language_selector(ui);

                ui.vertical(|ui| {
                    self.add_environment_options(ui);

                    ui.add_space(15.0);
                    self.add_minecraft_version(ui);
                    ui.add_space(15.0);
                    self.add_loader(ui);

                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        ui.add_space(15.0);
                        self.add_location_picker(_frame, ui);
                    }
                });

                ui.add_space(15.0);
                self.add_additional_options(ui);

                ui.add_space(15.0);
                ui.vertical_centered(|ui| {
                    #[cfg(target_arch = "wasm32")]
                    let install_text = t!("gui.button.install_web");
                    #[cfg(not(target_arch = "wasm32"))]
                    let install_text = t!("gui.button.install");
                    if Button::new(RichText::new(install_text).heading())
                        .min_size(Vec2::new(100.0, 0.0))
                        .ui(ui)
                        .clicked()
                    {
                        self.run_installation();
                    }
                });
            });
        });
        match self.modal_channel.1.try_recv() {
            Ok(modal) => {
                info!("Displaying dialog: {}: {}", modal.title, modal.message);
                self.modals.push(modal)
            }
            Err(_) => {}
        }
        for i in 0..self.modals.len() {
            let modal = &self.modals[i];

            let remove =
                Modal::new(Id::new(modal.title.clone() + &modal.message)).show(ctx, |ui| {
                    ui.style_mut().interaction.selectable_labels = false;
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    ui.vertical_centered(|ui| ui.heading(&modal.title));
                    ui.add_space(15.0);
                    ui.label(&modal.message);
                    ui.add_space(15.0);
                    ui.set_max_width(ui.max_rect().width() - 40.0);
                    return ui
                        .horizontal(|ui| match &modal.buttons {
                            MessageButtons::Ok => {
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 20.0],
                                        Button::new(t!("gui.button.ok")),
                                    )
                                    .clicked()
                                {
                                    return Some(MessageDialogResult::Ok);
                                }
                                return None;
                            }
                            MessageButtons::OkCancel => {
                                if ui
                                    .add_sized(
                                        [ui.available_width() / 2.0, 20.0],
                                        Button::new(t!("gui.button.ok")),
                                    )
                                    .clicked()
                                {
                                    return Some(MessageDialogResult::Ok);
                                }
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 20.0],
                                        Button::new(t!("gui.button.cancel")),
                                    )
                                    .clicked()
                                {
                                    return Some(MessageDialogResult::Cancel);
                                }
                                return None;
                            }
                            MessageButtons::YesNo => {
                                if ui
                                    .add_sized(
                                        [ui.available_width() / 2.0, 20.0],
                                        Button::new(t!("gui.button.yes")),
                                    )
                                    .clicked()
                                {
                                    return Some(MessageDialogResult::Yes);
                                }
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 20.0],
                                        Button::new(t!("gui.button.no")),
                                    )
                                    .clicked()
                                {
                                    return Some(MessageDialogResult::No);
                                }
                                return None;
                            }
                            MessageButtons::YesNoCancel => {
                                if ui
                                    .add_sized(
                                        [ui.available_width() / 3.0, 20.0],
                                        Button::new(t!("gui.button.yes")),
                                    )
                                    .clicked()
                                {
                                    return Some(MessageDialogResult::Yes);
                                }
                                if ui
                                    .add_sized(
                                        [ui.available_width() / 2.0, 20.0],
                                        Button::new(t!("gui.button.no")),
                                    )
                                    .clicked()
                                {
                                    return Some(MessageDialogResult::No);
                                }
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 20.0],
                                        Button::new(t!("gui.button.cancel")),
                                    )
                                    .clicked()
                                {
                                    return Some(MessageDialogResult::Cancel);
                                }
                                return None;
                            }
                            MessageButtons::OkCustom(text) => {
                                if ui
                                    .add_sized([ui.available_width(), 20.0], Button::new(text))
                                    .clicked()
                                {
                                    return Some(MessageDialogResult::Ok);
                                }
                                return None;
                            }
                            MessageButtons::OkCancelCustom(ok_text, cancel_text) => {
                                if ui
                                    .add_sized(
                                        [ui.available_width() / 2.0, 20.0],
                                        Button::new(ok_text),
                                    )
                                    .clicked()
                                {
                                    return Some(MessageDialogResult::Ok);
                                }
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 20.0],
                                        Button::new(cancel_text),
                                    )
                                    .clicked()
                                {
                                    return Some(MessageDialogResult::Cancel);
                                }
                                return None;
                            }
                            MessageButtons::YesNoCancelCustom(yes, no, cancel) => {
                                if ui
                                    .add_sized([ui.available_width() / 3.0, 20.0], Button::new(yes))
                                    .clicked()
                                {
                                    return Some(MessageDialogResult::Yes);
                                }
                                if ui
                                    .add_sized([ui.available_width() / 2.0, 20.0], Button::new(no))
                                    .clicked()
                                {
                                    return Some(MessageDialogResult::No);
                                }
                                if ui
                                    .add_sized([ui.available_width(), 20.0], Button::new(cancel))
                                    .clicked()
                                {
                                    return Some(MessageDialogResult::Cancel);
                                }
                                return None;
                            }
                        })
                        .inner;
                });
            if let Some(result) = remove.inner {
                let m = self.modals.remove(i);
                (m.after)(result);
            }
        }

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
                        let ele_response = display(ui, text);

                        if ele_response.clicked() {
                            *buf = text.to_owned();
                            changed = true;

                            *open = false;
                        }
                        if ele_response.gained_focus() && !ui.is_rect_visible(ele_response.rect) {
                            ele_response.scroll_to_me(Some(Align::BOTTOM));
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
