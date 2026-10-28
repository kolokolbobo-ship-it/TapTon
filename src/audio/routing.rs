//! Pure helpers for the microphone route: classification of devices, pairing of a virtual cable's
//! playback ("Input") and recording ("Output") sides, the test signal and the interpretation of a
//! route test. Nothing here touches the audio hardware, so it is fully unit-tested.
//!
//! Background: Windows cannot send a program's sound into the *microphone* by itself. A virtual
//! audio cable driver (VB-CABLE, VoiceMeeter, Virtual Audio Cable, ...) provides a playback device
//! whose sound comes out of a matching recording device. TapTone plays into the playback side
//! ("CABLE Input"); Discord or a game then uses the recording side ("CABLE Output") as microphone.

use crate::audio::mixer::CHANNELS;

/// The two outputs of the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputRole {
    /// Headphones or speakers: what the user hears.
    Main,
    /// The microphone route: a virtual cable that voice chats use as microphone.
    Secondary,
}

impl OutputRole {
    pub fn label(self) -> &'static str {
        match self {
            OutputRole::Main => "Headphones",
            OutputRole::Secondary => "Microphone route",
        }
    }
}

/// What an output is doing right now. Shown in the UI; never hides a problem behind "OK".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteState {
    /// Not requested (the microphone route is switched off).
    Disabled,
    /// Requested, but no device was chosen yet.
    NotSelected,
    /// Sound is being sent to `device`.
    Active {
        device: String,
        rate: u32,
        channels: u16,
        fallback_from: Option<String>,
    },
    /// The chosen device is not present (yet). It is retried automatically.
    Missing { wanted: String },
    /// The device worked and then disappeared or failed. It is retried automatically.
    Lost { device: String, reason: String },
    /// The microphone route points at the output the user hears themselves (the desktop default
    /// or the same sound card as the headphones): sounds and the voice would play in their ears.
    Conflict { device: String },
    /// The device exists but cannot be opened.
    Failed { reason: String },
    /// Nothing is wrong, the output is just waiting for something else (for example the voice input
    /// waits until the microphone route is running).
    Standby { reason: String },
}

impl RouteState {
    pub fn is_active(&self) -> bool {
        matches!(self, RouteState::Active { .. })
    }

    /// One line for the user that says what is going on.
    pub fn describe(&self) -> String {
        match self {
            RouteState::Disabled => "Off".to_string(),
            RouteState::NotSelected => "No device selected".to_string(),
            RouteState::Active {
                device,
                fallback_from: None,
                ..
            } => format!("Playing on \"{device}\""),
            RouteState::Active {
                device,
                fallback_from: Some(wanted),
                ..
            } => {
                format!("\"{wanted}\" is not available, playing on \"{device}\" instead")
            }
            RouteState::Missing { wanted } => format!("\"{wanted}\" is not connected. TapTone keeps looking for it."),
            RouteState::Lost { device, reason } => {
                format!("\"{device}\" stopped working ({reason}). TapTone is trying to reconnect.")
            }
            RouteState::Conflict { device } => format!(
                "\"{device}\" is your normal listening output or the same sound card as the headphones. \
                 Pick the virtual cable or another device."
            ),
            RouteState::Failed { reason } => format!("The device cannot be used: {reason}"),
            RouteState::Standby { reason } => reason.clone(),
        }
    }

    /// True for states that the engine keeps trying to repair on its own.
    pub fn is_recoverable(&self) -> bool {
        matches!(self, RouteState::Missing { .. } | RouteState::Lost { .. })
    }
}

const VIRTUAL_MARKERS: [&str; 12] = [
    "cable",
    "vb-audio",
    "voicemeeter",
    "virtual",
    "vaio",
    "hi-fi",
    "blackhole",
    "loopback",
    "vac ",
    "line 1 (",
    // PipeWire auto-created null sinks and TapTone's own virtual cable pcm.
    "auto_null",
    "taptone",
];

/// Heuristic: does the name look like a virtual audio cable / mixer device?
pub fn is_virtual_device(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    VIRTUAL_MARKERS.iter().any(|m| lower.contains(m))
}

/// Virtual devices first, then the rest; each group alphabetical.
pub fn sort_for_mic_route(names: &[String]) -> Vec<String> {
    let mut v: Vec<String> = names.to_vec();
    v.sort_by_key(|a| (!is_virtual_device(a), a.to_lowercase()));
    v
}

/// Names under which the recording side that belongs to a playback device may appear:
/// `CABLE Input (VB-Audio Virtual Cable)` -> `CABLE Output (VB-Audio Virtual Cable)`; some
/// products (Virtual Audio Cable) use the same name for both sides.
pub fn paired_input_candidates(output_name: &str) -> Vec<String> {
    let lower = output_name.to_ascii_lowercase();
    let mut v = Vec::new();
    if let Some(i) = lower.find(" input") {
        // `to_ascii_lowercase` keeps byte offsets, so `i` is valid for the original string.
        v.push(format!(
            "{} Output{}",
            &output_name[..i],
            &output_name[i + " input".len()..]
        ));
    }
    v.push(output_name.to_string());
    v
}

/// Pick the recording device that belongs to `output_name` from the available input devices.
pub fn find_paired_input(output_name: &str, inputs: &[String]) -> Option<String> {
    paired_input_candidates(output_name)
        .iter()
        .find_map(|c| inputs.iter().find(|i| i.eq_ignore_ascii_case(c)).cloned())
}

/// Which kind of audio device a list contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// Playback devices: headphones, speakers, the playback side of a virtual cable.
    Output,
    /// Recording devices: microphones, headset microphones, the recording side of a virtual cable.
    Input,
}

impl DeviceKind {
    pub fn noun(self) -> &'static str {
        match self {
            DeviceKind::Output => "playback",
            DeviceKind::Input => "recording",
        }
    }
}

/// Real microphones first, virtual cables last; each group alphabetical. A virtual cable is almost
/// never what the user wants as their own voice input.
pub fn sort_for_voice_input(names: &[String]) -> Vec<String> {
    let mut v: Vec<String> = names.to_vec();
    v.sort_by_key(|a| (is_virtual_device(a), a.to_lowercase()));
    v
}

/// Explains, in words for the user, why a device list is unusable or empty. `None` when the list
/// is fine. Only states what is actually known: an enumeration error is quoted, an empty list gets
/// platform-appropriate advice.
pub fn explain_device_list(kind: DeviceKind, count: usize, error: Option<&str>) -> Option<String> {
    if let Some(e) = error {
        return Some(format!(
            "The system did not return the list of {} devices ({e}). Check that the audio service is running \
             and the driver is installed, then press \"Refresh devices\".",
            kind.noun()
        ));
    }
    if count > 0 {
        return None;
    }
    Some(match kind {
        DeviceKind::Input if std::env::consts::OS != "windows" => {
            "The system reports no recording devices. Plug in the microphone, check that it is enabled and not \
             muted in your sound settings (PipeWire/PulseAudio or ALSA), then press \"Refresh devices\"."
                .to_string()
        }
        DeviceKind::Output if std::env::consts::OS != "windows" => {
            "The system reports no playback devices. Plug in the output device, check that it is enabled in your \
             sound settings (PipeWire/PulseAudio or ALSA), then press \"Refresh devices\"."
                .to_string()
        }
        DeviceKind::Input => {
            "Windows reports no recording devices. Plug in the microphone or headset and check Settings > \
             System > Sound > Input. In the classic Sound panel (Recording tab) right-click the empty area and tick \
             \"Show disabled devices\", then enable your microphone."
                .to_string()
        }
        DeviceKind::Output => {
            "Windows reports no playback devices. Plug in the headphones or speakers and check Settings > \
             System > Sound > Output. In the classic Sound panel (Playback tab) right-click the empty area and tick \
             \"Show disabled devices\", then enable your device."
                .to_string()
        }
    })
}

/// True if `voice_input` is the recording side of the playback device `route_output`. Mixing that
/// device into the route would feed the route's own sound back into itself.
pub fn voice_feedback_conflict(route_output: &str, voice_input: &str) -> bool {
    paired_input_candidates(route_output)
        .iter()
        .any(|c| c.eq_ignore_ascii_case(voice_input))
}

// ---- Linux: ALSA device-name targets ---------------------------------------------------------

/// What physical destination an ALSA device name points at (relevant on Linux, where one
/// physical device appears under many names). Windows never has aliases, so name equality
/// was enough there; on Linux it silently fails and let the microphone route end up on the
/// very output the user is listening to (the reported "my voice plays in my headphones" bug).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceTarget {
    /// The desktop's default playback destination — whatever the user currently hears.
    /// ALSA names: "default", "pipewire", "pulse" (and an empty name).
    DefaultSink,
    /// A specific sound card addressed directly (`hw:CARD=PCH`, `front:CARD=PCH`, ...).
    /// Distinct cards are distinct destinations even when the desktop default is elsewhere.
    Card(String),
    /// Everything else: dedicated virtual devices (a null-sink/loopback pcm such as
    /// `taptone_route`) and any name without a `CARD=` part.
    Custom(String),
}

/// Classify an ALSA (or Windows) device name into its physical destination.
pub fn device_target(name: &str) -> DeviceTarget {
    let lower = name.to_ascii_lowercase();
    if lower.is_empty() || lower == "default" || lower == "pipewire" || lower == "pulse" {
        return DeviceTarget::DefaultSink;
    }
    if let Some(pos) = lower.find("card=") {
        let rest = &lower[pos + "card=".len()..];
        let end = rest.find([',', '/']).unwrap_or(rest.len());
        let card = rest[..end].trim();
        if !card.is_empty() {
            return DeviceTarget::Card(card.to_string());
        }
    }
    DeviceTarget::Custom(name.to_string())
}

/// True for names that are aliases of the desktop default playback ("what you hear").
pub fn is_default_sink_alias(name: &str) -> bool {
    matches!(device_target(name), DeviceTarget::DefaultSink)
}

/// True when `route_name` must not be used as the microphone route given `main_name`.
///
/// Rules:
/// * the route must never target the desktop default output — that is by definition what the
///   user hears, so sounds and the mixed-in voice would play in their own headphones;
/// * two devices that resolve to the same sound card are the same destination (the old code
///   compared names, and on Linux "front:CARD=PCH" vs "hw:CARD=PCH" passed as "different").
pub fn route_output_conflict(main_name: Option<&str>, route_name: &str) -> bool {
    if is_default_sink_alias(route_name) {
        return true;
    }
    match main_name {
        Some(main) => device_target(main) == device_target(route_name),
        None => false,
    }
}

/// Names that must not be offered as a microphone route on Linux:
/// * default-sink aliases ("default", "pipewire", ...) — the route would land in the user's
///   own headphones/speakers (the reported bug);
/// * `dmix:`/`dsnoop:` plugin devices — pure plumbing, they target a card another server
///   already owns and hang or fail with "device busy" instead of being a stable destination.
///   Everything else (hardware cards, dedicated virtual cable pcms) stays.
pub fn filter_route_output_names(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|n| !is_default_sink_alias(n))
        .filter(|n| {
            let l = n.to_ascii_lowercase();
            !(l.starts_with("dmix:") || l.starts_with("dsnoop:"))
        })
        .cloned()
        .collect()
}

/// Adds a privacy hint to a failure that looks like "access denied"
/// (Windows privacy settings or the Linux sound service, depending on the OS).
pub fn add_input_hint(reason: &str) -> String {
    let lower = reason.to_ascii_lowercase();
    if lower.contains("denied") || lower.contains("0x80070005") || lower.contains("access") {
        if std::env::consts::OS == "windows" {
            format!(
                "{reason}. Windows may be blocking microphone access for desktop apps (Settings > Privacy & security > \
                 Microphone > Let desktop apps access your microphone)."
            )
        } else {
            format!(
                "{reason}. Check that the input device is not muted and that the sound service (PipeWire/PulseAudio) is running."
            )
        }
    } else {
        reason.to_string()
    }
}

/// A short two-tone chirp (interleaved stereo) with soft fades, so it is clearly a test signal.
pub fn make_test_tone(rate: u32, secs: f32, amplitude: f32) -> Vec<f32> {
    let rate_f = rate.max(1) as f32;
    let frames = ((rate_f * secs) as usize).max(2);
    let fade = ((rate_f * 0.03) as usize).max(1);
    let mut buf = Vec::with_capacity(frames * CHANNELS);
    for i in 0..frames {
        let t = i as f32 / rate_f;
        let env = (i.min(frames - 1 - i).min(fade) as f32 / fade as f32).min(1.0);
        let freq = if i < frames / 2 { 523.25 } else { 783.99 };
        let s = (std::f32::consts::TAU * freq * t).sin() * amplitude * env;
        buf.push(s);
        buf.push(s);
    }
    buf
}

/// Level of a linear peak in dBFS (silence -> -120).
pub fn peak_to_db(peak: f32) -> f32 {
    if peak.is_finite() && peak > 1e-6 {
        20.0 * peak.log10()
    } else {
        -120.0
    }
}

/// Below this linear peak the capture is considered silent (about -60 dBFS).
pub const DETECTION_THRESHOLD: f32 = 0.001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportLevel {
    Ok,
    Warning,
    Failure,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteTestReport {
    pub level: ReportLevel,
    pub message: String,
}

/// What the capture side of a route test observed.
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureOutcome {
    /// No recording device belonging to the output was found.
    NoPairedInput,
    /// The recording device exists but could not be opened/started.
    CaptureFailed { input: String, error: String },
    /// Listening worked; `peak` is the highest absolute sample seen.
    Measured { input: String, peak: f32 },
}

pub fn analyze_route_test(output: &str, outcome: &CaptureOutcome) -> RouteTestReport {
    match outcome {
        CaptureOutcome::Measured { input, peak } if *peak >= DETECTION_THRESHOLD => RouteTestReport {
            level: ReportLevel::Ok,
            message: format!(
                "Works: the test signal came out of \"{input}\" ({:.0} dBFS). Choose \"{input}\" as the microphone in your voice chat.",
                peak_to_db(*peak)
            ),
        },
        CaptureOutcome::Measured { input, .. } => RouteTestReport {
            level: ReportLevel::Failure,
            message: format!(
                "No signal on \"{input}\". Make sure the volume of the microphone route is above 0 and it is not \
                 muted, and that \"{output}\" really is the playback side of that device."
            ),
        },
        CaptureOutcome::NoPairedInput => RouteTestReport {
            level: ReportLevel::Warning,
            message: format!(
                "The test sound was played on \"{output}\", but TapTone could not find the recording device that \
                 belongs to it, so it cannot confirm that the sound reaches the microphone. Look for the matching \
                 \"Output\" device in your voice chat's microphone list."
            ),
        },
        CaptureOutcome::CaptureFailed { input, error } => {
            let hint = if std::env::consts::OS == "windows" {
                ". Windows may be blocking microphone access for desktop apps (Settings > Privacy > Microphone)."
            } else {
                ". Check that the input device is not busy or muted and that the sound service (PipeWire/PulseAudio) is running."
            };
            RouteTestReport {
                level: ReportLevel::Warning,
                message: format!(
                    "The test sound was played on \"{output}\", but listening on \"{input}\" failed ({error}){hint}"
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_virtual_devices() {
        for n in [
            "CABLE Input (VB-Audio Virtual Cable)",
            "CABLE-A Input (VB-Audio Cable A)",
            "VoiceMeeter Input (VB-Audio VoiceMeeter VAIO)",
            "Hi-Fi Cable Input (VB-Audio Hi-Fi Cable)",
            "Line 1 (Virtual Audio Cable)",
        ] {
            assert!(is_virtual_device(n), "{n}");
        }
        for n in [
            "Speakers (Realtek High Definition Audio)",
            "Headphones (Razer Kraken)",
            "LG TV (NVIDIA HDMI)",
        ] {
            assert!(!is_virtual_device(n), "{n}");
        }
    }

    #[test]
    fn virtual_devices_are_listed_first() {
        let names = vec![
            "Speakers".to_string(),
            "CABLE Input (VB-Audio Virtual Cable)".to_string(),
            "Headphones".to_string(),
        ];
        let sorted = sort_for_mic_route(&names);
        assert_eq!(sorted[0], "CABLE Input (VB-Audio Virtual Cable)");
        assert_eq!(sorted[1], "Headphones");
        assert_eq!(sorted[2], "Speakers");
    }

    #[test]
    fn pairs_playback_and_recording_sides() {
        let inputs = vec![
            "Microphone (Realtek)".to_string(),
            "CABLE Output (VB-Audio Virtual Cable)".to_string(),
            "Line 1 (Virtual Audio Cable)".to_string(),
        ];
        assert_eq!(
            find_paired_input("CABLE Input (VB-Audio Virtual Cable)", &inputs).as_deref(),
            Some("CABLE Output (VB-Audio Virtual Cable)")
        );
        assert_eq!(
            find_paired_input("cable input (vb-audio virtual cable)", &inputs).as_deref(),
            Some("CABLE Output (VB-Audio Virtual Cable)")
        );
        assert_eq!(
            find_paired_input("Line 1 (Virtual Audio Cable)", &inputs).as_deref(),
            Some("Line 1 (Virtual Audio Cable)")
        );
        assert_eq!(find_paired_input("Speakers (Realtek)", &inputs), None);
        assert_eq!(
            paired_input_candidates("VoiceMeeter Aux Input (VB-Audio VoiceMeeter AUX VAIO)")[0],
            "VoiceMeeter Aux Output (VB-Audio VoiceMeeter AUX VAIO)"
        );
    }

    #[test]
    fn test_tone_is_stereo_bounded_and_fades() {
        let t = make_test_tone(48_000, 0.5, 0.3);
        assert_eq!(t.len(), 24_000 * 2);
        assert!(t.iter().all(|s| s.abs() <= 0.3 + 1e-6));
        assert!(t[0].abs() < 0.01 && t[t.len() - 1].abs() < 0.01);
        assert!(t.iter().any(|s| s.abs() > 0.25));
        assert_eq!(t[0], t[1]);
        assert!(make_test_tone(0, 0.0, 0.5).len() >= 2);
    }

    #[test]
    fn db_conversion() {
        assert!((peak_to_db(1.0)).abs() < 1e-4);
        assert!((peak_to_db(0.5) + 6.02).abs() < 0.01);
        assert_eq!(peak_to_db(0.0), -120.0);
        assert_eq!(peak_to_db(f32::NAN), -120.0);
    }

    #[test]
    fn reports_distinguish_success_silence_and_unknown() {
        let out = "CABLE Input (VB-Audio Virtual Cable)";
        let input = "CABLE Output (VB-Audio Virtual Cable)".to_string();
        let ok = analyze_route_test(
            out,
            &CaptureOutcome::Measured {
                input: input.clone(),
                peak: 0.2,
            },
        );
        assert_eq!(ok.level, ReportLevel::Ok);
        let silent = analyze_route_test(
            out,
            &CaptureOutcome::Measured {
                input: input.clone(),
                peak: 0.0,
            },
        );
        assert_eq!(silent.level, ReportLevel::Failure);
        let unknown = analyze_route_test(out, &CaptureOutcome::NoPairedInput);
        assert_eq!(unknown.level, ReportLevel::Warning);
        assert!(unknown.message.contains("cannot confirm"));
        let failed = analyze_route_test(
            out,
            &CaptureOutcome::CaptureFailed {
                input,
                error: "denied".into(),
            },
        );
        assert_eq!(failed.level, ReportLevel::Warning);
        assert!(failed.message.contains("denied"));
    }

    #[test]
    fn route_state_helpers() {
        assert!(RouteState::Lost {
            device: "x".into(),
            reason: "y".into()
        }
        .is_recoverable());
        assert!(RouteState::Missing { wanted: "x".into() }.is_recoverable());
        assert!(!RouteState::Failed { reason: "x".into() }.is_recoverable());
        assert!(RouteState::Active {
            device: "x".into(),
            rate: 48_000,
            channels: 2,
            fallback_from: None
        }
        .is_active());
        assert!(!RouteState::Disabled.is_active());
    }

    #[test]
    fn voice_inputs_list_real_microphones_first() {
        let names = vec![
            "CABLE Output (VB-Audio Virtual Cable)".to_string(),
            "Microphone (USB Headset)".to_string(),
            "Headset Microphone (Realtek)".to_string(),
        ];
        let sorted = sort_for_voice_input(&names);
        assert_eq!(sorted[0], "Headset Microphone (Realtek)");
        assert_eq!(sorted[1], "Microphone (USB Headset)");
        assert_eq!(sorted[2], "CABLE Output (VB-Audio Virtual Cable)");
    }

    #[test]
    fn empty_and_failed_lists_are_explained() {
        assert!(explain_device_list(DeviceKind::Input, 2, None).is_none());
        if std::env::consts::OS == "windows" {
            let empty_in = explain_device_list(DeviceKind::Input, 0, None).unwrap();
            assert!(empty_in.contains("no recording devices"));
            assert!(empty_in.contains("Show disabled devices"));
            let empty_out = explain_device_list(DeviceKind::Output, 0, None).unwrap();
            assert!(empty_out.contains("no playback devices"));
            assert!(empty_out.contains("Show disabled devices"));
        } else {
            let empty_in = explain_device_list(DeviceKind::Input, 0, None).unwrap();
            assert!(empty_in.contains("no recording devices"));
            assert!(empty_in.contains("PipeWire/PulseAudio"));
            let empty_out = explain_device_list(DeviceKind::Output, 0, None).unwrap();
            assert!(empty_out.contains("no playback devices"));
            assert!(empty_out.contains("PipeWire/PulseAudio"));
        }
        let failed = explain_device_list(DeviceKind::Input, 3, Some("the device is busy")).unwrap();
        assert!(
            failed.contains("the device is busy"),
            "an error wins over a non-empty list"
        );
        assert!(failed.contains("recording"));
    }

    #[test]
    fn alsa_names_resolve_to_physical_targets() {
        use DeviceTarget::{Card, Custom, DefaultSink};
        assert_eq!(device_target("default"), DefaultSink);
        assert_eq!(device_target("pipewire"), DefaultSink);
        assert_eq!(device_target("pulse"), DefaultSink);
        assert_eq!(device_target(""), DefaultSink);
        assert_eq!(device_target("hw:CARD=PCH,DEV=0"), Card("pch".to_string()));
        assert_eq!(device_target("front:CARD=PCH,DEV=0"), Card("pch".to_string()));
        assert_eq!(device_target("hdmi:CARD=NVidia,DEV=3"), Card("nvidia".to_string()));
        assert_eq!(device_target("sysdefault:CARD=PCH"), Card("pch".to_string()));
        assert_eq!(device_target("taptone_route"), Custom("taptone_route".to_string()));
    }

    #[test]
    fn route_never_targets_the_default_sink() {
        // The reported Linux bug: the route was silently allowed on the desktop default
        // output, so the mixed-in voice played in the user's own headphones.
        for alias in ["default", "pipewire", "pulse"] {
            assert!(route_output_conflict(Some("default"), alias), "{alias}");
            assert!(route_output_conflict(Some("hw:CARD=PCH,DEV=0"), alias), "{alias}");
            assert!(route_output_conflict(None, alias), "{alias}");
            assert!(is_default_sink_alias(alias), "{alias}");
        }
    }

    #[test]
    fn aliases_of_one_card_are_one_destination() {
        // Names differ, the physical device does not: the old name-equality check passed this.
        assert!(route_output_conflict(Some("hw:CARD=PCH,DEV=0"), "front:CARD=PCH,DEV=0"));
        assert!(route_output_conflict(
            Some("sysdefault:CARD=PCH"),
            "dmix:CARD=PCH,DEV=0"
        ));
        // Different cards are legitimately different destinations.
        assert!(!route_output_conflict(
            Some("hw:CARD=PCH,DEV=0"),
            "hdmi:CARD=NVidia,DEV=3"
        ));
        // The desktop default cannot be compared with a concrete card: allowed (e.g. main on
        // the USB headset, route on a second card).
        assert!(!route_output_conflict(Some("default"), "front:CARD=PCH,DEV=0"));
        // Dedicated virtual devices are always distinct destinations.
        assert!(!route_output_conflict(Some("default"), "taptone_route"));
        assert!(!route_output_conflict(Some("hw:CARD=PCH,DEV=0"), "taptone_route"));
    }

    #[test]
    fn route_output_list_is_cleaned_up() {
        let names = vec![
            "default".to_string(),
            "pipewire".to_string(),
            "dmix:CARD=PCH,DEV=0".to_string(),
            "dsnoop:CARD=PCH,DEV=0".to_string(),
            "taptone_route".to_string(),
            "front:CARD=PCH,DEV=0".to_string(),
        ];
        assert_eq!(
            filter_route_output_names(&names),
            vec!["taptone_route".to_string(), "front:CARD=PCH,DEV=0".to_string()]
        );
    }

    #[test]
    fn pipewire_virtual_sinks_sort_like_cables() {
        // PipeWire's auto null sinks and TapTone's cable pcm must be treated as virtual
        // devices: first in the route list, last in the voice-input list.
        assert!(is_virtual_device("auto_null.output.2"));
        assert!(is_virtual_device("taptone_route"));
        let route = sort_for_mic_route(&[
            "default".to_string(),
            "taptone_route".to_string(),
            "auto_null.output.2".to_string(),
        ]);
        // Virtual devices first (alphabetical inside the group), the rest after.
        assert_eq!(route[0], "auto_null.output.2");
        assert_eq!(route[1], "taptone_route");
        assert_eq!(route[2], "default");
        let voice = sort_for_voice_input(&["taptone_route".to_string(), "Microphone (USB Headset)".to_string()]);
        assert_eq!(voice[0], "Microphone (USB Headset)");
        assert_eq!(voice[1], "taptone_route");
    }

    #[test]
    fn feedback_conflict_is_detected_only_for_the_routes_own_recording_side() {
        let route = "CABLE Input (VB-Audio Virtual Cable)";
        assert!(voice_feedback_conflict(route, "CABLE Output (VB-Audio Virtual Cable)"));
        assert!(voice_feedback_conflict(route, "cable output (vb-audio virtual cable)"));
        assert!(!voice_feedback_conflict(route, "Microphone (USB Headset)"));
        // A different cable is not a feedback loop.
        assert!(!voice_feedback_conflict(route, "CABLE-A Output (VB-Audio Cable A)"));
        // Products that use one name for both sides.
        assert!(voice_feedback_conflict(
            "Line 1 (Virtual Audio Cable)",
            "Line 1 (Virtual Audio Cable)"
        ));
    }

    #[test]
    fn access_denied_gets_the_privacy_hint() {
        let hinted = add_input_hint("Access is denied. (0x80070005)");
        if std::env::consts::OS == "windows" {
            assert!(hinted.contains("Privacy"));
        } else {
            assert!(hinted.contains("PipeWire/PulseAudio"));
        }
        assert_eq!(add_input_hint("The device is busy"), "The device is busy");
    }

    #[test]
    fn standby_is_neither_active_nor_recoverable() {
        let s = RouteState::Standby {
            reason: "waiting".into(),
        };
        assert!(!s.is_active());
        assert!(!s.is_recoverable());
        assert_eq!(s.describe(), "waiting");
    }
}
