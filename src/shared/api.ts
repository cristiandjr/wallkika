import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

const MEDIA_BASE = convertFileSrc("", "wallkika");

export function mediaUrl(path: string): string {
  const segments = path.replace(/\\/g, "/").split("/").filter(Boolean);
  return MEDIA_BASE + segments.map(encodeURIComponent).join("/");
}

export type MediaKind = "image" | "gif" | "video" | "web";
export type RenderMode = "native" | "live";

export interface Wallpaper {
  path: string;
  name: string;
  kind: MediaKind;
  mode: RenderMode;
}

export interface DisplayInfo {
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  scaleFactor: number;
  primary: boolean;
}

export interface Overview {
  current: Wallpaper | null;
  displays: DisplayInfo[];
  platform: string;
}

export const api = {
  getOverview: () => invoke<Overview>("get_overview"),
  chooseWallpaper: () => invoke<Wallpaper | null>("choose_wallpaper"),
  setWallpaper: (path: string) => invoke<Wallpaper>("set_wallpaper", { path }),
  stopLiveWallpaper: () => invoke<void>("stop_live_wallpaper"),
  currentWallpaper: () => invoke<Wallpaper | null>("current_wallpaper"),
  reportRenderer: (status: string, detail?: string) =>
    invoke<void>("report_renderer", { status, detail }),
};

export const events = {
  onWallpaperChanged: (cb: (wallpaper: Wallpaper | null) => void): Promise<UnlistenFn> =>
    listen<Wallpaper | null>("wallpaper-changed", (e) => cb(e.payload)),
  onDisplaysChanged: (cb: (displays: DisplayInfo[]) => void): Promise<UnlistenFn> =>
    listen<DisplayInfo[]>("displays-changed", (e) => cb(e.payload)),
  onWallpaperError: (cb: (message: string) => void): Promise<UnlistenFn> =>
    listen<string>("wallpaper-error", (e) => cb(e.payload)),
};

export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
