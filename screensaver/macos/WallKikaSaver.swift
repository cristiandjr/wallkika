import AVFoundation
import ScreenSaver
import WebKit
import os

@objc(WallKikaSaverView)
final class WallKikaSaverView: ScreenSaverView {
    private struct Config: Decodable {
        let path: String?
        let kind: String?
    }

    private static let log = Logger(subsystem: "com.cristiandjr.wallkika", category: "saver")
    private var player: AVQueuePlayer?
    private var looper: AVPlayerLooper?
    private var stopObserver: NSObjectProtocol?

    override init?(frame: NSRect, isPreview: Bool) {
        super.init(frame: frame, isPreview: isPreview)
        setUp()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        setUp()
    }

    deinit {
        if let stopObserver {
            DistributedNotificationCenter.default().removeObserver(stopObserver)
        }
    }

    override var hasConfigureSheet: Bool { false }

    override func startAnimation() {
        super.startAnimation()
        player?.play()
    }

    override func stopAnimation() {
        super.stopAnimation()
        player?.pause()
    }

    private func setUp() {
        wantsLayer = true
        layer?.backgroundColor = NSColor.black.cgColor
        layer?.masksToBounds = true
        animationTimeInterval = 1
        let wallpaper = Self.wallpaper()
        Self.log.info("Starting (preview: \(self.isPreview)) with \(wallpaper.map { "\($0.kind) \($0.url.path)" } ?? "nothing configured", privacy: .public)")
        show(wallpaper)

        // Since macOS 14 the saver host never releases its views, so playback stops here and the host exits.
        stopObserver = DistributedNotificationCenter.default().addObserver(
            forName: Notification.Name("com.apple.screensaver.willstop"),
            object: nil,
            queue: .main
        ) { [weak self] _ in
            guard let self else { return }
            Self.log.info("Stopping")
            player?.pause()
            if #available(macOS 14, *), !isPreview {
                DispatchQueue.main.asyncAfter(deadline: .now() + 1) { exit(0) }
            }
        }
    }

    private static func wallpaper() -> (url: URL, kind: String)? {
        // The saver runs in a sandbox container, so NSHomeDirectory() is not the user's home.
        let home = getpwuid(getuid()).map { String(cString: $0.pointee.pw_dir) } ?? NSHomeDirectory()
        let config = URL(fileURLWithPath: home)
            .appendingPathComponent("Library/Application Support/com.cristiandjr.wallkika/screensaver/config.json")
        guard
            let data = try? Data(contentsOf: config),
            let decoded = try? JSONDecoder().decode(Config.self, from: data),
            let path = decoded.path,
            let kind = decoded.kind,
            FileManager.default.isReadableFile(atPath: path)
        else { return nil }
        return (URL(fileURLWithPath: path), kind)
    }

    private func show(_ wallpaper: (url: URL, kind: String)?) {
        guard let wallpaper else { return showMessage() }
        switch wallpaper.kind {
        case "video": showVideo(wallpaper.url)
        case "web": showWeb(wallpaper.url)
        case "gif": showAnimatedImage(wallpaper.url)
        default: showImage(wallpaper.url)
        }
    }

    private func showVideo(_ url: URL) {
        let player = AVQueuePlayer()
        player.isMuted = true
        player.preventsDisplaySleepDuringVideoPlayback = false
        looper = AVPlayerLooper(player: player, templateItem: AVPlayerItem(url: url))
        let playerLayer = AVPlayerLayer(player: player)
        playerLayer.videoGravity = .resizeAspectFill
        addLayer(playerLayer)
        self.player = player
        player.play()
        DispatchQueue.main.asyncAfter(deadline: .now() + 3) { [weak self] in
            guard let self, let item = player.currentItem else { return }
            let error = item.error.map { " error: \($0.localizedDescription)" } ?? ""
            Self.log.info("Video status \(item.status.rawValue) rate \(player.rate) at \(player.currentTime().seconds, format: .fixed(precision: 1))s in \(Int(self.bounds.width))x\(Int(self.bounds.height))\(error, privacy: .public)")
        }
    }

    private func showImage(_ url: URL) {
        guard let image = NSImage(contentsOf: url) else { return showMessage() }
        let imageLayer = CALayer()
        imageLayer.contents = image
        imageLayer.contentsGravity = .resizeAspectFill
        addLayer(imageLayer)
    }

    private func showAnimatedImage(_ url: URL) {
        guard let image = NSImage(contentsOf: url), image.size.width > 0, image.size.height > 0 else {
            return showMessage()
        }
        let scale = max(bounds.width / image.size.width, bounds.height / image.size.height)
        let size = NSSize(width: image.size.width * scale, height: image.size.height * scale)
        let imageView = NSImageView(frame: NSRect(
            x: (bounds.width - size.width) / 2,
            y: (bounds.height - size.height) / 2,
            width: size.width,
            height: size.height
        ))
        imageView.image = image
        imageView.imageScaling = .scaleAxesIndependently
        imageView.animates = true
        addSubview(imageView)
    }

    private func showWeb(_ url: URL) {
        let configuration = WKWebViewConfiguration()
        configuration.mediaTypesRequiringUserActionForPlayback = []
        let webView = WKWebView(frame: bounds, configuration: configuration)
        webView.autoresizingMask = [.width, .height]
        webView.loadFileURL(url, allowingReadAccessTo: url.deletingLastPathComponent())
        addSubview(webView)
    }

    private func showMessage() {
        let label = NSTextField(labelWithString: "Choose a wallpaper for your screen saver in WallKika")
        label.textColor = NSColor(white: 1, alpha: 0.75)
        label.font = .systemFont(ofSize: isPreview ? 11 : 24, weight: .medium)
        label.alignment = .center
        label.sizeToFit()
        label.frame.origin = NSPoint(
            x: (bounds.width - label.frame.width) / 2,
            y: (bounds.height - label.frame.height) / 2
        )
        label.autoresizingMask = [.minXMargin, .maxXMargin, .minYMargin, .maxYMargin]
        addSubview(label)
    }

    private func addLayer(_ sublayer: CALayer) {
        sublayer.frame = bounds
        sublayer.autoresizingMask = [.layerWidthSizable, .layerHeightSizable]
        layer?.addSublayer(sublayer)
    }
}
