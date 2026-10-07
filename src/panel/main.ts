import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  api,
  errorText,
  events,
  mediaUrl,
  type DisplayInfo,
  type MediaKind,
  type Wallpaper,
} from "../shared/api";

const KIND_LABEL: Record<MediaKind, string> = {
  image: "Image",
  gif: "GIF",
  video: "Video",
  web: "Web page",
};

const byId = <T extends HTMLElement = HTMLElement>(id: string) =>
  document.getElementById(id) as T;

const preview = byId("preview");
const previewEmpty = byId("preview-empty");
const nowName = byId("now-name");
const nowMeta = byId("now-meta");
const chooseButton = byId<HTMLButtonElement>("choose");
const stopButton = byId<HTMLButtonElement>("stop");
const displayMap = byId("display-map");
const displayList = byId<HTMLUListElement>("display-list");
const displaysCount = byId("displays-count");
const dropOverlay = byId("drop-overlay");
const toast = byId("toast");

let current: Wallpaper | null = null;
let displays: DisplayInfo[] = [];
let previewKey = "";
let toastTimer: number | undefined;

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  text = "",
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  node.textContent = text;
  return node;
}

function renderCurrent(wallpaper: Wallpaper | null) {
  current = wallpaper;
  stopButton.hidden = wallpaper?.mode !== "live";

  if (!wallpaper) {
    nowName.textContent = "No WallKika wallpaper";
    nowName.title = "";
    nowMeta.textContent = "Pick an image, a GIF, a video or an HTML page.";
  } else {
    const kind = KIND_LABEL[wallpaper.kind];
    const screens = displays.length === 1 ? "1 display" : `${displays.length} displays`;
    nowName.textContent = wallpaper.name;
    nowName.title = wallpaper.path;
    nowMeta.textContent =
      wallpaper.mode === "live"
        ? `${kind} · live on ${screens}`
        : `${kind} · system wallpaper on ${screens}`;
  }
  renderPreview(wallpaper);
  renderDisplays();
}

function renderPreview(wallpaper: Wallpaper | null) {
  const key = wallpaper ? `${wallpaper.mode}:${wallpaper.kind}:${wallpaper.path}` : "";
  if (key === previewKey) return;
  previewKey = key;

  preview.querySelectorAll(".media, .badge").forEach((node) => node.remove());
  previewEmpty.hidden = wallpaper !== null;
  if (!wallpaper) return;

  const src = mediaUrl(wallpaper.path);
  let media: HTMLElement;
  if (wallpaper.kind === "video") {
    const video = document.createElement("video");
    video.muted = true;
    video.loop = true;
    video.autoplay = true;
    video.playsInline = true;
    video.src = src;
    media = video;
  } else if (wallpaper.kind === "web") {
    media = element("div", "web-placeholder", "Web page");
  } else {
    const image = new Image();
    image.alt = "";
    image.src = src;
    media = image;
  }
  media.classList.add("media");

  const badge = element("span", `badge ${wallpaper.mode}`);
  badge.append(element("span", "dot"), wallpaper.mode === "live" ? "Live" : "System wallpaper");
  preview.prepend(media);
  preview.append(badge);
}

function renderDisplays() {
  displaysCount.textContent = String(displays.length);

  displayList.replaceChildren(
    ...displays.map((display, index) => {
      const item = element("li", "display-item");
      const scale = display.scaleFactor === 1 ? "" : ` · ${+display.scaleFactor.toFixed(2)}x`;
      item.append(
        element("span", "display-index", String(index + 1)),
        element("span", "display-name", display.name),
      );
      if (display.primary) item.append(element("span", "tag", "Main"));
      item.append(element("span", "display-spec", `${display.width} × ${display.height}${scale}`));
      return item;
    }),
  );

  renderDisplayMap();
}

function renderDisplayMap() {
  const box = displayMap.getBoundingClientRect();
  if (!displays.length || !box.width) return displayMap.replaceChildren();

  const rects = displays.map((d) => ({
    x: d.x / d.scaleFactor,
    y: d.y / d.scaleFactor,
    w: d.width / d.scaleFactor,
    h: d.height / d.scaleFactor,
  }));
  const minX = Math.min(...rects.map((r) => r.x));
  const minY = Math.min(...rects.map((r) => r.y));
  const maxX = Math.max(...rects.map((r) => r.x + r.w));
  const maxY = Math.max(...rects.map((r) => r.y + r.h));
  const padding = 16;
  const scale = Math.min(
    (box.width - padding * 2) / (maxX - minX),
    (box.height - padding * 2) / (maxY - minY),
  );
  const offsetX = (box.width - (maxX - minX) * scale) / 2;
  const offsetY = (box.height - (maxY - minY) * scale) / 2;

  const still = current !== null && (current.kind === "image" || current.kind === "gif");
  const thumbnail = still ? `url("${mediaUrl(current!.path)}")` : "";

  displayMap.replaceChildren(
    ...rects.map((rect, index) => {
      const screen = element("div", "screen", String(index + 1));
      screen.classList.toggle("primary", displays[index].primary);
      screen.classList.toggle("filled", current !== null);
      screen.classList.toggle("animated", current !== null && !still);
      Object.assign(screen.style, {
        left: `${offsetX + (rect.x - minX) * scale + 2}px`,
        top: `${offsetY + (rect.y - minY) * scale + 2}px`,
        width: `${rect.w * scale - 4}px`,
        height: `${rect.h * scale - 4}px`,
        backgroundImage: thumbnail,
      });
      return screen;
    }),
  );
}

function showToast(message: string, kind: "info" | "error" = "info") {
  toast.textContent = message;
  toast.className = `toast ${kind}`;
  toast.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (toast.hidden = true), kind === "error" ? 6000 : 3000);
}

async function run(task: () => Promise<unknown>) {
  chooseButton.disabled = stopButton.disabled = true;
  try {
    await task();
  } catch (error) {
    showToast(errorText(error), "error");
  } finally {
    chooseButton.disabled = stopButton.disabled = false;
  }
}

function announce(wallpaper: Wallpaper | null) {
  if (wallpaper) showToast(`Done: ${wallpaper.name}`);
}

chooseButton.addEventListener("click", () =>
  run(async () => announce(await api.chooseWallpaper())),
);

stopButton.addEventListener("click", () =>
  run(async () => {
    await api.stopLiveWallpaper();
    showToast("Live wallpaper stopped");
  }),
);

void getCurrentWebview().onDragDropEvent(({ payload }) => {
  if (payload.type === "enter" || payload.type === "over") {
    dropOverlay.hidden = false;
  } else if (payload.type === "leave") {
    dropOverlay.hidden = true;
  } else if (payload.type === "drop") {
    dropOverlay.hidden = true;
    const [path] = payload.paths;
    if (path) void run(async () => announce(await api.setWallpaper(path)));
  }
});

window.addEventListener("resize", renderDisplayMap);

void events.onWallpaperChanged(renderCurrent);
void events.onDisplaysChanged((list) => {
  displays = list;
  renderCurrent(current);
});
void events.onWallpaperError((message) => showToast(message, "error"));

api.getOverview().then(
  (overview) => {
    displays = overview.displays;
    renderCurrent(overview.current);
  },
  (error) => showToast(errorText(error), "error"),
);
