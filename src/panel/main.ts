import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  api,
  errorText,
  events,
  mediaUrl,
  type DisplayInfo,
  type Layout,
  type LayoutMode,
  type MediaKind,
  type Screensaver,
  type ScreensaverInfo,
  type Wallpaper,
} from "../shared/api";

const KIND_LABEL: Record<MediaKind, string> = {
  image: "Image",
  gif: "GIF",
  video: "Video",
  web: "Web page",
};

const STILL_MAX_SIDE = 3840;
const STILL_TIMEOUT_MS = 20000;
const MOON_ICON =
  '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>';

const byId = <T extends HTMLElement = HTMLElement>(id: string) =>
  document.getElementById(id) as T;

const modeButtons = Array.from(document.querySelectorAll<HTMLButtonElement>(".segmented button"));
const preview = byId("preview");
const previewEmpty = byId("preview-empty");
const previewEmptyText = byId("preview-empty-text");
const nowName = byId("now-name");
const nowMeta = byId("now-meta");
const chooseButton = byId<HTMLButtonElement>("choose");
const stopButton = byId<HTMLButtonElement>("stop");
const hint = byId("hint");
const displayMap = byId("display-map");
const displayList = byId<HTMLUListElement>("display-list");
const displaysCount = byId("displays-count");
const backgroundNote = byId("background-note");
const saverTitle = byId("saver-title");
const saverHint = byId("saver-hint");
const saverSwitch = byId<HTMLInputElement>("saver-switch");
const saverSettings = byId<HTMLButtonElement>("saver-settings");
const updateBanner = byId<HTMLButtonElement>("update-banner");
const dropOverlay = byId("drop-overlay");
const dropText = byId("drop-text");
const about = byId("about");
const aboutVersion = byId("about-version");
const toast = byId("toast");

let layout: Layout = { mode: "mirror", all: null, displays: {}, screensaver: { enabled: false, display: null } };
let displays: DisplayInfo[] = [];
let saverInfo: ScreensaverInfo = { screensaver: false, selected: null };
let lockScreenRunning = false;
let lockScreenAgain = false;
let selectedId: string | null = null;
let busy = false;
let currentVersion = "";
let updateFeed: string | null = null;
let lastUpdateCheck = 0;
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

const perDisplay = () => layout.mode === "perDisplay";

function selectedDisplay(): DisplayInfo | undefined {
  return displays.find((d) => d.id === selectedId) ?? displays.find((d) => d.primary) ?? displays[0];
}

function displayNumber(display: DisplayInfo): string {
  return `Display ${displays.indexOf(display) + 1}`;
}

function wallpaperOn(display: DisplayInfo | undefined): Wallpaper | null {
  if (!perDisplay()) return layout.all;
  return display ? (layout.displays[display.id] ?? null) : null;
}

function target(): string | null {
  return perDisplay() ? (selectedDisplay()?.id ?? null) : null;
}

function render() {
  renderMode();
  renderCurrent();
  renderActions();
  renderSaver();
  renderDisplays();
}

function renderMode() {
  for (const button of modeButtons) {
    button.setAttribute("aria-checked", String(button.dataset.mode === layout.mode));
  }
}

function renderCurrent() {
  const display = selectedDisplay();
  const wallpaper = wallpaperOn(display);
  const screens = displays.length === 1 ? "1 display" : `${displays.length} displays`;

  if (perDisplay() && display) {
    const kind = wallpaper ? ` · ${KIND_LABEL[wallpaper.kind]}` : "";
    nowName.textContent = wallpaper ? wallpaper.name : "System wallpaper";
    nowMeta.textContent = `${displayNumber(display)} · ${display.name}${kind}`;
  } else if (wallpaper) {
    const kind = KIND_LABEL[wallpaper.kind];
    nowName.textContent = wallpaper.name;
    nowMeta.textContent =
      wallpaper.mode === "live"
        ? `${kind} · live on ${screens}`
        : `${kind} · system wallpaper on ${screens}`;
  } else {
    nowName.textContent = "No WallKika wallpaper";
    nowMeta.textContent = "Pick an image, a GIF, a video or an HTML page.";
  }
  nowName.title = wallpaper?.path ?? "";
  previewEmptyText.textContent =
    perDisplay() && display ? `${displayNumber(display)} shows the system wallpaper` : "No WallKika wallpaper yet";
  renderPreview(wallpaper);
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

function renderActions() {
  const display = selectedDisplay();
  if (perDisplay() && display) {
    const number = displayNumber(display);
    chooseButton.textContent = `Choose file for ${number}…`;
    stopButton.textContent = `Clear ${number}`;
    stopButton.hidden = !layout.displays[display.id];
    hint.textContent = `or drop a file here to use it on ${number}`;
    dropText.textContent = `Drop the file to use it on ${number}`;
  } else {
    chooseButton.textContent = "Choose file…";
    stopButton.textContent = "Stop live wallpaper";
    stopButton.hidden = layout.all?.mode !== "live";
    hint.textContent = "or drop a file on this window";
    dropText.textContent = "Drop the file to use it on every display";
  }
}

function saverDisplayId(): string | null {
  const chosen = layout.screensaver.display;
  if (chosen && layout.displays[chosen]) return chosen;
  return Object.keys(layout.displays).sort()[0] ?? null;
}

function saverDisplay(): DisplayInfo | undefined {
  const id = saverDisplayId();
  return displays.find((d) => d.id === id);
}

function saverWallpaper(): Wallpaper | null {
  if (!layout.screensaver.enabled) return null;
  if (!perDisplay()) return layout.all;
  const id = saverDisplayId();
  return id ? layout.displays[id] : null;
}

function saverNeedsChoosing(): boolean {
  return saverInfo.screensaver && saverInfo.selected === false && saverWallpaper() !== null;
}

function saverHintText(): string {
  const both = saverInfo.screensaver;
  if (!layout.screensaver.enabled) {
    return both ? "Show this wallpaper as screen saver and on the lock screen." : "Show this wallpaper on the lock screen.";
  }
  const wallpaper = saverWallpaper();
  if (!wallpaper) {
    return perDisplay() ? "Choose a wallpaper and pick its display with the moon." : "It starts once you choose a wallpaper.";
  }
  if (saverNeedsChoosing()) return "Last step: click Settings… and choose WallKika.";
  const display = saverDisplay();
  const source = perDisplay() && display ? displayNumber(display) : "Your wallpaper";
  const still = wallpaper.kind === "web" ? "HTML can't go on the lock screen" : "a still on the lock screen";
  return both ? `${source} is your screen saver, with ${still}.` : `${source} shows as ${still}.`;
}

function renderSaver() {
  saverTitle.textContent = saverInfo.screensaver ? "Screen saver & lock screen" : "Lock screen";
  saverSwitch.checked = layout.screensaver.enabled;
  saverSettings.hidden = !saverInfo.screensaver;
  saverHint.textContent = saverHintText();
  saverHint.classList.toggle("pending", saverNeedsChoosing());
}

function saverToggle(display: DisplayInfo): HTMLButtonElement {
  const number = displayNumber(display);
  const chosen = layout.screensaver.enabled && saverDisplay() === display;
  const button = element("button", "saver-pick");
  button.type = "button";
  button.innerHTML = MOON_ICON;
  button.setAttribute("aria-pressed", String(chosen));
  button.disabled = !layout.displays[display.id];
  button.title = chosen
    ? `${number} is your screen saver and lock screen`
    : button.disabled
      ? `Choose a wallpaper for ${number} first`
      : `Use ${number} for the screen saver and lock screen`;
  button.addEventListener("click", () => {
    if (!chosen) void updateSaver({ enabled: true, display: display.id }, `${number} goes to the screen saver and lock screen`);
  });
  return button;
}

function renderDisplays() {
  displaysCount.textContent = String(displays.length);
  const selected = selectedDisplay();

  displayList.replaceChildren(
    ...displays.map((display, index) => {
      const main = perDisplay() ? element("button", "display-select") : element("div", "display-select");
      const body = element("span", "display-body");
      body.append(element("span", "display-name", display.name));
      if (perDisplay()) {
        body.append(element("span", "display-assigned", layout.displays[display.id]?.name ?? "System wallpaper"));
      }
      const scale = display.scaleFactor === 1 ? "" : ` · ${+display.scaleFactor.toFixed(2)}x`;
      main.append(element("span", "display-index", String(index + 1)), body);
      if (display.primary) main.append(element("span", "tag", "Main"));
      main.append(element("span", "display-spec", `${display.width} × ${display.height}${scale}`));

      const row = element("div", "display-item");
      row.append(main);
      if (main instanceof HTMLButtonElement) {
        main.type = "button";
        main.addEventListener("click", () => select(display.id));
        row.classList.add("selectable");
        row.classList.toggle("selected", display === selected);
        row.append(saverToggle(display));
      }
      const item = element("li", "display-row");
      item.append(row);
      return item;
    }),
  );

  renderDisplayMap(selected);
}

function renderDisplayMap(selected = selectedDisplay()) {
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

  displayMap.replaceChildren(
    ...rects.map((rect, index) => {
      const display = displays[index];
      const wallpaper = wallpaperOn(display);
      const still = wallpaper !== null && (wallpaper.kind === "image" || wallpaper.kind === "gif");
      const screen = element("div", "screen", String(index + 1));
      screen.classList.toggle("primary", display.primary);
      screen.classList.toggle("filled", wallpaper !== null);
      screen.classList.toggle("animated", wallpaper !== null && !still);
      screen.classList.toggle("selectable", perDisplay());
      screen.classList.toggle("selected", perDisplay() && display === selected);
      if (perDisplay()) {
        screen.title = `${displayNumber(display)} · ${display.name}`;
        screen.addEventListener("click", () => select(display.id));
      }
      Object.assign(screen.style, {
        left: `${offsetX + (rect.x - minX) * scale + 2}px`,
        top: `${offsetY + (rect.y - minY) * scale + 2}px`,
        width: `${rect.w * scale - 4}px`,
        height: `${rect.h * scale - 4}px`,
        backgroundImage: still ? `url("${mediaUrl(wallpaper!.path)}")` : "",
      });
      return screen;
    }),
  );
}

function select(displayId: string) {
  selectedId = displayId;
  render();
}

function showToast(message: string, kind: "info" | "error" = "info") {
  toast.textContent = message;
  toast.className = `toast ${kind}`;
  toast.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (toast.hidden = true), kind === "error" ? 6000 : 3000);
}

async function run(task: () => Promise<unknown>) {
  if (busy) return;
  busy = true;
  chooseButton.disabled = stopButton.disabled = true;
  try {
    await task();
  } catch (error) {
    showToast(errorText(error), "error");
  } finally {
    busy = false;
    chooseButton.disabled = stopButton.disabled = false;
  }
}

function announce(wallpaper: Wallpaper | null) {
  if (!wallpaper) return;
  const display = selectedDisplay();
  const where = perDisplay() && display ? ` on ${displayNumber(display)}` : "";
  showToast(`Done: ${wallpaper.name}${where}`);
}

function choose() {
  void run(async () => announce(await api.chooseWallpaper(target())));
}

async function updateSaver(next: Screensaver, message: string) {
  try {
    saverInfo = await api.setScreensaver(next);
    layout = { ...layout, screensaver: next };
    showToast(saverNeedsChoosing() ? "Almost done: choose WallKika in Settings" : message);
  } catch (error) {
    showToast(errorText(error), "error");
  }
  renderSaver();
}

async function refreshSaverInfo() {
  try {
    saverInfo = await api.getScreensaverInfo();
    renderSaver();
  } catch {
    return;
  }
}

function once(media: HTMLMediaElement, event: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("the video took too long to load")), STILL_TIMEOUT_MS);
    media.addEventListener(
      event,
      () => {
        clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
    media.addEventListener("error", () => reject(new Error(`could not read the video (code ${media.error?.code})`)), {
      once: true,
    });
  });
}

async function captureStill(wallpaper: Wallpaper): Promise<ArrayBuffer> {
  let source: CanvasImageSource;
  let width: number;
  let height: number;
  if (wallpaper.kind === "video") {
    const video = document.createElement("video");
    video.crossOrigin = "anonymous";
    video.muted = true;
    video.preload = "auto";
    video.src = mediaUrl(wallpaper.path);
    await once(video, "loadeddata");
    video.currentTime = Math.min(video.duration / 3, 30);
    await once(video, "seeked");
    [source, width, height] = [video, video.videoWidth, video.videoHeight];
  } else {
    const image = new Image();
    image.crossOrigin = "anonymous";
    image.src = mediaUrl(wallpaper.path);
    await image.decode();
    [source, width, height] = [image, image.naturalWidth, image.naturalHeight];
  }
  const scale = Math.min(1, STILL_MAX_SIDE / Math.max(width, height));
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(width * scale);
  canvas.height = Math.round(height * scale);
  canvas.getContext("2d")!.drawImage(source, 0, 0, canvas.width, canvas.height);
  const still = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/jpeg", 0.92));
  if (!still) throw new Error("could not encode the picture");
  return still.arrayBuffer();
}

async function updateLockScreen() {
  if (lockScreenRunning) {
    lockScreenAgain = true;
    return;
  }
  lockScreenRunning = true;
  try {
    do {
      lockScreenAgain = false;
      const wallpaper = await api.lockScreenRequest();
      if (wallpaper) await api.setLockScreenImage(wallpaper.path, await captureStill(wallpaper));
    } while (lockScreenAgain);
  } catch (error) {
    showToast(`Lock screen: ${errorText(error)}`, "error");
  } finally {
    lockScreenRunning = false;
  }
}

function openAbout() {
  about.hidden = false;
  byId<HTMLButtonElement>("about-close").focus();
}

function closeAbout() {
  about.hidden = true;
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.append(area);
    area.select();
    document.execCommand("copy");
    area.remove();
  }
  showToast(`Copied: ${text}`);
}

for (const button of modeButtons) {
  button.addEventListener("click", () => {
    const mode = button.dataset.mode as LayoutMode;
    if (mode !== layout.mode) void run(() => api.setLayoutMode(mode));
  });
}

chooseButton.addEventListener("click", choose);

saverSwitch.addEventListener("change", () => {
  const enabled = saverSwitch.checked;
  const display = layout.screensaver.display ?? (perDisplay() ? (selectedDisplay()?.id ?? null) : null);
  void updateSaver(
    { enabled, display },
    enabled ? "Screen saver and lock screen on" : "Screen saver and lock screen off",
  );
});
saverSettings.addEventListener("click", () =>
  api.openScreensaverSettings().catch((error) => showToast(errorText(error), "error")),
);

stopButton.addEventListener("click", () =>
  run(async () => {
    const display = selectedDisplay();
    await api.clearWallpaper(target());
    showToast(perDisplay() && display ? `${displayNumber(display)} cleared` : "Live wallpaper stopped");
  }),
);

byId("about-open").addEventListener("click", openAbout);
byId("about-close").addEventListener("click", closeAbout);
about.addEventListener("click", (event) => {
  if (event.target === about) closeAbout();
});
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && !about.hidden) closeAbout();
});
for (const button of document.querySelectorAll<HTMLButtonElement>("[data-copy]")) {
  button.addEventListener("click", () => void copyText(button.dataset.copy ?? ""));
}

void getCurrentWebview().onDragDropEvent(({ payload }) => {
  if (payload.type === "enter" || payload.type === "over") {
    dropOverlay.hidden = false;
  } else if (payload.type === "leave") {
    dropOverlay.hidden = true;
  } else if (payload.type === "drop") {
    dropOverlay.hidden = true;
    const [path] = payload.paths;
    if (path) void run(async () => announce(await api.setWallpaper(path, target())));
  }
});

window.addEventListener("resize", () => renderDisplayMap());

void events.onLayoutChanged((next) => {
  layout = next;
  render();
  void updateLockScreen();
});
void events.onDisplaysChanged((list) => {
  displays = list;
  render();
});
void events.onWallpaperError((message) => showToast(message, "error"));
void events.onShowAbout(openAbout);

const UPDATE_INTERVAL_MS = 12 * 60 * 60 * 1000;

function isNewer(latest: string, current: string): boolean {
  const parse = (version: string) =>
    version.split(/[.-]/).slice(0, 3).map((part) => Number.parseInt(part, 10) || 0);
  const [next, now] = [parse(latest), parse(current)];
  for (let i = 0; i < 3; i++) {
    if (next[i] !== now[i]) return next[i] > now[i];
  }
  return false;
}

function showUpdate(version: string) {
  updateBanner.textContent = `WallKika ${version} is available · Download`;
  updateBanner.hidden = false;
}

async function checkForUpdates() {
  if (!updateFeed || Date.now() - lastUpdateCheck < UPDATE_INTERVAL_MS) return;
  lastUpdateCheck = Date.now();
  try {
    const response = await fetch(updateFeed, { headers: { Accept: "application/vnd.github+json" } });
    if (!response.ok) return;
    const release: { tag_name?: string; html_url?: string } = await response.json();
    const latest = release.tag_name?.replace(/^v/, "");
    if (latest && release.html_url && isNewer(latest, currentVersion)) {
      await api.reportUpdate(latest, release.html_url);
      showUpdate(latest);
    }
  } catch {
    lastUpdateCheck = 0;
  }
}

updateBanner.addEventListener("click", () =>
  api.openUpdate().catch((error) => showToast(errorText(error), "error")),
);
document.addEventListener("visibilitychange", () => {
  if (document.hidden) return;
  void checkForUpdates();
  void refreshSaverInfo();
});
window.addEventListener("focus", () => void refreshSaverInfo());
setInterval(() => void checkForUpdates(), UPDATE_INTERVAL_MS);

api.getOverview().then(
  (overview) => {
    layout = overview.layout;
    displays = overview.displays;
    currentVersion = overview.version;
    updateFeed = overview.updateFeed;
    saverInfo = overview.screensaver;
    if (overview.update) showUpdate(overview.update.version);
    void checkForUpdates();
    aboutVersion.textContent = `Version ${overview.version}`;
    backgroundNote.textContent = `Closing this window keeps WallKika running in the ${
      overview.platform === "macos" ? "menu bar" : "system tray"
    }.`;
    render();
    void updateLockScreen();
  },
  (error) => showToast(errorText(error), "error"),
);
