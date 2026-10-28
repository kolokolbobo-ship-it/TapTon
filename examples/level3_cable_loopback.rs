//! Level 3 verification: does the soundboard mix really arrive on a Windows capture endpoint?
//!
//! This tool uses the **virtual audio cable route already implemented by TapTone** (variant A):
//! it plays the TapTone engine test tone into the playback side of an installed virtual cable and
//! records the matching recording side with an **independent client** (a separate recorder
//! process), then analyses the recorded WAV.
//!
//! It performs no system changes, installs no drivers, and never opens the physical microphone
//! unless `--with-mic` is used. The physical microphone is only opened for the optional manual
//! mixed test, exactly like the app does it (shared mode capture via `LiveInput`).
//!
//! Modes:
//! * `cargo run --release --example level3_cable_loopback` — automated loopback check
//!   (tone only, no microphone, nothing on the speakers).
//! * `cargo run --release --example level3_cable_loopback -- --with-mic` — manual mixed test:
//!   the user speaks while the tone plays; the WAV is written to `build\logs` for review.
//! * `--record <seconds> <wav> <sidecar> <device>` — recorder child used internally
//!   (an independent client: a plain WASAPI capture with no TapTone code in the audio path).
//!
//! Exit codes: 0 = verified, 1 = signal analysis failed, 2 = route never became active,
//! 3 = no virtual cable found, 4 = recorder spawn failed, other codes from the recorder.

use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};

use taptone::audio::engine::{DeviceConfig, Engine, EngineEvent, Levels};
use taptone::audio::routing::DETECTION_THRESHOLD;
use taptone::audio::routing::{find_paired_input, is_virtual_device, peak_to_db, OutputRole, ReportLevel, RouteState};

const TONE_1: f32 = 523.25; // C5: first half of the engine test tone
const TONE_2: f32 = 783.99; // G5: second half of the engine test tone
const CTL_1: f32 = 200.0; // control bins: they must stay quiet
const CTL_2: f32 = 1500.0;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 5 && args[1] == "--record" {
        let secs: f64 = args[2].parse().unwrap_or(4.0);
        std::process::exit(record_with_cpal(
            secs,
            &args[3],
            &args[4],
            args.get(5).map(String::as_str),
        ));
    }
    let with_mic = args.iter().any(|a| a == "--with-mic");
    std::process::exit(if with_mic {
        manual_mixed_test()
    } else {
        automated_loopback_test()
    });
}

// ---- Shared setup ------------------------------------------------------------------------------

/// An installed virtual cable: (playback endpoint name, matching recording endpoint name).
fn find_cable() -> Option<(String, String)> {
    let host = cpal::default_host();
    let outputs: Vec<String> = host
        .output_devices()
        .ok()?
        .filter_map(|d| d.name().ok())
        .filter(|n| is_virtual_device(n))
        .collect();
    let inputs: Vec<String> = host.input_devices().ok()?.filter_map(|d| d.name().ok()).collect();
    outputs
        .iter()
        .filter_map(|o| find_paired_input(o, &inputs).map(|i| (o.clone(), i)))
        .next()
}

fn start_engine_route(output: &str, voice: Option<String>) -> Engine {
    Engine::start(
        DeviceConfig {
            main: None, // default headphones; this tool never plays a sound on the main route
            secondary: Some(output.to_string()),
            secondary_enabled: true,
            voice: voice.clone(),
            voice_enabled: voice.is_some(),
        },
        Levels::default(),
    )
}

/// A real (non-virtual) recording device for the voice mix. The system default input may itself
/// be the cable's recording side, so the tool picks the same real microphone the app's GUI list
/// would suggest first.
fn pick_real_microphone() -> Option<String> {
    let host = cpal::default_host();
    let inputs: Vec<String> = host.input_devices().ok()?.filter_map(|d| d.name().ok()).collect();
    taptone::audio::routing::sort_for_voice_input(&inputs)
        .into_iter()
        .find(|n| !is_virtual_device(n))
}

fn wait_active(engine: &Engine) -> Option<String> {
    for _ in 0..100 {
        match engine.route_state(OutputRole::Secondary) {
            RouteState::Active { device, .. } => return Some(device),
            _ => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    None
}

/// Waits until the voice input is no longer in its initial "Off"/"NotSelected" state, so the
/// microphone knows it is really opened (or really refused) before the recording starts.
fn wait_voice(engine: &Engine) -> RouteState {
    for _ in 0..100 {
        let state = engine.voice_state();
        if !matches!(state, RouteState::Disabled | RouteState::NotSelected) {
            return state;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    engine.voice_state()
}

fn spawn_recorder(secs: u64, wav: &str, sidecar: &str, device: &str) -> Result<std::process::Child, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Command::new(exe)
        .args([
            "--record".to_string(),
            secs.to_string(),
            wav.to_string(),
            sidecar.to_string(),
            device.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())
}

fn wait_route_test(engine: &Engine, timeout: Duration) -> Option<(ReportLevel, String)> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        for event in engine.poll_events() {
            if let EngineEvent::RouteTest(report) = event {
                return Some((report.level, report.message));
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

fn append_log(line: &str) {
    let path = std::path::Path::new("build").join("logs").join("cable_loopback.log");
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new("build")));
    if !path.exists() {
        let _ = std::fs::write(&path, b"# TapTone level 3 cable loopback log\n");
    }
    let _ = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, format!("[{}] {}\n", hhmmss(), line).as_bytes()));
}

/// `HH:MM:SS` (UTC) without pulling in a time crate; good enough to order log lines.
fn hhmmss() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{:02}:{:02}:{:02}", (secs / 3600) % 24, (secs / 60) % 60, secs % 60)
}

// ---- Recorder results --------------------------------------------------------------------------

fn read_sidecar(sidecar: &str) -> (u32, usize, usize) {
    let side = std::fs::read_to_string(sidecar).unwrap_or_default();
    let rate: u32 = side
        .lines()
        .find_map(|l| l.strip_prefix("rate=").and_then(|v| v.trim().parse().ok()))
        .unwrap_or(0);
    let channels: usize = side
        .lines()
        .find_map(|l| l.strip_prefix("channels=").and_then(|v| v.trim().parse().ok()))
        .unwrap_or(0);
    let frames: usize = side
        .lines()
        .find_map(|l| l.strip_prefix("frames=").and_then(|v| v.trim().parse().ok()))
        .unwrap_or(0);
    (rate, channels, frames)
}

// ---- Analysis ----------------------------------------------------------------------------------

fn mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 || channels == 0 {
        return samples.to_vec();
    }
    samples
        .chunks_exact(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Rough amplitude estimate of `freq` in `samples` (Goertzel over the whole buffer).
fn goertzel(samples: &[f32], rate: u32, freq: f32) -> f32 {
    let n = samples.len();
    if n < 64 || rate == 0 {
        return 0.0;
    }
    let k = ((n as f32 * freq / rate as f32).round() as usize).max(1);
    let w = 2.0 * std::f32::consts::PI * k as f32 / n as f32;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    for &x in samples {
        let s0 = x + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    (2.0 * power.sqrt()) / n as f32
}

fn window(data: &[f32], rate: u32, from: f64, to: f64) -> Vec<f32> {
    let len = data.len();
    let a = ((from * rate as f64) as usize).min(len);
    let b = ((to * rate as f64) as usize).min(len);
    if a >= b {
        return Vec::new();
    }
    data[a..b].to_vec()
}

#[derive(Debug, Clone, Copy)]
struct Analysis {
    frames: usize,
    rate: u32,
    channels: usize,
    peak: f32,
    tone1: f32,
    tone2: f32,
    ctl1: f32,
    ctl2: f32,
    voice_peak: f32,
    voice_rms: f32,
}

/// Parses a 16-bit PCM WAV written by this tool and analyses the requested windows.
fn analyze(
    wav_bytes: &[u8],
    expected_rate: u32,
    expected_channels: usize,
    tone_from: f64,
    tone_to: f64,
    voice_from: f64,
    voice_to: f64,
) -> Option<Analysis> {
    if wav_bytes.len() < 44
        || &wav_bytes[0..4] != b"RIFF"
        || &wav_bytes[8..12] != b"WAVE"
        || &wav_bytes[12..16] != b"fmt "
    {
        return None;
    }
    let mut pos = 12;
    let mut valid_format = false;
    let mut data_pos: Option<usize> = None;
    while pos + 8 <= wav_bytes.len() {
        let id = &wav_bytes[pos..pos + 4];
        let size = u32::from_le_bytes([
            wav_bytes[pos + 4],
            wav_bytes[pos + 5],
            wav_bytes[pos + 6],
            wav_bytes[pos + 7],
        ]) as usize;
        if id == b"fmt " {
            if let Some(chunk) = wav_bytes.get(pos + 8..pos + 8 + 16) {
                let format_tag = u16::from_le_bytes([chunk[0], chunk[1]]);
                valid_format = format_tag == 1 && u16::from_le_bytes([chunk[14], chunk[15]]) == 16;
            }
        } else if id == b"data" {
            data_pos = Some(pos + 8);
            break;
        }
        pos += 8 + size + (size & 1);
    }
    let data_pos = data_pos?;
    if !valid_format {
        return None;
    }
    let data = &wav_bytes[data_pos.min(wav_bytes.len())..];
    let samples: Vec<f32> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
        .collect();
    let mon = mono(&samples, expected_channels.max(1));
    let peak = mon.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    let tone = window(&mon, expected_rate, tone_from, tone_to);
    let voice = window(&mon, expected_rate, voice_from, voice_to);
    let voice_peak = voice.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    let voice_rms = if voice.is_empty() {
        0.0
    } else {
        (voice.iter().map(|s| s * s).sum::<f32>() / voice.len() as f32).sqrt()
    };
    Some(Analysis {
        frames: mon.len(),
        rate: expected_rate,
        channels: expected_channels,
        peak,
        tone1: goertzel(&tone, expected_rate, TONE_1),
        tone2: goertzel(&tone, expected_rate, TONE_2),
        ctl1: goertzel(&tone, expected_rate, CTL_1),
        ctl2: goertzel(&tone, expected_rate, CTL_2),
        voice_peak,
        voice_rms,
    })
}

/// True when the recording contains the engine test tone (both bins above controls, controls quiet).
fn tone_verified(an: &Analysis) -> bool {
    an.frames > 0
        && an.peak >= DETECTION_THRESHOLD
        && an.tone1 > 0.02
        && an.tone2 > 0.02
        && an.tone1 > 4.0 * an.ctl1.max(1e-4)
        && an.tone2 > 4.0 * an.ctl2.max(1e-4)
}

// ---- Automated loopback test (tone only) -------------------------------------------------------

fn automated_loopback_test() -> i32 {
    println!("TapTone level 3 check: virtual cable loopback (tone only, no microphone, no system changes)");
    let Some((output, input)) = find_cable() else {
        append_log("RESULT cable_missing: no installed virtual cable playback/recording pair found");
        println!("FAIL: no virtual cable (playback + recording pair) is installed. Exit 3.");
        return 3;
    };
    println!("Route under test: Engine -> \"{output}\" (playback side) -> [OS] -> \"{input}\" (recording side)");
    append_log(&format!("ROUTE output=\"{output}\" input=\"{input}\""));

    let engine = start_engine_route(&output, None);
    let Some(route_device) = wait_active(&engine) else {
        let why = engine.route_state(OutputRole::Secondary).describe();
        append_log(&format!("RESULT route_inactive: {why}"));
        println!("FAIL: the microphone route never became active: {why}");
        engine.shutdown();
        return 2;
    };
    append_log(&format!("ROUTE_ACTIVE engine_device=\"{route_device}\""));

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let wav = format!("build\\logs\\cable_loopback_{stamp}.wav");
    let sidecar = format!("build\\logs\\cable_loopback_{stamp}.info.txt");
    let mut child = match spawn_recorder(4, &wav, &sidecar, &input) {
        Ok(c) => c,
        Err(e) => {
            append_log(&format!("RESULT recorder_spawn_failed: {e}"));
            eprintln!("FAIL: could not start the recorder: {e}");
            engine.shutdown();
            return 4;
        }
    };
    // Time the recorder's stream before the tone starts (the engine test also listens on the input).
    std::thread::sleep(Duration::from_millis(500));
    engine.test_route();
    let report = wait_route_test(&engine, Duration::from_secs(8));

    let result = child.wait();
    let _ = child.stderr.take();
    let (rate, channels, frames) = read_sidecar(&sidecar);
    let bytes = std::fs::read(&wav).unwrap_or_default();

    let mut pass = false;
    if let (Ok(child_status), Some(an)) = (result, analyze(&bytes, rate, channels, 0.05, 3.5, 3.6, 4.0)) {
        let child_code = child_status.code().unwrap_or(-1);
        append_log(&format!(
            "ANALYSIS frames={} rate={} channels={} peak={:.6}({:.1} dBFS) tone1={:.6} tone2={:.6} ctl1={:.6} ctl2={:.6} recorder_exit={}",
            an.frames, an.rate, an.channels, an.peak, peak_to_db(an.peak), an.tone1, an.tone2, an.ctl1, an.ctl2, child_code
        ));
        println!(
            "Recorder (independent process) got {frames} frames at {rate} Hz / {channels} ch; peak {:.1} dBFS",
            peak_to_db(an.peak)
        );
        println!(
            "Tone content: 523.25 Hz {:.4}, 783.99 Hz {:.4} | control bins: 200 Hz {:.4}, 1500 Hz {:.4}",
            an.tone1, an.tone2, an.ctl1, an.ctl2
        );
        pass = tone_verified(&an);
        if child_code != 0 {
            pass = false;
            println!("FAIL: the recorder exited with code {child_code}.");
        }
        if pass {
            append_log(&format!(
                "RESULT PASS route=\"{route_device}\" tone_bins_ok=true recorder_exit=0"
            ));
            println!("PASS: the TapTone engine signal arrived on capture endpoint \"{input}\" and contains exactly the test tone.");
        } else {
            append_log(&format!(
                "RESULT FAIL route=\"{route_device}\" explanation=missing_or_wrong_content"
            ));
            println!("FAIL: \"{input}\" did not contain the expected test tone.");
        }
    } else {
        append_log("RESULT FAIL reason=no_analysable_recording");
        println!("FAIL: no analysable recording was produced.");
    }

    if let Some((level, message)) = report {
        let text = match level {
            ReportLevel::Ok => "OK",
            ReportLevel::Warning => "WARNING",
            ReportLevel::Failure => "FAILURE",
        };
        append_log(&format!("ENGINE_ROUTE_TEST level={text}: {message}"));
        println!("Engine route test [{text}]: {message}");
    }

    println!("Recording kept for review: {wav}");
    engine.shutdown();
    if pass {
        0
    } else {
        1
    }
}

// ---- Manual mixed test (voice + tone) ----------------------------------------------------------

fn manual_mixed_test() -> i32 {
    println!("TapTone level 3 check: microphone + sounds mixed on the virtual cable (manual / interactive)");
    println!("You will be asked to SPEAK into your microphone while the tool records the cable endpoint.");
    let Some((output, input)) = find_cable() else {
        append_log("RESULT cable_missing (manual mixed test): no installed virtual cable pair found");
        println!("FAIL: no virtual cable is installed. Exit 3.");
        return 3;
    };
    println!("Route under test: mic + tone -> Engine -> \"{output}\" -> [OS] -> \"{input}\"");
    let engine = start_engine_route(&output, pick_real_microphone());
    let voice = wait_voice(&engine);
    let voice_active = voice.is_active();
    println!("Microphone input state: {}", voice.describe());

    let Some(route_device) = wait_active(&engine) else {
        append_log("RESULT route_inactive (manual mixed test)");
        println!("FAIL: the microphone route never became active.");
        engine.shutdown();
        return 2;
    };
    append_log(&format!(
        "ROUTE_ACTIVE_MANUAL engine_device=\"{route_device}\" voice_active={voice_active}"
    ));

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let wav = format!("build\\logs\\cable_loopback_mic_{stamp}.wav");
    let sidecar = format!("build\\logs\\cable_loopback_mic_{stamp}.info.txt");
    let mut child = match spawn_recorder(14, &wav, &sidecar, &input) {
        Ok(c) => c,
        Err(e) => {
            append_log(&format!("RESULT recorder_spawn_failed: {e}"));
            eprintln!("FAIL: could not start the recorder: {e}");
            engine.shutdown();
            return 4;
        }
    };
    std::thread::sleep(Duration::from_millis(1200));
    println!("Test tone plays on the cable now (not on your speakers). Keep SPEAKING for about 8 seconds.");
    engine.test_route();
    let report = wait_route_test(&engine, Duration::from_secs(8));
    println!("Continue speaking, or stay silent if you want the tone alone; analysis starts now.");

    let result = child.wait();
    let _ = child.stderr.take();
    let (rate, channels, frames) = read_sidecar(&sidecar);
    let bytes = std::fs::read(&wav).unwrap_or_default();

    let mut pass = false;
    if let (Ok(child_status), Some(an)) = (result, analyze(&bytes, rate, channels, 1.2, 3.0, 4.0, 12.0)) {
        let child_code = child_status.code().unwrap_or(-1);
        append_log(&format!(
            "MANUAL_ANALYSIS frames={} rate={} channels={} peak={:.6}({:.1} dBFS) tone1={:.6} tone2={:.6} ctl1={:.6} ctl2={:.6} voice_peak={:.6}({:.1} dBFS) voice_rms={:.6} recorder_exit={} voice_active={}",
            an.frames, an.rate, an.channels, an.peak, peak_to_db(an.peak), an.tone1, an.tone2, an.ctl1, an.ctl2,
            an.voice_peak, peak_to_db(an.voice_peak), an.voice_rms, child_code, voice_active
        ));
        println!(
            "Recorder: {frames} frames at {rate} Hz / {channels} ch; total peak {:.1} dBFS",
            peak_to_db(an.peak)
        );
        println!(
            "Tone window analysis: 523.25 Hz {:.4}, 783.99 Hz {:.4} | control bins {:.4}/{:.4}",
            an.tone1, an.tone2, an.ctl1, an.ctl2
        );
        println!(
            "Voice window (4..12 s): peak {:.1} dBFS, rms {:.1} dBFS",
            peak_to_db(an.voice_peak),
            peak_to_db(an.voice_rms)
        );
        pass = tone_verified(&an) && child_code == 0;
        if !pass {
            println!("FAIL: the tone did not arrive on \"{input}\".");
            append_log("RESULT FAIL (manual mixed test): tone missing on the recording side");
        } else {
            let note = if an.voice_rms > DETECTION_THRESHOLD * 0.5 && voice_active {
                "the voice window contains signal; confirm by ear that it is your voice"
            } else if !voice_active {
                "the microphone input was not active, so no voice could be mixed (check the app's voice switch)"
            } else {
                "the voice window looks quiet - either you did not speak during the window, or speak again"
            };
            println!("PASS (tone verified on the endpoint). {note}");
            println!(
                "Final mixed confirmation is manual: listen to {wav} (voice + tone together), then set \"{input}\" as the Discord input."
            );
            append_log(&format!(
                "RESULT PASS_WITH_MANUAL_CONFIRMATION route=\"{route_device}\" see={wav}"
            ));
        }
    } else {
        append_log("RESULT FAIL (manual mixed test): no analysable recording");
        println!("FAIL: no analysable recording was produced.");
    }

    if let Some((_, message)) = report {
        append_log(&format!("ENGINE_ROUTE_TEST (manual): {message}"));
        println!("Engine route test: {message}");
    }
    println!("Recording kept for review: {wav}");
    engine.shutdown();
    if pass {
        0
    } else {
        1
    }
}

// ---- Recorder child (independent client) -------------------------------------------------------

/// Records the requested endpoint with plain WASAPI/cpal and writes a 16-bit PCM WAV plus a
/// sidecar with `frames/rate/channels`. This child intentionally contains no TapTone DSP code.
fn record_with_cpal(secs: f64, wav_path: &str, sidecar_path: &str, device_name: Option<&str>) -> i32 {
    let host = cpal::default_host();
    let device = match device_name {
        Some(name) if !name.is_empty() => host
            .input_devices()
            .ok()
            .and_then(|mut it| it.find(|d| d.name().map(|n| n.eq_ignore_ascii_case(name)).unwrap_or(false))),
        _ => host.default_input_device(),
    };
    let Some(device) = device else {
        eprintln!("recorder: recording device was not found");
        return 5;
    };
    let supported = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("recorder: cannot read the device format: {e}");
            return 6;
        }
    };
    let config = supported.config();
    let rate = supported.sample_rate().0;
    let channels = supported.channels().max(1) as usize;
    let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let stream_res = match supported.sample_format() {
        SampleFormat::F32 => build_input::<f32>(&device, &config, Arc::clone(&samples), Arc::clone(&stop)),
        SampleFormat::I16 => build_input::<i16>(&device, &config, Arc::clone(&samples), Arc::clone(&stop)),
        SampleFormat::U16 => build_input::<u16>(&device, &config, Arc::clone(&samples), Arc::clone(&stop)),
        other => {
            eprintln!("recorder: unsupported sample format {other:?}");
            return 7;
        }
    };
    let stream = match stream_res {
        Ok(s) => s,
        Err(e) => {
            eprintln!("recorder: cannot open the stream: {e}");
            return 8;
        }
    };
    if let Err(e) = stream.play() {
        eprintln!("recorder: cannot start the stream: {e}");
        return 9;
    }
    std::thread::sleep(Duration::from_secs_f64(secs.max(0.0)));
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    drop(stream);

    let data = samples.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let frames = data.len() / channels;
    if let Err(e) = write_wav16(wav_path, &data, rate, channels) {
        eprintln!("recorder: cannot write the WAV: {e}");
        return 10;
    }
    if let Err(e) = std::fs::write(
        sidecar_path,
        format!("frames={frames}\nrate={rate}\nchannels={channels}\n"),
    ) {
        eprintln!("recorder: cannot write the sidecar: {e}");
        return 11;
    }
    println!("recorder: wrote {frames} frames ({secs} s at {rate} Hz, {channels} ch) to {wav_path}");
    0
}

fn build_input<T: SizedSample>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Arc<Mutex<Vec<f32>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> Result<cpal::Stream, String>
where
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                if !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let conv: Vec<f32> = data.iter().map(|&s| f32::from_sample(s)).collect();
                    samples
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .extend_from_slice(&conv);
                }
            },
            move |e| eprintln!("recorder stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

fn write_wav16(path: &str, samples: &[f32], rate: u32, channels: usize) -> Result<(), String> {
    use std::io::Write;
    let mut pcm: Vec<u8> = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let s = if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 };
        let v = (s * 32767.0) as i16;
        pcm.extend_from_slice(&v.to_le_bytes());
    }
    let data_len = pcm.len() as u32;
    let channels = channels.max(1) as u16;
    let byte_rate = rate * channels as u32 * 2;
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + data_len).to_le_bytes());
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes()); // PCM
    header.extend_from_slice(&channels.to_le_bytes());
    header.extend_from_slice(&rate.to_le_bytes());
    header.extend_from_slice(&byte_rate.to_le_bytes());
    header.extend_from_slice(&(channels * 2).to_le_bytes()); // block align
    header.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_len.to_le_bytes());
    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    f.write_all(&header).map_err(|e| e.to_string())?;
    f.write_all(&pcm).map_err(|e| e.to_string())?;
    Ok(())
}
