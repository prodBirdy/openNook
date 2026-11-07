// Crops a PNG to its non-transparent pixels plus a margin, in place: `opennook shot` renders the
// island on a full transparent canvas, which leaves README screenshots mostly empty.
// usage: swift scripts/trim-png.swift <file.png> [margin px]
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let args = CommandLine.arguments
guard args.count >= 2 else {
    FileHandle.standardError.write("usage: trim-png.swift <file.png> [margin px]\n".data(using: .utf8)!)
    exit(2)
}
let url = URL(fileURLWithPath: args[1])
let margin = args.count > 2 ? Int(args[2]) ?? 0 : 0
guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
      let image = CGImageSourceCreateImageAtIndex(src, 0, nil) else {
    FileHandle.standardError.write("trim-png: can't read \(url.path)\n".data(using: .utf8)!)
    exit(1)
}
let (w, h) = (image.width, image.height)
var px = [UInt8](repeating: 0, count: w * h * 4)
let ctx = CGContext(data: &px, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                    space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
var (minX, minY, maxX, maxY) = (w, h, -1, -1)
for y in 0..<h {
    for x in 0..<w where px[(y * w + x) * 4 + 3] > 8 {
        minX = min(minX, x); maxX = max(maxX, x)
        minY = min(minY, y); maxY = max(maxY, y)
    }
}
guard maxX >= 0 else { exit(0) }
let rect = CGRect(x: max(minX - margin, 0), y: max(minY - margin, 0),
                  width: min(maxX - minX + 1 + 2 * margin, w), height: min(maxY - minY + 1 + 2 * margin, h))
guard let cropped = image.cropping(to: rect),
      let dest = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil) else { exit(1) }
CGImageDestinationAddImage(dest, cropped, nil)
CGImageDestinationFinalize(dest)
print("\(url.lastPathComponent) \(Int(rect.width))x\(Int(rect.height))")
