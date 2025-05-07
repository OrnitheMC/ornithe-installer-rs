use std::{fmt::Debug, path::StripPrefixError};

#[derive(Debug)]
pub struct InstallerError(pub String);

#[cfg(feature = "gui")]
impl From<eframe::Error> for InstallerError {
    fn from(value: eframe::Error) -> Self {
        InstallerError(format!("{:?}", value))
    }
}

impl From<reqwest::Error> for InstallerError {
    fn from(value: reqwest::Error) -> Self {
        InstallerError(format!("{:?}", value))
    }
}

impl From<serde_json::Error> for InstallerError {
    fn from(value: serde_json::Error) -> Self {
        InstallerError(format!("{:?}", value))
    }
}

impl From<std::io::Error> for InstallerError {
    fn from(value: std::io::Error) -> Self {
        InstallerError(format!("{:?}", value))
    }
}

impl From<zip::result::ZipError> for InstallerError {
    fn from(value: zip::result::ZipError) -> Self {
        InstallerError(format!("{:?}", value))
    }
}

impl From<StripPrefixError> for InstallerError {
    fn from(value: StripPrefixError) -> Self {
        InstallerError(format!("{:?}", value))
    }
}
