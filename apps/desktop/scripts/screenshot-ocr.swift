// Reads the text in translator screenshots with macOS Vision, for the coupler's OCR check
// (`screenshot-ocr.ts` compiles and runs this; `couple-screenshots.ts` reads the result).
//
// Usage: screenshot-ocr <png>...
// Prints one JSON object per image on its own line: {"file": "<path>", "lines": ["…", …]},
// or {"file": "<path>", "error": "…"} for an image it couldn't read. Never exits non-zero
// for one bad image, so a batch always reports on every file.
//
// Accurate recognition, English only, and no language correction: the check compares
// against the exact catalog English, and correction "fixes" UI words (and `[N]`-style
// tokens) into dictionary ones.

import AppKit
import Foundation
import Vision

func emit(_ object: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: object, options: []),
          let line = String(data: data, encoding: .utf8)
    else { return }
    print(line)
}

@main
struct ScreenshotOcr {
    static func main() {
        for path in CommandLine.arguments.dropFirst() {
            guard let image = NSImage(contentsOfFile: path),
                  let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil)
            else {
                emit(["file": path, "error": "couldn't load the image"])
                continue
            }
            let request = VNRecognizeTextRequest()
            request.recognitionLevel = .accurate
            request.usesLanguageCorrection = false
            request.recognitionLanguages = ["en-US"]
            request.minimumTextHeight = 0
            do {
                try VNImageRequestHandler(cgImage: cgImage, options: [:]).perform([request])
            } catch {
                emit(["file": path, "error": "\(error)"])
                continue
            }
            let lines = (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }
            emit(["file": path, "lines": lines])
        }
    }
}
