// ScreenCaptureKit audio only: video frames are never registered, stored or sent.
// stdout is a bounded binary PCM protocol consumed only by Golos's native worker.
import Foundation
import ScreenCaptureKit
import AVFoundation
import CoreMedia

struct App: Codable { let pid: Int32; let name: String }
func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data(message.utf8))
    exit(1)
}
func hostSeconds() -> Double { CMTimeGetSeconds(CMClockGetTime(CMClockGetHostTimeClock())) }

@available(macOS 13.0, *)
final class Capture: NSObject, SCStreamOutput, SCStreamDelegate {
    let queue = DispatchQueue(label: "com.golos.capture")
    var stream: SCStream?
    var origin = 0.0
    var pausedAt: Double?
    var pausedTotal = 0.0
    var stopped = false
    var started = false
    var converters: [Int: AVAudioConverter] = [:]
    var timer: DispatchSourceTimer?
    let output = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 16000, channels: 1, interleaved: false)!

    // kind: 0=system, 1=microphone, 2=timeline heartbeat, 3=ready, 4=finished.
    func packet(_ kind: UInt32, _ time: Double, _ samples: [Float] = []) {
        var k = kind.littleEndian, t = time.bitPattern.littleEndian, n = UInt32(samples.count).littleEndian
        var data = Data(bytes: &k, count: 4)
        data.append(Data(bytes: &t, count: 8)); data.append(Data(bytes: &n, count: 4))
        samples.withUnsafeBytes { data.append(contentsOf: $0) }
        FileHandle.standardOutput.write(data)
    }
    func activeTime(_ now: Double) -> Double { max(0, (pausedAt ?? now) - origin - pausedTotal) }
    func stream(_ stream: SCStream, didStopWithError error: Error) {
        fail("Audio capture stopped: \(error.localizedDescription). The captured audio can still be saved.")
    }
    func stream(_ stream: SCStream, didOutputSampleBuffer sample: CMSampleBuffer, of type: SCStreamOutputType) {
        guard started, !stopped, pausedAt == nil, sample.isValid else { return }
        let kind: Int
        if type == .audio { kind = 0 }
        else if #available(macOS 15.0, *), type == .microphone { kind = 1 }
        else { return }
        guard let description = sample.formatDescription,
              let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(description),
              let format = AVAudioFormat(streamDescription: asbd) else { fail("Unsupported captured audio format.") }
        let frames = AVAudioFrameCount(sample.numSamples)
        guard frames > 0, frames <= 192000,
              let input = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames) else { return }
        input.frameLength = frames
        let status = CMSampleBufferCopyPCMDataIntoAudioBufferList(sample, at: 0, frameCount: Int32(frames), into: input.mutableAudioBufferList)
        guard status == noErr else { fail("Could not read captured audio (\(status)).") }
        if converters[kind]?.inputFormat != format {
            converters[kind] = AVAudioConverter(from: format, to: output)
        }
        guard let converter = converters[kind],
              let converted = AVAudioPCMBuffer(pcmFormat: output, frameCapacity: AVAudioFrameCount(ceil(Double(frames) * 16000 / format.sampleRate)) + 64) else { fail("Could not resample captured audio.") }
        converter.primeMethod = .none
        converter.downmix = true
        var supplied = false
        var error: NSError?
        let result = converter.convert(to: converted, error: &error) { _, state in
            if supplied { state.pointee = .noDataNow; return nil }
            supplied = true; state.pointee = .haveData; return input
        }
        if result == .error { fail("Audio conversion failed: \(error?.localizedDescription ?? "unknown error")") }
        guard let floats = converted.floatChannelData else { return }
        // ScreenCaptureKit timestamps both output types in the host clock domain.
        let time = CMTimeGetSeconds(sample.presentationTimeStamp) - origin - pausedTotal
        if !time.isFinite { fail("The audio source returned an invalid clock.") }
        packet(UInt32(kind), time, Array(UnsafeBufferPointer(start: floats[0], count: Int(converted.frameLength))))
    }
    func start(pid: Int32, parent: Int32, mixed: Bool) async throws {
        if mixed {
            guard #available(macOS 15.0, *) else { throw NSError(domain: "Golos", code: 1, userInfo: [NSLocalizedDescriptionKey:"Microphone plus application audio requires macOS 15 or later."]) }
            let granted = await AVCaptureDevice.requestAccess(for: .audio)
            if !granted { fail("Microphone access denied. Enable Golos Audio Capture in System Settings → Privacy & Security → Microphone, then restart Golos.") }
        }
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        guard let display = content.displays.first else { fail("No display is available for ScreenCaptureKit audio capture.") }
        let ownPids = [parent, ProcessInfo.processInfo.processIdentifier]
        let filter: SCContentFilter
        if pid != 0 {
            guard let selected = content.applications.first(where: { $0.processID == pid && !ownPids.contains($0.processID) }) else { fail("The selected application has closed. Refresh applications and select it again.") }
            filter = SCContentFilter(display: display, including: [selected], exceptingWindows: [])
        } else {
            filter = SCContentFilter(display: display, excludingApplications: content.applications.filter { ownPids.contains($0.processID) }, exceptingWindows: [])
        }
        let config = SCStreamConfiguration()
        config.width = 2; config.height = 2; config.minimumFrameInterval = CMTime(value: 1, timescale: 1)
        config.capturesAudio = true; config.sampleRate = 48000; config.channelCount = 1
        config.excludesCurrentProcessAudio = true
        if #available(macOS 15.0, *) { config.captureMicrophone = mixed }
        let stream = SCStream(filter: filter, configuration: config, delegate: self)
        try stream.addStreamOutput(self, type: .audio, sampleHandlerQueue: queue)
        if #available(macOS 15.0, *), mixed { try stream.addStreamOutput(self, type: .microphone, sampleHandlerQueue: queue) }
        self.stream = stream
        try await stream.startCapture()
        queue.sync {
            origin = hostSeconds(); started = true
            packet(3, 0)
            let timer = DispatchSource.makeTimerSource(queue: queue)
            timer.schedule(deadline: .now(), repeating: .milliseconds(100))
            timer.setEventHandler { [weak self] in
                guard let self, !self.stopped, self.pausedAt == nil else { return }
                self.packet(2, self.activeTime(hostSeconds()))
            }
            timer.resume(); self.timer = timer
        }
    }
    func command(_ command: String) async {
        if command == "stop" {
            let end = queue.sync { stopped = true; timer?.cancel(); return activeTime(hostSeconds()) }
            try? await stream?.stopCapture()
            queue.sync { packet(4, end) }
            exit(0)
        }
        queue.sync {
            if command == "pause", pausedAt == nil { pausedAt = hostSeconds(); converters.removeAll() }
            if command == "resume", let pause = pausedAt { pausedTotal += hostSeconds() - pause; pausedAt = nil; converters.removeAll() }
        }
    }
}

@main struct Main {
    static func main() async {
        guard #available(macOS 13.0, *) else { fail("Application audio capture requires macOS 13 or later.") }
        let args = CommandLine.arguments
        if args.count > 1, args[1] == "capabilities" {
            let version = ProcessInfo.processInfo.operatingSystemVersion
            print("{\"system\":true,\"mixed\":\(version.majorVersion >= 15)}")
            return
        }
        if args.count > 1, args[1] == "list" {
            do {
                let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
                let apps = content.applications.map { App(pid: $0.processID, name: $0.applicationName) }.sorted { $0.name < $1.name }
                FileHandle.standardOutput.write(try JSONEncoder().encode(apps))
                return
            } catch { fail("Application audio access denied or unavailable: \(error.localizedDescription). Enable Golos Audio Capture in System Settings → Privacy & Security → Screen & System Audio Recording, then restart Golos and refresh applications.") }
        }
        guard args.count == 4, let pid = Int32(args[1]), let parent = Int32(args[2]) else { fail("Invalid capture arguments.") }
        let capture = Capture()
        do { try await capture.start(pid: pid, parent: parent, mixed: args[3] == "mixed") }
        catch { fail("Could not start application audio: \(error.localizedDescription). Enable Golos Audio Capture in System Settings → Privacy & Security → Screen & System Audio Recording, then restart Golos.") }
        while let command = readLine() { await capture.command(command) }
        await capture.command("stop")
    }
}
