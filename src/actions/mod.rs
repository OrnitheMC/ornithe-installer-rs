use std::{
    io::{Seek, Write},
    path::PathBuf,
};

#[cfg(target_arch = "wasm32")]
use web_sys::{
    Blob, BlobPropertyBag,
    js_sys::{Array, Uint8Array},
};
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::errors::InstallerError;
pub mod client;
pub mod prism_pack;
pub mod server;

#[cfg(target_arch = "wasm32")]
pub fn download_file(name: impl Into<String>, buf: &Vec<u8>) {
    let arr = Uint8Array::new_from_slice(buf);
    let blob_options = BlobPropertyBag::new();
    blob_options.set_type("application/octet-stream");
    let parts = Array::new();
    parts.push(&arr);
    let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &blob_options)
        .expect("vec<u8> should be a valid u8 array seq");
    saveFile(blob, name.into());
}

#[wasm_bindgen::prelude::wasm_bindgen(module = "/file-saver-adapter.js")]
#[cfg(target_arch = "wasm32")]
extern "C" {
    fn saveFile(buf: Blob, name: String);
}

trait Writer {
    fn write_file(&mut self, path: &str, buf: &[u8]) -> Result<(), InstallerError>;

    fn create_dir(&mut self, path: &str) -> Result<(), InstallerError>;
}

impl Writer for PathBuf {
    fn write_file(&mut self, path: &str, buf: &[u8]) -> Result<(), InstallerError> {
        let new_file = self.join(path);
        let mut file = std::fs::File::create(new_file)?;
        file.write_all(buf)?;
        Ok(())
    }

    fn create_dir(&mut self, path: &str) -> Result<(), InstallerError> {
        let new_file = self.join(path);
        std::fs::create_dir_all(new_file)?;
        Ok(())
    }
}

impl<T> Writer for ZipWriter<T>
where
    T: Write + Seek,
{
    fn write_file(&mut self, path: &str, buf: &[u8]) -> Result<(), InstallerError> {
        self.start_file(path, SimpleFileOptions::default())?;
        Ok(self.write_all(buf)?)
    }

    fn create_dir(&mut self, path: &str) -> Result<(), InstallerError> {
        Ok(self.add_directory(path, SimpleFileOptions::default())?)
    }
}
