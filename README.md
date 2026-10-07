# WallKika

One wallpaper, every screen. Pick an image, a GIF, a video or an HTML page and WallKika mirrors it on all connected displays.

## Stack

[Tauri 2](https://v2.tauri.app): Rust core + TypeScript UI (Vite, no framework).

- **Always-on friendly**: uses the system webview instead of bundling Chromium (~40 MB RAM for the core process).
- **Native where it matters**: Rust talks directly to AppKit, Win32 and GTK/X11.
- **One codebase** for macOS, Windows and Linux; web tech renders video, GIFs and HTML wallpapers.

## How it works

| Content | Rendering |
| --- | --- |
| Image (JPG, PNG, HEIC, AVIF, WebP, BMP, TIFF) | Native OS wallpaper API: zero cost and it stays after quitting |
| GIF, video (MP4, MOV, WebM), HTML | One borderless window per display, placed behind the desktop icons, running `wallpaper.html` |

How a window gets behind the icons:

- **macOS**: `kCGDesktopWindowLevel`, joins all Spaces, ignores the mouse.
- **Windows**: reparented into Explorer's `WorkerW` (classic desktop) or into `Progman` below `SHELLDLL_DefView` (24H2+ raised desktop).
- **Linux**: X11 `_NET_WM_WINDOW_TYPE_DESKTOP` (Wayland sessions run through XWayland).

Files are served to the webviews through a custom `wallkika://` protocol with HTTP range support, limited to the files you picked. Tauri's built-in asset protocol caps each response at 1 MB, which breaks MP4s with large indexes.

## Project layout

```
src/
  panel/                 control panel window
  wallpaper/             renderer loaded in every per-display window
  shared/api.ts          typed bridge to the Rust commands and events
src-tauri/src/
  engine.rs              native vs live decision, per-display windows
  display.rs             display list and hot-plug watcher
  protocol.rs            wallkika:// media protocol (ranges + scope)
  platform/              macos.rs, windows.rs, linux.rs
  commands.rs  tray.rs  settings.rs  media.rs  error.rs
```

## Platform status

| | Static image | Live wallpaper | Tested on hardware |
| --- | --- | --- | --- |
| macOS 13+ | NSWorkspace, every display | Desktop-level windows | Yes (macOS 27, mixed DPI, 2 displays) |
| Windows 10/11 | IDesktopWallpaper | WorkerW / Progman (24H2+) | No, compiles only |
| Linux | GNOME, KDE, Cinnamon, MATE, XFCE, LXQt, LXDE, Sway, Hyprland, swww, feh, xwallpaper, nitrogen | X11 / XWayland (GNOME, KDE) | No, compiles only |

## Requirements

- Node 20+ and Rust. The Rust version is pinned in `rust-toolchain.toml`; rustup installs it automatically.
- Platform prerequisites from the [Tauri guide](https://v2.tauri.app/start/prerequisites/). On Linux, video playback also needs GStreamer plugins (`gstreamer1.0-plugins-good`, `gstreamer1.0-plugins-bad`, `gstreamer1.0-libav`).

## Run

```bash
npm install
npm run tauri dev      # development
npm run tauri build    # installers in src-tauri/target/release/bundle
npm run check:cross    # lint the Windows and Linux backends from any OS
```

## Usage

- **Panel**: "Choose file…" or drop a file on the window.
- **Tray / menu bar**: choose a wallpaper, stop the live wallpaper, quit. Closing the panel keeps WallKika running.
- **CLI**: `wallkika /path/to/video.mp4` sends the file to the running instance.
- The last live wallpaper comes back on startup, and displays are re-checked every 2 seconds.
- Settings: `~/Library/Application Support/com.cristiandjr.wallkika/settings.json` (macOS). Logs: `~/Library/Logs/com.cristiandjr.wallkika/`.

## Known limitations

- macOS applies native images to the active Space of each display only.
- Wlroots compositors (Sway, Hyprland) can't host live wallpapers yet; that needs `wlr-layer-shell`.
- Videos are decoded once per display.

## Roadmap

- A different wallpaper per display, fit modes (cover, contain, stretch)
- Playlists, rotation and schedules
- Pause when a fullscreen app is active or on battery
- Launch at login
- Native video renderers (AVPlayer, Media Foundation, mpv) that decode once for all displays
- Wayland layer-shell support
