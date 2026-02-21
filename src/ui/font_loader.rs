use egui::FontData;
use egui::FontFamily::{Monospace, Proportional};
use egui::epaint::text::FontPriority::Lowest;
use egui::epaint::text::{FontInsert, InsertFontFamily};
use serde::Deserialize;
use std::collections::HashMap;
use log::warn;

const FONT_LIST: &str = include_str!("../../res/font/fonts.json");

#[cfg(target_os = "windows")]
const WINDOWS_FONT_PATH: &str = r"C:\Windows\Fonts\";
#[cfg(target_os = "macos")]
const MACOS_FONT_PATH: &str = "/System/Library/Fonts/";
#[cfg(target_os = "macos")]
const MACOS_FONT_PATH_SHARED: &str = "/Library/Fonts/";

#[derive(Deserialize)]
struct SystemFontList {
    #[cfg(target_os = "windows")]
    windows: PlatformFonts,
    #[cfg(target_os = "linux")]
    linux: PlatformFonts,
    #[cfg(target_os = "macos")]
    macos: PlatformFonts,
}

type PlatformFonts = HashMap<String, Vec<String>>;

pub fn load_system_font_to_egui(ctx: &egui::Context) {
    let system_font = find_system_font();

    if system_font.is_empty() {
        warn!("No system font found, some languages may not display properly.");
        return;
    }

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
}

fn find_system_font() -> HashMap<String, FontData> {
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

    result
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn load_fonts_from_paths(
    platform_fonts: &PlatformFonts,
    search_paths: &[&str],
    result: &mut HashMap<String, FontData>,
) {
    for (language, font_files) in platform_fonts {
        let mut loaded = false;
        for font_file in font_files {
            for search_path in search_paths {
                let font_path = format!("{}{}", search_path, font_file);
                if let Ok(font_data) = std::fs::read(&font_path) {
                    result.insert(language.to_string(), FontData::from_owned(font_data));
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
fn load_fonts_from_fontconfig(
    platform_fonts: &PlatformFonts,
    result: &mut HashMap<String, FontData>,
) {
    use fontconfig::Fontconfig;

    if let Some(fc) = Fontconfig::new() {
        platform_fonts.iter().for_each(|(language, font_names)| {
            for font_name in font_names {
                if let Some(font) = fc.find(font_name, None) {
                    if let Ok(data) = std::fs::read(font.path) {
                        result.insert(language.to_string(), FontData::from_owned(data));
                        break;
                    }
                }
            }
        })
    } else {
        warn!("Failed to init Fontconfig")
    }
}
