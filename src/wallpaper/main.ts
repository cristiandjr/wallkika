import {
  api,
  errorText,
  events,
  liveWallpaperFor,
  mediaUrl,
  type Layout,
  type Wallpaper,
} from "../shared/api";

declare global {
  interface Window {
    __WALLKIKA_DISPLAY__?: string;
  }
}

const displayId = window.__WALLKIKA_DISPLAY__ ?? "";

const FADE_MS = 600;
const WATCHDOG_MS = 5000;
const stage = document.getElementById("stage")!;
let shownKey: string | null = null;

function keyOf(wallpaper: Wallpaper | null): string | null {
  return wallpaper?.mode === "live" ? `${wallpaper.kind}:${wallpaper.path}` : null;
}

function show(wallpaper: Wallpaper | null) {
  const key = keyOf(wallpaper);
  if (key === shownKey) return;
  shownKey = key;
  if (!wallpaper || !key) {
    stage.replaceChildren();
    return;
  }

  const layer = createLayer(wallpaper);
  layer.classList.add("layer");
  stage.append(layer);

  whenReady(layer)
    .then(() => {
      if (shownKey !== key) return layer.remove();
      layer.classList.add("visible");
      for (const old of Array.from(stage.children)) {
        if (old !== layer) setTimeout(() => old.remove(), FADE_MS);
      }
      void api.reportRenderer("showing", describe(wallpaper, layer));
    })
    .catch((error) => void api.reportRenderer("error", `${wallpaper.name}: ${errorText(error)}`));
}

function createLayer(wallpaper: Wallpaper): HTMLElement {
  const src = mediaUrl(wallpaper.path);
  switch (wallpaper.kind) {
    case "video": {
      const video = document.createElement("video");
      video.muted = true;
      video.defaultMuted = true;
      video.loop = true;
      video.autoplay = true;
      video.playsInline = true;
      video.preload = "auto";
      video.disablePictureInPicture = true;
      video.src = src;
      return video;
    }
    case "web": {
      const frame = document.createElement("iframe");
      // No allow-same-origin: the page stays isolated from the WallKika API.
      frame.setAttribute("sandbox", "allow-scripts");
      frame.src = src;
      return frame;
    }
    default: {
      const image = new Image();
      image.decoding = "async";
      image.src = src;
      return image;
    }
  }
}

function whenReady(layer: HTMLElement): Promise<void> {
  if (layer instanceof HTMLImageElement) return layer.decode();
  return new Promise((resolve, reject) => {
    if (layer instanceof HTMLVideoElement) {
      layer.addEventListener("playing", () => resolve(), { once: true });
      layer.addEventListener(
        "error",
        () => reject(`the video could not be played (code ${layer.error?.code}) ${layer.error?.message ?? ""}`),
        { once: true },
      );
      layer.play().catch(() => {});
    } else {
      layer.addEventListener("load", () => resolve(), { once: true });
    }
  });
}

function describe(wallpaper: Wallpaper, layer: HTMLElement): string {
  const screen = `on ${innerWidth}×${innerHeight}`;
  if (layer instanceof HTMLVideoElement) {
    return `${wallpaper.name} (video ${layer.videoWidth}×${layer.videoHeight}) ${screen}`;
  }
  if (layer instanceof HTMLImageElement) {
    return `${wallpaper.name} (${layer.naturalWidth}×${layer.naturalHeight}) ${screen}`;
  }
  return `${wallpaper.name} ${screen}`;
}

setInterval(() => {
  const video = stage.querySelector<HTMLVideoElement>("video:last-child");
  if (video?.paused) {
    video.play().then(
      () => void api.reportRenderer("video resumed"),
      () => {},
    );
  }
}, WATCHDOG_MS);

function render(layout: Layout) {
  show(liveWallpaperFor(layout, displayId));
}

void events.onLayoutChanged(render);
api.getLayout().then(render, (error) => void api.reportRenderer("error", errorText(error)));
