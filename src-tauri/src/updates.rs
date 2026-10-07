use serde::Serialize;
use std::sync::{Mutex, PoisonError};

const REPOSITORY_URL: &str = env!("CARGO_PKG_REPOSITORY");

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub url: String,
}

#[derive(Default)]
pub struct Updates(Mutex<Option<UpdateInfo>>);

impl Updates {
    pub fn get(&self) -> Option<UpdateInfo> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set(&self, update: UpdateInfo) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(update);
    }
}

pub fn feed_url() -> Option<String> {
    feed_url_for(REPOSITORY_URL)
}

pub fn is_release_url(url: &str) -> bool {
    is_release_url_for(REPOSITORY_URL, url)
}

fn feed_url_for(repository: &str) -> Option<String> {
    let path = repository
        .strip_prefix("https://github.com/")?
        .trim_end_matches('/');
    (path.split('/').count() == 2)
        .then(|| format!("https://api.github.com/repos/{path}/releases/latest"))
}

fn is_release_url_for(repository: &str, url: &str) -> bool {
    let releases = format!("{}/releases/", repository.trim_end_matches('/'));
    !repository.is_empty() && url.starts_with(&releases)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str = "https://github.com/cristiandjr/WallKika";

    #[test]
    fn builds_the_latest_release_feed() {
        assert_eq!(
            feed_url_for(REPO).as_deref(),
            Some("https://api.github.com/repos/cristiandjr/WallKika/releases/latest")
        );
        assert_eq!(feed_url_for(""), None);
        assert_eq!(feed_url_for("https://gitlab.com/someone/project"), None);
    }

    #[test]
    fn only_opens_release_pages_of_this_repository() {
        assert!(is_release_url_for(
            REPO,
            "https://github.com/cristiandjr/WallKika/releases/tag/v0.3.0"
        ));
        assert!(!is_release_url_for(
            REPO,
            "https://github.com/someone/else/releases/tag/v1.0.0"
        ));
        assert!(!is_release_url_for(
            REPO,
            "https://evil.example/cristiandjr/WallKika/releases/"
        ));
        assert!(!is_release_url_for("", "https://github.com//releases/x"));
    }
}
