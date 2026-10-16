// Draws the app icon, the website's mark: a white microphone on a black rounded square, on Apple's
// macOS icon grid (an 824-point body on a 1024-point canvas, with its drop shadow). Writes
// packaging/Asides.icns, which `make bundle` copies into the app.
//
// Usage: swift scripts/make-icon.swift  (needs only the Command Line Tools)
//
// The microphone is drawn with paths, like docs/favicon.svg: SF Symbols may not be used in app icons.

import AppKit

let root = URL(fileURLWithPath: CommandLine.arguments[0]).deletingLastPathComponent().deletingLastPathComponent()

func png(size: Int) -> Data {
    let ctx = CGContext(
        data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    let scale = CGFloat(size) / 1024
    // Draw on the 1024 grid with the origin at the top left, like the SVG mark.
    ctx.scaleBy(x: scale, y: scale)
    ctx.translateBy(x: 0, y: 1024)
    ctx.scaleBy(x: 1, y: -1)

    let body = CGPath(
        roundedRect: CGRect(x: 100, y: 100, width: 824, height: 824), cornerWidth: 185, cornerHeight: 185,
        transform: nil)
    // Apple's template shadow: 10 pt down, 10 pt blur, black at 30%. Shadows ignore the CTM.
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -10 * scale), blur: 20 * scale, color: CGColor(gray: 0, alpha: 0.3))
    ctx.addPath(body)
    ctx.setFillColor(CGColor(gray: 0, alpha: 1))
    ctx.fillPath()
    ctx.restoreGState()

    // A little light from the top, so it doesn't read as a hole in the Dock.
    ctx.saveGState()
    ctx.addPath(body)
    ctx.clip()
    let gradient = CGGradient(
        colorsSpace: CGColorSpace(name: CGColorSpace.sRGB)!,
        colors: [CGColor(srgbRed: 0.24, green: 0.24, blue: 0.25, alpha: 1), CGColor(srgbRed: 0.03, green: 0.03, blue: 0.035, alpha: 1)] as CFArray,
        locations: [0, 1])!
    ctx.drawLinearGradient(gradient, start: CGPoint(x: 512, y: 100), end: CGPoint(x: 512, y: 924), options: [])
    ctx.restoreGState()

    // The mark's 24-unit microphone, centred: a filled capsule, its stand's arc and stem.
    let unit: CGFloat = 22
    let origin = CGPoint(x: 512 - 12 * unit, y: 512 - 12 * unit)
    func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: origin.x + x * unit, y: origin.y + y * unit) }
    ctx.setFillColor(CGColor(gray: 1, alpha: 1))
    ctx.setStrokeColor(CGColor(gray: 1, alpha: 1))
    ctx.addPath(CGPath(
        roundedRect: CGRect(origin: p(8.5, 2.5), size: CGSize(width: 7 * unit, height: 12 * unit)),
        cornerWidth: 3.5 * unit, cornerHeight: 3.5 * unit, transform: nil))
    ctx.fillPath()
    ctx.setLineWidth(2 * unit)
    ctx.setLineCap(.round)
    let stand = CGMutablePath()
    stand.addArc(center: p(12, 11), radius: 7 * unit, startAngle: .pi, endAngle: 0, clockwise: true)
    stand.move(to: p(12, 18))
    stand.addLine(to: p(12, 21.5))
    ctx.addPath(stand)
    ctx.strokePath()

    let image = ctx.makeImage()!
    return NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])!
}

let iconset = FileManager.default.temporaryDirectory.appendingPathComponent("Asides.iconset")
try? FileManager.default.removeItem(at: iconset)
try! FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
for points in [16, 32, 128, 256, 512] {
    try! png(size: points).write(to: iconset.appendingPathComponent("icon_\(points)x\(points).png"))
    try! png(size: points * 2).write(to: iconset.appendingPathComponent("icon_\(points)x\(points)@2x.png"))
}
let out = root.appendingPathComponent("packaging/Asides.icns")
let iconutil = Process()
iconutil.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
iconutil.arguments = ["-c", "icns", iconset.path, "-o", out.path]
try! iconutil.run()
iconutil.waitUntilExit()
precondition(iconutil.terminationStatus == 0, "iconutil failed")
print("Wrote \(out.path)")
