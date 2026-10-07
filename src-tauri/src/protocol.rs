use percent_encoding::percent_decode_str;
use std::{
    collections::HashSet,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
};
use tauri::{
    http::{header, Request, Response, StatusCode},
    AppHandle, Manager, Runtime, UriSchemeContext, UriSchemeResponder,
};

pub const SCHEME: &str = "wallkika";
const MAX_CHUNK: u64 = 16 * 1024 * 1024;
const MAX_FULL_READ: u64 = 64 * 1024 * 1024;

#[derive(Default)]
pub struct MediaScope {
    files: Mutex<HashSet<PathBuf>>,
    dirs: Mutex<HashSet<PathBuf>>,
}

impl MediaScope {
    pub fn allow_file(&self, path: &Path) {
        lock(&self.files).insert(path.to_path_buf());
    }

    pub fn allow_dir(&self, dir: &Path) {
        lock(&self.dirs).insert(dir.to_path_buf());
    }

    pub fn allows(&self, path: &Path) -> bool {
        let Ok(path) = path.canonicalize() else {
            return false;
        };
        lock(&self.files).contains(&path) || lock(&self.dirs).iter().any(|dir| path.starts_with(dir))
    }
}

pub fn handle<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || responder.respond(respond(&app, &request)));
}

fn respond<R: Runtime>(app: &AppHandle<R>, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let path = path_from_uri(request.uri().path());
    if !app.state::<MediaScope>().allows(&path) {
        log::warn!("Blocked media request outside the allowed files: {}", path.display());
        return empty(StatusCode::FORBIDDEN);
    }
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok());
    serve(&path, range).unwrap_or_else(|err| {
        log::warn!("Could not serve {}: {err}", path.display());
        empty(StatusCode::NOT_FOUND)
    })
}

fn serve(path: &Path, range: Option<&str>) -> io::Result<Response<Vec<u8>>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let builder = Response::builder()
        .header(header::CONTENT_TYPE, mime_type(path))
        .header(header::ACCEPT_RANGES, "bytes");

    let (start, end) = match (range, len) {
        (None, len) if len <= MAX_FULL_READ => {
            let mut body = Vec::with_capacity(len as usize);
            file.read_to_end(&mut body)?;
            return Ok(builder
                .header(header::CONTENT_LENGTH, len)
                .body(body)
                .expect("valid response"));
        }
        (None, len) => (0, len.min(MAX_CHUNK) - 1),
        (Some(range), len) => match byte_range(range, len) {
            Some(bounds) => bounds,
            None => {
                return Ok(Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                    .body(Vec::new())
                    .expect("valid response"))
            }
        },
    };

    let size = end - start + 1;
    let mut body = Vec::with_capacity(size as usize);
    file.seek(SeekFrom::Start(start))?;
    file.take(size).read_to_end(&mut body)?;
    Ok(builder
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
        .header(header::CONTENT_LENGTH, size)
        .body(body)
        .expect("valid response"))
}

fn byte_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?.split(',').next()?.trim();
    let (first, last) = spec.split_once('-')?;
    let (start, end) = match (first.trim(), last.trim()) {
        ("", suffix) => {
            let suffix: u64 = suffix.parse().ok()?;
            (len.checked_sub(suffix.min(len))?, len.checked_sub(1)?)
        }
        (first, "") => {
            let start: u64 = first.parse().ok()?;
            (start, len.checked_sub(1)?)
        }
        (first, last) => (first.parse().ok()?, last.parse::<u64>().ok()?.min(len.checked_sub(1)?)),
    };
    if start >= len || end < start {
        return None;
    }
    Some((start, end.min(start + MAX_CHUNK - 1)))
}

fn path_from_uri(uri_path: &str) -> PathBuf {
    let decoded = percent_decode_str(uri_path).decode_utf8_lossy();
    if cfg!(windows) {
        PathBuf::from(decoded.trim_start_matches('/'))
    } else {
        PathBuf::from(decoded.as_ref())
    }
}

fn mime_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "gif" => "image/gif",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "heic" | "heif" => "image/heic",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

fn empty(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .body(Vec::new())
        .expect("valid response")
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    #[test]
    fn serves_exact_bounded_ranges() {
        assert_eq!(byte_range("bytes=0-1", 100), Some((0, 1)));
        assert_eq!(byte_range("bytes=10-5960419", 3_929_264_414), Some((10, 5_960_419)));
        assert_eq!(byte_range("bytes=90-500", 100), Some((90, 99)));
    }

    #[test]
    fn caps_open_and_huge_ranges() {
        assert_eq!(byte_range("bytes=0-", 4_000 * MB), Some((0, MAX_CHUNK - 1)));
        assert_eq!(byte_range("bytes=0-", 100), Some((0, 99)));
        assert_eq!(byte_range("bytes=0-99999999999", 4_000 * MB), Some((0, MAX_CHUNK - 1)));
    }

    #[test]
    fn supports_suffix_ranges() {
        assert_eq!(byte_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(byte_range("bytes=-500", 100), Some((0, 99)));
    }

    #[test]
    fn rejects_unsatisfiable_or_malformed_ranges() {
        assert_eq!(byte_range("bytes=100-", 100), None);
        assert_eq!(byte_range("bytes=5-2", 100), None);
        assert_eq!(byte_range("items=0-1", 100), None);
        assert_eq!(byte_range("bytes=abc", 100), None);
        assert_eq!(byte_range("bytes=0-1", 0), None);
    }

    #[test]
    fn decodes_paths_from_urls() {
        let path = path_from_uri("/Users/me/My%20Videos/fondo%20matrix.mp4");
        if cfg!(windows) {
            assert_eq!(path, PathBuf::from("Users/me/My Videos/fondo matrix.mp4"));
        } else {
            assert_eq!(path, PathBuf::from("/Users/me/My Videos/fondo matrix.mp4"));
        }
    }

    #[test]
    fn scope_only_allows_registered_files_and_folders() {
        let dir = std::env::temp_dir().join(format!("wallkika-scope-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("web")).unwrap();
        let video = dir.join("video.mp4");
        let other = dir.join("secret.txt");
        let page = dir.join("web/index.html");
        for file in [&video, &other, &page] {
            std::fs::write(file, b"x").unwrap();
        }
        let dir = dir.canonicalize().unwrap();

        let scope = MediaScope::default();
        scope.allow_file(&dir.join("video.mp4"));
        scope.allow_dir(&dir.join("web"));
        assert!(scope.allows(&dir.join("video.mp4")));
        assert!(scope.allows(&dir.join("web/index.html")));
        assert!(!scope.allows(&dir.join("secret.txt")));
        assert!(!scope.allows(&dir.join("web/../secret.txt")));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
