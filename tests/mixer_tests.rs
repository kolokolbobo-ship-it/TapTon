use std::sync::Arc;
use taptone::audio::mixer::*;
use taptone::settings::RetriggerPolicy;

fn buf(v: f32, frames: usize) -> Arc<[f32]> {
    Arc::from(vec![v; frames * 2])
}

#[test]
fn silence_when_idle() {
    let mut m = Mixer::new();
    let mut out = vec![1.0f32; 16];
    m.mix(&mut out);
    assert!(out.iter().all(|s| *s == 0.0));
}

#[test]
fn two_voices_are_summed() {
    let mut m = Mixer::new();
    m.play("a", buf(0.2, 8), 1.0, RetriggerPolicy::Overlap);
    m.play("b", buf(0.3, 8), 1.0, RetriggerPolicy::Overlap);
    let mut out = vec![0.0f32; 16];
    m.mix(&mut out);
    assert!(out.iter().all(|s| (*s - 0.5).abs() < 1e-6), "{out:?}");
}

#[test]
fn per_voice_gain_and_master_volume() {
    let mut m = Mixer::new();
    m.play("a", buf(0.4, 4), 0.5, RetriggerPolicy::Overlap);
    m.set_master_volume(0.5);
    let mut out = vec![0.0f32; 8];
    m.mix(&mut out);
    assert!(out.iter().all(|s| (*s - 0.1).abs() < 1e-6));
}

#[test]
fn output_never_clips() {
    let mut m = Mixer::new();
    for i in 0..10 {
        m.play(&format!("s{i}"), buf(1.0, 4), 2.0, RetriggerPolicy::Overlap);
    }
    let mut out = vec![0.0f32; 8];
    m.mix(&mut out);
    assert!(out.iter().all(|s| s.abs() < 1.0 && *s > 0.9), "{out:?}");
    assert!(soft_clip(-100.0) > -1.0 && soft_clip(-100.0) < -0.9);
    assert_eq!(soft_clip(0.5), 0.5);
}

#[test]
fn short_file_ends_cleanly_across_blocks() {
    let mut m = Mixer::new();
    m.play("a", buf(0.5, 3), 1.0, RetriggerPolicy::Restart);
    let mut out = vec![0.0f32; 4]; // 2 frames per block
    m.mix(&mut out);
    assert_eq!(out, vec![0.5; 4]);
    assert!(m.is_playing("a"));
    m.mix(&mut out); // 1 frame left, then silence
    assert_eq!(out, vec![0.5, 0.5, 0.0, 0.0]);
    assert!(!m.is_playing("a"));
    assert_eq!(m.voice_count(), 0);
}

#[test]
fn stop_and_stop_all() {
    let mut m = Mixer::new();
    m.play("a", buf(0.1, 100), 1.0, RetriggerPolicy::Overlap);
    m.play("a", buf(0.1, 100), 1.0, RetriggerPolicy::Overlap);
    m.play("b", buf(0.1, 100), 1.0, RetriggerPolicy::Overlap);
    assert_eq!(m.stop("a"), 2);
    assert!(m.is_playing("b"));
    m.stop_all();
    assert_eq!(m.voice_count(), 0);
}

#[test]
fn retrigger_policies() {
    let mut m = Mixer::new();
    assert!(matches!(
        m.play("a", buf(0.1, 100), 1.0, RetriggerPolicy::Ignore),
        PlayOutcome::Started(_)
    ));
    assert_eq!(
        m.play("a", buf(0.1, 100), 1.0, RetriggerPolicy::Ignore),
        PlayOutcome::Ignored
    );
    assert_eq!(m.voice_count(), 1);

    let mut out = vec![0.0f32; 20];
    m.mix(&mut out);
    assert_eq!(m.progress("a"), Some((10, 100)));
    m.play("a", buf(0.1, 100), 1.0, RetriggerPolicy::Restart);
    assert_eq!(m.voice_count(), 1);
    assert_eq!(m.progress("a"), Some((0, 100)));

    m.play("a", buf(0.1, 100), 1.0, RetriggerPolicy::Overlap);
    assert_eq!(m.voice_count(), 2);
}

#[test]
fn voice_limit_and_empty_buffers() {
    let mut m = Mixer::new();
    for i in 0..MAX_VOICES {
        assert!(matches!(
            m.play(&format!("s{i}"), buf(0.01, 10), 1.0, RetriggerPolicy::Overlap),
            PlayOutcome::Started(_)
        ));
    }
    assert_eq!(
        m.play("extra", buf(0.01, 10), 1.0, RetriggerPolicy::Overlap),
        PlayOutcome::LimitReached
    );
    assert_eq!(
        m.play("e", Arc::from(Vec::<f32>::new()), 1.0, RetriggerPolicy::Overlap),
        PlayOutcome::Empty
    );
}

#[test]
fn non_finite_samples_become_silence() {
    let mut m = Mixer::new();
    m.play(
        "a",
        Arc::from(vec![f32::NAN, f32::INFINITY, 0.25, 0.25]),
        1.0,
        RetriggerPolicy::Overlap,
    );
    let mut out = vec![0.0f32; 4];
    m.mix(&mut out);
    assert_eq!(out, vec![0.0, 0.0, 0.25, 0.25]);
}

#[test]
fn output_gain_ramps_without_a_step() {
    let mut m = Mixer::new();
    m.play("a", buf(0.5, 400), 1.0, RetriggerPolicy::Overlap);
    m.set_output_gain(0.0);
    let mut out = vec![0.0f32; 200]; // 100 frames
    m.mix(&mut out);
    // Strictly falling from about 0.5 down to 0, never jumping by more than one ramp step.
    assert!(out[0] > 0.45, "{}", out[0]);
    assert!(out[198].abs() < 1e-6);
    for pair in out.chunks(2).collect::<Vec<_>>().windows(2) {
        assert!(pair[0][0] >= pair[1][0]);
        assert!(pair[0][0] - pair[1][0] < 0.02);
    }
    // Afterwards the route is silent but the voice keeps advancing (the sound is not paused).
    let mut next = vec![1.0f32; 20];
    m.mix(&mut next);
    assert!(next.iter().all(|s| *s == 0.0));
    assert!(m.is_playing("a"));
    assert_eq!(m.progress("a"), Some((110, 400)));
}

#[test]
fn snap_output_gain_is_immediate_and_routes_are_independent() {
    let mut phones = Mixer::new();
    let mut mic = Mixer::new();
    let data = buf(0.4, 8);
    phones.play("a", data.clone(), 1.0, RetriggerPolicy::Overlap);
    mic.play("a", data, 1.0, RetriggerPolicy::Overlap);
    phones.snap_output_gain(1.0);
    mic.snap_output_gain(0.25);
    let (mut p, mut q) = (vec![0.0f32; 8], vec![0.0f32; 8]);
    phones.mix(&mut p);
    mic.mix(&mut q);
    assert!(p.iter().all(|s| (*s - 0.4).abs() < 1e-6));
    assert!(q.iter().all(|s| (*s - 0.1).abs() < 1e-6));
    assert_eq!(mic.output_gain(), 0.25);
}

#[test]
fn invalid_output_gain_is_sanitized() {
    let mut m = Mixer::new();
    m.snap_output_gain(f32::NAN);
    assert_eq!(m.output_gain(), 1.0);
    m.snap_output_gain(50.0);
    assert_eq!(m.output_gain(), MAX_OUTPUT_GAIN);
    m.snap_output_gain(-3.0);
    assert_eq!(m.output_gain(), 0.0);
}

#[test]
fn loud_stacks_are_limited_and_recover() {
    let mut m = Mixer::new();
    for i in 0..8 {
        m.play(&format!("s{i}"), buf(0.9, 64), 1.0, RetriggerPolicy::Overlap);
    }
    let mut out = vec![0.0f32; 128];
    m.mix(&mut out);
    assert!(out.iter().all(|s| s.abs() < 1.0), "{out:?}");
    // A quiet sound right afterwards starts attenuated and returns towards unity.
    let mut m2 = Mixer::new();
    m2.play("loud", buf(1.0, 4), 2.0, RetriggerPolicy::Overlap);
    let mut o = vec![0.0f32; 8];
    m2.mix(&mut o);
    m2.play("quiet", buf(0.1, 20_000), 1.0, RetriggerPolicy::Overlap);
    let mut tail = vec![0.0f32; 40_000];
    m2.mix(&mut tail);
    assert!(tail[0] < 0.1, "limiter is still reducing the gain");
    assert!(
        (tail[39_998] - 0.1).abs() < 0.005,
        "gain has recovered: {}",
        tail[39_998]
    );
}

#[test]
fn sound_gain_can_be_changed_while_playing() {
    let mut m = Mixer::new();
    m.play("a", buf(0.4, 100), 1.0, RetriggerPolicy::Overlap);
    m.play("b", buf(0.4, 100), 1.0, RetriggerPolicy::Overlap);
    m.set_sound_gain("a", 0.0);
    let mut out = vec![0.0f32; 4];
    m.mix(&mut out);
    assert!(out.iter().all(|s| (*s - 0.4).abs() < 1e-6));
}

#[test]
fn soft_clip_is_transparent_below_the_knee_and_symmetric() {
    for i in -80..=80 {
        let x = i as f32 / 100.0;
        assert_eq!(soft_clip(x), x, "x={x}");
    }
    for &x in &[0.81f32, 0.9, 1.0, 1.5, 2.0, 2.6, 5.0, 100.0, f32::MAX, f32::INFINITY] {
        assert_eq!(soft_clip(-x), -soft_clip(x), "x={x}");
    }
}

#[test]
fn soft_clip_magnitude_stays_below_one_for_extreme_inputs() {
    for &x in &[0.81f32, 1.0, 2.6, 3.0, 10.0, 1.0e6, f32::MAX, f32::INFINITY] {
        let y = soft_clip(x);
        assert!(y > 0.8 && y < 1.0, "x={x} y={y}");
        let n = soft_clip(-x);
        assert!(n < -0.8 && n > -1.0, "x=-{x} y={n}");
    }
}

#[test]
fn soft_clip_is_monotonic_and_maps_nan_to_silence() {
    let mut prev = soft_clip(0.0);
    let mut x = 0.0f32;
    while x < 4.0 {
        x += 0.001;
        let y = soft_clip(x);
        assert!(y >= prev, "not monotonic at {x}: {y} < {prev}");
        prev = y;
    }
    assert_eq!(soft_clip(f32::NAN), 0.0);
    assert_eq!(soft_clip(0.0), 0.0);
}
