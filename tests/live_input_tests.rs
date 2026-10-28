//! The user's microphone ("voice") as a live source inside a `Mixer`. These tests need no audio
//! hardware: they push samples the way the capture callback does and mix the way the playback
//! callback does.

use std::sync::Arc;
use taptone::audio::mixer::*;
use taptone::settings::RetriggerPolicy;

fn stereo(v: f32, frames: usize) -> Vec<f32> {
    vec![v; frames * 2]
}

fn mixer_with_live(rate: u32, out_rate: u32) -> (Mixer, Arc<LiveInput>) {
    let mut m = Mixer::new();
    let live = Arc::new(LiveInput::new(rate));
    m.attach_live(Arc::clone(&live), out_rate);
    (m, live)
}

fn mix_frames(m: &mut Mixer, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * 2];
    m.mix(&mut out);
    out
}

#[test]
fn live_audio_reaches_the_output_at_its_own_gain() {
    let (mut m, live) = mixer_with_live(48_000, 48_000);
    live.set_gain(0.5);
    live.push_stereo(&stereo(0.4, 2000));
    let out = mix_frames(&mut m, 500);
    assert!(out.iter().all(|s| (*s - 0.2).abs() < 1e-5), "{:?}", &out[..4]);
}

#[test]
fn the_voice_ignores_master_volume_and_route_mute() {
    let (mut m, live) = mixer_with_live(48_000, 48_000);
    m.set_master_volume(0.0);
    m.snap_output_gain(0.0);
    live.push_stereo(&stereo(0.3, 2000));
    let out = mix_frames(&mut m, 200);
    assert!(
        out.iter().all(|s| (*s - 0.3).abs() < 1e-5),
        "the user's voice must stay audible while sounds are muted"
    );
}

#[test]
fn sounds_and_voice_are_summed() {
    let (mut m, live) = mixer_with_live(48_000, 48_000);
    live.push_stereo(&stereo(0.2, 2000));
    m.play("a", Arc::from(stereo(0.3, 400)), 1.0, RetriggerPolicy::Overlap);
    let out = mix_frames(&mut m, 200);
    assert!(out.iter().all(|s| (*s - 0.5).abs() < 1e-5), "{:?}", &out[..4]);
}

#[test]
fn sounds_are_unchanged_without_a_live_source() {
    let mut m = Mixer::new();
    assert!(!m.has_live());
    m.play("a", Arc::from(stereo(0.3, 100)), 1.0, RetriggerPolicy::Overlap);
    let out = mix_frames(&mut m, 50);
    assert!(out.iter().all(|s| (*s - 0.3).abs() < 1e-6));
}

#[test]
fn nothing_is_played_until_enough_audio_has_arrived() {
    let (mut m, live) = mixer_with_live(48_000, 48_000);
    live.push_stereo(&stereo(0.3, 100)); // less than the 20 ms priming amount
    let out = mix_frames(&mut m, 100);
    assert!(out.iter().all(|s| *s == 0.0));

    live.push_stereo(&stereo(0.3, 900)); // 1000 frames in total: now it starts
    let out = mix_frames(&mut m, 100);
    assert!(out.iter().all(|s| (*s - 0.3).abs() < 1e-5));
}

#[test]
fn running_dry_gives_silence_not_garbage() {
    let (mut m, live) = mixer_with_live(48_000, 48_000);
    live.push_stereo(&stereo(0.3, 1000));
    let out = mix_frames(&mut m, 3000);
    assert!((out[0] - 0.3).abs() < 1e-5);
    assert_eq!(out[out.len() - 1], 0.0);
    assert!(out.iter().all(|s| s.is_finite()));
    assert!(live.queued_frames() <= 1);
}

#[test]
fn a_lower_capture_rate_is_resampled_up() {
    let (mut m, live) = mixer_with_live(24_000, 48_000);
    live.push_stereo(&stereo(0.2, 1200));
    let out = mix_frames(&mut m, 800);
    assert!(out.iter().all(|s| (*s - 0.2).abs() < 1e-5));
    // 800 output frames at half speed consume about 400 input frames.
    assert!(
        (live.queued_frames() as i64 - 800).abs() <= 2,
        "{}",
        live.queued_frames()
    );
}

#[test]
fn a_higher_capture_rate_is_resampled_down() {
    let (mut m, live) = mixer_with_live(96_000, 48_000);
    live.push_stereo(&stereo(0.2, 5000));
    let out = mix_frames(&mut m, 1000);
    assert!(out.iter().all(|s| (*s - 0.2).abs() < 1e-5));
    assert!(
        (live.queued_frames() as i64 - 3000).abs() <= 2,
        "{}",
        live.queued_frames()
    );
}

#[test]
fn the_backlog_is_bounded_so_the_voice_never_lags() {
    let live = LiveInput::new(48_000);
    live.push_stereo(&stereo(0.1, 48_000)); // one second at once
    let queued = live.queued_frames();
    assert!(queued > 0 && queued <= 5760, "{queued}");
}

#[test]
fn bad_samples_and_odd_lengths_are_handled() {
    let live = LiveInput::new(48_000);
    live.push_stereo(&[0.1; 5]); // the odd trailing sample is ignored
    assert_eq!(live.queued_frames(), 2);

    let (mut m, live) = mixer_with_live(48_000, 48_000);
    let mut bad = stereo(f32::NAN, 600);
    bad.extend(stereo(f32::INFINITY, 600));
    live.push_stereo(&bad);
    let out = mix_frames(&mut m, 100);
    assert!(out.iter().all(|s| *s == 0.0));
}

#[test]
fn a_loud_voice_is_limited() {
    let (mut m, live) = mixer_with_live(48_000, 48_000);
    live.set_gain(2.0);
    live.push_stereo(&stereo(1.0, 2000));
    let out = mix_frames(&mut m, 500);
    assert!(out.iter().all(|s| s.abs() < 1.0));
}

#[test]
fn detaching_the_live_source_silences_it() {
    let (mut m, live) = mixer_with_live(48_000, 48_000);
    assert!(m.has_live());
    live.push_stereo(&stereo(0.3, 2000));
    m.detach_live();
    assert!(!m.has_live());
    let out = mix_frames(&mut m, 100);
    assert!(out.iter().all(|s| *s == 0.0));
}

#[test]
fn gain_is_sanitized() {
    let live = LiveInput::new(48_000);
    live.set_gain(f32::NAN);
    assert_eq!(live.gain(), 1.0);
    live.set_gain(-3.0);
    assert_eq!(live.gain(), 0.0);
    live.set_gain(50.0);
    assert_eq!(live.gain(), MAX_OUTPUT_GAIN);
}
