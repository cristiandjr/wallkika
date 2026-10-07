use crate::engine::Wallpaper;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub current: Option<Wallpaper>,
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_else(|err| {
            log::warn!("Ignoring unreadable {}: {err}", path.display());
            Self::default()
        })
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine::RenderMode, media::MediaKind};
    use std::path::PathBuf;

    fn temp_dir(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wallkika-{}-{test}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn roundtrip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("settings.json");
        let settings = Settings {
            current: Some(Wallpaper {
                path: PathBuf::from("/wallpapers/matrix.mp4"),
                name: "matrix.mp4".into(),
                kind: MediaKind::Video,
                mode: RenderMode::Live,
            }),
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_or_corrupt_file_falls_back_to_default() {
        let dir = temp_dir("fallback");
        assert_eq!(
            Settings::load(&dir.join("missing.json")),
            Settings::default()
        );

        let path = dir.join("corrupt.json");
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        fs::remove_dir_all(dir).unwrap();
    }
}
