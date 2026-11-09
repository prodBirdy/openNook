// The Messages mod's helper, built once with swiftc into ~/Library/Caches/openNook/messages.
// AVFoundation reads attachments in this process, which openNook's Full Disk Access covers
// (Quick Look reads them in a service of its own, which it doesn't).
//
//   helper thumb <video> <out.jpg>   a frame from the start, at most 640 px; prints "w h"
//   helper wave <audio> <bars>       prints {"duration": seconds, "peaks": [0…1, …]}

import AVFoundation
import AppKit

func fail(_ why: String) -> Never {
    FileHandle.standardError.write(Data((why + "\n").utf8))
    exit(1)
}

let args = CommandLine.arguments
guard args.count >= 4 else { fail("usage: helper thumb <video> <out.jpg> | wave <audio> <bars>") }
let url = URL(fileURLWithPath: args[2])

switch args[1] {
case "thumb":
    let generator = AVAssetImageGenerator(asset: AVURLAsset(url: url))
    generator.appliesPreferredTrackTransform = true
    generator.maximumSize = CGSize(width: 640, height: 640)
    let image: CGImage
    do { image = try generator.copyCGImage(at: CMTime(seconds: 0.1, preferredTimescale: 600), actualTime: nil) }
    catch { fail("\(error.localizedDescription)") }
    guard let jpeg = NSBitmapImageRep(cgImage: image).representation(using: .jpeg, properties: [.compressionFactor: 0.82]) else { fail("couldn't encode") }
    do { try jpeg.write(to: URL(fileURLWithPath: args[3])) } catch { fail("\(error.localizedDescription)") }
    print("\(image.width) \(image.height)")

case "wave":
    let bars = max(1, Int(args[3]) ?? 40)
    let file: AVAudioFile
    do { file = try AVAudioFile(forReading: url) } catch { fail("\(error.localizedDescription)") }
    let rate = file.processingFormat.sampleRate
    let frames = AVAudioFrameCount(file.length)
    guard frames > 0, let buffer = AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: frames) else { fail("empty") }
    do { try file.read(into: buffer) } catch { fail("\(error.localizedDescription)") }
    guard let samples = buffer.floatChannelData?[0] else { fail("not float audio") }
    let count = Int(buffer.frameLength)
    // Loudness per bar: RMS over its slice, then scaled so the loudest bar is 1.
    var levels = [Float](repeating: 0, count: bars)
    for bar in 0..<bars {
        let start = bar * count / bars, end = max(start + 1, (bar + 1) * count / bars)
        var sum: Float = 0
        for i in start..<min(end, count) { sum += samples[i] * samples[i] }
        levels[bar] = (sum / Float(end - start)).squareRoot()
    }
    let loudest = max(levels.max() ?? 0, 0.0001)
    let peaks = levels.map { (($0 / loudest) * 100).rounded() / 100 }
    let duration = Double(count) / rate
    print("{\"duration\": \(String(format: "%.2f", duration)), \"peaks\": [\(peaks.map { String($0) }.joined(separator: ","))]}")

default:
    fail("unknown command \(args[1])")
}
