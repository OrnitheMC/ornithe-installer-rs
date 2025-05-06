use env_logger::Env;
use log::info;

mod actions;
mod errors;
mod net;
mod ui;

static VERSION: &str = env!("CARGO_PKG_VERSION");
static USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);
static ORNITHE_ICON_BYTES: &[u8] = include_bytes!("../res/icon.png");
const OSL_MODRINTH_URL: &str = "https://modrinth.com/mod/osl";

#[tokio::main]
async fn main() {
    env_logger::init_from_env(Env::default().default_filter_or("ornithe_installer_rs=info"));

    info!("Ornithe Installer v{}", VERSION);

    // The first argument is the binary name
    #[cfg(feature = "gui")]
    if std::env::args().count() <= 1 {
        if let Ok(_) = crate::ui::gui::run().await {
            return;
        }
    }

    crate::ui::cli::run().await
}
