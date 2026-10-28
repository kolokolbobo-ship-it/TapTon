//! Device-independent software mixer. It sums stereo f32 voices into an output block, applies the
//! master volume, the per-route output gain (ramped, so changes and mutes never click), a peak
//! limiter and a final soft clipper. `mix` performs no allocation, I/O or locking, so it is safe to
//! run from the engine's mixing thread. Voices hold `Arc<[f32]>` samples; the engine keeps its own
//! reference in a cache so dropping a finished voice never frees the sample buffer on the mix path.
//!
//! All sample data is interleaved stereo at the mixer's sample rate.

use crate::settings::RetriggerPolicy;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

pub const CHANNELS: usize = 2;
pub const MAX_VOICES: usize = 32;
const SOFT_KNEE: f32 = 0.8;
/// Peaks above this level are scaled down by the limiter before the soft clipper sees them.
const LIMIT: f32 = 0.95;
/// Per-frame recovery speed of the limiter gain (about 40 ms at 48 kHz).
const LIMITER_RELEASE: f32 = 0.0005;
/// Highest route gain accepted by `set_output_gain`.
pub const MAX_OUTPUT_GAIN: f32 = 2.0;

/// Longest backlog of live audio that is kept, in seconds. Older audio is dropped so the voice
/// never lags further behind than this.
const LIVE_MAX_SECS: f64 = 0.12;
/// Live audio is held back until this much has arrived, so small hiccups do not crackle.
const LIVE_PRIME_SECS: f64 = 0.02;

struct LiveInner {
    /// Interleaved stereo at `rate`.
    queue: VecDeque<f32>,
    rate: u32,
    /// Fractional read position, in frames, relative to the front of the queue.
    pos: f64,
    /// False while the queue is refilling after start or after running dry.
    primed: bool,
}

/// A live audio source (the user's microphone) that is mixed into one output. The capture side
/// pushes interleaved stereo at the capture device's rate; the output side pulls frames at its own
/// rate (linear resampling). Shared through an `Arc`; every method is safe to call from audio
/// callbacks (short critical sections, no allocation once the queue has its capacity).
pub struct LiveInput {
    inner: Mutex<LiveInner>,
    /// Gain as `f32` bits, so the volume can change without locking the queue.
    gain: AtomicU32,
}

impl LiveInput {
    pub fn new(rate: u32) -> LiveInput {
        let rate = rate.max(1);
        let capacity = ((rate as f64 * LIVE_MAX_SECS) as usize + 64) * CHANNELS;
        LiveInput {
            inner: Mutex::new(LiveInner {
                queue: VecDeque::with_capacity(capacity),
                rate,
                pos: 0.0,
                primed: false,
            }),
            gain: AtomicU32::new(1.0f32.to_bits()),
        }
    }

    fn lock_inner(&self) -> MutexGuard<'_, LiveInner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn set_gain(&self, g: f32) {
        let g = if g.is_finite() {
            g.clamp(0.0, MAX_OUTPUT_GAIN)
        } else {
            1.0
        };
        self.gain.store(g.to_bits(), Ordering::Relaxed);
    }

    pub fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }

    /// Sample rate of the pushed audio.
    pub fn rate(&self) -> u32 {
        self.lock_inner().rate
    }

    /// Frames waiting to be played.
    pub fn queued_frames(&self) -> usize {
        self.lock_inner().queue.len() / CHANNELS
    }

    /// Append interleaved stereo samples (a trailing odd sample is ignored). If more than
    /// `LIVE_MAX_SECS` is waiting, the oldest audio is dropped.
    pub fn push_stereo(&self, samples: &[f32]) {
        let samples = &samples[..samples.len() & !1];
        let mut g = self.lock_inner();
        let max_samples = ((g.rate as f64 * LIVE_MAX_SECS) as usize).max(4) * CHANNELS;
        g.queue
            .extend(samples.iter().map(|s| if s.is_finite() { *s } else { 0.0 }));
        let excess = g.queue.len().saturating_sub(max_samples);
        if excess > 0 {
            for _ in 0..((excess + 1) & !1) {
                g.queue.pop_front();
            }
            g.pos = 0.0;
        }
    }

    /// Lock the queue once for a whole output block and read it frame by frame at `out_rate`.
    fn reader(&self, out_rate: u32) -> LiveReader<'_> {
        let inner = self.lock_inner();
        let step = inner.rate as f64 / out_rate.max(1) as f64;
        LiveReader {
            inner,
            step,
            gain: self.gain(),
        }
    }
}

/// Holds the queue lock while one output block is mixed.
struct LiveReader<'a> {
    inner: MutexGuard<'a, LiveInner>,
    step: f64,
    gain: f32,
}

impl LiveReader<'_> {
    /// The next output frame, or silence while the queue is empty or refilling.
    fn next_frame(&mut self) -> [f32; 2] {
        let step = self.step;
        let g = &mut *self.inner;
        let avail = g.queue.len() / CHANNELS;
        if !g.primed {
            let prime = ((g.rate as f64 * LIVE_PRIME_SECS) as usize).max(2);
            if avail < prime {
                return [0.0, 0.0];
            }
            g.primed = true;
        }
        let i0 = g.pos.floor() as usize;
        if i0 + 1 >= avail {
            g.primed = false;
            g.pos = 0.0;
            return [0.0, 0.0];
        }
        let frac = (g.pos - i0 as f64) as f32;
        let (a_l, a_r) = (g.queue[i0 * CHANNELS], g.queue[i0 * CHANNELS + 1]);
        let (b_l, b_r) = (g.queue[(i0 + 1) * CHANNELS], g.queue[(i0 + 1) * CHANNELS + 1]);
        let frame = [a_l + (b_l - a_l) * frac, a_r + (b_r - a_r) * frac];
        g.pos += step;
        let whole = g.pos.floor() as usize;
        if whole > 0 {
            if whole >= avail {
                g.queue.clear();
                g.pos = 0.0;
            } else {
                for _ in 0..whole * CHANNELS {
                    g.queue.pop_front();
                }
                g.pos -= whole as f64;
            }
        }
        frame
    }
}

struct Voice {
    id: u64,
    sound_id: String,
    data: Arc<[f32]>,
    pos: usize,
    gain: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayOutcome {
    Started(u64),
    /// The sound was already playing and the policy is `Ignore`.
    Ignored,
    LimitReached,
    /// The sample buffer contained no complete stereo frame.
    Empty,
}

pub struct Mixer {
    voices: Vec<Voice>,
    master: f32,
    /// Gain of this output (headphones or microphone route); `out_gain` ramps towards `out_target`.
    out_gain: f32,
    out_target: f32,
    limiter: f32,
    next_id: u64,
    /// Optional live source (the user's microphone) and the sample rate of this output.
    live: Option<Arc<LiveInput>>,
    live_out_rate: u32,
}

fn sanitize_output_gain(g: f32) -> f32 {
    if g.is_finite() {
        g.clamp(0.0, MAX_OUTPUT_GAIN)
    } else {
        1.0
    }
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

/// Largest `f32` strictly below 1.0 (1 - 2^-24). In `f32`, `tanh(t)` rounds to exactly 1.0 for
/// t > ~9, so `SOFT_KNEE + (1 - SOFT_KNEE) * tanh(t)` reaches 1.0 for |x| > ~2.6. The saturated
/// output is therefore pinned to this value to keep the "magnitude < 1" guarantee.
const SOFT_CEIL: f32 = 1.0 - f32::EPSILON / 2.0;

/// Transparent below the knee, smoothly saturating above it; output magnitude is always < 1.
/// Odd-symmetric. NaN maps to silence; +/-infinity saturates to +/-`SOFT_CEIL`.
pub fn soft_clip(x: f32) -> f32 {
    if x.is_nan() {
        return 0.0;
    }
    let a = x.abs();
    if a <= SOFT_KNEE {
        return x;
    }
    let shaped = SOFT_KNEE + (1.0 - SOFT_KNEE) * ((a - SOFT_KNEE) / (1.0 - SOFT_KNEE)).tanh();
    x.signum() * shaped.min(SOFT_CEIL)
}

impl Mixer {
    pub fn new() -> Self {
        Self {
            voices: Vec::with_capacity(MAX_VOICES),
            master: 1.0,
            out_gain: 1.0,
            out_target: 1.0,
            limiter: 1.0,
            next_id: 1,
            live: None,
            live_out_rate: 48_000,
        }
    }

    /// Mix a live source into this output. Its volume is its own (`LiveInput::set_gain`): neither
    /// the master volume nor the route gain or mute applies to it, because those control the
    /// soundboard sounds, not the user's voice.
    pub fn attach_live(&mut self, live: Arc<LiveInput>, out_rate: u32) {
        self.live = Some(live);
        self.live_out_rate = out_rate.max(1);
    }

    pub fn detach_live(&mut self) {
        self.live = None;
    }

    pub fn has_live(&self) -> bool {
        self.live.is_some()
    }

    pub fn set_master_volume(&mut self, v: f32) {
        self.master = if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
    }

    pub fn master_volume(&self) -> f32 {
        self.master
    }

    /// Change the gain of this output. The change is ramped over the next block (no click).
    pub fn set_output_gain(&mut self, g: f32) {
        self.out_target = sanitize_output_gain(g);
    }

    /// Set the output gain immediately (used when an output is first opened).
    pub fn snap_output_gain(&mut self, g: f32) {
        self.out_target = sanitize_output_gain(g);
        self.out_gain = self.out_target;
    }

    pub fn output_gain(&self) -> f32 {
        self.out_target
    }

    /// Change the gain of the voices already playing for `sound_id`.
    pub fn set_sound_gain(&mut self, sound_id: &str, gain: f32) {
        let gain = if gain.is_finite() { gain.clamp(0.0, 2.0) } else { 1.0 };
        for v in self.voices.iter_mut().filter(|v| v.sound_id == sound_id) {
            v.gain = gain;
        }
    }

    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    /// Whether at least one soundboard sample still has frames to render.
    pub fn has_active_voices(&self) -> bool {
        !self.voices.is_empty()
    }

    pub fn is_playing(&self, sound_id: &str) -> bool {
        self.voices.iter().any(|v| v.sound_id == sound_id)
    }

    /// (position, total) in frames of the most recently started voice of this sound.
    pub fn progress(&self, sound_id: &str) -> Option<(u64, u64)> {
        self.voices
            .iter()
            .rev()
            .find(|v| v.sound_id == sound_id)
            .map(|v| ((v.pos / CHANNELS) as u64, (v.data.len() / CHANNELS) as u64))
    }

    pub fn play(&mut self, sound_id: &str, data: Arc<[f32]>, gain: f32, policy: RetriggerPolicy) -> PlayOutcome {
        if data.len() < CHANNELS {
            return PlayOutcome::Empty;
        }
        match policy {
            RetriggerPolicy::Ignore if self.is_playing(sound_id) => return PlayOutcome::Ignored,
            RetriggerPolicy::Restart => {
                self.stop(sound_id);
            }
            _ => {}
        }
        if self.voices.len() >= MAX_VOICES {
            return PlayOutcome::LimitReached;
        }
        let id = self.next_id;
        self.next_id += 1;
        let gain = if gain.is_finite() { gain.clamp(0.0, 2.0) } else { 1.0 };
        self.voices.push(Voice {
            id,
            sound_id: sound_id.to_string(),
            data,
            pos: 0,
            gain,
        });
        PlayOutcome::Started(id)
    }

    /// Stop every voice of `sound_id`; returns how many were stopped.
    pub fn stop(&mut self, sound_id: &str) -> usize {
        let before = self.voices.len();
        self.voices.retain(|v| v.sound_id != sound_id);
        before - self.voices.len()
    }

    pub fn stop_voice(&mut self, voice_id: u64) -> bool {
        let before = self.voices.len();
        self.voices.retain(|v| v.id != voice_id);
        before != self.voices.len()
    }

    pub fn stop_all(&mut self) {
        self.voices.clear();
    }

    /// Fill `out` (interleaved stereo; an odd trailing sample is written as silence) with the mix of
    /// all active voices. Finished voices are removed.
    pub fn mix(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let len = out.len() & !1;
        for v in &mut self.voices {
            let n = (v.data.len() - v.pos).min(len);
            for (o, s) in out[..n].iter_mut().zip(&v.data[v.pos..v.pos + n]) {
                *o += *s * v.gain;
            }
            v.pos += n;
        }
        self.voices.retain(|v| v.pos < v.data.len());

        let m = self.master;
        let frames = (len / CHANNELS).max(1);
        let (start, end) = (self.out_gain, self.out_target);
        let ramping = (start - end).abs() > 1e-6;
        let step = if ramping { (end - start) / frames as f32 } else { 0.0 };
        let mut limiter = self.limiter;
        let mut live = self.live.as_ref().map(|l| l.reader(self.live_out_rate));
        let live_gain = live.as_ref().map_or(0.0, |r| r.gain);
        for (i, frame) in out.chunks_mut(CHANNELS).enumerate() {
            let g = if ramping { start + step * (i + 1) as f32 } else { end };
            let voice = match live.as_mut() {
                Some(r) => r.next_frame(),
                None => [0.0, 0.0],
            };
            let mut peak = 0.0f32;
            for (c, o) in frame.iter_mut().enumerate() {
                let x = *o * m * g + voice[c.min(1)] * live_gain;
                *o = if x.is_finite() { x } else { 0.0 };
                peak = peak.max(o.abs());
            }
            // Instant attack, smooth release: loud stacks of voices are scaled down instead of
            // being hard-clipped.
            let wanted = if peak > LIMIT { LIMIT / peak } else { 1.0 };
            limiter = if wanted < limiter {
                wanted
            } else {
                limiter + (1.0 - limiter) * LIMITER_RELEASE
            };
            for o in frame.iter_mut() {
                *o = soft_clip(*o * limiter);
            }
        }
        drop(live);
        self.limiter = limiter;
        self.out_gain = end;
    }
}
