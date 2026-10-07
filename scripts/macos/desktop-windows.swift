import CoreGraphics
import Foundation

let desktop = Int(CGWindowLevelForKey(.desktopWindow))
let icons = Int(CGWindowLevelForKey(.desktopIconWindow))
print("levels: desktop=\(desktop) icons=\(icons)")

let windows = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? []
for window in windows {
    let owner = window[kCGWindowOwnerName as String] as? String ?? ""
    let layer = window[kCGWindowLayer as String] as? Int ?? 0
    guard owner.lowercased().contains("wallkika") || (layer <= icons && owner != "Window Server") else { continue }
    let bounds = window[kCGWindowBounds as String] as? [String: Double] ?? [:]
    let onscreen = window[kCGWindowIsOnscreen as String] as? Bool ?? false
    let rect = "x=\(Int(bounds["X"] ?? 0)) y=\(Int(bounds["Y"] ?? 0)) w=\(Int(bounds["Width"] ?? 0)) h=\(Int(bounds["Height"] ?? 0))"
    print("\(owner.padding(toLength: 12, withPad: " ", startingAt: 0)) layer=\(layer) \(rect) onscreen=\(onscreen)")
}
