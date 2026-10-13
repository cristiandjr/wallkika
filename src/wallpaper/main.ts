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
    __WALLKIKA_SCREENSAVER__?: Wallpaper | null;
  }
}

const displayId = window.__WALLKIKA_DISPLAY__ ?? "";
const screensaver = window.__WALLKIKA_SCREENSAVER__;

const FADE_MS = 600;
const WATCHDOG_MS = 5000;
const RESYNC_AFTER_MS = 2000;
const MAX_SPEED = 1.5;
const SPEED_SAMPLE_MS = 2000;
const EXIT_DISTANCE = 12;
const stage = document.getElementById("stage")!;
let shownKey: string | null = null;
let pausedAt = 0;
let lastTick = Date.now();

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
      guardSpeed(video);
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

function currentVideo(): HTMLVideoElement | null {
  return stage.querySelector<HTMLVideoElement>("video:last-child");
}

function resync(video: HTMLVideoElement, reason: string) {
  video.defaultPlaybackRate = 1;
  video.playbackRate = 1;
  // WebKit ignores a seek to the exact current time, so step back a frame to rebuild the pipeline.
  video.currentTime = Math.max(0, video.currentTime - 0.04);
  video.play().catch(() => {});
  void api.reportRenderer("video resynced", reason);
}

function guardSpeed(video: HTMLVideoElement) {
  let sample: { at: number; media: number } | null = null;
  video.addEventListener("seeking", () => (sample = null));
  const check = (now: number, frame: VideoFrameCallbackMetadata) => {
    if (!video.isConnected) return;
    if (!sample || video.paused || frame.mediaTime < sample.media) {
      sample = { at: now, media: frame.mediaTime };
    } else if (now - sample.at >= SPEED_SAMPLE_MS) {
      const speed = ((frame.mediaTime - sample.media) * 1000) / (now - sample.at);
      sample = { at: now, media: frame.mediaTime };
      if (speed > MAX_SPEED) resync(video, `it was playing at ${speed.toFixed(1)}x`);
    }
    video.requestVideoFrameCallback(check);
  };
  if ("requestVideoFrameCallback" in video) video.requestVideoFrameCallback(check);
}

function pausePlayback() {
  if (!pausedAt) pausedAt = Date.now();
  currentVideo()?.pause();
}

function resumePlayback() {
  if (!pausedAt || document.hidden) return;
  const away = Date.now() - pausedAt;
  pausedAt = 0;
  const video = currentVideo();
  if (!video) return;
  if (away > RESYNC_AFTER_MS) resync(video, `after ${Math.round(away / 1000)}s paused`);
  else video.play().catch(() => {});
}

document.addEventListener("visibilitychange", () => (document.hidden ? pausePlayback() : resumePlayback()));
void events.onSystemPause(pausePlayback);
void events.onSystemResume(resumePlayback);

setInterval(() => {
  const now = Date.now();
  const gap = now - lastTick;
  lastTick = now;
  const video = currentVideo();
  if (!video || pausedAt || document.hidden) return;
  if (gap > WATCHDOG_MS * 3) {
    resync(video, `after a ${Math.round(gap / 1000)}s freeze`);
  } else if (video.paused) {
    video.play().then(
      () => void api.reportRenderer("video resumed"),
      () => {},
    );
  }
}, WATCHDOG_MS);

function exitOnInput() {
  let origin: { x: number; y: number } | null = null;
  const exit = () => void api.exitScreensaver();
  for (const type of ["keydown", "mousedown", "wheel", "touchstart"]) {
    window.addEventListener(type, exit, { once: true });
  }
  window.addEventListener("mousemove", (event) => {
    origin ??= { x: event.screenX, y: event.screenY };
    if (Math.hypot(event.screenX - origin.x, event.screenY - origin.y) > EXIT_DISTANCE) exit();
  });
}

function render(layout: Layout) {
  show(liveWallpaperFor(layout, displayId));
}

if (screensaver !== undefined) {
  show(screensaver);
  exitOnInput();
} else {
  void events.onLayoutChanged(render);
  api.getLayout().then(render, (error) => void api.reportRenderer("error", errorText(error)));
}
