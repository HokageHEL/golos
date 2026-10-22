//! Rust-owned global dictation runtime.
//!
//! One recording session, one serialized pipeline and one explicit review step
//! live here. The webview never starts a microphone, never sees a native target
//! token and never triggers an insertion on its own: it asks for a start, a
//! stop/save, a retry, a review or an explicit insert, and the runtime decides
//! what is allowed. The native platform adapter is injected, so a build
//! without one records and transcribes but never claims insertion support.
use crate::dictation::{
    self, CapturedTarget, ClipboardOutcome, InsertOutcome, KeyEdge, PermissionKind, Permissions,
    Phase, Platform, ShortcutAction, ShortcutLatch, ShortcutMode,
};
use crate::dictation_settings::{self, JobRow, Settings};
use crate::{ActiveRecording, AppState, BusyGuard, RecordingOwner};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager};

/// The editable preview is bounded so a long transcript cannot become an
/// unbounded clipboard or paste payload.
pub const PREVIEW_LIMIT: usize = 200_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetInfo {
    pub bundle_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub enabled: bool,
    pub shortcut: String,
    pub shortcut_display: String,
    pub mode: ShortcutMode,
    pub language: String,
    pub phase: Phase,
    pub active: bool,
    pub overlay_visible: bool,
    pub show_overlay: bool,
    pub overlay_always_on_top: bool,
    pub restore_clipboard: bool,
    pub ai_enabled: bool,
    pub ai_kind: String,
    pub ai_prompt: String,
    pub app_overrides: Vec<dictation_settings::AppOverride>,
    pub shortcut_error: Option<String>,
    pub startup_failed: bool,
    pub insertion_supported: bool,
    pub permissions: Option<Permissions>,
    pub target: Option<TargetInfo>,
    pub note_id: Option<String>,
    pub original_text: String,
    pub processed_text: Option<String>,
    pub preview_text: String,
    pub processed_model: Option<String>,
    pub processed_prompt: Option<String>,
    pub error: Option<String>,
    pub warning: Option<String>,
    pub paste_message: Option<String>,
    pub clipboard: Option<ClipboardOutcome>,
    pub recovered: bool,
    pub can_start: bool,
    pub can_pause: bool,
    pub can_discard: bool,
    pub can_retry_save: bool,
    pub can_retry_transcribe: bool,
    pub can_retry_process: bool,
    pub can_insert: bool,
    pub can_dismiss: bool,
}

/// The recording/AI steps the runtime drives. Production uses the shared
/// recording engine and the existing local AI pipeline; tests substitute a
/// deterministic fake so no microphone, model or clipboard is touched.
pub trait Pipeline: Send + Sync {
    fn start_capture(&self, state: &AppState) -> Result<crate::recording::Session, String>;
    fn save(
        &self,
        state: &AppState,
        recording: &mut ActiveRecording,
    ) -> Result<SavedCapture, String>;
    fn transcribe(
        &self,
        app: Option<&tauri::AppHandle>,
        state: &AppState,
        note_id: &str,
        language: &str,
    ) -> Result<(), String>;
    fn original_text(&self, state: &AppState, note_id: &str) -> Result<String, String>;
    fn process(
        &self,
        app: Option<&tauri::AppHandle>,
        state: &AppState,
        note_id: &str,
        kind: &str,
        prompt: &str,
        language: &str,
    ) -> Result<Processed, String>;
}

#[derive(Debug, Clone)]
pub struct SavedCapture {
    pub note_id: String,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Processed {
    pub text: String,
    pub model: String,
    pub prompt: String,
}

struct Job {
    id: String,
    phase: Phase,
    note_id: Option<String>,
    target: Option<CapturedTarget>,
    original_text: String,
    processed_text: Option<String>,
    processed_model: Option<String>,
    processed_prompt: Option<String>,
    preview_text: String,
    error: Option<String>,
    warning: Option<String>,
    paste_message: Option<String>,
    clipboard: Option<ClipboardOutcome>,
    insert_dispatched: bool,
    recovered: bool,
    guard: Option<BusyGuard>,
    created_at: i64,
    updated_at: i64,
}

struct RuntimeState {
    settings: Settings,
    job: Option<Job>,
    overlay_visible: bool,
    permissions: Option<Permissions>,
}

pub struct DictationRuntime {
    app: Option<tauri::AppHandle>,
    platform: Option<Arc<dyn Platform>>,
    pipeline: Arc<dyn Pipeline>,
    state: Mutex<RuntimeState>,
    latch: Mutex<ShortcutLatch>,
    registration: Mutex<crate::dictation_shortcut::Registration>,
}

impl DictationRuntime {
    pub fn new(app: tauri::AppHandle, platform: Option<Arc<dyn Platform>>) -> Self {
        Self::with_parts(Some(app), platform, Arc::new(RealPipeline))
    }

    pub fn with_parts(
        app: Option<tauri::AppHandle>,
        platform: Option<Arc<dyn Platform>>,
        pipeline: Arc<dyn Pipeline>,
    ) -> Self {
        Self {
            app,
            platform,
            pipeline,
            state: Mutex::new(RuntimeState {
                settings: Settings::default(),
                job: None,
                overlay_visible: false,
                permissions: None,
            }),
            latch: Mutex::new(ShortcutLatch::default()),
            registration: Mutex::new(crate::dictation_shortcut::Registration::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RuntimeState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    pub fn set_settings(&self, settings: Settings) {
        self.lock().settings = settings;
    }

    /// Read the persisted settings and register the shortcut. A registration
    /// failure stays visible instead of pretending the feature is armed.
    pub fn load(&self, state: &AppState) -> Result<(), String> {
        let db = state.db()?;
        let settings = dictation_settings::load(&db)?;
        self.set_settings(settings.clone());
        self.register_shortcut(&settings)
    }

    /// Register a new validated shortcut before the old one is removed.
    pub fn configure(&self, state: &AppState, settings: Settings) -> Result<Settings, String> {
        settings.validate()?;
        if settings.enabled {
            self.register_shortcut(&settings)?;
        } else {
            self.unregister_shortcut();
        }
        let db = state.db()?;
        dictation_settings::save(&db, &settings)?;
        self.set_settings(settings.clone());
        self.emit_state();
        Ok(settings)
    }

    fn register_shortcut(&self, settings: &Settings) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            let validated = dictation::shortcut::validate(&settings.shortcut)?;
            let Some(app) = self.app.clone() else {
                return Err("Global shortcuts need the running application.".into());
            };
            let registrar = crate::dictation_shortcut::PluginRegistrar::new(app);
            let mut registration = self.registration.lock().unwrap();
            if registration.current().is_some() {
                registration.apply(validated, &registrar)
            } else {
                registration.start(validated, &registrar)
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = settings;
            Err("Global dictation shortcuts are available only on macOS.".into())
        }
    }

    fn unregister_shortcut(&self) {
        #[cfg(target_os = "macos")]
        {
            if let Some(app) = self.app.clone() {
                let registrar = crate::dictation_shortcut::PluginRegistrar::new(app);
                self.registration.lock().unwrap().clear(&registrar);
            }
        }
    }

    /// Handle one key edge. The latch swallows OS key repeat, and the work is
    /// enqueued on a worker instead of running on the shortcut callback thread.
    pub fn shortcut_edge(self: &Arc<Self>, edge: KeyEdge) {
        let (mode, phase, busy) = {
            let runtime = self.lock();
            let phase = runtime
                .job
                .as_ref()
                .map(|job| job.phase)
                .unwrap_or(Phase::Idle);
            (
                runtime.settings.mode,
                phase,
                runtime.job.is_some() && !matches!(phase, Phase::Idle),
            )
        };
        let action = self.latch.lock().unwrap().edge(edge, mode, phase, busy);
        match action {
            ShortcutAction::None => {}
            ShortcutAction::Busy => {
                self.set_error("Dictation is busy. Finish the current step first.");
            }
            ShortcutAction::Start => self.spawn(|runtime, state| runtime.start(state).map(|_| ())),
            ShortcutAction::StopAndSave => self.spawn(|runtime, state| runtime.stop_and_run(state)),
            ShortcutAction::ShowReview => self.spawn(|runtime, state| {
                runtime.show_review(state);
                Ok(())
            }),
        }
    }

    /// Run work on a worker thread. The shortcut callback never performs AX,
    /// database or recording work itself.
    fn spawn<F>(self: &Arc<Self>, work: F)
    where
        F: FnOnce(&DictationRuntime, &AppState) -> Result<(), String> + Send + 'static,
    {
        let Some(app) = self.app.clone() else {
            return;
        };
        let runtime = self.clone();
        std::thread::spawn(move || {
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            if let Err(error) = work(&runtime, &state) {
                runtime.set_error(&error);
            }
        });
    }

    pub fn set_error(&self, message: &str) {
        {
            let mut runtime = self.lock();
            if let Some(job) = runtime.job.as_mut() {
                job.error = Some(message.to_string());
            }
        }
        self.emit_state();
    }

    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs() as i64)
            .unwrap_or_default()
    }

    fn emit_state(&self) {
        let status = self.status();
        if let Some(app) = &self.app {
            let _ = app.emit("dictation-state", status.clone());
            // A shared recording-state event so both windows agree on which
            // workflow owns the single recording session.
            let _ = app.emit(
                "recording-state",
                serde_json::json!({
                    "owner": "dictation",
                    "phase": status.phase,
                    "jobId": status.note_id,
                }),
            );
        }
    }

    pub fn status(&self) -> Status {
        let runtime = self.lock();
        let settings = runtime.settings.clone();
        let job = runtime.job.as_ref();
        let phase = job.map(|job| job.phase).unwrap_or(Phase::Idle);
        let target = job.and_then(|job| {
            job.target.as_ref().map(|target| TargetInfo {
                bundle_id: target.app.bundle_id.clone(),
                name: target.app.name.clone(),
            })
        });
        let insertion_supported = self.platform.is_some();
        let note_id = job.and_then(|job| job.note_id.clone());
        let original_text = job.map(|job| job.original_text.clone()).unwrap_or_default();
        let processed_text = job.and_then(|job| job.processed_text.clone());
        let has_processed = processed_text.is_some();
        let preview_text = job.map(|job| job.preview_text.clone()).unwrap_or_default();
        let has_text = !preview_text.trim().is_empty();
        let saved = note_id.is_some();
        let insert_dispatched = job.map(|job| job.insert_dispatched).unwrap_or(false);
        let target_ready = job
            .and_then(|job| job.target.as_ref())
            .map(|target| target.insertion_available)
            .unwrap_or(false);
        let registration = self.registration.lock().unwrap();
        let active = matches!(
            phase,
            Phase::Starting
                | Phase::Recording
                | Phase::Paused
                | Phase::Stopped
                | Phase::Saving
                | Phase::SaveFailed
                | Phase::Transcribing
                | Phase::Processing
                | Phase::Inserting
        );
        Status {
            enabled: settings.enabled,
            shortcut: settings.shortcut.clone(),
            shortcut_display: registration
                .current()
                .map(|current| current.display.clone())
                .unwrap_or_default(),
            mode: settings.mode,
            language: settings.language.clone(),
            phase,
            active,
            overlay_visible: runtime.overlay_visible,
            show_overlay: settings.show_overlay,
            overlay_always_on_top: settings.overlay_always_on_top,
            restore_clipboard: settings.restore_clipboard,
            ai_enabled: settings.ai_enabled,
            ai_kind: settings.ai_kind.clone(),
            ai_prompt: settings.ai_prompt.clone(),
            app_overrides: settings.app_overrides.clone(),
            shortcut_error: registration.error().map(str::to_string),
            startup_failed: registration.startup_failed(),
            insertion_supported,
            permissions: runtime.permissions,
            target,
            note_id,
            original_text,
            processed_text,
            preview_text,
            processed_model: job.and_then(|job| job.processed_model.clone()),
            processed_prompt: job.and_then(|job| job.processed_prompt.clone()),
            error: job.and_then(|job| job.error.clone()),
            warning: job.and_then(|job| job.warning.clone()),
            paste_message: job.and_then(|job| job.paste_message.clone()),
            clipboard: job.and_then(|job| job.clipboard),
            recovered: job.map(|job| job.recovered).unwrap_or(false),
            can_start: phase == Phase::Idle,
            can_pause: matches!(phase, Phase::Recording | Phase::Paused),
            can_discard: matches!(
                phase,
                Phase::Recording | Phase::Paused | Phase::Stopped | Phase::SaveFailed
            ),
            can_retry_save: phase == Phase::SaveFailed,
            can_retry_transcribe: phase == Phase::Failed && saved,
            can_retry_process: phase == Phase::Preview && !has_processed && saved,
            can_insert: phase == Phase::Preview
                && saved
                && has_text
                && insertion_supported
                && target_ready
                && !insert_dispatched,
            can_dismiss: !matches!(
                phase,
                Phase::Idle
                    | Phase::Starting
                    | Phase::Recording
                    | Phase::Paused
                    | Phase::Stopped
                    | Phase::Saving
                    | Phase::SaveFailed
            ),
        }
    }

    /// Start a dictation capture. The native target is captured before the
    /// overlay is presented, so the overlay never steals focus from the app
    /// the user is dictating into.
    /// The feature is macOS only. Other targets report a clear error instead
    /// of pretending a native capability exists.
    pub fn supported() -> bool {
        cfg!(target_os = "macos")
    }

    pub fn start(&self, state: &AppState) -> Result<(), String> {
        if !Self::supported() {
            return Err("Global dictation is available only on macOS.".into());
        }
        {
            let runtime = self.lock();
            if let Some(job) = runtime.job.as_ref() {
                if job.phase != Phase::Idle {
                    return Err("A dictation recording is already in progress.".into());
                }
            }
        }
        let guard = state.claim()?;
        let target = self.capture_target();
        let session = match self.pipeline.start_capture(state) {
            Ok(session) => session,
            Err(error) => {
                drop(guard);
                return Err(error);
            }
        };
        let job_id = uuid::Uuid::new_v4().to_string();
        let recording_id = uuid::Uuid::new_v4().to_string();
        let title = format!("Dictation · {}", Self::now());
        {
            let mut slot = state
                .recording
                .lock()
                .map_err(|_| "Recording is unavailable. Restart Golos.".to_string())?;
            *slot = Some(ActiveRecording {
                session,
                _guard: None,
                id: recording_id,
                created_at: Self::now(),
                title,
                owner: RecordingOwner::Dictation,
                job_id: Some(job_id.clone()),
            });
        }
        let now = Self::now();
        {
            let mut runtime = self.lock();
            runtime.job = Some(Job {
                id: job_id,
                phase: Phase::Recording,
                note_id: None,
                target,
                original_text: String::new(),
                processed_text: None,
                processed_model: None,
                processed_prompt: None,
                preview_text: String::new(),
                error: None,
                warning: None,
                paste_message: None,
                clipboard: None,
                insert_dispatched: false,
                recovered: false,
                guard: Some(guard),
                created_at: now,
                updated_at: now,
            });
            runtime.overlay_visible = runtime.settings.show_overlay;
        }
        self.persist(state);
        self.present_overlay();
        self.emit_state();
        Ok(())
    }

    /// The native target is optional: recording and transcription work without
    /// an insertion grant, and the absence of an adapter never pretends native
    /// capability.
    fn capture_target(&self) -> Option<CapturedTarget> {
        let platform = self.platform.as_ref()?;
        match platform.capture_target() {
            Ok(target) => Some(target),
            Err(error) => {
                self.set_error(&format!(
                    "Text insertion is unavailable: {error}. Recording and transcription still work."
                ));
                None
            }
        }
    }

    pub fn pause(&self, state: &AppState, paused: bool) -> Result<(), String> {
        let slot = state
            .recording
            .lock()
            .map_err(|_| "Recording is unavailable.".to_string())?;
        let recording = slot
            .as_ref()
            .filter(|recording| recording.owner == RecordingOwner::Dictation)
            .ok_or("No dictation recording is active.")?;
        recording.session.set_paused(paused)?;
        drop(slot);
        {
            let mut runtime = self.lock();
            if let Some(job) = runtime.job.as_mut() {
                job.phase = if paused {
                    Phase::Paused
                } else {
                    Phase::Recording
                };
            }
        }
        self.persist(state);
        self.emit_state();
        Ok(())
    }

    /// Stop the capture, save it and continue with transcription. The worker
    /// owns the whole sequence so hiding the overlay cannot interrupt it.
    fn mark_saving(&self) -> Result<(), String> {
        {
            let mut runtime = self.lock();
            let job = runtime
                .job
                .as_mut()
                .ok_or("No dictation recording is active.")?;
            if !matches!(
                job.phase,
                Phase::Recording | Phase::Paused | Phase::Stopped | Phase::SaveFailed
            ) {
                return Err("The dictation recording is already stopping.".into());
            }
            job.phase = Phase::Saving;
            job.error = None;
        }
        Ok(())
    }

    /// Stop and save, then transcribe and optionally process. This blocks on
    /// the worker thread, never on the webview thread.
    pub fn stop_and_run(&self, state: &AppState) -> Result<(), String> {
        self.mark_saving()?;
        self.persist(state);
        self.emit_state();
        self.run_stop(state);
        Ok(())
    }

    /// Run the save → transcribe → optional processing sequence. It is a
    /// separate method so a retry after a failed save reuses the same source
    /// and the same guard.
    pub fn run_stop(&self, state: &AppState) {
        match self.save_capture(state) {
            Ok(saved) => self.after_save(state, saved),
            Err(error) => {
                {
                    let mut runtime = self.lock();
                    if let Some(job) = runtime.job.as_mut() {
                        job.phase = Phase::SaveFailed;
                        job.error = Some(error);
                    }
                }
                self.persist(state);
                self.emit_state();
            }
        }
    }

    fn save_capture(&self, state: &AppState) -> Result<SavedCapture, String> {
        let job_id = self
            .lock()
            .job
            .as_ref()
            .map(|job| job.id.clone())
            .ok_or("No dictation recording is active.")?;
        let mut slot = state
            .recording
            .lock()
            .map_err(|_| "Recording is unavailable. Restart Golos.".to_string())?;
        let recording = slot
            .as_mut()
            .filter(|recording| recording.owner == RecordingOwner::Dictation)
            .ok_or("No dictation recording is active.")?;
        // Only the recording that belongs to this job may be saved.
        if recording.job_id.as_deref() != Some(job_id.as_str()) {
            return Err("The dictation recording no longer belongs to this job.".into());
        }
        let saved = self.pipeline.save(state, recording)?;
        // The source is released only after the library owns the normalized WAV.
        slot.take();
        Ok(saved)
    }

    fn after_save(&self, state: &AppState, saved: SavedCapture) {
        {
            let mut runtime = self.lock();
            if let Some(job) = runtime.job.as_mut() {
                job.note_id = Some(saved.note_id.clone());
                job.warning = saved.warning.clone();
                job.phase = Phase::Transcribing;
                job.error = None;
            }
        }
        self.persist(state);
        // The library list refreshes, but the new recording is never selected
        // and the editor draft is never replaced.
        if let Some(app) = &self.app {
            let _ = app.emit("queue-changed", ());
        }
        self.emit_state();
        let language = self.settings().language;
        let note_id = saved.note_id.clone();
        if let Err(error) = self
            .pipeline
            .transcribe(self.app.as_ref(), state, &note_id, &language)
        {
            {
                let mut runtime = self.lock();
                if let Some(job) = runtime.job.as_mut() {
                    job.phase = Phase::Failed;
                    job.error = Some(format!("Transcription failed: {error}"));
                }
            }
            self.persist(state);
            self.emit_state();
            return;
        }
        let original_text = self
            .pipeline
            .original_text(state, &note_id)
            .unwrap_or_default();
        let policy_bundle = self
            .lock()
            .job
            .as_ref()
            .and_then(|job| job.target.as_ref())
            .map(|target| target.app.bundle_id.clone());
        let policy =
            dictation_settings::resolve_processing(&self.settings(), policy_bundle.as_deref());
        {
            let mut runtime = self.lock();
            if let Some(job) = runtime.job.as_mut() {
                job.original_text = original_text.clone();
                job.preview_text = original_text.clone();
                job.phase = if policy.ai_enabled {
                    Phase::Processing
                } else {
                    Phase::Preview
                };
            }
        }
        self.persist(state);
        self.emit_state();
        if policy.ai_enabled {
            self.run_processing(state, &note_id, &policy);
        } else {
            self.finish_preview(state);
        }
    }

    fn run_processing(
        &self,
        state: &AppState,
        note_id: &str,
        policy: &dictation_settings::ProcessingPolicy,
    ) {
        let language = self.settings().language;
        match self.pipeline.process(
            self.app.as_ref(),
            state,
            note_id,
            &policy.ai_kind,
            &policy.ai_prompt,
            &language,
        ) {
            Ok(processed) => {
                let mut runtime = self.lock();
                if let Some(job) = runtime.job.as_mut() {
                    job.processed_text = Some(processed.text.clone());
                    job.processed_model = Some(processed.model.clone());
                    job.processed_prompt = Some(processed.prompt.clone());
                    job.preview_text = processed.text.clone();
                    job.error = None;
                }
            }
            Err(error) => {
                let mut runtime = self.lock();
                if let Some(job) = runtime.job.as_mut() {
                    job.error = Some(format!(
                        "Local processing failed: {error}. The original transcript is unchanged."
                    ));
                }
            }
        }
        self.finish_preview(state);
    }

    fn finish_preview(&self, state: &AppState) {
        {
            let mut runtime = self.lock();
            if let Some(job) = runtime.job.as_mut() {
                job.phase = Phase::Preview;
                // The pipeline is free again: the review is an explicit user step.
                job.guard = None;
            }
            runtime.overlay_visible = false;
        }
        self.persist(state);
        self.emit_state();
        self.hide_overlay();
        self.focus_main_window();
    }

    /// Retry a failed save. The captured source and the operation guard are
    /// kept, so the retry cannot race another workflow.
    pub fn retry_save(&self, state: &AppState) -> Result<(), String> {
        {
            let runtime = self.lock();
            let job = runtime.job.as_ref().ok_or("No dictation job is active.")?;
            if job.phase != Phase::SaveFailed {
                return Err("There is no failed save to retry.".into());
            }
            if job.guard.is_none() {
                return Err("The dictation recording is no longer available.".into());
            }
        }
        self.mark_saving()?;
        self.persist(state);
        self.emit_state();
        self.run_stop(state);
        Ok(())
    }

    /// Retry a failed transcription. The saved recording is reused.
    pub fn retry_transcribe(&self, state: &AppState) -> Result<(), String> {
        let note_id = {
            let mut runtime = self.lock();
            let job = runtime.job.as_mut().ok_or("No dictation job is active.")?;
            let note_id = job
                .note_id
                .clone()
                .ok_or("There is no saved recording to retry.")?;
            if job.guard.is_none() {
                job.guard = Some(state.claim()?);
            }
            job.phase = Phase::Transcribing;
            job.error = None;
            note_id
        };
        self.persist(state);
        self.emit_state();
        let language = self.settings().language;
        if let Err(error) = self
            .pipeline
            .transcribe(self.app.as_ref(), state, &note_id, &language)
        {
            let mut runtime = self.lock();
            if let Some(job) = runtime.job.as_mut() {
                job.phase = Phase::Failed;
                job.error = Some(format!("Transcription failed: {error}"));
                job.guard = None;
            }
            self.persist(state);
            self.emit_state();
            return Err(error);
        }
        let original_text = self
            .pipeline
            .original_text(state, &note_id)
            .unwrap_or_default();
        let policy_bundle = self
            .lock()
            .job
            .as_ref()
            .and_then(|job| job.target.as_ref())
            .map(|target| target.app.bundle_id.clone());
        let policy =
            dictation_settings::resolve_processing(&self.settings(), policy_bundle.as_deref());
        {
            let mut runtime = self.lock();
            if let Some(job) = runtime.job.as_mut() {
                job.original_text = original_text.clone();
                job.preview_text = original_text.clone();
                job.phase = if policy.ai_enabled {
                    Phase::Processing
                } else {
                    Phase::Preview
                };
            }
        }
        self.persist(state);
        self.emit_state();
        if policy.ai_enabled {
            self.run_processing(state, &note_id, &policy);
        } else {
            self.finish_preview(state);
        }
        Ok(())
    }

    /// Retry optional local processing from the saved transcript.
    pub fn retry_process(&self, state: &AppState) -> Result<(), String> {
        let note_id = {
            let mut runtime = self.lock();
            let job = runtime.job.as_mut().ok_or("No dictation job is active.")?;
            let note_id = job
                .note_id
                .clone()
                .ok_or("There is no saved recording to process.")?;
            if job.guard.is_none() {
                job.guard = Some(state.claim()?);
            }
            job.phase = Phase::Processing;
            job.error = None;
            note_id
        };
        self.persist(state);
        self.emit_state();
        let policy_bundle = self
            .lock()
            .job
            .as_ref()
            .and_then(|job| job.target.as_ref())
            .map(|target| target.app.bundle_id.clone());
        let policy =
            dictation_settings::resolve_processing(&self.settings(), policy_bundle.as_deref());
        self.run_processing(state, &note_id, &policy);
        Ok(())
    }

    /// Store the reviewed text. It stays local to the job and is bounded.
    pub fn update_preview(&self, text: &str) -> Result<(), String> {
        if text.len() > PREVIEW_LIMIT {
            return Err("The reviewed text is too long to insert.".into());
        }
        {
            let mut runtime = self.lock();
            let job = runtime.job.as_mut().ok_or("No dictation job is active.")?;
            job.preview_text = text.to_string();
        }
        self.emit_state();
        Ok(())
    }

    /// Ask the native adapter to paste. The request is single-use: once a paste
    /// was requested, no automatic retry can follow, not even after a timeout.
    pub fn insert(&self, state: &AppState) -> Result<InsertOutcome, String> {
        let (token, text, restore) = {
            let runtime = self.lock();
            let job = runtime
                .job
                .as_ref()
                .ok_or("There is no dictation to insert.")?;
            if job.phase != Phase::Preview {
                return Err("Review the transcript before inserting it.".into());
            }
            if job.insert_dispatched {
                return Err(
                    "A paste was already requested for this dictation. Verify the target app, then dismiss it."
                        .into(),
                );
            }
            let target = job
                .target
                .as_ref()
                .ok_or("This dictation has no insertion target. Copy the text instead.")?;
            if !target.insertion_available {
                return Err(
                    "This target cannot receive inserted text. Copy the text instead.".into(),
                );
            }
            if job.preview_text.trim().is_empty() {
                return Err("There is no text to insert.".into());
            }
            (
                target.token.clone(),
                job.preview_text.clone(),
                runtime.settings.restore_clipboard,
            )
        };
        let Some(platform) = self.platform.clone() else {
            return Err(
                "Text insertion is not available in this build. Copy the text instead.".into(),
            );
        };
        let request_id = uuid::Uuid::new_v4().to_string();
        {
            let mut runtime = self.lock();
            let job = runtime
                .job
                .as_mut()
                .ok_or("There is no dictation to insert.")?;
            // Mark the request as spent before dispatching, so a double click
            // can never post a second paste.
            job.insert_dispatched = true;
            job.phase = Phase::Inserting;
        }
        self.emit_state();
        let outcome = platform.insert(&token, &request_id, &text, restore);
        match outcome {
            Ok(InsertOutcome::PasteRequested { clipboard, message }) => {
                {
                    let mut runtime = self.lock();
                    if let Some(job) = runtime.job.as_mut() {
                        job.phase = Phase::PasteRequested;
                        job.paste_message = Some(message.clone());
                        job.clipboard = Some(clipboard);
                        job.error = None;
                    }
                }
                self.persist(state);
                self.emit_state();
                Ok(InsertOutcome::PasteRequested { clipboard, message })
            }
            Ok(InsertOutcome::Refused { reason, message }) => {
                {
                    let mut runtime = self.lock();
                    if let Some(job) = runtime.job.as_mut() {
                        job.phase = Phase::Preview;
                        job.error = Some(message.clone());
                        // A refusal never reached the clipboard, so the user may
                        // try again with a fresh single-use request.
                        job.insert_dispatched = false;
                    }
                }
                self.emit_state();
                Ok(InsertOutcome::Refused { reason, message })
            }
            Ok(InsertOutcome::Failed { clipboard, message }) => {
                {
                    let mut runtime = self.lock();
                    if let Some(job) = runtime.job.as_mut() {
                        job.phase = Phase::Preview;
                        job.error = Some(message.clone());
                        job.clipboard = Some(clipboard);
                        // The outcome is uncertain, so no second paste is offered.
                        job.insert_dispatched = true;
                    }
                }
                self.persist(state);
                self.emit_state();
                Ok(InsertOutcome::Failed { clipboard, message })
            }
            Err(error) => {
                {
                    let mut runtime = self.lock();
                    if let Some(job) = runtime.job.as_mut() {
                        job.phase = Phase::Preview;
                        job.error = Some(error.clone());
                        job.insert_dispatched = true;
                    }
                }
                self.persist(state);
                self.emit_state();
                Err(error)
            }
        }
    }

    /// Dismiss the job. The saved recording and results stay in the library;
    /// the native target lease is released.
    pub fn dismiss(&self, state: &AppState) -> Result<(), String> {
        let (token, job_id) = {
            let runtime = self.lock();
            let job = runtime
                .job
                .as_ref()
                .ok_or("There is no dictation to dismiss.")?;
            if matches!(
                job.phase,
                Phase::Starting
                    | Phase::Recording
                    | Phase::Paused
                    | Phase::Stopped
                    | Phase::Saving
                    | Phase::SaveFailed
            ) {
                return Err("Stop or discard the recording before dismissing it.".into());
            }
            (
                job.target.as_ref().map(|target| target.token.clone()),
                job.id.clone(),
            )
        };
        self.release_target(token);
        {
            let mut runtime = self.lock();
            runtime.job = None;
            runtime.overlay_visible = false;
        }
        if let Ok(db) = state.db() {
            let _ = dictation_settings::delete_job(&db, &job_id);
        }
        self.hide_overlay();
        self.emit_state();
        Ok(())
    }

    /// Explicitly discard the captured audio. The source and the job are
    /// dropped; a short tap is never discarded automatically.
    pub fn discard(&self, state: &AppState) -> Result<(), String> {
        let (token, job_id) = {
            let runtime = self.lock();
            let job = runtime
                .job
                .as_ref()
                .ok_or("There is no dictation to discard.")?;
            if !matches!(
                job.phase,
                Phase::Recording | Phase::Paused | Phase::Stopped | Phase::SaveFailed
            ) {
                return Err("There is no captured audio to discard.".into());
            }
            (
                job.target.as_ref().map(|target| target.token.clone()),
                job.id.clone(),
            )
        };
        {
            let mut slot = state
                .recording
                .lock()
                .map_err(|_| "Recording is unavailable.".to_string())?;
            if slot
                .as_ref()
                .is_some_and(|recording| recording.owner == RecordingOwner::Dictation)
            {
                // Dropping the session removes the pending capture.
                slot.take();
            }
        }
        self.release_target(token);
        {
            let mut runtime = self.lock();
            if let Some(job) = runtime.job.as_mut() {
                job.guard = None;
                job.phase = Phase::Idle;
            }
            runtime.job = None;
            runtime.overlay_visible = false;
        }
        if let Ok(db) = state.db() {
            let _ = dictation_settings::delete_job(&db, &job_id);
        }
        self.hide_overlay();
        self.emit_state();
        Ok(())
    }

    fn release_target(&self, token: Option<String>) {
        let (Some(platform), Some(token)) = (self.platform.as_ref(), token) else {
            return;
        };
        platform.release_target(&token);
    }

    /// Bring the review forward in the main window.
    pub fn show_review(&self, _state: &AppState) {
        self.focus_main_window();
        self.emit_state();
    }

    pub fn hide_overlay(&self) {
        {
            let mut runtime = self.lock();
            runtime.overlay_visible = false;
        }
        if let Some(app) = &self.app {
            crate::dictation_overlay::hide(app);
        }
    }

    fn present_overlay(&self) {
        let (show, always_on_top) = {
            let runtime = self.lock();
            (
                runtime.settings.show_overlay,
                runtime.settings.overlay_always_on_top,
            )
        };
        if !show {
            return;
        }
        if let Some(app) = &self.app {
            if let Err(error) = crate::dictation_overlay::present(app, always_on_top) {
                self.set_error(&error);
            }
        }
    }

    fn focus_main_window(&self) {
        if let Some(app) = &self.app {
            crate::dictation_overlay::focus_main(app);
        }
    }

    pub fn permissions(&self) -> Result<Permissions, String> {
        let Some(platform) = self.platform.as_ref() else {
            return Err("Native insertion is not available in this build.".into());
        };
        let permissions = platform.permissions()?;
        self.lock().permissions = Some(permissions);
        Ok(permissions)
    }

    pub fn request_permission(&self, kind: PermissionKind) -> Result<Permissions, String> {
        let Some(platform) = self.platform.as_ref() else {
            return Err("Native insertion is not available in this build.".into());
        };
        let permissions = platform.request_permission(kind)?;
        self.lock().permissions = Some(permissions);
        // A granted permission never triggers a deferred insertion.
        self.emit_state();
        Ok(permissions)
    }

    pub fn open_permission_settings(&self, kind: PermissionKind) -> Result<(), String> {
        let Some(platform) = self.platform.as_ref() else {
            return Err("Native insertion is not available in this build.".into());
        };
        platform.open_permission_settings(kind)?;
        Ok(())
    }

    /// Persist the job so an interrupted run can be reviewed after a restart.
    fn persist(&self, state: &AppState) {
        let row = {
            let runtime = self.lock();
            runtime.job.as_ref().map(|job| JobRow {
                id: job.id.clone(),
                note_id: job.note_id.clone(),
                phase: format!("{:?}", job.phase),
                original_text: job.original_text.clone(),
                processed_text: job.processed_text.clone(),
                model: job.processed_model.clone().unwrap_or_default(),
                prompt: job.processed_prompt.clone().unwrap_or_default(),
                target_bundle_id: job
                    .target
                    .as_ref()
                    .map(|target| target.app.bundle_id.clone())
                    .unwrap_or_default(),
                target_name: job
                    .target
                    .as_ref()
                    .map(|target| target.app.name.clone())
                    .unwrap_or_default(),
                error: job.error.clone(),
                created_at: job.created_at,
                updated_at: job.updated_at,
            })
        };
        let Some(row) = row else { return };
        if let Ok(db) = state.db() {
            let _ = dictation_settings::save_job(&db, &row);
        }
    }

    /// Restore an interrupted job for review only. No target lease survives a
    /// restart, so insertion stays unavailable until the user starts again.
    pub fn recover(&self, state: &AppState) -> Result<(), String> {
        let db = state.db()?;
        let rows = dictation_settings::list_jobs(&db)?;
        let Some(row) = rows.into_iter().next() else {
            return Ok(());
        };
        if row.phase == "Idle" {
            return Ok(());
        }
        let now = Self::now();
        let mut runtime = self.lock();
        runtime.job = Some(Job {
            id: row.id.clone(),
            phase: if row.note_id.is_some() {
                Phase::Preview
            } else {
                Phase::Failed
            },
            note_id: row.note_id.clone(),
            target: None,
            original_text: row.original_text.clone(),
            processed_text: row.processed_text.clone(),
            processed_model: None,
            processed_prompt: None,
            preview_text: row
                .processed_text
                .clone()
                .unwrap_or_else(|| row.original_text.clone()),
            error: Some(row.error.clone().unwrap_or_else(|| {
                "Dictation was interrupted. Review the saved text or copy it.".to_string()
            })),
            warning: None,
            paste_message: None,
            clipboard: None,
            insert_dispatched: true,
            recovered: true,
            guard: None,
            created_at: row.created_at,
            updated_at: now,
        });
        Ok(())
    }
}

/// Production pipeline: the shared recording engine, the shared transcription
/// path and the existing loopback-only local AI pipeline.
pub struct RealPipeline;

impl Pipeline for RealPipeline {
    fn start_capture(&self, state: &AppState) -> Result<crate::recording::Session, String> {
        // Global dictation uses the microphone source only. Other capture
        // sources stay available in the main window.
        crate::recording::Session::start(&state.root, "microphone", 0)
    }

    fn save(
        &self,
        state: &AppState,
        recording: &mut ActiveRecording,
    ) -> Result<SavedCapture, String> {
        let saved = crate::save_recording(state, recording)?;
        Ok(SavedCapture {
            note_id: saved.note.id,
            warning: saved.warning,
        })
    }

    fn transcribe(
        &self,
        app: Option<&tauri::AppHandle>,
        state: &AppState,
        note_id: &str,
        language: &str,
    ) -> Result<(), String> {
        let Some(app) = app else {
            return Err("Transcription needs the running application.".into());
        };
        crate::transcribe_note(app, state, note_id, language)
    }

    fn original_text(&self, state: &AppState, note_id: &str) -> Result<String, String> {
        let db = state.db()?;
        let note = crate::library::get(&db, note_id)?;
        Ok(note
            .segments
            .iter()
            .map(|segment| segment.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"))
    }

    fn process(
        &self,
        app: Option<&tauri::AppHandle>,
        state: &AppState,
        note_id: &str,
        kind: &str,
        prompt: &str,
        language: &str,
    ) -> Result<Processed, String> {
        let Some(app) = app else {
            return Err("Local processing needs the running application.".into());
        };
        crate::ai_commands::process_for_dictation(app, state, note_id, kind, prompt, language)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation::{ClipboardOutcome, InsertRefusal, Permissions, TargetApp};
    use crate::recording;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// A native adapter stand-in. It records every call, so the tests can
    /// prove that the runtime captures a target exactly once and dispatches at
    /// most one paste.
    struct FakePlatform {
        target: Mutex<Option<CapturedTarget>>,
        inserts: Mutex<Vec<(String, String, String, bool)>>,
        released: Mutex<Vec<String>>,
        refuse: Mutex<bool>,
        fail: Mutex<bool>,
    }
    impl FakePlatform {
        fn new() -> Arc<FakePlatform> {
            Arc::new(FakePlatform {
                target: Mutex::new(Some(CapturedTarget {
                    token: "target-1".into(),
                    app: TargetApp {
                        pid: 42,
                        bundle_id: "com.example.Notes".into(),
                        name: "Notes".into(),
                        launch_time_ms: 1,
                    },
                    insertion_available: true,
                    refusal: None,
                })),
                inserts: Mutex::new(vec![]),
                released: Mutex::new(vec![]),
                refuse: Mutex::new(false),
                fail: Mutex::new(false),
            })
        }
    }
    impl Platform for FakePlatform {
        fn permissions(&self) -> Result<Permissions, String> {
            Ok(Permissions {
                accessibility: true,
                post_events: true,
            })
        }
        fn request_permission(&self, _kind: PermissionKind) -> Result<Permissions, String> {
            Ok(Permissions {
                accessibility: true,
                post_events: true,
            })
        }
        fn open_permission_settings(&self, _kind: PermissionKind) -> Result<(), String> {
            Ok(())
        }
        fn capture_target(&self) -> Result<CapturedTarget, String> {
            self.target
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| "No target is available.".to_string())
        }
        fn release_target(&self, token: &str) {
            self.released.lock().unwrap().push(token.to_string());
        }
        fn insert(
            &self,
            target: &str,
            request_id: &str,
            text: &str,
            restore_previous_clipboard: bool,
        ) -> Result<InsertOutcome, String> {
            self.inserts.lock().unwrap().push((
                target.to_string(),
                request_id.to_string(),
                text.to_string(),
                restore_previous_clipboard,
            ));
            if *self.fail.lock().unwrap() {
                return Err("The native paste did not finish in time.".into());
            }
            if *self.refuse.lock().unwrap() {
                return Ok(InsertOutcome::Refused {
                    reason: InsertRefusal::FocusChanged,
                    message: "The target app changed the focused element.".into(),
                });
            }
            Ok(InsertOutcome::PasteRequested {
                clipboard: ClipboardOutcome::Unchanged,
                message: "Paste requested; verify the target app.".into(),
            })
        }
    }

    /// A deterministic pipeline: no microphone, no model, no clipboard.
    struct FakePipeline {
        root: PathBuf,
        save_failures: Mutex<u32>,
        transcribe_failures: Mutex<u32>,
        process_failures: Mutex<u32>,
        save_calls: AtomicUsize,
        transcribe_calls: AtomicUsize,
        process_calls: AtomicUsize,
        text: String,
    }
    impl FakePipeline {
        fn new(root: PathBuf) -> Arc<FakePipeline> {
            Arc::new(FakePipeline {
                root,
                save_failures: Mutex::new(0),
                transcribe_failures: Mutex::new(0),
                process_failures: Mutex::new(0),
                save_calls: AtomicUsize::new(0),
                transcribe_calls: AtomicUsize::new(0),
                process_calls: AtomicUsize::new(0),
                text: "Dictated words.".into(),
            })
        }
    }
    impl Pipeline for FakePipeline {
        fn start_capture(&self, _state: &AppState) -> Result<recording::Session, String> {
            let source = self.root.join("dictation-fixture.wav");
            if !source.exists() {
                std::fs::write(&source, b"fixture").unwrap();
            }
            Ok(recording::Session::from_saved_fixture(source))
        }
        fn save(
            &self,
            state: &AppState,
            recording: &mut ActiveRecording,
        ) -> Result<SavedCapture, String> {
            self.save_calls.fetch_add(1, Ordering::SeqCst);
            let mut failures = self.save_failures.lock().unwrap();
            if *failures > 0 {
                *failures -= 1;
                return Err("The recording could not be saved.".into());
            }
            drop(failures);
            let note = crate::library::Note {
                id: recording.id.clone(),
                title: recording.title.clone(),
                created_at: recording.created_at,
                duration_seconds: 1.0,
                status: "ready".into(),
                segments: vec![],
                error: None,
                folder: String::new(),
                tags: vec![],
            };
            let db = state.db()?;
            crate::library::insert(&db, &note)?;
            Ok(SavedCapture {
                note_id: note.id,
                warning: None,
            })
        }
        fn transcribe(
            &self,
            _app: Option<&tauri::AppHandle>,
            _state: &AppState,
            _note_id: &str,
            _language: &str,
        ) -> Result<(), String> {
            self.transcribe_calls.fetch_add(1, Ordering::SeqCst);
            let mut failures = self.transcribe_failures.lock().unwrap();
            if *failures > 0 {
                *failures -= 1;
                return Err("The local model is unavailable.".into());
            }
            Ok(())
        }
        fn original_text(&self, _state: &AppState, _note_id: &str) -> Result<String, String> {
            Ok(self.text.clone())
        }
        fn process(
            &self,
            _app: Option<&tauri::AppHandle>,
            _state: &AppState,
            _note_id: &str,
            _kind: &str,
            _prompt: &str,
            _language: &str,
        ) -> Result<Processed, String> {
            self.process_calls.fetch_add(1, Ordering::SeqCst);
            let mut failures = self.process_failures.lock().unwrap();
            if *failures > 0 {
                *failures -= 1;
                return Err("The local model is unavailable.".into());
            }
            drop(failures);
            Ok(Processed {
                text: "Cleaned dictated words.".into(),
                model: "local-model".into(),
                prompt: "Fix punctuation.".into(),
            })
        }
    }

    struct Fixture {
        root: PathBuf,
        state: AppState,
        pipeline: Arc<FakePipeline>,
        platform: Arc<FakePlatform>,
    }

    fn fixture(name: &str) -> Fixture {
        let root =
            std::env::temp_dir().join(format!("golos-dictation-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("audio")).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::library::init(&db, Path::new("in-memory")).unwrap();
        db.execute_batch(crate::dictation_settings::SCHEMA).unwrap();
        let pipeline = FakePipeline::new(root.clone());
        let platform = FakePlatform::new();
        let dictation = Arc::new(DictationRuntime::with_parts(
            None,
            Some(platform.clone()),
            pipeline.clone(),
        ));
        let state = AppState {
            root: root.clone(),
            db: Mutex::new(db),
            busy: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            recording: Mutex::new(None),
            automation_label: Mutex::new(None),
            automation_paused: AtomicBool::new(false),
            dictation,
        };
        Fixture {
            root,
            state,
            pipeline,
            platform,
        }
    }

    #[test]
    fn start_captures_the_target_before_the_overlay_and_holds_the_pipeline() {
        let fixture = fixture("start");
        fixture.state.dictation.start(&fixture.state).unwrap();
        let status = fixture.state.dictation.status();
        assert_eq!(status.phase, Phase::Recording);
        assert!(status.active);
        assert_eq!(
            status.target.map(|target| target.bundle_id),
            Some("com.example.Notes".into())
        );
        // The dictation owns the single recording session.
        let slot = fixture.state.recording.lock().unwrap();
        assert_eq!(
            slot.as_ref().map(|recording| recording.owner),
            Some(RecordingOwner::Dictation)
        );
        drop(slot);
        // The pipeline is busy while dictation records.
        assert!(fixture.state.claim().is_err());
        std::fs::remove_dir_all(fixture.root).ok();
    }

    #[test]
    fn a_failed_save_keeps_the_capture_and_the_guard_for_a_retry() {
        let fixture = fixture("save-failure");
        fixture.state.dictation.start(&fixture.state).unwrap();
        *fixture.pipeline.save_failures.lock().unwrap() = 1;
        fixture
            .state
            .dictation
            .stop_and_run(&fixture.state)
            .unwrap();
        let status = fixture.state.dictation.status();
        assert_eq!(status.phase, Phase::SaveFailed);
        assert!(status.can_retry_save);
        assert!(status.error.unwrap().contains("could not be saved"));
        // The captured source and the operation guard are still there.
        assert!(fixture
            .state
            .recording
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|recording| recording.owner == RecordingOwner::Dictation));
        assert!(fixture.state.claim().is_err());
        // The retry reuses the same source and reaches the review step.
        fixture.state.dictation.retry_save(&fixture.state).unwrap();
        let status = fixture.state.dictation.status();
        assert_eq!(status.phase, Phase::Preview);
        assert_eq!(status.original_text, "Dictated words.");
        // The pipeline is free again after the explicit review step.
        assert!(fixture.state.claim().is_ok());
        std::fs::remove_dir_all(fixture.root).ok();
    }

    #[test]
    fn a_failed_transcription_keeps_the_saved_recording_for_a_retry() {
        let fixture = fixture("transcribe-failure");
        fixture.state.dictation.start(&fixture.state).unwrap();
        *fixture.pipeline.transcribe_failures.lock().unwrap() = 1;
        fixture
            .state
            .dictation
            .stop_and_run(&fixture.state)
            .unwrap();
        let status = fixture.state.dictation.status();
        assert_eq!(status.phase, Phase::Failed);
        assert!(status.can_retry_transcribe);
        assert!(status.note_id.is_some());
        fixture
            .state
            .dictation
            .retry_transcribe(&fixture.state)
            .unwrap();
        let status = fixture.state.dictation.status();
        assert_eq!(status.phase, Phase::Preview);
        assert_eq!(status.original_text, "Dictated words.");
        assert!(status.can_insert);
        std::fs::remove_dir_all(fixture.root).ok();
    }

    #[test]
    fn local_processing_is_separate_from_the_original_text_and_can_be_retried() {
        let fixture = fixture("processing");
        // Local processing is opt-in and off by default.
        fixture
            .state
            .dictation
            .set_settings(crate::dictation_settings::Settings {
                ai_enabled: true,
                ..crate::dictation_settings::Settings::default()
            });
        fixture.state.dictation.start(&fixture.state).unwrap();
        *fixture.pipeline.process_failures.lock().unwrap() = 1;
        fixture
            .state
            .dictation
            .stop_and_run(&fixture.state)
            .unwrap();
        let status = fixture.state.dictation.status();
        assert_eq!(status.phase, Phase::Preview);
        assert_eq!(status.original_text, "Dictated words.");
        assert!(status.processed_text.is_none());
        assert!(status.can_retry_process);
        assert!(status.error.unwrap().contains("Local processing failed"));
        fixture
            .state
            .dictation
            .retry_process(&fixture.state)
            .unwrap();
        let status = fixture.state.dictation.status();
        assert_eq!(
            status.processed_text.as_deref(),
            Some("Cleaned dictated words.")
        );
        assert_eq!(status.processed_model.as_deref(), Some("local-model"));
        assert_eq!(status.preview_text, "Cleaned dictated words.");
        std::fs::remove_dir_all(fixture.root).ok();
    }

    #[test]
    fn insertion_is_explicit_single_use_and_never_reported_as_inserted() {
        let fixture = fixture("insert");
        fixture.state.dictation.start(&fixture.state).unwrap();
        fixture
            .state
            .dictation
            .stop_and_run(&fixture.state)
            .unwrap();
        let outcome = fixture.state.dictation.insert(&fixture.state).unwrap();
        match outcome {
            InsertOutcome::PasteRequested { message, .. } => {
                assert!(message.contains("Paste requested"));
            }
            other => panic!("unexpected outcome {other:?}"),
        }
        let status = fixture.state.dictation.status();
        assert_eq!(status.phase, Phase::PasteRequested);
        assert!(!status.can_insert);
        // A second request is refused: the dispatch is single-use.
        assert!(fixture.state.dictation.insert(&fixture.state).is_err());
        assert_eq!(fixture.platform.inserts.lock().unwrap().len(), 1);
        std::fs::remove_dir_all(fixture.root).ok();
    }

    #[test]
    fn a_refused_insert_allows_a_fresh_request_but_an_uncertain_one_does_not() {
        let fixture = fixture("insert-refused");
        fixture.state.dictation.start(&fixture.state).unwrap();
        fixture
            .state
            .dictation
            .stop_and_run(&fixture.state)
            .unwrap();
        *fixture.platform.refuse.lock().unwrap() = true;
        let outcome = fixture.state.dictation.insert(&fixture.state).unwrap();
        assert!(matches!(outcome, InsertOutcome::Refused { .. }));
        assert!(fixture.state.dictation.status().can_insert);
        *fixture.platform.refuse.lock().unwrap() = false;
        fixture.state.dictation.insert(&fixture.state).unwrap();
        let inserts = fixture.platform.inserts.lock().unwrap().clone();
        assert_eq!(inserts.len(), 2);
        assert_ne!(inserts[0].1, inserts[1].1);
        std::fs::remove_dir_all(fixture.root).ok();
    }

    #[test]
    fn a_failed_insert_is_never_retried_automatically() {
        let fixture = fixture("insert-timeout");
        fixture.state.dictation.start(&fixture.state).unwrap();
        fixture
            .state
            .dictation
            .stop_and_run(&fixture.state)
            .unwrap();
        *fixture.platform.fail.lock().unwrap() = true;
        assert!(fixture.state.dictation.insert(&fixture.state).is_err());
        let status = fixture.state.dictation.status();
        assert!(!status.can_insert);
        assert_eq!(fixture.platform.inserts.lock().unwrap().len(), 1);
        std::fs::remove_dir_all(fixture.root).ok();
    }

    #[test]
    fn dismiss_keeps_the_saved_note_and_releases_the_target() {
        let fixture = fixture("dismiss");
        fixture.state.dictation.start(&fixture.state).unwrap();
        // An active capture cannot be dismissed.
        assert!(fixture.state.dictation.dismiss(&fixture.state).is_err());
        fixture
            .state
            .dictation
            .stop_and_run(&fixture.state)
            .unwrap();
        let note_id = fixture.state.dictation.status().note_id.clone().unwrap();
        fixture.state.dictation.dismiss(&fixture.state).unwrap();
        let status = fixture.state.dictation.status();
        assert_eq!(status.phase, Phase::Idle);
        assert!(status.note_id.is_none());
        // The saved recording stays in the library.
        let db = fixture.state.db().unwrap();
        let notes = crate::library::list(&db).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, note_id);
        drop(db);
        assert_eq!(
            fixture.platform.released.lock().unwrap().clone(),
            vec!["target-1"]
        );
        std::fs::remove_dir_all(fixture.root).ok();
    }

    #[test]
    fn discard_is_explicit_and_removes_the_capture_without_a_note() {
        let fixture = fixture("discard");
        fixture.state.dictation.start(&fixture.state).unwrap();
        fixture.state.dictation.discard(&fixture.state).unwrap();
        let status = fixture.state.dictation.status();
        assert_eq!(status.phase, Phase::Idle);
        assert!(status.note_id.is_none());
        assert!(fixture.state.recording.lock().unwrap().is_none());
        // The pipeline is free again.
        assert!(fixture.state.claim().is_ok());
        let db = fixture.state.db().unwrap();
        assert!(crate::library::list(&db).unwrap().is_empty());
        std::fs::remove_dir_all(fixture.root).ok();
    }

    #[test]
    fn a_restored_job_can_only_be_reviewed_and_never_inserts() {
        let fixture = fixture("recovered");
        {
            let db = fixture.state.db().unwrap();
            crate::dictation_settings::save_job(
                &db,
                &crate::dictation_settings::JobRow {
                    id: "job-1".into(),
                    note_id: Some("note-1".into()),
                    phase: "Transcribing".into(),
                    original_text: "Half said words.".into(),
                    processed_text: None,
                    model: String::new(),
                    prompt: String::new(),
                    target_bundle_id: "com.example.Notes".into(),
                    target_name: "Notes".into(),
                    error: None,
                    created_at: 1,
                    updated_at: 1,
                },
            )
            .unwrap();
        }
        fixture.state.dictation.recover(&fixture.state).unwrap();
        let status = fixture.state.dictation.status();
        assert_eq!(status.phase, Phase::Preview);
        assert!(status.recovered);
        assert!(status.target.is_none());
        assert!(!status.can_insert);
        assert!(status.can_dismiss);
        // No capture is started and the pipeline stays free.
        assert!(fixture.state.recording.lock().unwrap().is_none());
        assert!(fixture.state.claim().is_ok());
        std::fs::remove_dir_all(fixture.root).ok();
    }

    #[test]
    fn without_a_platform_the_runtime_records_but_never_claims_insertion() {
        let root = std::env::temp_dir().join(format!(
            "golos-dictation-noplatform-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(root.join("audio")).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::library::init(&db, Path::new("in-memory")).unwrap();
        db.execute_batch(crate::dictation_settings::SCHEMA).unwrap();
        let dictation = Arc::new(DictationRuntime::with_parts(
            None,
            None,
            FakePipeline::new(root.clone()),
        ));
        let state = AppState {
            root: root.clone(),
            db: Mutex::new(db),
            busy: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            recording: Mutex::new(None),
            automation_label: Mutex::new(None),
            automation_paused: AtomicBool::new(false),
            dictation,
        };
        state.dictation.start(&state).unwrap();
        state.dictation.stop_and_run(&state).unwrap();
        let status = state.dictation.status();
        assert!(!status.insertion_supported);
        assert!(!status.can_insert);
        assert!(state.dictation.insert(&state).is_err());
        std::fs::remove_dir_all(root).ok();
    }
}
