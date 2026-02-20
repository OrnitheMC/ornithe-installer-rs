use egui::FontData;
use egui::FontFamily::{Monospace, Proportional};
use egui::epaint::text::FontPriority::Lowest;
use egui::epaint::text::{FontInsert, InsertFontFamily};
use serde::Deserialize;
use std::collections::HashMap;

const FONT_LIST: &str = include_str!("../../res/font/fonts.json");

const WINDOWS_FONT_PATH: &str = r"C:\Windows\Fonts\";
const MACOS_FONT_PATH: &str = "/System/Library/Fonts/";
const MACOS_FONT_PATH_SHARED: &str = "/Library/Fonts/";

#[derive(Deserialize)]
struct SystemFontList {
    windows: PlatformFonts,
    linux: PlatformFonts,
    macos: PlatformFonts,
}

type PlatformFonts = HashMap<String, Vec<String>>;

pub fn load_system_font_to_egui(ctx: &egui::Context) -> Result<(), String> {
    let system_font = find_system_font()?;

    for font in system_font {
        let font_insert = FontInsert::new(
            &font.0,
            font.1,
            vec![
                InsertFontFamily {
                    family: Proportional,
                    priority: Lowest, // low priority to not override existing fonts
                },
                InsertFontFamily {
                    family: Monospace,
                    priority: Lowest,
                },
            ],
        );

        ctx.add_font(font_insert);
    }
    Ok(())
}

fn find_system_font() -> Result<HashMap<String, FontData>, String> {
    let sys_font_list: SystemFontList =
        serde_json::from_str(FONT_LIST).expect("failed to parse font list");

    let mut result: HashMap<String, FontData> = HashMap::new();

    #[cfg(target_os = "windows")]
    {
        load_fonts_from_paths(&sys_font_list.windows, &[WINDOWS_FONT_PATH], &mut result);
    }

    #[cfg(target_os = "macos")]
    {
        load_fonts_from_paths(
            &sys_font_list.macos,
            &[MACOS_FONT_PATH, MACOS_FONT_PATH_SHARED],
            &mut result,
        );
    }

    #[cfg(target_os = "linux")]
    {
        // use fontconfig for linux fo find fonts
        load_fonts_from_fontconfig(&sys_font_list.linux, &mut result);
    }

    Ok(result)
}

fn load_fonts_from_paths(
    platform_fonts: &PlatformFonts,
    search_paths: &[&str],
    result: &mut HashMap<String, FontData>,
) {
    for (_language, font_files) in platform_fonts {
        let mut loaded = false;
        for font_file in font_files {
            for search_path in search_paths {
                let font_path = format!("{}{}", search_path, font_file);
                if let Ok(font_data) = std::fs::read(&font_path) {
                    result.insert(_language.to_string(), FontData::from_owned(font_data));
                    loaded = true;
                    break;
                }
            }
            if loaded {
                break;
            }
        }
    }
}

#[cfg(all(target_os = "linux", feature = "gui"))]
fn load_fonts_from_fontconfig(platform_fonts: &PlatformFonts, result: &mut HashMap<String, FontData>) {
    use fontconfig::Fontconfig;
    let fc = Fontconfig::new().unwrap();

    for (_language, font_names) in platform_fonts {
        for font_name in font_names {
            if let Some(font) = fc.find(font_name, None) {
                if let Ok(data) = std::fs::read(font.path) {
                    result.insert(_language.to_string(), FontData::from_owned(data));
                    break;
                }
            }
        }
    }
}
