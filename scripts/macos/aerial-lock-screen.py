#!/usr/bin/env python3
"""Experiment: register a video as a macOS Aerial so the lock screen and screen saver play it.

    python3 scripts/macos/aerial-lock-screen.py enable /path/to/video.mp4
    python3 scripts/macos/aerial-lock-screen.py disable

It edits Apple's private wallpaper files under ~/Library/Application Support/com.apple.wallpaper
(backups go to WallKika's config folder) and needs ffmpeg for the muted copy.
"""
import datetime
import json
import os
import plistlib
import shutil
import subprocess
import sys
import uuid

HOME = os.path.expanduser("~")
WALLPAPER = os.path.join(HOME, "Library/Application Support/com.apple.wallpaper")
STORE = os.path.join(WALLPAPER, "Store/Index.plist")
MANIFEST = os.path.join(WALLPAPER, "aerials/manifest/entries.json")
VIDEOS = os.path.join(WALLPAPER, "aerials/videos")
THUMBNAILS = os.path.join(WALLPAPER, "aerials/thumbnails")
BACKUP = os.path.join(HOME, "Library/Application Support/com.cristiandjr.wallkika/aerial-backup")
ASSET_ID = str(uuid.uuid5(uuid.NAMESPACE_DNS, "wallkika.cristiandjr.com")).upper()
CATEGORY_ID = str(uuid.uuid5(uuid.NAMESPACE_DNS, "category.wallkika")).upper()
SUBCATEGORY_ID = str(uuid.uuid5(uuid.NAMESPACE_DNS, "subcategory.wallkika")).upper()
AERIALS = "com.apple.wallpaper.choice.aerials"


def fail(message):
    print(f"error: {message}", file=sys.stderr)
    sys.exit(1)


def ffmpeg():
    for candidate in ("ffmpeg", "/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"):
        if shutil.which(candidate):
            return candidate
    fail("ffmpeg is needed to make a muted copy of the video (brew install ffmpeg)")


def backup():
    os.makedirs(BACKUP, exist_ok=True)
    for source in (STORE, MANIFEST):
        target = os.path.join(BACKUP, os.path.basename(source))
        if not os.path.exists(target):
            shutil.copy2(source, target)
    print(f"backups in {BACKUP}")


def write_media(video):
    os.makedirs(VIDEOS, exist_ok=True)
    os.makedirs(THUMBNAILS, exist_ok=True)
    tool = ffmpeg()
    mov = os.path.join(VIDEOS, f"{ASSET_ID}.mov")
    png = os.path.join(THUMBNAILS, f"{ASSET_ID}.png")
    subprocess.run([tool, "-y", "-loglevel", "error", "-i", video, "-map", "0:v:0", "-c", "copy", "-an",
                    "-movflags", "+faststart", mov], check=True)
    subprocess.run([tool, "-y", "-loglevel", "error", "-ss", "10", "-i", video, "-frames:v", "1", "-vf",
                    "scale=214:130:force_original_aspect_ratio=increase,crop=214:130", png], check=True)
    print(f"muted copy: {mov} ({os.path.getsize(mov) // 1_000_000} MB)")
    return mov, png


def edit_manifest(mov, png):
    with open(MANIFEST) as handle:
        manifest = json.load(handle)
    manifest["assets"] = [a for a in manifest["assets"] if a["id"] != ASSET_ID]
    manifest["categories"] = [c for c in manifest["categories"] if c["id"] != CATEGORY_ID]
    template = max(manifest["assets"], key=lambda a: len(a))
    asset = {key: value for key, value in template.items() if key != "group"}
    asset.update({
        "accessibilityLabel": "WallKika", "categories": [CATEGORY_ID], "id": ASSET_ID, "includeInShuffle": False,
        "localizedNameKey": "WallKika", "pointsOfInterest": {}, "preferredOrder": 0, "previewImage": "file://" + png,
        "shotID": "WALLKIKA", "showInTopLevel": True, "subcategories": [SUBCATEGORY_ID], "url-4K-SDR-240FPS": "file://" + mov,
    })
    model = manifest["categories"][-1]
    subcategory = dict(model["subcategories"][0])
    subcategory.update({"id": SUBCATEGORY_ID, "localizedDescriptionKey": "WallKika", "localizedNameKey": "WallKika",
                        "preferredOrder": 0, "previewImage": "file://" + png, "representativeAssetID": ASSET_ID})
    category = dict(model)
    category.update({"id": CATEGORY_ID, "localizedDescriptionKey": "WallKika", "localizedNameKey": "WallKika",
                     "preferredOrder": len(manifest["categories"]), "previewImage": "file://" + png,
                     "representativeAssetID": ASSET_ID, "subcategories": [subcategory]})
    manifest["assets"].append(asset)
    manifest["categories"].append(category)
    save_json(MANIFEST, manifest)
    print("entries.json: WallKika aerial added")


def choose_aerial():
    with open(STORE, "rb") as handle:
        store = plistlib.load(handle)
    options = store["AllSpacesAndDisplays"]["Idle"]["Content"].get("EncodedOptionValues")
    if not isinstance(options, bytes) or b"aerialShuffleFrequency" not in options:
        options = plistlib.dumps({"values": {}}, fmt=plistlib.FMT_BINARY)
    choice = {"Configuration": plistlib.dumps({"assetID": ASSET_ID}, fmt=plistlib.FMT_BINARY), "Files": [], "Provider": AERIALS}
    now = datetime.datetime.utcnow()
    count = 0

    def visit(node):
        nonlocal count
        if not isinstance(node, dict):
            return
        for key, value in node.items():
            if key == "SystemDefault":
                continue
            if key in ("Desktop", "Idle") and isinstance(value, dict) and "Content" in value:
                value["Content"]["Choices"] = [dict(choice)]
                value["Content"]["EncodedOptionValues"] = options
                value["LastSet"] = now
                count += 1
            else:
                visit(value)

    visit(store)
    save_plist(STORE, store)
    print(f"Index.plist: {count} wallpaper and screen saver entries now use the WallKika aerial")


def save_json(path, data):
    tmp = path + ".tmp"
    with open(tmp, "w") as handle:
        json.dump(data, handle, indent=2, ensure_ascii=False)
    os.replace(tmp, path)


def save_plist(path, data):
    tmp = path + ".tmp"
    with open(tmp, "wb") as handle:
        plistlib.dump(data, handle, fmt=plistlib.FMT_BINARY)
    os.replace(tmp, path)


def restart_agent():
    subprocess.run(["killall", "WallpaperAgent"], check=False)
    print("WallpaperAgent restarted; lock the screen (Ctrl+Cmd+Q) to see the result")


def enable(video):
    if not os.path.isfile(video):
        fail(f"{video} is not a file")
    if not os.path.isfile(STORE) or not os.path.isfile(MANIFEST):
        fail("this macOS version keeps wallpapers somewhere else; nothing changed")
    backup()
    mov, png = write_media(os.path.abspath(video))
    edit_manifest(mov, png)
    choose_aerial()
    restart_agent()


def disable():
    for name in ("Index.plist", "entries.json"):
        saved = os.path.join(BACKUP, name)
        target = STORE if name == "Index.plist" else MANIFEST
        if os.path.exists(saved):
            shutil.copy2(saved, target)
            print(f"restored {target}")
    for path in (os.path.join(VIDEOS, f"{ASSET_ID}.mov"), os.path.join(THUMBNAILS, f"{ASSET_ID}.png")):
        if os.path.exists(path):
            os.remove(path)
    restart_agent()


if __name__ == "__main__":
    if sys.platform != "darwin":
        fail("macOS only")
    if len(sys.argv) == 3 and sys.argv[1] == "enable":
        enable(sys.argv[2])
    elif len(sys.argv) == 2 and sys.argv[1] == "disable":
        disable()
    else:
        print(__doc__)
        sys.exit(2)
