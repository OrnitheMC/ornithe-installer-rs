use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    fmt::{Display, Write},
    path::PathBuf,
    rc::Rc,
};

use log::info;
use tokio::sync::mpsc::unbounded_channel;
use wasm_bindgen::{JsCast, JsValue, prelude::Closure};
use web_sys::{
    Document, Event, HtmlElement, HtmlInputElement, HtmlProgressElement, HtmlSelectElement, js_sys,
    window,
};

use crate::{
    errors::InstallerError,
    net::{
        self, GameSide,
        manifest::{self, MinecraftVersion},
        meta::{IntermediaryVersion, LoaderType, LoaderVersion},
    },
};

mod panic_handler {
    use wasm_bindgen::prelude::*;

    /// Detects panics and logs them
    /// Derived from https://github.com/emilk/egui/blob/e6eb00a31c7089d4458c55fcbe5f1253311a7176/crates/eframe/src/web/panic_handler.rs (MIT OR Apache-2.0)

    /// Install a panic hook.
    pub fn install() {
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            // Log it using console.error
            log::error!(
                "{}\n\nStack:\n\n{}",
                panic_info.to_string(),
                Error::new().stack(),
            );
            previous_hook(panic_info);
        }));
    }

    #[wasm_bindgen]
    extern "C" {
        type Error;

        #[wasm_bindgen(constructor)]
        fn new() -> Error;

        #[wasm_bindgen(structural, method, getter)]
        fn stack(error: &Error) -> String;
    }
}

const LANGUAGE_COOKIE_NAME: &str = "ornithe-installer-rs_lang";

#[derive(PartialEq, Clone, Copy, Debug)]
enum Mode {
    Client,
    Server,
    PrismLauncher,
}

struct State {
    mode: Cell<Mode>,
    selected_minecraft_version: RefCell<String>,
    available_minecraft_versions: Vec<MinecraftVersion>,
    intermediary_versions: HashMap<String, IntermediaryVersion>,
    available_intermediary_versions: Vec<String>,
    show_snapshots: Cell<bool>,
    show_historical: Cell<bool>,
    selected_loader_type: Cell<LoaderType>,
    selected_loader_version: RefCell<String>,
    available_loader_versions: HashMap<LoaderType, Vec<LoaderVersion>>,
    show_betas: Cell<bool>,
    download_minecraft_server: Cell<bool>,
    include_flap: Cell<bool>,
}

impl State {
    pub fn abort<M: Into<String> + Display>(message: M) -> Result<Self, InstallerError> {
        return Err(InstallerError(message.into()));
    }
    pub async fn create() -> Result<Self, InstallerError> {
        let mut available_minecraft_versions = Vec::new();
        let mut available_intermediary_versions = Vec::new();
        let available_loader_versions;
        let mut intermediary_versions = HashMap::new();
        let manifest_future = manifest::fetch_versions(&None);
        let intermediary_future = net::meta::fetch_intermediary_versions(&None);
        let loader_future = net::meta::fetch_loader_versions(&None);

        info!("Loading versions...");
        match manifest_future.await {
            Ok(versions) => {
                for ele in versions.versions {
                    available_minecraft_versions.push(ele);
                }
            }
            _ => {
                return Self::abort(t!("gui.error.loading.minecraft_versions"));
            }
        }

        match intermediary_future.await {
            Ok(versions) => {
                for v in versions {
                    if v.1.stable {
                        available_intermediary_versions.push(v.0.clone());
                        intermediary_versions.insert(v.0, v.1);
                    }
                }
            }
            _ => {
                return Self::abort(t!("gui.error.loading.intermediary_versions"));
            }
        }
        if available_minecraft_versions.is_empty() {
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

        match loader_future.await {
            Ok(versions) => {
                available_loader_versions = versions;
            }
            _ => {
                return Self::abort(t!("gui.error.loading.loader_versions"));
            }
        }
        info!(
            "Loaded versions for {} loaders",
            available_loader_versions.len()
        );
        Ok(State {
            mode: Cell::new(Mode::Client),
            selected_minecraft_version: RefCell::new(String::new()),
            available_minecraft_versions,
            intermediary_versions,
            available_intermediary_versions,
            show_snapshots: Cell::new(false),
            show_historical: Cell::new(false),
            selected_loader_type: Cell::new(LoaderType::Fabric),
            selected_loader_version: RefCell::new(
                available_loader_versions
                    .get(&LoaderType::Fabric)
                    .map(|v| v.first().unwrap().version.clone())
                    .unwrap_or(String::new()),
            ),
            available_loader_versions,
            show_betas: Cell::new(false),
            download_minecraft_server: Cell::new(true),
            include_flap: Cell::new(true),
        })
    }
}

fn handle_error(initialized: bool, error: InstallerError) {
    if !initialized {
        log::error!("Failed to load installer: {}", error.0);
        if let Some(loading_text) = get_document().get_element_by_id("loading_text") {
            loading_text.set_inner_html(&format!(
                "<h3>{}:</h3><p style=\"overflow: scroll;\">{}</p>",
                t!("gui.error.loading"),
                &error.0
            ));
        }
        let _ = window()
            .expect("Window unavailable")
            .alert_with_message(&format!("{}:\n\n{}", t!("gui.error.loading"), error.0));
        return;
    }
    display_error(t!("gui.error.loading"), error.0);
}

fn display_error(title: impl Into<String>, message: impl Into<String>) {
    let title = title.into();
    let message = message.into();
    log::error!("Error: {}: {}", title, message);
    let errors_div = get_document()
        .get_element_by_id("errors")
        .unwrap()
        .dyn_into::<HtmlElement>()
        .unwrap();
    let _ = errors_div.style().set_property("display", "block");
    errors_div.set_inner_html(&format!(
        r#"
    <b style="font-weight: 1em;">{}</b>
    <p style="margin-block-end: 0;">{}</p>
    "#,
        title, message
    ));
    errors_div.scroll_into_view();
}

fn update_progress(progress: f32, status: &str) {
    let document = get_document();
    let progress_bar = document
        .get_element_by_id("output_progress")
        .unwrap()
        .dyn_into::<HtmlProgressElement>()
        .unwrap();
    progress_bar.set_value(progress as f64);
    let output_log = document
        .get_element_by_id("output")
        .unwrap()
        .dyn_into::<HtmlElement>()
        .unwrap();
    let output_pane = document
        .get_element_by_id("output_pane")
        .unwrap()
        .dyn_into::<HtmlElement>()
        .unwrap();
    let _ = output_pane.style().set_property("display", "block");
    if !status.is_empty() {
        if !output_log.inner_text().is_empty() {
            let _ = output_log.insert_adjacent_text("beforeend", "\n");
        }
        let _ = output_log.insert_adjacent_text("beforeend", status);
    }
    output_pane.scroll_into_view_with_bool(false);
}

pub async fn run() {
    panic_handler::install();
    if let Err(e) = run0().await {
        handle_error(false, e);
    }
}

async fn run0() -> Result<(), InstallerError> {
    let search = web_sys::window()
        .expect("Window not available")
        .location()
        .search()
        .unwrap_or(String::new());
    let mut used_lang_from_query = false;
    if !search.is_empty() {
        let queries = search[1..].split("&").collect::<Vec<&str>>();

        for params in queries {
            let mut s = params.split("=");
            if let Some(name) = s.next()
                && name == "lang"
                && let Some(value) = s.next()
            {
                used_lang_from_query = true;
                rust_i18n::set_locale(value);
            }
        }
    }
    if !used_lang_from_query
        && let Some(previous_locale) = window()
            .unwrap()
            .cookie_store()
            .get_with_name(LANGUAGE_COOKIE_NAME)
            .await
            .and_then(|o| js_sys::Reflect::get(&o, &JsValue::from_str("value")))
            .ok()
            .and_then(|v| v.as_string())
    {
        rust_i18n::set_locale(&previous_locale);
    }

    let mut state = Rc::new(State::create().await?);
    initialize(&mut state)?;
    Ok(())
}

fn initialize(mut state: &mut Rc<State>) -> Result<(), InstallerError> {
    setup()?;
    update_minecraft_versions(&mut state);
    update_loader_versions(&mut state);
    update_options(&mut state);
    setup_callbacks(&mut state);
    Ok(())
}

fn setup() -> Result<(), InstallerError> {
    let available_locales = rust_i18n::available_locales!();
    let document = get_document();
    let body = document
        .get_element_by_id("loading_text")
        .and_then(|e| e.parent_element())
        .or(document.get_element_by_id("web_installer"))
        .and_then(|e| e.parent_element())
        .unwrap();
    let _ = body.set_attribute("style", "display: flex; justify-content: center;");
    let mut html = String::new();
    html.write_str(
        r#"
        <style>
        #web_installer {
            color: #f0f0f0;
            font-size: 24px;
            font-family: Ubuntu-Light, Helvetica, sans-serif;
            position: absolute;
        }

        h3 {
            margin-block: 1em 2px;
        }

        #env_header {
            display: flex;
            align-items: center;
        }

        #language_container {
            margin-block: 1em 2px;
        }

        #options {
            margin-block-start: 1em;
        }

        .info_pane {
            border-radius: 12px;
            padding: 10px;
        }

        #output_pane_content {
            outline: #525252 solid;
            background: #777777;
            font-size: 16px;
        }

        #output {
            overflow: scroll;
            white-space: pre-wrap;
        }

        #errors {
            outline: #f00 solid;
            background: #ff000080;
            color: #2b2b2b;
            margin-block-start: 1em;
        }

        #download {
            margin: 30px;
            padding: 0 20px 0 20px;
            font-weight: bold;
            font-size: 30px;
        }

        #output_progress {
            width: 100%;
        }
        </style>
        <div id="web_installer">"#,
    )?;
    write!(
        html,
        r#"<h1 style="text-align: center">{}</h1>"#,
        t!("gui.ui.title")
    )?;
    write!(html, "<div id=\"inputs\">")?;
    write!(
        html,
        r#"
        <div id="env_header">
            <h3 style="flex-grow: 1;">{}</h3>
            <div id="language_container">
                <label for="language_selector">{}</label>
                <select id="language_selector">{}</select>
            </div>
        </div>
        "#,
        t!("gui.ui.environment"),
        t!("gui.ui.language"),
        available_locales
            .iter()
            .map(|s| format!(
                "<option id=\"{}\">{}</option>",
                s,
                t!("language_name", locale = s)
            ))
            .collect::<String>()
    )?;
    write!(
        html,
        r#"
            <div id="env">
                <input type="radio" checked id="env_client">
                <label for="env_client">{}</label>
                <input type="radio" id="env_prism">
                <label for="env_prism">{}</label>
                <input type="radio" id="env_server">
                <label for="env_server">{}</label>
            </div>"#,
        t!("gui.mode.client"),
        t!("gui.mode.prism"),
        t!("gui.mode.server"),
    )?;
    write!(
        html,
        r#"<h3>{}</h3>
            <div id="mc_version_options">
                <input list="mc_versions" id="mc_version" placeholder="{}"></input>
                <input id="mc_snapshots" type="checkbox" />
                <label for="mc_snapshots">{}</label>
                <input id="mc_historical" type="checkbox" />
                <label for="mc_historical">{}</label>
                <datalist id="mc_versions">
                    <!--Filled in using some magic below-->
                </datalist>
            </div>"#,
        t!("gui.ui.minecraft_version"),
        t!("gui.ui.search_available_versions"),
        t!("gui.ui.checkbox.snapshots"),
        t!("gui.ui.checkbox.historical")
    )?;
    write!(
        html,
        r#"<h3>{}</h3>
            <div id="loader">
                <select id="loader_type">
                    <option value="fabric">{}</option>
                    <option value="quilt">{}</option>
                </select>
                <select id="loader_versions">
                    <!--Filled in using some magic below-->
                </select>
                <input type="checkbox" id="loader_betas">{}</input>
            </div>"#,
        t!("gui.ui.loader"),
        t!(
            "gui.ui.selection.loader.name",
            name = LoaderType::Fabric.get_localized_name()
        ),
        t!(
            "gui.ui.selection.loader.name",
            name = LoaderType::Quilt.get_localized_name()
        ),
        t!("gui.ui.show_loader_betas")
    )?;
    write!(
        html,
        r#"<div id="options">
            <input type="checkbox" checked id="include_flap" />
            <label for="include_flap">{}</label>
            <input type="checkbox" checked id="download_server" style="display: none;" />
            <label id="download_server_label" for="download_server" style="display: none;">{}</label>
        </div>"#,
        t!("gui.checkbox.include_flap"),
        t!("gui.checkbox.download_minecraft_server")
    )?;
    write!(html, "</div>")?;
    write!(
        html,
        r#"<div id="output_pane" style="display: none;">
            <h3>{}</h3>
            <div class="info_pane" id="output_pane_content">
                <pre id="output"></pre>
                <progress id="output_progress" max="1"></progress>
            </div>
        </div>"#,
        t!("gui.ui.output")
    )?;
    write!(
        html,
        r#"<div class="info_pane" id="errors" style="display: none;"></div>"#
    )?;
    write!(
        html,
        r#"
        <div style="text-align: center;">
            <button id="download">{}</button>
        </div>
        "#,
        t!("gui.button.install_web")
    )?;
    write!(html, "</div>")?;
    let mut current_locale_index = 0;
    let current_locale = rust_i18n::locale();
    for locale in available_locales {
        if *current_locale == locale {
            break;
        }
        current_locale_index += 1;
    }
    info!(
        "Current locale: {} ({current_locale_index})",
        current_locale.to_owned()
    );
    body.set_inner_html(&html);
    get_document()
        .get_element_by_id("language_selector")
        .unwrap()
        .dyn_into::<HtmlSelectElement>()
        .unwrap()
        .set_selected_index(current_locale_index);

    Ok(())
}

fn update_options(state: &mut Rc<State>) {
    let _ = get_document()
        .get_element_by_id("download_server_label")
        .unwrap()
        .set_attribute(
            "style",
            if state.mode.get() == Mode::Server {
                ""
            } else {
                "display: none;"
            },
        );
    let _ = get_document()
        .get_element_by_id("download_server")
        .unwrap()
        .set_attribute(
            "style",
            if state.mode.get() == Mode::Server {
                ""
            } else {
                "display: none;"
            },
        );
}

fn update_loader_versions(state: &mut Rc<State>) {
    let selected_type = state.selected_loader_type.get();
    let available = match state.available_loader_versions.get(&selected_type) {
        Some(vec) => vec,
        None => &Vec::new(),
    };
    get_document()
        .get_element_by_id("loader_versions")
        .unwrap()
        .set_inner_html(
            &available
                .iter()
                .filter(|v| state.show_betas.get() || v.is_stable())
                .map(|s| format!("<option value=\"{}\">{}</option>", s.version, s.version))
                .collect::<String>(),
        );
}

fn update_minecraft_versions(state: &mut Rc<State>) {
    let filtered = state
        .available_minecraft_versions
        .iter()
        .filter(|v| {
            state.available_intermediary_versions.contains(&v.id)
                || state.available_intermediary_versions.contains(
                    &(v.id.clone()
                        + "-"
                        + match state.mode.get() {
                            Mode::Server => "server",
                            _ => "client",
                        }),
                )
        })
        .filter(|v| {
            if state.show_snapshots.get() && state.show_historical.get() {
                return true;
            }
            let mut displayed = v.is_release();
            if !displayed && state.show_snapshots.get() {
                displayed = v.is_snapshot();
            }
            if !displayed && state.show_historical.get() {
                displayed = v.is_historical();
            }
            displayed
        })
        .map(|v| v.id.clone())
        .collect::<Vec<String>>();
    get_document()
        .get_element_by_id("mc_versions")
        .unwrap()
        .set_inner_html(
            &filtered
                .iter()
                .map(|s| format!("<option value=\"{}\">", s))
                .collect::<String>(),
        );
    info!(
        "Filtered {} valid minecraft versions to display out of {} total",
        filtered.len(),
        state.available_minecraft_versions.len()
    );
}

fn setup_callbacks(state: &mut Rc<State>) {
    set_onchange(
        state,
        "language_selector",
        Box::new(|mut s, e| {
            let e = e.dyn_into::<HtmlSelectElement>().unwrap();
            let selected = rust_i18n::available_locales!()
                .iter()
                .skip(e.selected_index() as usize)
                .next()
                .map(|s| s.clone())
                .unwrap_or(std::borrow::Cow::Borrowed("en"));
            info!("Setting locale to {}", selected);
            rust_i18n::set_locale(&selected);
            let _ = window()
                .unwrap()
                .cookie_store()
                .set_with_name_and_value(LANGUAGE_COOKIE_NAME, &selected);
            if let Err(e) = initialize(&mut s) {
                handle_error(false, e);
            }
        }),
    );
    set_onchange(
        state,
        "env_client",
        Box::new(|mut s, _| {
            unselect_element("env_prism");
            unselect_element("env_server");
            s.mode.set(Mode::Client);
            update_minecraft_versions(&mut s);
            update_options(&mut s);
        }),
    );
    set_onchange(
        state,
        "env_prism",
        Box::new(|mut s, _| {
            unselect_element("env_client");
            unselect_element("env_server");
            s.mode.set(Mode::PrismLauncher);
            update_minecraft_versions(&mut s);
            update_options(&mut s);
        }),
    );
    set_onchange(
        state,
        "env_server",
        Box::new(|mut s, _| {
            unselect_element("env_client");
            unselect_element("env_prism");
            s.mode.set(Mode::Server);
            update_minecraft_versions(&mut s);
            update_options(&mut s);
        }),
    );
    set_onchange(
        state,
        "mc_version",
        Box::new(|s, e| {
            let mut borrow = s.selected_minecraft_version.borrow_mut();
            *borrow = e.dyn_into::<HtmlInputElement>().unwrap().value();
            drop(borrow);
            info!(
                "Selected minecraft version: {}",
                s.selected_minecraft_version.borrow()
            );
        }),
    );
    set_onchange(
        state,
        "mc_snapshots",
        Box::new(|mut s, e| {
            s.show_snapshots
                .set(e.dyn_into::<HtmlInputElement>().unwrap().checked());
            update_minecraft_versions(&mut s);
        }),
    );
    set_onchange(
        state,
        "mc_historical",
        Box::new(|mut s, e| {
            s.show_historical
                .set(e.dyn_into::<HtmlInputElement>().unwrap().checked());
            update_minecraft_versions(&mut s);
        }),
    );
    set_onchange(
        state,
        "loader_type",
        Box::new(|mut s, e| {
            s.selected_loader_type.set(
                match e.dyn_into::<HtmlSelectElement>().unwrap().value().as_str() {
                    "quilt" => LoaderType::Quilt,
                    _ => LoaderType::Fabric,
                },
            );
            update_loader_versions(&mut s);
        }),
    );
    set_onchange(
        state,
        "loader_betas",
        Box::new(|mut s, e| {
            s.show_betas
                .set(e.dyn_into::<HtmlInputElement>().unwrap().checked());
            update_loader_versions(&mut s);
        }),
    );
    set_onchange(
        state,
        "include_flap",
        Box::new(|s, e| {
            s.include_flap
                .set(e.dyn_into::<HtmlInputElement>().unwrap().checked());
        }),
    );
    set_onchange(
        state,
        "download_server",
        Box::new(|s, e| {
            s.download_minecraft_server
                .set(e.dyn_into::<HtmlInputElement>().unwrap().checked());
        }),
    );
    set_onclick(
        state,
        "download",
        Box::new(|s, _| {
            wasm_bindgen_futures::spawn_local(async move {
                if let Err(e) = run_installation(s).await {
                    handle_error(true, e);
                }
            });
        }),
    );
}

fn get_document() -> Document {
    let window = window().expect("Window unavailable");
    window.document().expect("Document unavailable")
}

fn unselect_element(element_id: &str) {
    get_document()
        .get_element_by_id(element_id)
        .unwrap()
        .dyn_into::<HtmlInputElement>()
        .unwrap()
        .set_checked(false);
}

fn set_onclick(
    state: &mut Rc<State>,
    element_id: &str,
    func: Box<dyn FnMut(Rc<State>, HtmlElement)>,
) {
    set_event_listener(state, element_id, "click", func);
}

fn set_onchange(
    state: &mut Rc<State>,
    element_id: &str,
    func: Box<dyn FnMut(Rc<State>, HtmlElement)>,
) {
    set_event_listener(state, element_id, "change", func);
}

fn set_event_listener(
    state: &mut Rc<State>,
    element_id: &str,
    event_id: &str,
    mut func: Box<dyn FnMut(Rc<State>, HtmlElement)>,
) {
    if let Some(ele) = get_document().get_element_by_id(element_id) {
        let element = ele.dyn_into::<HtmlElement>().unwrap();

        let state = state.clone();
        let id = element_id.to_owned();
        let closure = Closure::<dyn FnMut(_)>::new(move |_event: Event| {
            let element = get_document()
                .get_element_by_id(&id)
                .unwrap()
                .dyn_into::<HtmlElement>()
                .unwrap();
            func(state.clone(), element)
        });
        element
            .add_event_listener_with_callback(event_id, closure.as_ref().unchecked_ref())
            .unwrap();
        closure.forget();
    } else {
        info!("Cannot find element with id {element_id}!")
    }
}

async fn run_installation(state: Rc<State>) -> Result<(), InstallerError> {
    let errors_div = get_document()
        .get_element_by_id("errors")
        .unwrap()
        .dyn_into::<HtmlElement>()
        .unwrap();
    errors_div.set_inner_text("");
    let _ = errors_div.style().set_property("display", "none");
    if let Some(version) = state
        .available_minecraft_versions
        .iter()
        .find(|v| &v.id == state.selected_minecraft_version.borrow().as_str())
    {
        info!("Starting installation!");
        let (send, mut recv) = unbounded_channel();
        let selected_version = version.clone();
        let loader_version = state
            .available_loader_versions
            .get(&state.selected_loader_type.get())
            .unwrap()
            .iter()
            .find(|v| &v.version == state.selected_loader_version.borrow().as_str())
            .unwrap()
            .clone();
        let include_flap = state.include_flap.get();
        let intermediary_version = match crate::ui::get_intermediary_version(
            state.intermediary_versions.clone(),
            &selected_version,
            match state.mode.get() {
                Mode::Server => GameSide::Server,
                _ => GameSide::Client,
            },
        ) {
            Ok(v) => v,
            Err(e) => {
                handle_error(true, e);
                return Ok(());
            }
        };
        let loader_type = state.selected_loader_type.get();
        if !include_flap {
            let _ = send.send((0.0, t!("gui.message.excluding_flap").into()));
        }
        let fut = async {
            match state.mode.get() {
                Mode::Client => {
                    crate::actions::client::install(
                        send,
                        selected_version,
                        intermediary_version,
                        loader_type,
                        loader_version,
                        None,
                        PathBuf::new(),
                        false,
                        include_flap,
                    )
                    .await
                }
                Mode::Server => {
                    let download_server = state.download_minecraft_server.get();
                    crate::actions::server::install(
                        send,
                        selected_version,
                        intermediary_version,
                        loader_type,
                        loader_version,
                        None,
                        PathBuf::new(),
                        download_server,
                        include_flap,
                    )
                    .await
                }
                Mode::PrismLauncher => {
                    crate::actions::prism_pack::install(
                        send,
                        selected_version,
                        intermediary_version,
                        loader_type,
                        loader_version,
                        PathBuf::new(),
                        false,
                        true,
                        None,
                        include_flap,
                    )
                    .await
                }
            }
        };
        let mut pinned = std::pin::pin!(fut);
        loop {
            tokio::select! {
                biased;
                Some((progress, status)) = recv.recv() => {
                    update_progress(progress, &status);
                }
                res = pinned.as_mut() => {
                    if !recv.is_empty() {
                        while !recv.is_empty() {
                            if let Some((progress, status)) = recv.recv().await {
                                update_progress(progress, &status);
                            }
                        }
                    }
                    return res;
                }
            }
        }
    } else {
        display_error(
            t!("gui.error.installation_failed"),
            t!("gui.error.no_supported_minecraft_version_selected"),
        );
    }
    Ok(())
}
