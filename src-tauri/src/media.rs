use serde::{Deserialize, Serialize};
use std::path::Path;

pub const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "avif", "heic", "heif", "bmp", "tif", "tiff",
];
pub const GIF_EXTENSIONS: &[&str] = &["gif"];
pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "m4v", "mov", "webm"];
pub const WEB_EXTENSIONS: &[&str] = &["html", "htm"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    Gif,
    Video,
    Web,
}

impl MediaKind {
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        let ext = ext.as_str();
        if IMAGE_EXTENSIONS.contains(&ext) {
            Some(Self::Image)
        } else if GIF_EXTENSIONS.contains(&ext) {
            Some(Self::Gif)
        } else if VIDEO_EXTENSIONS.contains(&ext) {
            Some(Self::Video)
        } else if WEB_EXTENSIONS.contains(&ext) {
            Some(Self::Web)
        } else {
            None
        }
    }

    pub fn is_static(self) -> bool {
        self == Self::Image
    }
}

pub fn all_extensions() -> Vec<&'static str> {
    [
        IMAGE_EXTENSIONS,
        GIF_EXTENSIONS,
        VIDEO_EXTENSIONS,
        WEB_EXTENSIONS,
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_kind_by_extension_ignoring_case() {
        assert_eq!(
            MediaKind::from_path(Path::new("/a/photo.JPG")),
            Some(MediaKind::Image)
        );
        assert_eq!(
            MediaKind::from_path(Path::new("x.avif")),
            Some(MediaKind::Image)
        );
        assert_eq!(
            MediaKind::from_path(Path::new("x.gif")),
            Some(MediaKind::Gif)
        );
        assert_eq!(
            MediaKind::from_path(Path::new("matrix wallpaper.Mp4")),
            Some(MediaKind::Video)
        );
        assert_eq!(
            MediaKind::from_path(Path::new("clock/index.html")),
            Some(MediaKind::Web)
        );
    }

    #[test]
    fn rejects_unknown_or_missing_extension() {
        assert_eq!(MediaKind::from_path(Path::new("notes.txt")), None);
        assert_eq!(MediaKind::from_path(Path::new("no-extension")), None);
        assert_eq!(MediaKind::from_path(Path::new(".mp4")), None);
    }

    #[test]
    fn only_images_are_static() {
        assert!(MediaKind::Image.is_static());
        assert!(!MediaKind::Gif.is_static());
        assert!(!MediaKind::Video.is_static());
        assert!(!MediaKind::Web.is_static());
    }
}
