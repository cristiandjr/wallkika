use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("File not found: {0}")]
    NotFound(PathBuf),

    #[error("Unsupported format ({0}). Use an image, a GIF, a video (mp4, mov, webm) or an HTML page.")]
    Unsupported(String),

    #[error("{0}")]
    Platform(String),

    #[error(transparent)]
    Tauri(#[from] tauri::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    pub fn platform(message: impl Into<String>) -> Self {
        Self::Platform(message.into())
    }
}

impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
