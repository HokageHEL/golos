use crate::{audio, library::Segment, model};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

type ProgressCallback = std::sync::Mutex<Box<dyn FnMut(i32) + Send>>;

unsafe extern "C" fn report_progress(
    _: *mut whisper_rs::WhisperSysContext,
    _: *mut whisper_rs::WhisperSysState,
    percent: i32,
    data: *mut std::ffi::c_void,
) {
    // `transcribe` keeps this mutex alive until synchronous `full` returns.
    let callback = unsafe { &*(data as *const ProgressCallback) };
    if let Ok(mut callback) = callback.lock() {
        // Never unwind through a C callback, including if a caller's UI hook panics.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(percent)));
    }
}

unsafe extern "C" fn should_abort(data: *mut std::ffi::c_void) -> bool {
    // The owning Arc is held by `transcribe` for the entire inference call.
    unsafe { &*(data as *const AtomicBool) }.load(Ordering::SeqCst)
}

pub fn transcribe(
    model: &Path,
    audio: &Path,
    language: &str,
    cancelled: Arc<AtomicBool>,
    progress: impl FnMut(i32) + Send + 'static,
) -> Result<Vec<Segment>, String> {
    model::inspect(model)?;
    let samples = audio::read_audio(audio)?;
    if cancelled.load(Ordering::SeqCst) {
        return Err("Transcription cancelled. You can try again.".into());
    }
    let mut options = WhisperContextParameters::default();
    options.use_gpu(false);
    let context =
        WhisperContext::new_with_params(model.to_str().ok_or("Unsupported model path")?, options)
            .map_err(|e| format!("Cannot load model. Import a compatible Whisper model: {e}"))?;
    let mut engine = context.create_state().map_err(|e| e.to_string())?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(
        std::thread::available_parallelism()
            .map(|n| n.get().min(8) as i32)
            .unwrap_or(2),
    );
    params.set_translate(false);
    params.set_language(if language == "auto" {
        None
    } else {
        Some(language)
    });
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    let callback: ProgressCallback = std::sync::Mutex::new(Box::new(progress));
    // whisper-rs 0.16's safe abort helper casts a boxed trait object to the
    // concrete closure type, and its safe progress helper leaks its allocation.
    // Use explicit borrowed callbacks instead. Both data pointers stay valid
    // through `full`; callbacks never access the Whisper context/state.
    unsafe {
        params.set_progress_callback(Some(report_progress));
        params.set_progress_callback_user_data(
            &callback as *const ProgressCallback as *mut std::ffi::c_void,
        );
        params.set_abort_callback(Some(should_abort));
        params.set_abort_callback_user_data(Arc::as_ptr(&cancelled) as *mut std::ffi::c_void);
    }
    engine.full(params, &samples).map_err(|e| {
        if cancelled.load(Ordering::SeqCst) {
            "Transcription cancelled. You can try again.".into()
        } else {
            format!("Transcription failed: {e}")
        }
    })?;
    if cancelled.load(Ordering::SeqCst) {
        return Err("Transcription cancelled. You can try again.".into());
    }
    engine
        .as_iter()
        .map(|segment| {
            Ok(Segment {
                start_ms: segment.start_timestamp() * 10,
                end_ms: segment.end_timestamp() * 10,
                speaker: None,
                text: segment.to_str().map_err(|e| e.to_string())?.trim().into(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Explicit opt-in: no network calls, model downloads, or private recordings in tests.
    #[test]
    #[ignore = "Set GOLOS_TEST_MODEL and GOLOS_TEST_AUDIO to a supported model and the public whisper.cpp JFK sample"]
    fn transcribes_real_audio_offline() {
        whisper_rs::install_logging_hooks();
        let model = std::env::var("GOLOS_TEST_MODEL").expect("GOLOS_TEST_MODEL required");
        let audio = std::env::var("GOLOS_TEST_AUDIO").expect("GOLOS_TEST_AUDIO required");
        let result = transcribe(
            Path::new(&model),
            Path::new(&audio),
            "en",
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .unwrap();
        let text = result
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        assert!(text.contains("country"), "Unexpected transcript: {text}");
        assert!(text.contains("ask"), "Unexpected transcript: {text}");
        assert!(result
            .iter()
            .all(|s| s.start_ms >= 0 && s.end_ms >= s.start_ms));
        let abort_flag = Arc::new(AtomicBool::new(false));
        let callback_flag = abort_flag.clone();
        let aborted = transcribe(
            Path::new(&model),
            Path::new(&audio),
            "en",
            abort_flag.clone(),
            move |_| {
                callback_flag.store(true, Ordering::SeqCst);
            },
        );
        assert!(
            abort_flag.load(Ordering::SeqCst),
            "Progress callback should run"
        );
        assert!(aborted.unwrap_err().contains("cancelled"));
        let cancelled = transcribe(
            Path::new(&model),
            Path::new(&audio),
            "en",
            Arc::new(AtomicBool::new(true)),
            |_| {},
        );
        assert!(cancelled.unwrap_err().contains("cancelled"));
    }
}
