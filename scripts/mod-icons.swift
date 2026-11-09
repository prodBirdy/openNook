// Draws the shipped mods' tab icons (crates/nook/mods/<mod>/icon.png) as macOS app icons: the
// squircle on Apple's 1024 grid (an 824 body with a transparent margin, as an app's own icon
// has), a gradient, one bold glyph, and a glassy rim. Run from the repo root:
//   swift scripts/mod-icons.swift
import AppKit
import CoreGraphics

let size = 256.0
let grid = size / 1024
let body = CGRect(x: 100 * grid, y: 100 * grid, width: 824 * grid, height: 824 * grid)

func rgb(_ hex: UInt32, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 255) / 255, green: CGFloat((hex >> 8) & 255) / 255, blue: CGFloat(hex & 255) / 255, alpha: a)
}

// Apple's icon outline is close to a superellipse of exponent 5.
func squircle(_ r: CGRect) -> CGPath {
    let p = CGMutablePath()
    let (a, b, n) = (r.width / 2, r.height / 2, 5.0)
    for i in 0...360 {
        let t = Double(i) / 360 * 2 * .pi
        let (c, s) = (cos(t), sin(t))
        let x = r.midX + a * (c < 0 ? -1 : 1) * pow(abs(c), 2 / n)
        let y = r.midY + b * (s < 0 ? -1 : 1) * pow(abs(s), 2 / n)
        i == 0 ? p.move(to: CGPoint(x: x, y: y)) : p.addLine(to: CGPoint(x: x, y: y))
    }
    p.closeSubpath()
    return p
}

func linear(_ cx: CGContext, _ top: CGColor, _ bottom: CGColor, in r: CGRect) {
    let g = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: [top, bottom] as CFArray, locations: [0, 1])!
    cx.drawLinearGradient(g, start: CGPoint(x: r.midX, y: r.maxY), end: CGPoint(x: r.midX, y: r.minY), options: [])
}

func icon(_ out: String, top: UInt32, bottom: UInt32, glyph: (CGContext) -> Void) {
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let cx = CGContext(data: nil, width: Int(size), height: Int(size), bitsPerComponent: 8, bytesPerRow: 0, space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    let shape = squircle(body)
    // The soft shadow app icons sit on.
    cx.saveGState()
    cx.setShadow(offset: CGSize(width: 0, height: -6 * grid), blur: 18 * grid, color: rgb(0x000000, 0.35))
    cx.addPath(shape)
    cx.setFillColor(rgb(bottom))
    cx.fillPath()
    cx.restoreGState()

    cx.saveGState()
    cx.addPath(shape)
    cx.clip()
    linear(cx, rgb(top), rgb(bottom), in: body)
    // Light from above: a broad sheen over the upper half.
    let sheen = CGGradient(colorsSpace: cs, colors: [rgb(0xffffff, 0.22), rgb(0xffffff, 0)] as CFArray, locations: [0, 1])!
    cx.drawRadialGradient(sheen, startCenter: CGPoint(x: body.midX, y: body.maxY + body.height * 0.15), startRadius: 0,
                          endCenter: CGPoint(x: body.midX, y: body.maxY + body.height * 0.15), endRadius: body.height * 0.85, options: [])
    glyph(cx)
    cx.restoreGState()

    // The glass rim: bright at the top, fading down the sides.
    cx.saveGState()
    cx.addPath(shape)
    cx.setLineWidth(10 * grid)
    cx.replacePathWithStrokedPath()
    cx.clip()
    linear(cx, rgb(0xffffff, 0.55), rgb(0xffffff, 0.06), in: body)
    cx.restoreGState()

    let rep = NSBitmapImageRep(cgImage: cx.makeImage()!)
    try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
    print(out)
}

func glyphShadow(_ cx: CGContext) {
    cx.setShadow(offset: CGSize(width: 0, height: -8 * grid), blur: 20 * grid, color: rgb(0x000000, 0.28))
}

// Timer: the page's ring, three quarters left, under a stopwatch crown.
icon("crates/nook/mods/timer/icon.png", top: 0xffb340, bottom: 0xff7a00) { cx in
    let c = CGPoint(x: body.midX, y: body.midY - 30 * grid)
    let r = 250 * grid
    let w = 74 * grid
    cx.saveGState()
    glyphShadow(cx)
    cx.beginTransparencyLayer(auxiliaryInfo: nil)
    cx.setLineCap(.round)
    cx.setLineWidth(w)
    cx.setStrokeColor(rgb(0xffffff, 0.38))
    cx.addArc(center: c, radius: r, startAngle: 0, endAngle: 2 * .pi, clockwise: false)
    cx.strokePath()
    cx.setStrokeColor(rgb(0xffffff))
    // From twelve o'clock, clockwise, three quarters round.
    cx.addArc(center: c, radius: r, startAngle: .pi / 2, endAngle: .pi / 2 - 1.5 * .pi, clockwise: true)
    cx.strokePath()
    cx.setFillColor(rgb(0xffffff))
    let crown = CGRect(x: c.x - 70 * grid, y: c.y + r + w / 2 + 26 * grid, width: 140 * grid, height: 56 * grid)
    cx.addPath(CGPath(roundedRect: crown, cornerWidth: 28 * grid, cornerHeight: 28 * grid, transform: nil))
    cx.fillPath()
    cx.fill(CGRect(x: c.x - 22 * grid, y: c.y + r + w / 2 - 4 * grid, width: 44 * grid, height: 34 * grid))
    cx.addEllipse(in: CGRect(x: c.x - 34 * grid, y: c.y - 34 * grid, width: 68 * grid, height: 68 * grid))
    cx.fillPath()
    cx.endTransparencyLayer()
    cx.restoreGState()
}

// ISS: the station over the Earth's lit limb, in a night sky.
icon("crates/nook/mods/iss/icon.png", top: 0x24357e, bottom: 0x060a20) { cx in
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    for (x, y, s) in [(250.0, 760.0, 9.0), (720.0, 800.0, 7.0), (800.0, 600.0, 6.0), (330.0, 560.0, 5.0), (600.0, 700.0, 5.0)] {
        cx.setFillColor(rgb(0xffffff, 0.8))
        cx.fillEllipse(in: CGRect(x: x * grid, y: y * grid, width: s * grid, height: s * grid))
    }
    // The Earth: a great disc below, its rim lit and haloed by the atmosphere.
    let earth = CGRect(x: body.midX - 900 * grid, y: 100 * grid - 1500 * grid, width: 1800 * grid, height: 1800 * grid)
    cx.saveGState()
    cx.setShadow(offset: .zero, blur: 40 * grid, color: rgb(0x5ac8fa, 0.9))
    cx.addEllipse(in: earth)
    cx.setFillColor(rgb(0x0b3a8f))
    cx.fillPath()
    cx.restoreGState()
    cx.saveGState()
    cx.addEllipse(in: earth)
    cx.clip()
    let sea = CGGradient(colorsSpace: cs, colors: [rgb(0x64b5ff), rgb(0x1f6fe0), rgb(0x0b2f78)] as CFArray, locations: [0, 0.08, 0.3])!
    cx.drawLinearGradient(sea, start: CGPoint(x: 0, y: earth.maxY), end: CGPoint(x: 0, y: earth.maxY - 520 * grid), options: [.drawsAfterEndLocation])
    cx.restoreGState()

    // The station, tilted a little: a truss with two pairs of gold arrays and a white core.
    cx.saveGState()
    cx.translateBy(x: body.midX, y: body.midY + 70 * grid)
    cx.rotate(by: 0.22)
    glyphShadow(cx)
    cx.beginTransparencyLayer(auxiliaryInfo: nil)
    let panel = CGGradient(colorsSpace: cs, colors: [rgb(0xffd77a), rgb(0xe9a530)] as CFArray, locations: [0, 1])!
    for x in [-300.0, -185.0, 105.0, 220.0] {
        for y in [28.0, -188.0] {
            let r = CGRect(x: x * grid, y: y * grid, width: 80 * grid, height: 160 * grid)
            cx.saveGState()
            cx.addPath(CGPath(roundedRect: r, cornerWidth: 8 * grid, cornerHeight: 8 * grid, transform: nil))
            cx.clip()
            cx.drawLinearGradient(panel, start: CGPoint(x: 0, y: r.maxY), end: CGPoint(x: 0, y: r.minY), options: [])
            cx.restoreGState()
        }
    }
    cx.setFillColor(rgb(0xffffff))
    cx.fill(CGRect(x: -320 * grid, y: -14 * grid, width: 640 * grid, height: 28 * grid))
    cx.addPath(CGPath(roundedRect: CGRect(x: -48 * grid, y: -90 * grid, width: 96 * grid, height: 180 * grid), cornerWidth: 26 * grid, cornerHeight: 26 * grid, transform: nil))
    cx.fillPath()
    cx.endTransparencyLayer()
    cx.restoreGState()
}

// Containers: a shipping container in isometric view, three faces in white at different strengths, so it reads as solid.
icon("crates/nook/mods/containers/icon.png", top: 0x3aa0ff, bottom: 0x0a4fd6) { cx in
    let c = CGPoint(x: body.midX, y: body.midY - 10 * grid)
    let r = 270 * grid              // centre to a side corner
    let h = r * 0.5                 // half the top face's height (30° isometric)
    let top = CGPoint(x: c.x, y: c.y + 2 * h)
    let left = CGPoint(x: c.x - r * 0.866 * 1.15, y: c.y + h)
    let right = CGPoint(x: c.x + r * 0.866 * 1.15, y: c.y + h)
    let drop = 300 * grid
    let mid = c
    func face(_ pts: [CGPoint], _ a: CGFloat) {
        cx.setFillColor(rgb(0xffffff, a))
        cx.addLines(between: pts)
        cx.closePath()
        cx.fillPath()
    }
    cx.saveGState()
    glyphShadow(cx)
    cx.beginTransparencyLayer(auxiliaryInfo: nil)
    face([top, right, mid, left], 1)                                                               // lid
    face([left, mid, CGPoint(x: mid.x, y: mid.y - drop), CGPoint(x: left.x, y: left.y - drop)], 0.86)   // front left
    face([mid, right, CGPoint(x: right.x, y: right.y - drop), CGPoint(x: mid.x, y: mid.y - drop)], 0.68) // front right
    // Seams: the edges between faces, cut back into the blue.
    cx.setBlendMode(.clear)
    cx.setLineWidth(14 * grid)
    cx.setLineJoin(.round)
    cx.addLines(between: [left, mid, right])
    cx.move(to: mid); cx.addLine(to: CGPoint(x: mid.x, y: mid.y - drop))
    cx.strokePath()
    // The container's ribs, on the right face.
    cx.setLineWidth(10 * grid)
    for k in 1...3 {
        let t = CGFloat(k) / 4
        let a = CGPoint(x: mid.x + (right.x - mid.x) * t, y: mid.y + (right.y - mid.y) * t)
        cx.move(to: CGPoint(x: a.x, y: a.y - 40 * grid)); cx.addLine(to: CGPoint(x: a.x, y: a.y - drop + 40 * grid))
    }
    cx.strokePath()
    cx.endTransparencyLayer()
    cx.restoreGState()
}
