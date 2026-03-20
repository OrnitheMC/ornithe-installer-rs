mod actions;
mod errors;
mod net;
mod ui;

pub static VERSION: &str = env!("CARGO_PKG_VERSION");
pub static USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);
pub static ORNITHE_ICON_BYTES: &[u8] = include_bytes!("../res/icon.png");
pub const OSL_MODRINTH_URL: &str = "https://modrinth.com/mod/osl";

#[macro_use]
extern crate rust_i18n;
i18n!("locales", fallback = "en", minify_key = true);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(main)]
fn main() {
    #[cfg(feature = "gui")]
    eframe::web::PanicHandler::install();
    console_log::init().expect("Failed to setup logger!");
    console_error_panic_hook::set_once();
    wasm_bindgen_futures::spawn_local(start_installer());
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() {
    env_logger::init_from_env(
        env_logger::Env::default().default_filter_or("ornithe_installer_rs=info"),
    );
    start_installer().await;
}

async fn start_installer() {
    rust_i18n::set_locale("en");

    // The first argument is the binary name
    #[cfg(feature = "gui")]
    {
        #[cfg(target_arch = "wasm32")]
        let gui = web_sys::window()
            .expect("Window not available")
            .location()
            .search()
            .unwrap_or(String::new())
            .is_empty();
        #[cfg(not(target_arch = "wasm32"))]
        let gui = std::env::args().count() <= 1;
        if gui {
            #[cfg(windows)]
            hide_console_ng::hide_console();
            log::info!("Ornithe Installer v{}", VERSION);
            if let Ok(_) = crate::ui::gui::run().await {
                return;
            }
            #[cfg(windows)]
            hide_console_ng::show_unconditionally();
        }
    }

    crate::ui::cli::run().await
}
