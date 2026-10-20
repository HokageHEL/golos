//! Timestamped, bounded mixing of ScreenCaptureKit's host-clock audio streams.
//! Each source is converted by AVAudioConverter to 16 kHz mono before this point.
use std::collections::VecDeque;

pub struct Timeline {
    cursor: u64,
    frames: VecDeque<([f32; 2], u8)>,
    mixed: bool,
    last_end: [i64; 2],
}
impl Timeline {
    pub fn new(mixed: bool) -> Self {
        Self {
            cursor: 0,
            frames: VecDeque::new(),
            mixed,
            last_end: [0; 2],
        }
    }
    pub fn push(&mut self, source: usize, time: f64, samples: &[f32]) -> Result<(), String> {
        if source > 1
            || (!self.mixed && source != 0)
            || !time.is_finite()
            || !(-1.0..=1801.0).contains(&time)
            || samples.len() > 16000
        {
            return Err("Invalid audio source or capture clock.".into());
        }
        let start = (time * 16000.).round() as i64;
        let end = start + samples.len() as i64;
        if start >= 0 && start < self.cursor as i64 && end > self.last_end[source] {
            return Err(
                "An audio source arrived too late to remain synchronized. Captured audio was kept."
                    .into(),
            );
        }
        self.last_end[source] = self.last_end[source].max(end);
        for (n, sample) in samples.iter().enumerate() {
            if !sample.is_finite() {
                return Err("Audio source returned non-finite samples.".into());
            }
            let position = start + n as i64;
            if position < self.cursor as i64 || position >= 1800 * 16000 {
                continue;
            }
            let offset = position as u64 - self.cursor;
            if offset >= 32000 {
                return Err("Audio sources lost synchronization. Captured audio was kept.".into());
            }
            while self.frames.len() <= offset as usize {
                self.frames.push_back(([0.; 2], 0));
            }
            let frame = &mut self.frames[offset as usize];
            // Overlapping/duplicated callbacks for one source contribute only once.
            if frame.1 & (1 << source) == 0 {
                frame.0[source] = *sample;
                frame.1 |= 1 << source;
            }
        }
        Ok(())
    }
    pub fn render(
        &mut self,
        end: u64,
        mut write: impl FnMut(i16) -> Result<(), String>,
    ) -> Result<(), String> {
        let end = end.min(1800 * 16000);
        while self.cursor < end {
            let (samples, _) = self.frames.pop_front().unwrap_or(([0.; 2], 0));
            let value = if self.mixed {
                (samples[0] + samples[1]) * 0.5
            } else {
                samples[0]
            };
            write((value.clamp(-1., 1.) * i16::MAX as f32).round() as i16)?;
            self.cursor += 1;
        }
        Ok(())
    }
    pub fn written(&self) -> u64 {
        self.cursor
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    use std::{
        fs,
        io::Read,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        process::{Command, Stdio},
    };
    pub fn helper(root: &Path) -> Result<PathBuf, String> {
        let binary = include_bytes!(concat!(env!("OUT_DIR"), "/golos-capture"));
        let target = root.join("golos-audio-capture");
        if fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Audio helper must not be a symbolic link.".into());
        }
        if fs::read(&target).ok().as_deref() != Some(binary.as_slice()) {
            let temp = root.join(format!(".capture-{}", uuid::Uuid::new_v4()));
            fs::write(&temp, binary).map_err(|e| e.to_string())?;
            fs::set_permissions(&temp, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
            fs::rename(&temp, &target).map_err(|e| e.to_string())?;
        }
        Ok(target)
    }
    pub fn info(root: &Path, argument: &str) -> Result<serde_json::Value, String> {
        let output = Command::new(helper(root)?)
            .arg(argument)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into());
        }
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
    }
    pub struct Packet {
        pub kind: u32,
        pub time: f64,
        pub samples: Vec<f32>,
    }
    pub fn packet(reader: &mut impl Read) -> Result<Packet, String> {
        let mut header = [0; 16];
        reader.read_exact(&mut header).map_err(|e| e.to_string())?;
        let kind = u32::from_le_bytes(header[..4].try_into().unwrap());
        let time = f64::from_le_bytes(header[4..12].try_into().unwrap());
        let count = u32::from_le_bytes(header[12..16].try_into().unwrap());
        if kind > 4 || count > 16000 || !time.is_finite() || (kind > 1 && count != 0) {
            return Err("Invalid native audio packet.".into());
        }
        let mut bytes = vec![0; count as usize * 4];
        reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
        let samples = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        Ok(Packet {
            kind,
            time,
            samples,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixes_host_timestamps_without_duplicate_audio_and_clamps() {
        let mut timeline = Timeline::new(true);
        timeline.push(0, 0., &[0.8; 160]).unwrap();
        timeline.push(0, 0., &[0.8; 160]).unwrap();
        timeline.push(1, 0., &[0.4; 80]).unwrap();
        timeline.push(1, 0.005, &[0.4; 80]).unwrap();
        let mut output = vec![];
        timeline
            .render(160, |s| {
                output.push(s);
                Ok(())
            })
            .unwrap();
        assert!(output
            .iter()
            .all(|s| (*s as f32 / i16::MAX as f32 - 0.6).abs() < 0.001));
        // A resumed section is placed on the compressed active timeline.
        timeline.push(0, 0.01, &[1.; 160]).unwrap();
        timeline
            .render(320, |s| {
                output.push(s);
                Ok(())
            })
            .unwrap();
        assert_eq!(output.len(), 320);
        assert_eq!(timeline.written(), 320);
    }
    #[test]
    fn rejects_clock_jumps_invalid_samples_and_unsupported_sources() {
        let mut timeline = Timeline::new(false);
        assert!(timeline.push(0, 3., &[0.]).is_err());
        assert!(timeline.push(1, 0., &[0.]).is_err());
        assert!(timeline.push(0, f64::NAN, &[]).is_err());
        assert!(timeline.push(0, 0., &[f32::INFINITY]).is_err());
    }
}
