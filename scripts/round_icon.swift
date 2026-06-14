// Apply the macOS app-icon shape to a square source: centered rounded-rect tile
// with the standard ~80% inset and corner radius. Usage: round_icon <in> <out>
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let args = CommandLine.arguments
guard args.count == 3 else {
    FileHandle.standardError.write(Data("usage: round_icon <in.png> <out.png>\n".utf8))
    exit(1)
}

let size = 1024.0
let margin = 100.0 // transparent border per Apple's icon grid
let tile = size - 2 * margin // 824
let radius = tile * 0.2237 // squircle-approximating corner radius

guard
    let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: args[1]) as CFURL, nil),
    let img = CGImageSourceCreateImageAtIndex(src, 0, nil),
    let ctx = CGContext(
        data: nil, width: Int(size), height: Int(size), bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
else { exit(1) }

ctx.interpolationQuality = .high
let rect = CGRect(x: margin, y: margin, width: tile, height: tile)
ctx.addPath(CGPath(roundedRect: rect, cornerWidth: radius, cornerHeight: radius, transform: nil))
ctx.clip()
ctx.draw(img, in: rect)

guard
    let out = ctx.makeImage(),
    let dst = CGImageDestinationCreateWithURL(
        URL(fileURLWithPath: args[2]) as CFURL, UTType.png.identifier as CFString, 1, nil)
else { exit(1) }
CGImageDestinationAddImage(dst, out, nil)
CGImageDestinationFinalize(dst)
