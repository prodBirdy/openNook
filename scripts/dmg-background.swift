// Draws the installer DMG's window background, at 1x and 2x, in the website's style: black, the
// real closed island (resources/dmg/island.png, a capture over macOS's wallpaper) as a card at
// the top, a thin arrow from where openNook.app sits to where Applications does, and what to do
// in white and grey. The window is 660 x 400 points; installer.sh puts the icons at the centres
// below and runs this on every build:
//   swift scripts/dmg-background.swift <out dir>
import AppKit
import CoreGraphics

let outDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "."
let size = CGSize(width: 660, height: 400)
/// Icon centres, in points from the window's top-left.
let app = CGPoint(x: 165, y: 190)
let applications = CGPoint(x: 495, y: 190)

func rgb(_ hex: UInt32, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 255) / 255, green: CGFloat((hex >> 8) & 255) / 255, blue: CGFloat(hex & 255) / 255, alpha: a)
}

func draw(scale: CGFloat, to out: String) {
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let cx = CGContext(data: nil, width: Int(size.width * scale), height: Int(size.height * scale), bitsPerComponent: 8, bytesPerRow: 0,
                       space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    // Points, top-left origin, like Finder's.
    cx.scaleBy(x: scale, y: scale)
    cx.translateBy(x: 0, y: size.height)
    cx.scaleBy(x: 1, y: -1)

    // Black, like the website; the only colour is the real island, as the site's is.
    cx.setFillColor(rgb(0x000000))
    cx.fill(CGRect(origin: .zero, size: size))

    // The closed island over macOS's wallpaper: a rounded card at the top, no frame.
    if let data = try? Data(contentsOf: URL(fileURLWithPath: "resources/dmg/island.png")),
       let island = NSBitmapImageRep(data: data)?.cgImage {
        let card = CGRect(x: (size.width - 300) / 2, y: 28, width: 300, height: 55)
        cx.saveGState()
        cx.addPath(CGPath(roundedRect: card, cornerWidth: 12, cornerHeight: 12, transform: nil))
        cx.clip()
        // Flipped context: draw the image upside down so it lands upright.
        cx.translateBy(x: card.minX, y: card.maxY)
        cx.scaleBy(x: 1, y: -1)
        cx.draw(island, in: CGRect(origin: .zero, size: card.size))
        cx.restoreGState()
    }

    // The arrow: one thin line and a small head, from the app to Applications.
    let y = app.y
    let from = app.x + 84, to = applications.x - 84
    cx.setStrokeColor(rgb(0xebebf5, 0.6))
    cx.setLineWidth(1.5)
    cx.setLineCap(.round)
    cx.setLineJoin(.round)
    cx.move(to: CGPoint(x: from, y: y))
    cx.addLine(to: CGPoint(x: to, y: y))
    cx.move(to: CGPoint(x: to - 7, y: y - 6))
    cx.addLine(to: CGPoint(x: to, y: y))
    cx.addLine(to: CGPoint(x: to - 7, y: y + 6))
    cx.strokePath()

    // What to do, clear of the icons and their labels, in the site's white and grey.
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(cgContext: cx, flipped: true)
    let lines: [(String, NSFont, NSColor, CGFloat)] = [
        ("Drag openNook to Applications", .systemFont(ofSize: 15, weight: .semibold), NSColor(white: 1, alpha: 1), 314),
        ("Then open it once: it lives around your camera.", .systemFont(ofSize: 12, weight: .regular), NSColor(srgbRed: 0xeb / 255, green: 0xeb / 255, blue: 0xf5 / 255, alpha: 0.6), 338),
    ]
    for (string, font, color, top) in lines {
        let text = NSAttributedString(string: string, attributes: [.font: font, .foregroundColor: color])
        text.draw(at: CGPoint(x: (size.width - text.size().width) / 2, y: top))
    }
    NSGraphicsContext.restoreGraphicsState()

    let rep = NSBitmapImageRep(cgImage: cx.makeImage()!)
    rep.size = size
    try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
}

draw(scale: 1, to: "\(outDir)/background.png")
draw(scale: 2, to: "\(outDir)/background@2x.png")
