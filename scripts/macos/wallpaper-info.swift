import AppKit

for (index, screen) in NSScreen.screens.enumerated() {
    let url = NSWorkspace.shared.desktopImageURL(for: screen)
    print("[\(index)] \(screen.localizedName) frame=\(screen.frame) scale=\(screen.backingScaleFactor)")
    print("    \(url?.path ?? "no wallpaper")")
}
