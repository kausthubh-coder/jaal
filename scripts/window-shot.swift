// Screenshot one app's window as the screen shows it, child windows and their
// shadows included, without other apps' windows that sit above it.
//
//   swift scripts/window-shot.swift <pid> <out.png>
//
// Why not `screencapture -l <id>`: when the command bar is a child window
// (CEF Views overlays on macOS, GPUI fix A), `-l` captures the window group
// but leaves out the child's shadow, which is the thing we need to judge. A
// region capture (`-R`) shows the shadow but also picks up other threads'
// windows and system dialogs on the shared screen.
//
// This captures the app's largest window's frame, drawing only the app's
// frontmost window and what is below it: our child window, our main window,
// and nothing that floats above us. Anything of another app's that is BELOW
// our main window can't show, because our window is opaque.

import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let args = CommandLine.arguments
guard args.count == 3, let pid = Int(args[1]) else {
    FileHandle.standardError.write("usage: swift window-shot.swift <pid> <out.png>\n".data(using: .utf8)!)
    exit(2)
}

let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
    as? [[String: Any]] ?? []
// Front to back.
let ours = info.compactMap { w -> (id: UInt32, bounds: CGRect)? in
    guard (w[kCGWindowOwnerPID as String] as? Int) == pid,
          let id = w[kCGWindowNumber as String] as? Int,
          let dict = w[kCGWindowBounds as String] as? NSDictionary,
          let bounds = CGRect(dictionaryRepresentation: dict) else { return nil }
    return (UInt32(id), bounds)
}
guard let front = ours.first,
      let main = ours.max(by: { $0.bounds.width * $0.bounds.height < $1.bounds.width * $1.bounds.height }) else {
    FileHandle.standardError.write("no on-screen windows for pid \(pid)\n".data(using: .utf8)!)
    exit(1)
}

// CGWindowListCreateImage is marked unavailable in the Swift SDK since macOS
// 15 but still ships; look it up at run time.
typealias CreateImage = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?
guard let symbol = dlsym(dlopen(nil, RTLD_NOW), "CGWindowListCreateImage") else {
    FileHandle.standardError.write("CGWindowListCreateImage is gone from this macOS\n".data(using: .utf8)!)
    exit(1)
}
let createImage = unsafeBitCast(symbol, to: CreateImage.self)
let onScreenBelowWindow: UInt32 = 1 << 2, includingWindow: UInt32 = 1 << 3
let imageBestResolution: UInt32 = 1 << 3
guard let image = createImage(main.bounds, onScreenBelowWindow | includingWindow, front.id,
                              imageBestResolution)?.takeRetainedValue() else {
    FileHandle.standardError.write("capture failed (Screen Recording permission?)\n".data(using: .utf8)!)
    exit(1)
}

let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: args[2]) as CFURL,
                                           UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(dest, image, nil)
CGImageDestinationFinalize(dest)
print("wrote \(args[2]): \(ours.count) windows, \(image.width)x\(image.height)")
