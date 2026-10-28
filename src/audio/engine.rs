//! Audio engine: device enumeration, WAV/MP3 decoding and playback on up to two outputs.
//!
//! Design:
//! * `Engine` is a cheap, cloneable, `Send + Sync` handle. The GUI thread and the hotkey thread
//!   both use it.
//! * One dedicated worker thread owns the `cpal` streams (they are not `Send` on every platform),
//!   decodes files and feeds the device-independent `Mixer`s. The GUI never blocks on decoding.
//! * There are two outputs, each with its own `Mixer`: `Main` (headphones/speakers) and
//!   `Secondary`, the **microphone route**. A sound is decoded once, resampled once per distinct
//!   output rate (not at all if the rates match) and started on both mixers in one step. Each
//!   route has its own volume and the microphone route can be muted without touching the other.
//! * Stop commands act on the mixers directly (they never queue behind a long decode) and a
//!   ticket system makes sure a play request issued before a stop can never start afterwards.
//! * The worker watches the devices: it publishes the device list, notices unplugged devices and
//!   failed streams, reports them and reconnects automatically when the device returns.
//!
//! Important: an output device is not a microphone. The microphone route is a *playback* device,
//! normally a virtual audio cable whose recording side the chat application uses as its
//! microphone. Playback devices and recording devices are listed separately (`DeviceList`).
//!
//! The user's own microphone is optional ("voice"): when it is switched on, a capture stream on a
//! real recording device feeds a `LiveInput` that is mixed into the microphone route only (never
//! into the headphones, so there is no echo). It is never opened unless the user asks for it, and
//! the recording side of the route itself is refused because that would be a feedback loop.

use crate::audio::cache::{AudioCache, DEFAULT_CACHE_BYTES};
use crate::audio::mixer::{LiveInput, Mixer, PlayOutcome, CHANNELS};
use crate::audio::routing::{
    add_input_hint, analyze_route_test, find_paired_input, make_test_tone, route_output_conflict,
    voice_feedback_conflict, DeviceKind,
};
pub use crate::audio::routing::{CaptureOutcome, OutputRole, ReportLevel, RouteState, RouteTestReport};
use crate::settings::RetriggerPolicy;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Longest sound that will be decoded into memory.
pub const MAX_SOUND_SECS: f64 = 300.0;
/// How often the worker lists the audio devices.
const SCAN_INTERVAL: Duration = Duration::from_secs(3);
/// How often a lost or missing device is tried again.
const RECOVER_INTERVAL: Duration = Duration::from_secs(3);
/// A device must be absent from this many consecutive scans before it counts as unplugged.
const MISSING_SCANS_BEFORE_LOST: u32 = 2;

/// Which devices to open. `None` means the system default device (headphones) or "nothing
/// selected" (microphone route).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceConfig {
    pub main: Option<String>,
    pub secondary: Option<String>,
    pub secondary_enabled: bool,
    /// Real microphone to mix into the microphone route. `None` = the system default microphone.
    pub voice: Option<String>,
    /// Whether the microphone is mixed into the microphone route at all.
    pub voice_enabled: bool,
}

/// Volumes of the two routes. The final level of a route is `master * route`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Levels {
    pub master: f32,
    pub main: f32,
    pub mic: f32,
    pub mic_muted: bool,
    /// Volume of the user's own voice in the microphone route. Independent of `master`, `mic` and
    /// `mic_muted`, which control the soundboard sounds.
    pub voice: f32,
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            master: 1.0,
            main: 1.0,
            mic: 1.0,
            mic_muted: false,
            voice: 1.0,
        }
    }
}

fn unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

impl Levels {
    pub fn sanitized(self) -> Levels {
        Levels {
            master: unit(self.master),
            main: unit(self.main),
            mic: unit(self.mic),
            mic_muted: self.mic_muted,
            voice: unit(self.voice),
        }
    }

    /// Output gain of one route (a muted microphone route has gain 0).
    pub fn route_gain(&self, role: OutputRole) -> f32 {
        match role {
            OutputRole::Main => self.main,
            OutputRole::Secondary => {
                if self.mic_muted {
                    0.0
                } else {
                    self.mic
                }
            }
        }
    }
}

/// Playback and recording devices of the default audio host, as last seen by the worker. The two
/// lists are never mixed: a microphone is not an output and the other way round.
#[derive(Debug, Clone, Default)]
pub struct DeviceList {
    /// Playback devices: headphones, speakers, the playback side of a virtual cable.
    pub outputs: Vec<String>,
    pub default_output: Option<String>,
    /// Recording devices: microphones, headset microphones, the recording side of a virtual cable.
    pub inputs: Vec<String>,
    pub default_input: Option<String>,
    /// Why the last attempt to list playback devices failed (the previous list is kept).
    pub output_error: Option<String>,
    /// Why the last attempt to list recording devices failed (the previous list is kept).
    pub input_error: Option<String>,
    /// False until the first scan has finished.
    pub scanned: bool,
}

/// Notifications from the audio worker to the GUI.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// A sound was decoded; its length is now known.
    Decoded { sound_id: String, duration_secs: f64 },
    /// Something went wrong. `message` is written for the end user.
    Error { sound_id: Option<String>, message: String },
    /// A device that was working stopped (unplugged, driver reset, ...).
    DeviceLost {
        role: OutputRole,
        device: String,
        reason: String,
    },
    /// A lost or missing device is working again.
    Recovered { role: OutputRole, device: String },
    /// The wanted headphones device is unavailable and the system default is used instead.
    FellBack {
        role: OutputRole,
        wanted: String,
        using: String,
    },
    /// Result of `Engine::test_route`.
    RouteTest(RouteTestReport),
    /// The microphone that is mixed into the route stopped working (unplugged, driver reset, ...).
    VoiceLost { device: String, reason: String },
    /// The microphone works again.
    VoiceRecovered { device: String },
}

/// Names of all playback devices of the default audio host.
pub fn output_device_names() -> Vec<String> {
    list_devices(DeviceKind::Output).unwrap_or_default()
}

/// Names of all recording devices (microphones) of the default audio host.
pub fn input_device_names() -> Vec<String> {
    list_devices(DeviceKind::Input).unwrap_or_default()
}

/// Sorted, de-duplicated device names of one kind. A failure is returned (not hidden), so the
/// settings page can tell the user why a list is missing. Devices that Windows does not provide
/// are never invented.
pub fn list_devices(kind: DeviceKind) -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let mut names: Vec<String> = match kind {
        DeviceKind::Output => host
            .output_devices()
            .map_err(|e| e.to_string())?
            .filter_map(|d| d.name().ok())
            .collect(),
        DeviceKind::Input => host
            .input_devices()
            .map_err(|e| e.to_string())?
            .filter_map(|d| d.name().ok())
            .collect(),
    };
    names.sort();
    names.dedup();
    Ok(names)
}

/// Name of the system default output device, if there is one.
pub fn default_output_name() -> Option<String> {
    cpal::default_host().default_output_device().and_then(|d| d.name().ok())
}

/// Name of the system default recording device (microphone), if there is one.
pub fn default_input_name() -> Option<String> {
    cpal::default_host().default_input_device().and_then(|d| d.name().ok())
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn idx(role: OutputRole) -> usize {
    match role {
        OutputRole::Main => 0,
        OutputRole::Secondary => 1,
    }
}

const ROLES: [OutputRole; 2] = [OutputRole::Main, OutputRole::Secondary];

#[derive(Clone)]
struct Output {
    role: OutputRole,
    name: String,
    rate: u32,
    mixer: Arc<Mutex<Mixer>>,
}

// ---- Stop tickets ------------------------------------------------------------------------------

/// Guarantees that "stop" wins over a "play" that was requested earlier but is still decoding.
#[derive(Default)]
struct Stops {
    counter: AtomicU64,
    all_before: AtomicU64,
    per_sound: Mutex<HashMap<String, u64>>,
}

impl Stops {
    fn ticket(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn stop_all(&self) {
        let t = self.ticket();
        self.all_before.store(t, Ordering::SeqCst);
        let mut map = lock(&self.per_sound);
        map.retain(|_, v| *v > t);
    }

    fn stop(&self, sound_id: &str) {
        let t = self.ticket();
        lock(&self.per_sound).insert(sound_id.to_string(), t);
    }

    fn is_cancelled(&self, sound_id: &str, ticket: u64) -> bool {
        ticket < self.all_before.load(Ordering::SeqCst)
            || lock(&self.per_sound).get(sound_id).is_some_and(|t| *t > ticket)
    }
}

// ---- Shared state ------------------------------------------------------------------------------

struct Shared {
    outputs: Mutex<Vec<Output>>,
    status: Mutex<[RouteState; 2]>,
    devices: Mutex<DeviceList>,
    levels: Mutex<Levels>,
    stops: Stops,
    /// State of the optional microphone input.
    voice_status: Mutex<RouteState>,
    /// The live source of the running microphone capture, if any.
    voice_live: Mutex<Option<Arc<LiveInput>>>,
}

fn apply_levels(shared: &Shared, levels: Levels) {
    {
        let outs = lock(&shared.outputs);
        for o in outs.iter() {
            let mut m = lock(&o.mixer);
            m.set_master_volume(levels.master);
            m.set_output_gain(levels.route_gain(o.role));
        }
    }
    if let Some(live) = lock(&shared.voice_live).as_ref() {
        live.set_gain(levels.voice);
    }
}

struct PlayCmd {
    sound_id: String,
    path: PathBuf,
    gain: f32,
    policy: RetriggerPolicy,
    allow_overlap: bool,
    ticket: u64,
}

enum Cmd {
    Configure(DeviceConfig),
    Play(PlayCmd),
    Preload {
        sound_id: String,
        path: PathBuf,
    },
    Forget(PathBuf),
    Rescan,
    StreamFailed {
        role: OutputRole,
        generation: u64,
        message: String,
    },
    VoiceFailed {
        generation: u64,
        message: String,
    },
    Shutdown,
}

/// Handle to the audio worker. Clone it freely.
#[derive(Clone)]
pub struct Engine {
    tx: Sender<Cmd>,
    events: Receiver<EngineEvent>,
    etx: Sender<EngineEvent>,
    shared: Arc<Shared>,
}

impl Engine {
    /// Start the worker thread and open the configured devices (asynchronously).
    pub fn start(config: DeviceConfig, levels: Levels) -> Engine {
        let (tx, rx) = unbounded::<Cmd>();
        let (etx, erx) = unbounded::<EngineEvent>();
        let shared = Arc::new(Shared {
            outputs: Mutex::new(Vec::new()),
            status: Mutex::new([RouteState::Disabled, RouteState::Disabled]),
            devices: Mutex::new(DeviceList::default()),
            levels: Mutex::new(levels.sanitized()),
            stops: Stops::default(),
            voice_status: Mutex::new(RouteState::Disabled),
            voice_live: Mutex::new(None),
        });

        let worker_shared = Arc::clone(&shared);
        let worker_events = etx.clone();
        let worker_tx = tx.clone();
        let spawned = std::thread::Builder::new()
            .name("taptone-audio".to_string())
            .spawn(move || {
                let mut worker = Worker::new(worker_events, worker_tx, worker_shared);
                worker.scan();
                worker.configure(&config);
                loop {
                    match rx.recv_timeout(Duration::from_millis(500)) {
                        Ok(cmd) => {
                            if !worker.handle(cmd) {
                                break;
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                    worker.tick();
                }
                worker.close_all();
            });
        if let Err(e) = spawned {
            tracing::error!("could not start the audio thread: {e}");
            let _ = etx.send(EngineEvent::Error {
                sound_id: None,
                message: format!("Could not start the audio engine: {e}"),
            });
        }
        Engine {
            tx,
            events: erx,
            etx,
            shared,
        }
    }

    /// Apply new device settings. Outputs whose device did not change keep playing; outputs that
    /// are unhealthy are retried right away.
    pub fn configure(&self, config: DeviceConfig) {
        let _ = self.tx.send(Cmd::Configure(config));
    }

    /// List the devices again right now (the worker also does this every few seconds).
    pub fn rescan_devices(&self) {
        let _ = self.tx.send(Cmd::Rescan);
    }

    /// Start a sound. Decoding happens on the worker thread; errors arrive as events.
    pub fn play(&self, sound_id: &str, path: &Path, gain: f32, policy: RetriggerPolicy, allow_overlap: bool) {
        let ticket = self.shared.stops.ticket();
        let _ = self.tx.send(Cmd::Play(PlayCmd {
            sound_id: sound_id.to_string(),
            path: path.to_path_buf(),
            gain,
            policy,
            allow_overlap,
            ticket,
        }));
    }

    /// Decode a sound in the background so its duration becomes known and the first play is instant.
    pub fn preload(&self, sound_id: &str, path: &Path) {
        let _ = self.tx.send(Cmd::Preload {
            sound_id: sound_id.to_string(),
            path: path.to_path_buf(),
        });
    }

    /// Drop cached audio of a file that is no longer needed.
    pub fn forget_file(&self, path: &Path) {
        let _ = self.tx.send(Cmd::Forget(path.to_path_buf()));
    }

    /// Stop one sound immediately (also cancels a start that is still being prepared).
    pub fn stop(&self, sound_id: &str) {
        self.shared.stops.stop(sound_id);
        for o in lock(&self.shared.outputs).iter() {
            lock(&o.mixer).stop(sound_id);
        }
    }

    /// Stop everything immediately.
    pub fn stop_all(&self) {
        self.shared.stops.stop_all();
        for o in lock(&self.shared.outputs).iter() {
            lock(&o.mixer).stop_all();
        }
    }

    /// Change the gain of the instances of a sound that are already playing.
    pub fn set_sound_gain(&self, sound_id: &str, gain: f32) {
        for o in lock(&self.shared.outputs).iter() {
            lock(&o.mixer).set_sound_gain(sound_id, gain);
        }
    }

    pub fn levels(&self) -> Levels {
        *lock(&self.shared.levels)
    }

    /// Set master, headphones and microphone volume and the microphone mute in one go.
    pub fn set_levels(&self, levels: Levels) {
        let levels = levels.sanitized();
        *lock(&self.shared.levels) = levels;
        apply_levels(&self.shared, levels);
    }

    pub fn set_master_volume(&self, volume: f32) {
        let mut l = self.levels();
        l.master = volume;
        self.set_levels(l);
    }

    pub fn set_mic_muted(&self, muted: bool) {
        let mut l = self.levels();
        l.mic_muted = muted;
        self.set_levels(l);
    }

    /// Toggle the microphone route mute; returns the new state (true = muted).
    pub fn toggle_mic_muted(&self) -> bool {
        let muted = {
            let mut l = lock(&self.shared.levels);
            l.mic_muted = !l.mic_muted;
            *l
        };
        apply_levels(&self.shared, muted);
        muted.mic_muted
    }

    /// What an output is doing right now.
    pub fn route_state(&self, role: OutputRole) -> RouteState {
        lock(&self.shared.status)[idx(role)].clone()
    }

    /// What the optional microphone input is doing right now.
    pub fn voice_state(&self) -> RouteState {
        lock(&self.shared.voice_status).clone()
    }

    /// Playback and recording devices as last listed by the worker (never blocks on the hardware).
    pub fn devices(&self) -> DeviceList {
        lock(&self.shared.devices).clone()
    }

    /// Play a short test tone on one output. If the output is not active, the reason is reported.
    pub fn test_tone(&self, role: OutputRole) {
        let target = lock(&self.shared.outputs)
            .iter()
            .find(|o| o.role == role)
            .map(|o| (o.rate, Arc::clone(&o.mixer)));
        match target {
            Some((rate, mixer)) => {
                let data: Arc<[f32]> = Arc::from(make_test_tone(rate, 0.8, 0.35));
                let id = match role {
                    OutputRole::Main => "__test_tone_main",
                    OutputRole::Secondary => "__test_tone_secondary",
                };
                lock(&mixer).play(id, data, 1.0, RetriggerPolicy::Restart);
            }
            None => {
                let why = self.route_state(role).describe();
                let _ = self.etx.send(EngineEvent::Error {
                    sound_id: None,
                    message: format!("{} is not active: {why}", role.label()),
                });
            }
        }
    }

    /// Check that sound really comes out of the microphone route: a test tone is played on the
    /// route and TapTone listens on the recording device that belongs to it (for a virtual cable,
    /// its "Output" side). The result arrives as `EngineEvent::RouteTest`. Only the cable's own
    /// recording side is opened; the physical microphone is never touched.
    pub fn test_route(&self) {
        let report = |level: ReportLevel, message: String| {
            let _ = self
                .etx
                .send(EngineEvent::RouteTest(RouteTestReport { level, message }));
        };
        let device = match self.route_state(OutputRole::Secondary) {
            RouteState::Active { device, .. } => device,
            other => {
                report(
                    ReportLevel::Failure,
                    format!("The microphone route is not active: {}", other.describe()),
                );
                return;
            }
        };
        let levels = self.levels();
        if levels.mic_muted {
            report(
                ReportLevel::Warning,
                "The microphone route is muted. Unmute it and test again.".to_string(),
            );
            return;
        }
        if levels.mic <= 0.0 || levels.master <= 0.0 {
            report(
                ReportLevel::Warning,
                "The microphone volume is at 0%. Raise it and test again.".to_string(),
            );
            return;
        }
        let shared = Arc::clone(&self.shared);
        let etx = self.etx.clone();
        let spawned = std::thread::Builder::new()
            .name("taptone-route-test".to_string())
            .spawn(move || {
                let outcome = capture_while_playing(&shared, &device);
                let _ = etx.send(EngineEvent::RouteTest(analyze_route_test(&device, &outcome)));
            });
        if let Err(e) = spawned {
            report(ReportLevel::Failure, format!("Could not start the test: {e}"));
        }
    }

    /// Stop the worker and release the audio devices.
    pub fn shutdown(&self) {
        let _ = self.tx.send(Cmd::Shutdown);
    }

    /// Drain all pending notifications.
    pub fn poll_events(&self) -> Vec<EngineEvent> {
        self.events.try_iter().collect()
    }

    pub fn is_playing(&self, sound_id: &str) -> bool {
        lock(&self.shared.outputs)
            .iter()
            .any(|o| lock(&o.mixer).is_playing(sound_id))
    }

    /// `(position, total)` in seconds of the most recent instance of a sound.
    pub fn progress(&self, sound_id: &str) -> Option<(f64, f64)> {
        let outs = lock(&self.shared.outputs);
        outs.iter().find_map(|o| {
            lock(&o.mixer)
                .progress(sound_id)
                .map(|(pos, total)| (pos as f64 / o.rate as f64, total as f64 / o.rate as f64))
        })
    }

    /// Number of sounds currently sounding.
    pub fn voice_count(&self) -> usize {
        lock(&self.shared.outputs)
            .iter()
            .map(|o| lock(&o.mixer).voice_count())
            .max()
            .unwrap_or(0)
    }

    /// True if the output of this role is open.
    pub fn has_output(&self, role: OutputRole) -> bool {
        lock(&self.shared.outputs).iter().any(|o| o.role == role)
    }
}

// ---- Route test --------------------------------------------------------------------------------

fn build_input<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    peak: Arc<AtomicU32>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mut m = 0.0f32;
                for &s in data {
                    let v: f32 = f32::from_sample(s);
                    m = m.max(v.abs());
                }
                if m > f32::from_bits(peak.load(Ordering::Relaxed)) {
                    peak.store(m.to_bits(), Ordering::Relaxed);
                }
            },
            |e| tracing::debug!("route test capture error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

/// Play a tone on the microphone route and measure what arrives on the paired recording device.
fn capture_while_playing(shared: &Shared, output_name: &str) -> CaptureOutcome {
    let host = cpal::default_host();
    let inputs: Vec<(String, cpal::Device)> = match host.input_devices() {
        Ok(it) => it.filter_map(|d| d.name().ok().map(|n| (n, d))).collect(),
        Err(_) => Vec::new(),
    };
    let names: Vec<String> = inputs.iter().map(|(n, _)| n.clone()).collect();
    let Some(input_name) = find_paired_input(output_name, &names) else {
        // Still play the tone so the user can at least hear that the route works.
        play_route_tone(shared);
        return CaptureOutcome::NoPairedInput;
    };
    let Some(device) = inputs.into_iter().find(|(n, _)| *n == input_name).map(|(_, d)| d) else {
        return CaptureOutcome::NoPairedInput;
    };
    let failed = |error: String| CaptureOutcome::CaptureFailed {
        input: input_name.clone(),
        error,
    };

    let supported = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => return failed(e.to_string()),
    };
    let config = supported.config();
    let peak = Arc::new(AtomicU32::new(0));
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_input::<f32>(&device, &config, Arc::clone(&peak)),
        cpal::SampleFormat::I16 => build_input::<i16>(&device, &config, Arc::clone(&peak)),
        cpal::SampleFormat::U16 => build_input::<u16>(&device, &config, Arc::clone(&peak)),
        cpal::SampleFormat::I32 => build_input::<i32>(&device, &config, Arc::clone(&peak)),
        other => Err(format!("unsupported sample format {other:?}")),
    };
    let stream = match stream {
        Ok(s) => s,
        Err(e) => return failed(e),
    };
    if let Err(e) = stream.play() {
        return failed(e.to_string());
    }
    std::thread::sleep(Duration::from_millis(250));
    peak.store(0, Ordering::Relaxed);
    if !play_route_tone(shared) {
        return failed("the microphone route was closed during the test".to_string());
    }
    std::thread::sleep(Duration::from_millis(1400));
    drop(stream);
    CaptureOutcome::Measured {
        input: input_name.clone(),
        peak: f32::from_bits(peak.load(Ordering::Relaxed)),
    }
}

fn play_route_tone(shared: &Shared) -> bool {
    let outs = lock(&shared.outputs);
    match outs.iter().find(|o| o.role == OutputRole::Secondary) {
        Some(o) => {
            let data: Arc<[f32]> = Arc::from(make_test_tone(o.rate, 0.8, 0.3));
            lock(&o.mixer).play("__route_test", data, 1.0, RetriggerPolicy::Restart);
            true
        }
        None => false,
    }
}

// ---- Worker ------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Want {
    Off,
    Default,
    Named(String),
}

struct Slot {
    want: Want,
    stream: Option<cpal::Stream>,
    generation: u64,
    missing_scans: u32,
    next_retry: Instant,
}

impl Slot {
    fn new() -> Self {
        Slot {
            want: Want::Off,
            stream: None,
            generation: 0,
            missing_scans: 0,
            next_retry: Instant::now(),
        }
    }
}

struct Worker {
    evt: Sender<EngineEvent>,
    cmd_tx: Sender<Cmd>,
    shared: Arc<Shared>,
    slots: [Slot; 2],
    voice: VoiceSlot,
    cache: AudioCache,
    generation: u64,
    last_scan: Instant,
    known_outputs: Option<Vec<String>>,
    known_inputs: Option<Vec<String>>,
}

/// The optional microphone capture.
struct VoiceSlot {
    want: Want,
    stream: Option<cpal::Stream>,
    /// Name of the device that is open.
    device: Option<String>,
    generation: u64,
    missing_scans: u32,
    next_retry: Instant,
}

impl VoiceSlot {
    fn new() -> Self {
        VoiceSlot {
            want: Want::Off,
            stream: None,
            device: None,
            generation: 0,
            missing_scans: 0,
            next_retry: Instant::now(),
        }
    }
}

fn find_output(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    host.output_devices()
        .ok()
        .and_then(|mut it| it.find(|d| d.name().map(|n| n == name).unwrap_or(false)))
}

fn find_input(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    host.input_devices()
        .ok()
        .and_then(|mut it| it.find(|d| d.name().map(|n| n == name).unwrap_or(false)))
}

impl Worker {
    fn new(evt: Sender<EngineEvent>, cmd_tx: Sender<Cmd>, shared: Arc<Shared>) -> Self {
        Worker {
            evt,
            cmd_tx,
            shared,
            slots: [Slot::new(), Slot::new()],
            voice: VoiceSlot::new(),
            cache: AudioCache::new(DEFAULT_CACHE_BYTES),
            generation: 0,
            last_scan: Instant::now(),
            known_outputs: None,
            known_inputs: None,
        }
    }

    fn error(&self, sound_id: Option<String>, message: String) {
        tracing::warn!("audio: {message}");
        let _ = self.evt.send(EngineEvent::Error { sound_id, message });
    }

    fn state(&self, role: OutputRole) -> RouteState {
        lock(&self.shared.status)[idx(role)].clone()
    }

    /// Store a new state; returns true if it differs from the previous one.
    fn transition(&self, role: OutputRole, new: RouteState) -> bool {
        let mut st = lock(&self.shared.status);
        let changed = st[idx(role)] != new;
        st[idx(role)] = new;
        changed
    }

    /// Returns false when the worker should exit.
    fn handle(&mut self, cmd: Cmd) -> bool {
        match cmd {
            Cmd::Configure(cfg) => self.configure(&cfg),
            Cmd::Play(p) => self.play(p),
            Cmd::Preload { sound_id, path } => {
                if let Err(message) = self.decoded_for(&sound_id, &path) {
                    self.error(Some(sound_id), message);
                }
            }
            Cmd::Forget(path) => self.cache.forget(&path),
            Cmd::Rescan => {
                self.scan();
                self.last_scan = Instant::now();
            }
            Cmd::StreamFailed {
                role,
                generation,
                message,
            } => self.stream_failed(role, generation, message),
            Cmd::VoiceFailed { generation, message } => self.voice_failed(generation, message),
            Cmd::Shutdown => return false,
        }
        true
    }

    // ---- Devices -------------------------------------------------------------------------------

    fn scan(&mut self) {
        let outputs = list_devices(DeviceKind::Output);
        let inputs = list_devices(DeviceKind::Input);
        let default_output = default_output_name();
        let default_input = default_input_name();
        let mut d = lock(&self.shared.devices);
        match &outputs {
            Ok(list) => {
                d.outputs = list.clone();
                d.output_error = None;
            }
            Err(e) => {
                tracing::warn!("could not list playback devices: {e}");
                d.output_error = Some(e.clone());
            }
        }
        match &inputs {
            Ok(list) => {
                d.inputs = list.clone();
                d.input_error = None;
            }
            Err(e) => {
                tracing::warn!("could not list recording devices: {e}");
                d.input_error = Some(e.clone());
            }
        }
        d.default_output = default_output;
        d.default_input = default_input;
        d.scanned = true;
        drop(d);
        if let Ok(list) = outputs {
            self.known_outputs = Some(list);
        }
        if let Ok(list) = inputs {
            self.known_inputs = Some(list);
        }
    }

    fn active_name(&self, role: OutputRole) -> Option<String> {
        lock(&self.shared.outputs)
            .iter()
            .find(|o| o.role == role)
            .map(|o| o.name.clone())
    }

    fn close_slot(&mut self, role: OutputRole) {
        // Remove the output first so nobody feeds a mixer whose stream is being torn down.
        lock(&self.shared.outputs).retain(|o| o.role != role);
        self.slots[idx(role)].stream = None;
    }

    fn close_all(&mut self) {
        self.close_voice();
        for role in ROLES {
            self.close_slot(role);
        }
    }

    fn healthy(&self, role: OutputRole) -> bool {
        self.slots[idx(role)].stream.is_some()
            && matches!(
                self.state(role),
                RouteState::Active {
                    fallback_from: None,
                    ..
                }
            )
    }

    fn configure(&mut self, cfg: &DeviceConfig) {
        let wants = [
            match &cfg.main {
                Some(n) => Want::Named(n.clone()),
                None => Want::Default,
            },
            if !cfg.secondary_enabled {
                Want::Off
            } else {
                match &cfg.secondary {
                    Some(n) => Want::Named(n.clone()),
                    None => Want::Default,
                }
            },
        ];
        for role in ROLES {
            let i = idx(role);
            if self.healthy(role) && self.slots[i].want == wants[i] {
                continue; // unchanged and working: do not interrupt playing sounds
            }
            self.slots[i].want = wants[i].clone();
            self.open_slot(role);
        }
        self.check_conflict();

        let voice_want = if !cfg.voice_enabled {
            Want::Off
        } else {
            match &cfg.voice {
                Some(n) => Want::Named(n.clone()),
                None => Want::Default,
            }
        };
        if !(self.voice_healthy() && self.voice.want == voice_want) {
            self.voice.want = voice_want;
            self.open_voice();
        }
    }

    /// The microphone route must not be the same device as the headphones: every sound would be
    /// played twice into one device.
    fn check_conflict(&mut self) {
        if let (Some(a), Some(b)) = (
            self.active_name(OutputRole::Main),
            self.active_name(OutputRole::Secondary),
        ) {
            if route_output_conflict(Some(&a), &b) {
                self.close_slot(OutputRole::Secondary);
                if self.transition(OutputRole::Secondary, RouteState::Conflict { device: b.clone() }) {
                    self.error(None, RouteState::Conflict { device: b }.describe());
                }
            }
        }
    }

    fn open_slot(&mut self, role: OutputRole) {
        let i = idx(role);
        self.close_slot(role);
        self.slots[i].missing_scans = 0;
        self.slots[i].next_retry = Instant::now() + RECOVER_INTERVAL;
        let want = self.slots[i].want.clone();

        match (&want, role) {
            (Want::Off, _) => {
                self.transition(role, RouteState::Disabled);
                return;
            }
            (Want::Default, OutputRole::Secondary) => {
                self.transition(role, RouteState::NotSelected);
                return;
            }
            _ => {}
        }

        let host = cpal::default_host();
        let mut fallback_from: Option<String> = None;
        let device = match &want {
            Want::Named(name) => match find_output(&host, name) {
                Some(d) => Some(d),
                // Headphones: better to play on the default device (and say so) than to be silent.
                // The microphone route must never fall back: it would play into the speakers.
                None if role == OutputRole::Main => {
                    fallback_from = Some(name.clone());
                    host.default_output_device()
                }
                None => None,
            },
            _ => host.default_output_device(),
        };
        let Some(device) = device else {
            let state = match (&want, role) {
                (Want::Named(n), OutputRole::Secondary) => RouteState::Missing { wanted: n.clone() },
                (Want::Named(n), OutputRole::Main) => RouteState::Missing { wanted: n.clone() },
                _ => RouteState::Failed {
                    reason: "no audio output device is available".to_string(),
                },
            };
            if self.transition(role, state.clone()) {
                self.error(None, format!("{}: {}", role.label(), state.describe()));
            }
            return;
        };
        let name = device.name().unwrap_or_else(|_| "Unknown device".to_string());

        if role == OutputRole::Secondary && route_output_conflict(self.active_name(OutputRole::Main).as_deref(), &name)
        {
            if self.transition(role, RouteState::Conflict { device: name.clone() }) {
                self.error(None, RouteState::Conflict { device: name }.describe());
            }
            return;
        }

        match self.build_output(role, &device, name.clone()) {
            Ok((output, stream, generation)) => {
                let state = RouteState::Active {
                    device: name.clone(),
                    rate: output.rate,
                    channels: 2,
                    fallback_from: fallback_from.clone(),
                };
                lock(&self.shared.outputs).push(output);
                if role == OutputRole::Secondary {
                    // A new mixer: hook the microphone (if it is running) into it again.
                    self.attach_voice();
                }
                self.slots[i].stream = Some(stream);
                self.slots[i].generation = generation;
                self.transition(role, state);
                if let Some(wanted) = fallback_from {
                    let _ = self.evt.send(EngineEvent::FellBack {
                        role,
                        wanted,
                        using: name,
                    });
                }
            }
            Err(reason) => {
                let state = RouteState::Failed { reason: reason.clone() };
                if self.transition(role, state) {
                    self.error(None, format!("{}: {reason}", role.label()));
                }
            }
        }
    }

    fn build_output(
        &mut self,
        role: OutputRole,
        device: &cpal::Device,
        name: String,
    ) -> Result<(Output, cpal::Stream, u64), String> {
        let supported = device
            .default_output_config()
            .map_err(|e| format!("cannot read the format of \"{name}\": {e}"))?;
        let rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        let format = supported.sample_format();
        let config = supported.config();

        self.generation += 1;
        let generation = self.generation;

        let levels = *lock(&self.shared.levels);
        let mut mixer = Mixer::new();
        mixer.set_master_volume(levels.master);
        mixer.snap_output_gain(levels.route_gain(role));
        let mixer = Arc::new(Mutex::new(mixer));

        let fail_tx = self.cmd_tx.clone();
        let on_error = move |e: cpal::StreamError| {
            tracing::warn!("audio stream error on {role:?}: {e}");
            let _ = fail_tx.send(Cmd::StreamFailed {
                role,
                generation,
                message: e.to_string(),
            });
        };

        let stream = match format {
            cpal::SampleFormat::F32 => build_stream::<f32, _>(device, &config, channels, Arc::clone(&mixer), on_error),
            cpal::SampleFormat::I16 => build_stream::<i16, _>(device, &config, channels, Arc::clone(&mixer), on_error),
            cpal::SampleFormat::U16 => build_stream::<u16, _>(device, &config, channels, Arc::clone(&mixer), on_error),
            cpal::SampleFormat::I32 => build_stream::<i32, _>(device, &config, channels, Arc::clone(&mixer), on_error),
            cpal::SampleFormat::U32 => build_stream::<u32, _>(device, &config, channels, Arc::clone(&mixer), on_error),
            cpal::SampleFormat::I8 => build_stream::<i8, _>(device, &config, channels, Arc::clone(&mixer), on_error),
            cpal::SampleFormat::U8 => build_stream::<u8, _>(device, &config, channels, Arc::clone(&mixer), on_error),
            cpal::SampleFormat::F64 => build_stream::<f64, _>(device, &config, channels, Arc::clone(&mixer), on_error),
            other => Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| format!("cannot open \"{name}\": {e}"))?;
        stream.play().map_err(|e| format!("cannot start \"{name}\": {e}"))?;

        tracing::info!("opened {role:?} output \"{name}\" at {rate} Hz, {channels} ch");
        Ok((
            Output {
                role,
                name,
                rate,
                mixer,
            },
            stream,
            generation,
        ))
    }

    fn stream_failed(&mut self, role: OutputRole, generation: u64, message: String) {
        let i = idx(role);
        if self.slots[i].stream.is_none() || self.slots[i].generation != generation {
            return; // an old stream, already replaced
        }
        let device = self.active_name(role).unwrap_or_else(|| "the audio device".to_string());
        self.mark_lost(role, device, message);
    }

    fn mark_lost(&mut self, role: OutputRole, device: String, reason: String) {
        self.close_slot(role);
        self.slots[idx(role)].next_retry = Instant::now() + RECOVER_INTERVAL;
        self.transition(
            role,
            RouteState::Lost {
                device: device.clone(),
                reason: reason.clone(),
            },
        );
        let _ = self.evt.send(EngineEvent::DeviceLost { role, device, reason });
    }

    /// Periodic work: refresh the device list, notice unplugged devices, reconnect.
    fn tick(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_scan) >= SCAN_INTERVAL {
            self.scan();
            self.last_scan = now;
            self.watch_devices();
            self.watch_voice();
        }
        for role in ROLES {
            self.try_recover(role, now);
        }
        self.tick_voice(now);
    }

    /// Some drivers do not report an unplugged device through the stream error callback, so the
    /// device list is checked as well. Two scans in a row must miss the device.
    fn watch_devices(&mut self) {
        let Some(known_outputs) = self.known_outputs.clone() else {
            return;
        };
        for role in ROLES {
            let i = idx(role);
            if self.slots[i].stream.is_none() {
                continue;
            }
            let Some(name) = self.active_name(role) else { continue };
            if known_outputs.contains(&name) {
                self.slots[i].missing_scans = 0;
            } else {
                self.slots[i].missing_scans += 1;
                if self.slots[i].missing_scans >= MISSING_SCANS_BEFORE_LOST {
                    self.mark_lost(role, name, "the device was removed".to_string());
                }
            }
        }
    }

    fn try_recover(&mut self, role: OutputRole, now: Instant) {
        let i = idx(role);
        if now < self.slots[i].next_retry {
            return;
        }
        let state = self.state(role);
        let retry = match &state {
            RouteState::Missing { .. } | RouteState::Lost { .. } | RouteState::Failed { .. } => true,
            // Headphones on the default device because the wanted one was missing: go back as
            // soon as the wanted device is listed again.
            RouteState::Active {
                fallback_from: Some(wanted),
                ..
            } => self.known_outputs.as_ref().is_some_and(|l| l.contains(wanted)),
            _ => false,
        };
        if !retry {
            return;
        }
        // Do not hammer a device that is not even listed.
        if let (Want::Named(n), Some(list)) = (&self.slots[i].want, &self.known_outputs) {
            if !list.contains(n) && !matches!(state, RouteState::Failed { .. }) && role == OutputRole::Secondary {
                self.slots[i].next_retry = now + RECOVER_INTERVAL;
                return;
            }
        }
        self.open_slot(role);
        if role == OutputRole::Secondary {
            self.check_conflict();
        }
        if let RouteState::Active {
            device,
            fallback_from: None,
            ..
        } = self.state(role)
        {
            let _ = self.evt.send(EngineEvent::Recovered { role, device });
        }
    }

    // ---- Voice input (optional microphone) -----------------------------------------------------

    fn voice_state(&self) -> RouteState {
        lock(&self.shared.voice_status).clone()
    }

    /// Store a new voice state; returns true if it differs from the previous one.
    fn set_voice_state(&self, new: RouteState) -> bool {
        let mut st = lock(&self.shared.voice_status);
        let changed = *st != new;
        *st = new;
        changed
    }

    fn voice_healthy(&self) -> bool {
        self.voice.stream.is_some() && matches!(self.voice_state(), RouteState::Active { .. })
    }

    fn voice_standby(&self) -> RouteState {
        RouteState::Standby {
            reason: "Waiting for the microphone route to start.".to_string(),
        }
    }

    /// Stop capturing and take the microphone out of the route's mixer.
    fn close_voice(&mut self) {
        for o in lock(&self.shared.outputs).iter() {
            lock(&o.mixer).detach_live();
        }
        *lock(&self.shared.voice_live) = None;
        self.voice.stream = None;
        self.voice.device = None;
    }

    /// Hook the running microphone into the microphone route's mixer (never into the headphones).
    fn attach_voice(&self) {
        let Some(live) = lock(&self.shared.voice_live).clone() else {
            return;
        };
        for o in lock(&self.shared.outputs)
            .iter()
            .filter(|o| o.role == OutputRole::Secondary)
        {
            lock(&o.mixer).attach_live(Arc::clone(&live), o.rate);
        }
    }

    fn open_voice(&mut self) {
        self.close_voice();
        self.voice.missing_scans = 0;
        self.voice.next_retry = Instant::now() + RECOVER_INTERVAL;
        let want = self.voice.want.clone();
        if want == Want::Off {
            self.set_voice_state(RouteState::Disabled);
            return;
        }
        // The microphone is only useful (and only opened) while the route it feeds is running.
        let Some(route_device) = self.active_name(OutputRole::Secondary) else {
            self.set_voice_state(self.voice_standby());
            return;
        };

        let host = cpal::default_host();
        let device = match &want {
            Want::Named(name) => find_input(&host, name),
            _ => host.default_input_device(),
        };
        let Some(device) = device else {
            let state = match &want {
                Want::Named(n) => RouteState::Missing { wanted: n.clone() },
                _ => RouteState::Failed {
                    reason: "no recording device (microphone) was found".to_string(),
                },
            };
            if self.set_voice_state(state.clone()) {
                self.error(None, format!("Your microphone: {}", state.describe()));
            }
            return;
        };
        let name = device.name().unwrap_or_else(|_| "Unknown device".to_string());

        if voice_feedback_conflict(&route_device, &name) {
            let state = RouteState::Failed {
                reason: format!(
                    "\"{name}\" is the recording side of the microphone route, so using it would feed the sound back \
                     into itself. Choose your real microphone."
                ),
            };
            if self.set_voice_state(state.clone()) {
                self.error(None, format!("Your microphone: {}", state.describe()));
            }
            return;
        }

        match self.build_voice(&device, &name) {
            Ok((stream, live, generation, rate, channels)) => {
                *lock(&self.shared.voice_live) = Some(live);
                self.voice.stream = Some(stream);
                self.voice.device = Some(name.clone());
                self.voice.generation = generation;
                self.attach_voice();
                self.set_voice_state(RouteState::Active {
                    device: name,
                    rate,
                    channels,
                    fallback_from: None,
                });
            }
            Err(reason) => {
                let state = RouteState::Failed {
                    reason: add_input_hint(&reason),
                };
                if self.set_voice_state(state.clone()) {
                    self.error(None, format!("Your microphone: {}", state.describe()));
                }
            }
        }
    }

    #[allow(clippy::type_complexity)]
    fn build_voice(
        &mut self,
        device: &cpal::Device,
        name: &str,
    ) -> Result<(cpal::Stream, Arc<LiveInput>, u64, u32, u16), String> {
        let supported = device
            .default_input_config()
            .map_err(|e| format!("cannot read the format of \"{name}\": {e}"))?;
        let rate = supported.sample_rate().0;
        let channels = supported.channels();
        let format = supported.sample_format();
        let config = supported.config();

        self.generation += 1;
        let generation = self.generation;

        let live = Arc::new(LiveInput::new(rate));
        let voice_level = lock(&self.shared.levels).voice;
        live.set_gain(voice_level);

        let fail_tx = self.cmd_tx.clone();
        let on_error = move |e: cpal::StreamError| {
            tracing::warn!("audio stream error on the microphone input: {e}");
            let _ = fail_tx.send(Cmd::VoiceFailed {
                generation,
                message: e.to_string(),
            });
        };
        let ch = channels as usize;
        let stream = match format {
            cpal::SampleFormat::F32 => build_voice_stream::<f32, _>(device, &config, ch, Arc::clone(&live), on_error),
            cpal::SampleFormat::I16 => build_voice_stream::<i16, _>(device, &config, ch, Arc::clone(&live), on_error),
            cpal::SampleFormat::U16 => build_voice_stream::<u16, _>(device, &config, ch, Arc::clone(&live), on_error),
            cpal::SampleFormat::I32 => build_voice_stream::<i32, _>(device, &config, ch, Arc::clone(&live), on_error),
            other => Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| format!("cannot open \"{name}\": {e}"))?;
        stream.play().map_err(|e| format!("cannot start \"{name}\": {e}"))?;

        tracing::info!("opened microphone input \"{name}\" at {rate} Hz, {channels} ch");
        Ok((stream, live, generation, rate, channels))
    }

    fn mark_voice_lost(&mut self, device: String, reason: String) {
        self.close_voice();
        self.voice.next_retry = Instant::now() + RECOVER_INTERVAL;
        self.set_voice_state(RouteState::Lost {
            device: device.clone(),
            reason: reason.clone(),
        });
        let _ = self.evt.send(EngineEvent::VoiceLost { device, reason });
    }

    fn voice_failed(&mut self, generation: u64, message: String) {
        if self.voice.stream.is_none() || self.voice.generation != generation {
            return; // an old stream, already replaced
        }
        let device = self
            .voice
            .device
            .clone()
            .unwrap_or_else(|| "the microphone".to_string());
        self.mark_voice_lost(device, message);
    }

    /// Some drivers do not report an unplugged microphone through the stream error callback, so the
    /// list of recording devices is checked as well. Two scans in a row must miss the device.
    fn watch_voice(&mut self) {
        if self.voice.stream.is_none() {
            return;
        }
        let (Some(name), Some(list)) = (self.voice.device.clone(), self.known_inputs.clone()) else {
            return;
        };
        if list.contains(&name) {
            self.voice.missing_scans = 0;
        } else {
            self.voice.missing_scans += 1;
            if self.voice.missing_scans >= MISSING_SCANS_BEFORE_LOST {
                self.mark_voice_lost(name, "the device was removed".to_string());
            }
        }
    }

    /// Start the microphone when the route is up, stop it when the route goes away, retry after a
    /// loss, and never keep a feedback loop running.
    fn tick_voice(&mut self, now: Instant) {
        if self.voice.want == Want::Off {
            return;
        }
        let route = self.active_name(OutputRole::Secondary);
        let running = self.voice.stream.is_some();
        match (route, running) {
            (None, true) => {
                self.close_voice();
                self.set_voice_state(self.voice_standby());
            }
            (None, false) => {
                self.set_voice_state(self.voice_standby());
            }
            (Some(route_device), true) => {
                let feedback = self
                    .voice
                    .device
                    .as_deref()
                    .is_some_and(|mic| voice_feedback_conflict(&route_device, mic));
                if feedback {
                    self.open_voice();
                }
            }
            (Some(_), false) => {
                let waiting = matches!(self.voice_state(), RouteState::Standby { .. });
                if !waiting && now < self.voice.next_retry {
                    return;
                }
                let before = self.voice_state();
                // Do not hammer a named microphone that is not even listed.
                if let (Want::Named(n), Some(list)) = (&self.voice.want, &self.known_inputs) {
                    if !list.contains(n) && matches!(before, RouteState::Missing { .. }) {
                        self.voice.next_retry = now + RECOVER_INTERVAL;
                        return;
                    }
                }
                self.open_voice();
                if let RouteState::Active { device, .. } = self.voice_state() {
                    if matches!(before, RouteState::Lost { .. } | RouteState::Missing { .. }) {
                        let _ = self.evt.send(EngineEvent::VoiceRecovered { device });
                    }
                }
            }
        }
    }

    // ---- Playback ------------------------------------------------------------------------------

    fn decoded_for(&mut self, sound_id: &str, path: &Path) -> Result<Arc<DecodedAudio>, String> {
        if let Some(d) = self.cache.get_decoded(path) {
            return Ok(d);
        }
        let audio = Arc::new(decode_file(path)?);
        self.cache.insert_decoded(path, Arc::clone(&audio));
        let _ = self.evt.send(EngineEvent::Decoded {
            sound_id: sound_id.to_string(),
            duration_secs: audio.duration_secs(),
        });
        Ok(audio)
    }

    fn resampled_for(&mut self, path: &Path, audio: &DecodedAudio, rate: u32) -> Arc<[f32]> {
        if audio.rate == rate {
            return Arc::clone(&audio.samples); // no copy, shared with the decoded cache
        }
        if let Some(d) = self.cache.get_resampled(path, rate) {
            return d;
        }
        let data: Arc<[f32]> = Arc::from(resample_stereo(&audio.samples, audio.rate, rate));
        self.cache.insert_resampled(path, rate, Arc::clone(&data));
        data
    }

    fn play(&mut self, cmd: PlayCmd) {
        let stops = &self.shared.stops;
        if stops.is_cancelled(&cmd.sound_id, cmd.ticket) {
            return;
        }
        let audio = match self.decoded_for(&cmd.sound_id, &cmd.path) {
            Ok(a) => a,
            Err(message) => {
                self.error(Some(cmd.sound_id.clone()), message);
                return;
            }
        };
        let outs: Vec<Output> = lock(&self.shared.outputs).clone();
        if outs.is_empty() {
            let why = self.state(OutputRole::Main).describe();
            self.error(
                Some(cmd.sound_id.clone()),
                format!("No audio output is open ({why}). Check Settings."),
            );
            return;
        }
        // Resample before taking any lock: this can take a while for long sounds.
        let prepared: Vec<(Output, Arc<[f32]>)> = outs
            .into_iter()
            .map(|o| {
                let data = self.resampled_for(&cmd.path, &audio, o.rate);
                (o, data)
            })
            .collect();

        // Check "cancelled" and start the voices under the outputs lock: `Engine::stop` takes the
        // same lock after recording its ticket, so a stop can never slip in between.
        let guard = lock(&self.shared.outputs);
        if self.shared.stops.is_cancelled(&cmd.sound_id, cmd.ticket) {
            return;
        }
        if !cmd.allow_overlap {
            for o in guard.iter() {
                lock(&o.mixer).stop_all();
            }
        }
        let mut limit_hit = false;
        for (out, data) in prepared {
            if lock(&out.mixer).play(&cmd.sound_id, data, cmd.gain, cmd.policy) == PlayOutcome::LimitReached {
                limit_hit = true;
            }
        }
        drop(guard);
        if limit_hit {
            self.error(Some(cmd.sound_id), "Too many sounds are playing at once.".to_string());
        }
    }
}

fn build_stream<T, E>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    mixer: Arc<Mutex<Mixer>>,
    on_error: E,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
    E: FnMut(cpal::StreamError) + Send + 'static,
{
    let channels = channels.max(1);
    let mut scratch: Vec<f32> = Vec::new();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let frames = data.len() / channels;
                let needed = frames * CHANNELS;
                if scratch.len() < needed {
                    scratch.resize(needed, 0.0);
                }
                let block = &mut scratch[..needed];
                lock(&mixer).mix(block);
                for (i, frame) in data.chunks_exact_mut(channels).enumerate() {
                    let l = block[i * 2];
                    let r = block[i * 2 + 1];
                    if channels == 1 {
                        frame[0] = T::from_sample((l + r) * 0.5);
                    } else {
                        frame[0] = T::from_sample(l);
                        frame[1] = T::from_sample(r);
                        for extra in frame[2..].iter_mut() {
                            *extra = T::EQUILIBRIUM;
                        }
                    }
                }
                // A trailing partial frame (never happens in practice) is silenced.
                for extra in data.chunks_exact_mut(channels).into_remainder() {
                    *extra = T::EQUILIBRIUM;
                }
            },
            on_error,
            None,
        )
        .map_err(|e| e.to_string())
}

/// Capture stream that converts whatever the device delivers into interleaved stereo `f32` and
/// hands it to the `LiveInput`. A mono microphone is copied to both channels; channels beyond the
/// first two are ignored.
fn build_voice_stream<T, E>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    live: Arc<LiveInput>,
    on_error: E,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
    E: FnMut(cpal::StreamError) + Send + 'static,
{
    let channels = channels.max(1);
    let mut scratch: Vec<f32> = Vec::with_capacity(8192);
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                scratch.clear();
                for frame in data.chunks_exact(channels) {
                    let l: f32 = f32::from_sample(frame[0]);
                    let r: f32 = if channels > 1 { f32::from_sample(frame[1]) } else { l };
                    scratch.push(l);
                    scratch.push(r);
                }
                live.push_stereo(&scratch);
            },
            on_error,
            None,
        )
        .map_err(|e| e.to_string())
}

// ---- Decoding and resampling -------------------------------------------------------------------

/// A fully decoded sound: interleaved stereo f32 at `rate` Hz.
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    pub samples: Arc<[f32]>,
    pub rate: u32,
}

impl DecodedAudio {
    pub fn duration_secs(&self) -> f64 {
        if self.rate == 0 {
            0.0
        } else {
            (self.samples.len() / CHANNELS) as f64 / self.rate as f64
        }
    }
}

/// Decode a WAV or MP3 file into stereo f32 samples.
pub fn decode_file(path: &Path) -> Result<DecodedAudio, String> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::errors::Error as SymError;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = File::open(path).map_err(|e| format!("Cannot open the file: {e}"))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("Unsupported or damaged audio file: {e}"))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| "The file contains no audio track".to_string())?;
    let track_id = track.id;
    let mut rate = track.codec_params.sample_rate.unwrap_or(0);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Unsupported audio format: {e}"))?;

    let mut out: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(format!("Decoding failed: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                let spec = *buf.spec();
                rate = spec.rate;
                let ch = spec.channels.count().max(1);
                let mut sbuf = SampleBuffer::<f32>::new(buf.capacity() as u64, spec);
                sbuf.copy_interleaved_ref(buf);
                let s = sbuf.samples();
                match ch {
                    1 => {
                        for &x in s {
                            out.push(x);
                            out.push(x);
                        }
                    }
                    2 => out.extend_from_slice(s),
                    n => {
                        // More than two channels: keep front left/right.
                        for frame in s.chunks_exact(n) {
                            out.push(frame[0]);
                            out.push(frame[1]);
                        }
                    }
                }
                if rate > 0 && (out.len() / CHANNELS) as f64 / rate as f64 > MAX_SOUND_SECS {
                    return Err(format!(
                        "The sound is longer than {} minutes, which is not supported",
                        (MAX_SOUND_SECS / 60.0) as u32
                    ));
                }
            }
            Err(SymError::DecodeError(e)) => {
                tracing::debug!("skipping a damaged packet: {e}");
                continue;
            }
            Err(SymError::IoError(_)) => break,
            Err(e) => return Err(format!("Decoding failed: {e}")),
        }
    }
    if out.len() < CHANNELS || rate == 0 {
        return Err("The file contains no audio data".to_string());
    }
    out.shrink_to_fit();
    Ok(DecodedAudio {
        samples: Arc::from(out),
        rate,
    })
}

/// Linear-interpolation resampler for interleaved stereo audio.
pub fn resample_stereo(src: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || from == 0 || to == 0 {
        return src.to_vec();
    }
    let in_frames = src.len() / CHANNELS;
    if in_frames == 0 {
        return Vec::new();
    }
    let out_frames = ((in_frames as f64 * to as f64 / from as f64).round() as usize).max(1);
    let step = from as f64 / to as f64;
    let mut out = Vec::with_capacity(out_frames * CHANNELS);
    for i in 0..out_frames {
        let pos = i as f64 * step;
        let i0 = (pos.floor() as usize).min(in_frames - 1);
        let i1 = (i0 + 1).min(in_frames - 1);
        let frac = (pos - i0 as f64) as f32;
        for ch in 0..CHANNELS {
            let a = src[i0 * CHANNELS + ch];
            let b = src[i1 * CHANNELS + ch];
            out.push(a + (b - a) * frac);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_same_rate_is_identity() {
        let v = vec![0.1, 0.2, 0.3, 0.4];
        assert_eq!(resample_stereo(&v, 48_000, 48_000), v);
    }

    #[test]
    fn resample_scales_length() {
        let v = vec![0.5f32; 44_100 * 2];
        let out = resample_stereo(&v, 44_100, 48_000);
        assert_eq!(out.len() % 2, 0);
        assert!((out.len() / 2).abs_diff(48_000) <= 1);
        assert!(out.iter().all(|s| (*s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn duration_is_computed_from_frames() {
        let a = DecodedAudio {
            samples: Arc::from(vec![0.0f32; 48_000 * 2]),
            rate: 48_000,
        };
        assert!((a.duration_secs() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_stop_cancels_earlier_plays_but_not_later_ones() {
        let s = Stops::default();
        let early = s.ticket();
        s.stop("a");
        let late = s.ticket();
        assert!(s.is_cancelled("a", early));
        assert!(!s.is_cancelled("a", late));
        assert!(!s.is_cancelled("b", early), "other sounds are not affected");
        let before_all = s.ticket();
        s.stop_all();
        let after_all = s.ticket();
        assert!(s.is_cancelled("b", before_all));
        assert!(s.is_cancelled("a", late));
        assert!(!s.is_cancelled("b", after_all));
    }

    #[test]
    fn levels_route_gain_and_mute() {
        let l = Levels {
            master: 1.0,
            main: 0.7,
            mic: 0.4,
            mic_muted: false,
            voice: 0.9,
        };
        assert_eq!(l.route_gain(OutputRole::Main), 0.7);
        assert_eq!(l.route_gain(OutputRole::Secondary), 0.4);
        let muted = Levels { mic_muted: true, ..l };
        assert_eq!(muted.route_gain(OutputRole::Secondary), 0.0);
        assert_eq!(
            muted.route_gain(OutputRole::Main),
            0.7,
            "muting the microphone route keeps the headphones"
        );
        let bad = Levels {
            master: f32::NAN,
            main: 5.0,
            mic: -1.0,
            mic_muted: false,
            voice: f32::INFINITY,
        }
        .sanitized();
        assert_eq!((bad.master, bad.main, bad.mic, bad.voice), (1.0, 1.0, 0.0, 1.0));
        assert_eq!(
            muted.voice, 0.9,
            "muting the microphone route does not change the voice level"
        );
    }

    #[test]
    fn device_lists_start_empty_and_unscanned() {
        let d = DeviceList::default();
        assert!(d.outputs.is_empty() && d.inputs.is_empty());
        assert!(d.output_error.is_none() && d.input_error.is_none());
        assert!(!d.scanned);
    }

    #[test]
    fn listing_devices_never_panics_without_audio_hardware() {
        // On a machine without audio (CI) the lists are empty or an error is returned: both are fine.
        // What must never happen is a panic or a made-up device.
        for kind in [DeviceKind::Output, DeviceKind::Input] {
            match list_devices(kind) {
                Ok(names) => {
                    let mut canonical = names.clone();
                    canonical.sort();
                    canonical.dedup();
                    assert_eq!(names, canonical, "lists are sorted and free of duplicates");
                }
                Err(e) => assert!(!e.is_empty(), "an error always carries a reason"),
            }
        }
        let _ = (
            output_device_names(),
            input_device_names(),
            default_output_name(),
            default_input_name(),
        );
    }

    #[test]
    fn voice_config_defaults_to_off() {
        let c = DeviceConfig::default();
        assert!(!c.voice_enabled);
        assert!(c.voice.is_none());
    }
}
