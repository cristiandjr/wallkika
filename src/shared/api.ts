import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

const MEDIA_BASE = convertFileSrc("", "wallkika");

export function mediaUrl(path: string): string {
  const segments = path.replace(/\\/g, "/").split("/").filter(Boolean);
  return MEDIA_BASE + segments.map(encodeURIComponent).join("/");
}

export type MediaKind = "image" | "gif" | "video" | "web";
export type RenderMode = "native" | "live";
export type LayoutMode = "mirror" | "perDisplay";

export interface Wallpaper {
  path: string;
  name: string;
  kind: MediaKind;
  mode: RenderMode;
}

export interface Layout {
  mode: LayoutMode;
  all: Wallpaper | null;
  displays: Record<string, Wallpaper>;
}

export interface DisplayInfo {
  id: string;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  scaleFactor: number;
  primary: boolean;
}

export interface UpdateInfo {
  version: string;
  url: string;
}

export interface Overview {
  layout: Layout;
  displays: DisplayInfo[];
  platform: string;
  version: string;
  updateFeed: string | null;
  update: UpdateInfo | null;
}

export function liveWallpaperFor(layout: Layout, displayId: string): Wallpaper | null {
  if (layout.mode === "mirror") return layout.all?.mode === "live" ? layout.all : null;
  return layout.displays[displayId] ?? null;
}

export const api = {
  getOverview: () => invoke<Overview>("get_overview"),
  chooseWallpaper: (display: string | null) =>
    invoke<Wallpaper | null>("choose_wallpaper", { display }),
  setWallpaper: (path: string, display: string | null) =>
    invoke<Wallpaper>("set_wallpaper", { path, display }),
  clearWallpaper: (display: string | null) => invoke<void>("clear_wallpaper", { display }),
  setLayoutMode: (mode: LayoutMode) => invoke<void>("set_layout_mode", { mode }),
  getLayout: () => invoke<Layout>("get_layout"),
  reportRenderer: (status: string, detail?: string) =>
    invoke<void>("report_renderer", { status, detail }),
  reportUpdate: (version: string, url: string) => invoke<void>("report_update", { version, url }),
  openUpdate: () => invoke<void>("open_update"),
};

export const events = {
  onLayoutChanged: (cb: (layout: Layout) => void): Promise<UnlistenFn> =>
    listen<Layout>("layout-changed", (e) => cb(e.payload)),
  onDisplaysChanged: (cb: (displays: DisplayInfo[]) => void): Promise<UnlistenFn> =>
    listen<DisplayInfo[]>("displays-changed", (e) => cb(e.payload)),
  onWallpaperError: (cb: (message: string) => void): Promise<UnlistenFn> =>
    listen<string>("wallpaper-error", (e) => cb(e.payload)),
  onShowAbout: (cb: () => void): Promise<UnlistenFn> => listen("show-about", () => cb()),
};

export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
