use log::info;

mod actions;
mod errors;
mod net;
mod ui;

static VERSION: &str = env!("CARGO_PKG_VERSION");
static USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);
static ORNITHE_ICON_BYTES: &[u8] = include_bytes!("../res/icon.png");
const OSL_MODRINTH_URL: &str = "https://modrinth.com/mod/osl";

#[macro_use]
extern crate rust_i18n;
i18n!("locales", fallback = "en", minify_key = true);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(main)]
fn main() {
    main0();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    main0();
}

fn main0() {
    #[cfg(target_arch = "wasm32")]
    {
        std::panic::set_hook(Box::new(console_error_panic_hook::hook));
        console_log::init().expect("Failed to setup logger!");
        /*let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            tokio::task::LocalSet::new()
                .run_until(start_installer())
                .await;
        });*/
        wasm_bindgen_futures::spawn_local(start_installer());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        env_logger::init_from_env(
            env_logger::Env::default().default_filter_or("ornithe_installer_rs=info"),
        );
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(start_installer());
    }
}

async fn start_installer() {
    rust_i18n::set_locale("en");

    info!("Ornithe Installer v{}", VERSION);

    // The first argument is the binary name
    #[cfg(feature = "gui")]
    if std::env::args().count() <= 1 {
        #[cfg(windows)]
        hide_console_ng::hide_console();
        if let Ok(_) = crate::ui::gui::run().await {
            return;
        }
        #[cfg(windows)]
        hide_console_ng::show_unconditionally();
    }

    crate::ui::cli::run().await
}
