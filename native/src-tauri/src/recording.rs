//! Native microphone capture. Only the capture worker owns the audio stream.
//! A bounded queue keeps disk I/O out of the audio callback; overload stops
//! capture rather than silently dropping samples. The source WAV is normalized
//! through the same pipeline as imports before it becomes a library entry.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub elapsed_seconds: f64,
    pub stopped: bool,
    pub error: Option<String>,
}

pub struct Session {
    pub source: PathBuf,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<Status>>,
    worker: Option<JoinHandle<Result<(), String>>>,
    result: Option<Result<(), String>>,
}

impl Session {
    #[cfg(test)]
    pub(crate) fn from_saved_fixture(source: PathBuf) -> Self {
        Self {
            source,
            stop: Arc::new(AtomicBool::new(true)),
            status: Arc::new(Mutex::new(Status {
                stopped: true,
                ..Status::default()
            })),
            worker: None,
            result: Some(Ok(())),
        }
    }

    pub fn start(root: &Path) -> Result<Self, String> {
        let source = root.join(format!("microphone-{}.pending.wav", uuid::Uuid::new_v4()));
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(Status::default()));
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker_path = source.clone();
        let worker_stop = stop.clone();
        let worker_status = status.clone();
        let worker = thread::spawn(move || {
            let result = capture(&worker_path, &worker_stop, &worker_status, ready_tx);
            if let Ok(mut status) = worker_status.lock() {
                status.stopped = true;
                if let Err(error) = &result {
                    status.error = Some(error.clone());
                }
            }
            result
        });
        let mut session = Self {
            source,
            stop,
            status,
            worker: Some(worker),
            result: None,
        };
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(session),
            _ => {
                let error = session
                    .finish()
                    .err()
                    .unwrap_or_else(|| "Could not start the microphone.".into());
                Err(format!("{error} Check microphone access for Golos in your system privacy settings and try again."))
            }
        }
    }

    pub fn status(&self) -> Result<Status, String> {
        self.status
            .lock()
            .map(|status| status.clone())
            .map_err(|_| "Recording status is unavailable.".into())
    }

    /// Idempotent: a failed library save can retry without losing the source.
    pub fn finish(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            self.result = Some(
                worker
                    .join()
                    .unwrap_or_else(|_| Err("The microphone worker stopped unexpectedly.".into())),
            );
        }
        self.result.clone().unwrap_or(Ok(()))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.finish();
        let _ = fs::remove_file(&self.source);
    }
}

fn queue<T>(data: &[T], channels: usize, sender: &SyncSender<Vec<i16>>, overflow: &AtomicBool)
where
    T: cpal::Sample,
    f32: cpal::FromSample<T>,
{
    let samples = data
        .chunks_exact(channels)
        .map(|frame| {
            let mono = frame
                .iter()
                .map(|sample| sample.to_sample::<f32>())
                .sum::<f32>()
                / channels as f32;
            (mono.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
        })
        .collect();
    if let Err(TrySendError::Full(_)) = sender.try_send(samples) {
        overflow.store(true, Ordering::SeqCst);
    }
}

fn stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sender: SyncSender<Vec<i16>>,
    overflow: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = config.channels as usize;
    device
        .build_input_stream(
            config,
            move |data: &[T], _| queue(data, channels, &sender, &overflow),
            move |_| {
                failed.store(true, Ordering::SeqCst);
            },
            None,
        )
        .map_err(|e| format!("Could not open the microphone: {e}"))
}

fn capture(
    path: &Path,
    stop: &AtomicBool,
    status: &Mutex<Status>,
    ready: SyncSender<Result<(), String>>,
) -> Result<(), String> {
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("No microphone was found.")?;
    let supported = device
        .default_input_config()
        .map_err(|e| format!("Microphone is unavailable: {e}"))?;
    let config: cpal::StreamConfig = supported.clone().into();
    if config.channels == 0 || !(8_000..=96_000).contains(&config.sample_rate.0) {
        return Err(
            "Use a microphone with an 8–96 kHz sample rate and at least one input channel.".into(),
        );
    }
    let (sender, receiver) = mpsc::sync_channel(32);
    let overflow = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    macro_rules! build {
        ($sample:ty) => {
            stream::<$sample>(&device, &config, sender, overflow.clone(), failed.clone())?
        };
    }
    let stream = match supported.sample_format() {
        cpal::SampleFormat::I8 => build!(i8),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::I32 => build!(i32),
        cpal::SampleFormat::I64 => build!(i64),
        cpal::SampleFormat::U8 => build!(u8),
        cpal::SampleFormat::U16 => build!(u16),
        cpal::SampleFormat::U32 => build!(u32),
        cpal::SampleFormat::U64 => build!(u64),
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::F64 => build!(f64),
        _ => return Err("This microphone's sample format is not supported.".into()),
    };
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: config.sample_rate.0,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .map_err(|e| format!("Could not create the recording: {e}"))?;
    stream
        .play()
        .map_err(|e| format!("Could not start the microphone: {e}"))?;
    let _ = ready.send(Ok(()));
    let max_samples = config.sample_rate.0 as u64 * crate::audio::MAX_SECONDS as u64;
    let mut written = 0;
    let result = (|| {
        while !stop.load(Ordering::SeqCst) && written < max_samples {
            if overflow.load(Ordering::SeqCst) || failed.load(Ordering::SeqCst) {
                break;
            }
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(samples) => write_samples(&mut writer, &samples, &mut written, max_samples)?,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if let Ok(mut status) = status.lock() {
                status.elapsed_seconds = written as f64 / config.sample_rate.0 as f64;
            }
        }
        Ok::<(), String>(())
    })();
    // Release the microphone before saving or normalizing.
    drop(stream);
    // Preserve callbacks queued just before Stop was pressed.
    let result = result.and_then(|()| drain(&receiver, &mut writer, &mut written, max_samples));
    let finalized = writer
        .finalize()
        .map_err(|e| format!("Could not finish the recording: {e}"));
    result?;
    finalized?;
    let warning = if overflow.load(Ordering::SeqCst) {
        Some("Recording stopped because audio could not be saved fast enough. The captured audio was kept.".into())
    } else if failed.load(Ordering::SeqCst) {
        Some(
            "The microphone disconnected or became unavailable. The captured audio was kept."
                .into(),
        )
    } else {
        None
    };
    if let Ok(mut status) = status.lock() {
        status.elapsed_seconds = written as f64 / config.sample_rate.0 as f64;
        status.error = warning;
    }
    if written == 0 {
        return Err("No audio was captured. Check microphone access and try again.".into());
    }
    Ok(())
}

fn write_samples(
    writer: &mut hound::WavWriter<std::io::BufWriter<fs::File>>,
    samples: &[i16],
    written: &mut u64,
    limit: u64,
) -> Result<(), String> {
    for sample in samples.iter().take((limit - *written) as usize) {
        writer
            .write_sample(*sample)
            .map_err(|e| format!("Could not save microphone audio: {e}"))?;
        *written += 1;
    }
    Ok(())
}

fn drain(
    receiver: &Receiver<Vec<i16>>,
    writer: &mut hound::WavWriter<std::io::BufWriter<fs::File>>,
    written: &mut u64,
    limit: u64,
) -> Result<(), String> {
    while let Ok(samples) = receiver.try_recv() {
        write_samples(writer, &samples, written, limit)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discarding_capture_removes_source_and_finish_is_repeatable() {
        let path = std::env::temp_dir().join(format!("golos-discard-{}", uuid::Uuid::new_v4()));
        fs::write(&path, b"capture").unwrap();
        let mut session = Session::from_saved_fixture(path.clone());
        assert!(session.finish().is_ok());
        assert!(session.finish().is_ok());
        assert!(path.exists());
        drop(session);
        assert!(!path.exists());
    }

    #[test]
    fn downmixes_clips_and_detects_queue_overload() {
        let (tx, rx) = mpsc::sync_channel(1);
        let overflow = AtomicBool::new(false);
        queue(&[1.0f32, -1.0, 2.0, 2.0], 2, &tx, &overflow);
        assert_eq!(rx.recv().unwrap(), vec![0, i16::MAX]);
        queue(&[0i16], 1, &tx, &overflow);
        queue(&[1i16], 1, &tx, &overflow);
        assert!(overflow.load(Ordering::SeqCst));
    }

    #[test]
    fn caps_capture_and_produces_importable_audio() {
        let root = std::env::temp_dir().join(format!("golos-capture-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source.wav");
        let target = root.join("target.wav");
        let mut writer = hound::WavWriter::create(
            &source,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        let mut written = 0;
        write_samples(&mut writer, &vec![1000; 60_000], &mut written, 48_000).unwrap();
        write_samples(&mut writer, &[1000], &mut written, 48_000).unwrap();
        writer.finalize().unwrap();
        assert_eq!(written, 48_000);
        let duration = crate::import::normalize(&source, &target).unwrap();
        assert!((duration - 1.0).abs() < 0.001);
        assert_eq!(crate::audio::read_audio(&target).unwrap().len(), 16_000);
        fs::remove_dir_all(root).unwrap();
    }
}
