//! Memory-bounded cache of decoded and resampled sounds.
//!
//! The old cache was bounded by the *number* of files, so a few long files could use hundreds of
//! megabytes. This cache is bounded by size and evicts the least recently used entries first.
//! Sounds that are currently playing stay alive through the `Arc` held by their voices, so
//! evicting an entry never interrupts playback.

use crate::audio::engine::DecodedAudio;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Default limit for all cached sample data together.
pub const DEFAULT_CACHE_BYTES: usize = 256 * 1024 * 1024;

struct Slot<T> {
    value: T,
    bytes: usize,
    used: u64,
}

pub struct AudioCache {
    decoded: HashMap<PathBuf, Slot<Arc<DecodedAudio>>>,
    resampled: HashMap<(PathBuf, u32), Slot<Arc<[f32]>>>,
    total: usize,
    limit: usize,
    tick: u64,
}

impl AudioCache {
    pub fn new(limit_bytes: usize) -> Self {
        Self {
            decoded: HashMap::new(),
            resampled: HashMap::new(),
            total: 0,
            limit: limit_bytes,
            tick: 0,
        }
    }

    fn touch(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    pub fn bytes(&self) -> usize {
        self.total
    }

    pub fn len(&self) -> usize {
        self.decoded.len() + self.resampled.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get_decoded(&mut self, path: &Path) -> Option<Arc<DecodedAudio>> {
        let t = self.touch();
        self.decoded.get_mut(path).map(|s| {
            s.used = t;
            Arc::clone(&s.value)
        })
    }

    pub fn insert_decoded(&mut self, path: &Path, audio: Arc<DecodedAudio>) {
        let bytes = audio.samples.len() * std::mem::size_of::<f32>();
        let used = self.touch();
        if let Some(old) = self.decoded.insert(
            path.to_path_buf(),
            Slot {
                value: audio,
                bytes,
                used,
            },
        ) {
            self.total -= old.bytes;
        }
        self.total += bytes;
        self.evict();
    }

    pub fn get_resampled(&mut self, path: &Path, rate: u32) -> Option<Arc<[f32]>> {
        let t = self.touch();
        self.resampled.get_mut(&(path.to_path_buf(), rate)).map(|s| {
            s.used = t;
            Arc::clone(&s.value)
        })
    }

    pub fn insert_resampled(&mut self, path: &Path, rate: u32, data: Arc<[f32]>) {
        let bytes = data.len() * std::mem::size_of::<f32>();
        let used = self.touch();
        if let Some(old) = self.resampled.insert(
            (path.to_path_buf(), rate),
            Slot {
                value: data,
                bytes,
                used,
            },
        ) {
            self.total -= old.bytes;
        }
        self.total += bytes;
        self.evict();
    }

    /// Forget everything about a file (for example after it was removed from the library).
    pub fn forget(&mut self, path: &Path) {
        if let Some(old) = self.decoded.remove(path) {
            self.total -= old.bytes;
        }
        let keys: Vec<(PathBuf, u32)> = self.resampled.keys().filter(|(p, _)| p == path).cloned().collect();
        for k in keys {
            if let Some(old) = self.resampled.remove(&k) {
                self.total -= old.bytes;
            }
        }
    }

    pub fn clear(&mut self) {
        self.decoded.clear();
        self.resampled.clear();
        self.total = 0;
    }

    /// Drop the least recently used entries until the cache fits its limit. The most recently
    /// used entry is always kept, even when it alone exceeds the limit.
    fn evict(&mut self) {
        while self.total > self.limit && self.len() > 1 {
            let oldest_decoded = self
                .decoded
                .iter()
                .min_by_key(|(_, s)| s.used)
                .map(|(k, s)| (k.clone(), s.used));
            let oldest_resampled = self
                .resampled
                .iter()
                .min_by_key(|(_, s)| s.used)
                .map(|(k, s)| (k.clone(), s.used));
            match (oldest_decoded, oldest_resampled) {
                (Some((k, a)), Some((_, b))) if a <= b => self.drop_decoded(&k),
                (Some((k, _)), None) => self.drop_decoded(&k),
                (_, Some((k, _))) => {
                    if let Some(old) = self.resampled.remove(&k) {
                        self.total -= old.bytes;
                    }
                }
                (None, None) => break,
            }
        }
    }

    fn drop_decoded(&mut self, key: &Path) {
        if let Some(old) = self.decoded.remove(key) {
            self.total -= old.bytes;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(frames: usize) -> Arc<DecodedAudio> {
        Arc::new(DecodedAudio {
            samples: Arc::from(vec![0.0f32; frames * 2]),
            rate: 48_000,
        })
    }

    #[test]
    fn counts_bytes_and_returns_entries() {
        let mut c = AudioCache::new(1 << 20);
        c.insert_decoded(Path::new("a"), audio(100));
        assert_eq!(c.bytes(), 800);
        assert!(c.get_decoded(Path::new("a")).is_some());
        assert!(c.get_decoded(Path::new("b")).is_none());
        c.insert_resampled(Path::new("a"), 44_100, Arc::from(vec![0.0f32; 10]));
        assert_eq!(c.bytes(), 840);
        assert!(c.get_resampled(Path::new("a"), 44_100).is_some());
        assert!(c.get_resampled(Path::new("a"), 48_000).is_none());
    }

    #[test]
    fn evicts_least_recently_used_first() {
        let mut c = AudioCache::new(2_000); // room for two 800-byte sounds
        c.insert_decoded(Path::new("a"), audio(100));
        c.insert_decoded(Path::new("b"), audio(100));
        assert!(c.get_decoded(Path::new("a")).is_some()); // "a" is now newer than "b"
        c.insert_decoded(Path::new("c"), audio(100));
        assert!(c.get_decoded(Path::new("b")).is_none(), "b was the least recently used");
        assert!(c.get_decoded(Path::new("a")).is_some());
        assert!(c.get_decoded(Path::new("c")).is_some());
        assert!(c.bytes() <= 2_000);
    }

    #[test]
    fn an_oversized_entry_is_kept_alone_and_playing_sounds_survive_eviction() {
        let mut c = AudioCache::new(100);
        let big = audio(1_000);
        c.insert_decoded(Path::new("big"), Arc::clone(&big));
        assert_eq!(c.len(), 1);
        c.insert_decoded(Path::new("next"), audio(1_000));
        assert!(c.get_decoded(Path::new("big")).is_none());
        assert_eq!(big.samples.len(), 2_000, "a voice still holding the Arc keeps its data");
    }

    #[test]
    fn forget_removes_all_variants_of_a_file() {
        let mut c = AudioCache::new(1 << 20);
        c.insert_decoded(Path::new("a"), audio(10));
        c.insert_resampled(Path::new("a"), 44_100, Arc::from(vec![0.0f32; 8]));
        c.insert_resampled(Path::new("b"), 44_100, Arc::from(vec![0.0f32; 8]));
        c.forget(Path::new("a"));
        assert_eq!(c.len(), 1);
        assert_eq!(c.bytes(), 32);
        c.clear();
        assert!(c.is_empty());
        assert_eq!(c.bytes(), 0);
    }
}
