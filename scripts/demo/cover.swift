// Draws the demo track's cover (scripts/demo/cover.png): soft colour fields, no text.
//   swift scripts/demo/cover.swift scripts/demo/cover.png
import AppKit
let n = 600
let cs = CGColorSpace(name: CGColorSpace.sRGB)!
let cx = CGContext(data: nil, width: n, height: n, bitsPerComponent: 8, bytesPerRow: 0, space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
func c(_ h: UInt32, _ a: CGFloat = 1) -> CGColor { CGColor(srgbRed: CGFloat(h >> 16 & 255) / 255, green: CGFloat(h >> 8 & 255) / 255, blue: CGFloat(h & 255) / 255, alpha: a) }
let base = CGGradient(colorsSpace: cs, colors: [c(0x2a0b5e), c(0x0b1f5c)] as CFArray, locations: [0, 1])!
cx.drawLinearGradient(base, start: CGPoint(x: 0, y: n), end: CGPoint(x: n, y: 0), options: [])
for (x, y, r, col) in [(170.0, 420.0, 300.0, 0xff5e62), (440.0, 380.0, 260.0, 0xff9f0a), (320.0, 160.0, 320.0, 0x7a5cff), (90.0, 120.0, 200.0, 0x18c8ff)] {
    let g = CGGradient(colorsSpace: cs, colors: [c(UInt32(col), 0.85), c(UInt32(col), 0)] as CFArray, locations: [0, 1])!
    cx.drawRadialGradient(g, startCenter: CGPoint(x: x, y: y), startRadius: 0, endCenter: CGPoint(x: x, y: y), endRadius: r, options: [])
}
cx.setStrokeColor(c(0xffffff, 0.85)); cx.setLineWidth(10)
cx.addEllipse(in: CGRect(x: 200, y: 200, width: 200, height: 200)); cx.strokePath()
let rep = NSBitmapImageRep(cgImage: cx.makeImage()!)
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
