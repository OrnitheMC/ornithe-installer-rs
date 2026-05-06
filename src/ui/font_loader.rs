use egui::{
    FontData,
    FontFamily::{Monospace, Proportional},
    epaint::text::{FontInsert, FontPriority::Lowest, InsertFontFamily},
};
use serde::Deserialize;
use std::collections::HashMap;

const FONT_LIST: &str = include_str!("../../res/font/fonts.json");

#[cfg(windows)]
const WINDOWS_FONT_PATH: &str = r"C:\Windows\Fonts\";
#[cfg(target_os = "macos")]
const MACOS_FONT_PATH: &str = "/System/Library/Fonts/";
#[cfg(target_os = "macos")]
const MACOS_FONT_PATH_SHARED: &str = "/Library/Fonts/";

#[derive(Deserialize)]
struct SystemFontList {
    #[cfg(windows)]
    windows: PlatformFonts,
    #[cfg(target_os = "linux")]
    linux: PlatformFonts,
    #[cfg(target_os = "macos")]
    macos: PlatformFonts,
    #[cfg(target_arch = "wasm32")]
    web: PlatformFonts,
}

type PlatformFonts = HashMap<String, Vec<String>>;

pub fn load_system_font_to_egui(ctx: &egui::Context) {
    #[cfg(target_arch = "wasm32")]
    {
        let sys_font_list: SystemFontList =
            serde_json::from_str(FONT_LIST).expect("failed to parse font list");
        let c = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let mut result: HashMap<String, FontData> = HashMap::new();
            load_fonts_from_url(&sys_font_list.web, &mut result).await;
            for font in result {
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

                c.add_font(font_insert);
            }
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let system_font = find_system_font();

        if system_font.is_empty() {
            log::warn!("No system font found, some languages may not display properly.");
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
}

#[cfg(not(target_arch = "wasm32"))]
fn find_system_font() -> HashMap<String, FontData> {
    let sys_font_list: SystemFontList =
        serde_json::from_str(FONT_LIST).expect("failed to parse font list");

    let mut result: HashMap<String, FontData> = HashMap::new();

    #[cfg(windows)]
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

#[cfg(target_arch = "wasm32")]
async fn load_fonts_from_url(
    platform_fonts: &PlatformFonts,
    result: &mut HashMap<String, FontData>,
) {
    use wasm_bindgen::JsCast;
    use web_sys::Cache;

    let cache = web_sys::window()
        .expect("Window not available")
        .caches()
        .expect("CacheStorage not available")
        .open(env!("CARGO_PKG_NAME"))
        .await
        .map(|v| v.dyn_into::<Cache>())
        .flatten()
        .expect("Cache not available");
    for (language, font_urls) in platform_fonts {
        for url in font_urls {
            use web_sys::Response;

            if let Ok(stored) = cache
                .match_with_str(url)
                .await
                .map(|v| v.dyn_into::<Response>())
                .flatten()
            {
                if let Some(buf) = stored.array_buffer().unwrap().await.ok() {
                    use web_sys::js_sys::Uint8Array;

                    result.insert(
                        language.clone(),
                        FontData::from_owned(Uint8Array::new(&buf).to_vec()),
                    );
                    log::info!("Loaded font for {language} ({url}) from cache");
                    continue;
                }
            }
            if let Ok(b) = crate::net::get_bytes_client(&crate::net::UNCONFIGURED_CLIENT, url).await
            {
                let mut response_buf = b.clone();
                let _ = cache
                    .put_with_str(
                        url,
                        &Response::new_with_opt_u8_array(Some(&mut response_buf)).unwrap(),
                    )
                    .await;
                result.insert(language.clone(), FontData::from_owned(b));
            }
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
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
                if let Some(font) = fc.find(font_name, None)
                    && let Ok(data) = std::fs::read(font.path)
                {
                    result.insert(language.to_string(), FontData::from_owned(data));
                    break;
                }
            }
        })
    } else {
        log::warn!("Failed to init Fontconfig")
    }
}
