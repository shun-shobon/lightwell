import AppKit

let args = CommandLine.arguments
let source = URL(fileURLWithPath: args[1])
let destination = URL(fileURLWithPath: args[2])
let px = Int(args[3])!

guard let image = NSImage(contentsOf: source) else { fatalError("cannot load \(args[1])") }
let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8, samplesPerPixel: 4,
    hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
image.draw(in: NSRect(x: 0, y: 0, width: px, height: px))
NSGraphicsContext.restoreGraphicsState()
try rep.representation(using: .png, properties: [:])!.write(to: destination)
