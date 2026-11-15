// Decode the QR codes in an image with macOS Core Image (an independent
// check of crates/crashqr's encoder): swift tools/scripts/qrdecode.swift IMAGE
import CoreImage
import Foundation

let url = URL(fileURLWithPath: CommandLine.arguments[1])
guard let image = CIImage(contentsOf: url) else { print("cannot read image"); exit(2) }
let detector = CIDetector(ofType: CIDetectorTypeQRCode, context: nil, options: [CIDetectorAccuracy: CIDetectorAccuracyHigh])!
let found = detector.features(in: image).compactMap { ($0 as? CIQRCodeFeature)?.messageString }
if found.isEmpty { print("no QR code found"); exit(1) }
for s in found { print(s) }
