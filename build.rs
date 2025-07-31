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
}
