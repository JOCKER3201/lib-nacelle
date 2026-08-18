//! Sound events, sound themes and mixing.
//!
//! A sound theme is a directory holding audio files plus a "meta" file
//! that maps interface EVENTS to those files:
//!
//! ```text
//! Click=click.wav
//! Key=key1.wav key2.wav key3.wav key4.wav
//! ```
//!
//! The theme is the single source of truth for what plays when: nothing
//! in the program hard-codes a file name. An event with no entry, or one
//! naming a file that will not load, is simply silent.
//!
//! Everything here is platform-independent. Widgets anywhere in the tree
//! call `emit()` when something happens; the application drains the queue
//! once per frame and feeds the clips to a `Mixer`, which renders plain
//! interleaved f32 frames. Handing those frames to an audio device is the
//! one platform-specific step and lives in the per-platform application,
//! exactly like window creation and rendering.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// Something the interface did that a theme may attach a sound to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Event {
    Boot,
    Shutdown,
    Key,
    KeyReturn,
    KeyErase,
    Hover,
    Click,
    ToggleOn,
    ToggleOff,
    PanelOpen,
    PanelClose,
    Alert,
    Error,
    Theme,
    Grab,
    Drop,
    Snap,
    Save,
    /// Looped continuously for as long as the program runs.
    Ambient,
}

impl Event {
    pub const ALL: [Event; 19] = [
        Event::Boot,
        Event::Shutdown,
        Event::Key,
        Event::KeyReturn,
        Event::KeyErase,
        Event::Hover,
        Event::Click,
        Event::ToggleOn,
        Event::ToggleOff,
        Event::PanelOpen,
        Event::PanelClose,
        Event::Alert,
        Event::Error,
        Event::Theme,
        Event::Grab,
        Event::Drop,
        Event::Snap,
        Event::Save,
        Event::Ambient,
    ];

    /// The key this event has in a theme's meta file.
    pub fn key(self) -> &'static str {
        match self {
            Event::Boot => "Boot",
            Event::Shutdown => "Shutdown",
            Event::Key => "Key",
            Event::KeyReturn => "KeyReturn",
            Event::KeyErase => "KeyErase",
            Event::Hover => "Hover",
            Event::Click => "Click",
            Event::ToggleOn => "ToggleOn",
            Event::ToggleOff => "ToggleOff",
            Event::PanelOpen => "PanelOpen",
            Event::PanelClose => "PanelClose",
            Event::Alert => "Alert",
            Event::Error => "Error",
            Event::Theme => "Theme",
            Event::Grab => "Grab",
            Event::Drop => "Drop",
            Event::Snap => "Snap",
            Event::Save => "Save",
            Event::Ambient => "Ambient",
        }
    }

    /// Stable number for crossing a plugin boundary, where an enum's
    /// layout cannot be relied on. Positions are fixed by `ALL`.
    pub fn id(self) -> u32 {
        Event::ALL.iter().position(|e| *e == self).unwrap_or(0) as u32
    }

    /// The event a stable number names; out of range is dropped.
    pub fn from_id(id: u32) -> Option<Event> {
        Event::ALL.get(id as usize).copied()
    }

    pub fn from_key(s: &str) -> Option<Event> {
        Event::ALL.into_iter().find(|e| e.key().eq_ignore_ascii_case(s))
    }

    /// Whether this event belongs to typing — the settings offer a
    /// separate mute for it, because it fires far more than the rest.
    pub fn is_typing(self) -> bool {
        matches!(self, Event::Key | Event::KeyReturn | Event::KeyErase)
    }
}

// ---------------------------------------------------------------- queue

/// Events emitted since the last drain. A single append-only queue for
/// the whole application: there is one audio output, and this way any
/// widget can report what it did without every call site having to be
/// handed a mixer.
fn queue() -> &'static Mutex<Vec<Event>> {
    static Q: OnceLock<Mutex<Vec<Event>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(Vec::new()))
}

/// Reports that something happened. Cheap and always safe to call, even
/// when the program has no audio at all.
pub fn emit(e: Event) {
    crate::runtime::shared(
        "sound::emit",
        || {
            if let Ok(mut q) = queue().lock() {
                // A frame that somehow produced a flood must not grow
                // forever.
                if q.len() < 64 {
                    q.push(e);
                }
            }
        },
        |api| (api.emit_sound)(e.id()),
        (),
    )
}

/// Takes everything emitted since the last call.
pub fn drain(out: &mut Vec<Event>) {
    out.clear();
    if let Ok(mut q) = queue().lock() {
        std::mem::swap(&mut *q, out);
        q.clear();
    }
}

// ------------------------------------------------------------------ wav

/// Decodes a RIFF/WAVE file into mono f32 samples plus its sample rate.
///
/// Handles 8/16/24/32-bit PCM and 32-bit float, any channel count
/// (downmixed to mono). This parses files the user can replace at will,
/// so every read is bounds-checked and a malformed file returns None
/// rather than panicking.
pub fn decode_wav(bytes: &[u8]) -> Option<(Vec<f32>, u32)> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let u16at = |b: &[u8], i: usize| -> Option<u16> {
        Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
    };
    let u32at = |b: &[u8], i: usize| -> Option<u32> {
        Some(u32::from_le_bytes([
            *b.get(i)?,
            *b.get(i + 1)?,
            *b.get(i + 2)?,
            *b.get(i + 3)?,
        ]))
    };

    let mut format = 0u16;
    let mut channels = 0u16;
    let mut rate = 0u32;
    let mut bits = 0u16;
    let mut data: Option<&[u8]> = None;

    let mut pos = 12usize;
    while pos + 8 <= bytes.len() {
        let id = bytes.get(pos..pos + 4)?;
        let size = u32at(bytes, pos + 4)? as usize;
        let body_start = pos + 8;
        // A truncated final chunk is used as far as it goes.
        let body_end = body_start.saturating_add(size).min(bytes.len());
        let body = bytes.get(body_start..body_end)?;
        match id {
            b"fmt " => {
                format = u16at(body, 0)?;
                channels = u16at(body, 2)?;
                rate = u32at(body, 4)?;
                bits = u16at(body, 14)?;
                // WAVE_FORMAT_EXTENSIBLE: the real format sits in the
                // extension's GUID; its first two bytes are the tag.
                if format == 0xFFFE {
                    format = u16at(body, 24).unwrap_or(1);
                }
            }
            b"data" => data = Some(body),
            _ => {}
        }
        // Chunks are word-aligned; guard against a zero/absurd size
        // making no progress or wrapping.
        let step = size.checked_add(size & 1)?.checked_add(8)?;
        pos = pos.checked_add(step)?;
    }

    let data = data?;
    if channels == 0 || rate == 0 {
        return None;
    }
    let ch = channels as usize;

    // Per-sample decode into interleaved f32.
    let flat: Vec<f32> = match (format, bits) {
        (1, 8) => data.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect(),
        (1, 16) => data
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect(),
        (1, 24) => data
            .chunks_exact(3)
            .map(|c| {
                let v = i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8;
                v as f32 / 8_388_608.0
            })
            .collect(),
        (1, 32) => data
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0)
            .collect(),
        (3, 32) => data
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
        _ => return None,
    };

    if flat.is_empty() {
        return None;
    }
    // Downmix to mono; a non-finite sample in a hand-made file would
    // otherwise poison the whole mix.
    let mono: Vec<f32> = flat
        .chunks_exact(ch)
        .map(|frame| {
            let sum: f32 = frame.iter().map(|s| if s.is_finite() { *s } else { 0.0 }).sum();
            sum / ch as f32
        })
        .collect();
    if mono.is_empty() {
        return None;
    }
    Some((mono, rate))
}

/// Linear resample to the output rate. UI sounds are short and broadband;
/// anything fancier would not be audible here.
fn resample(src: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || from == 0 || src.is_empty() {
        return src.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let n = ((src.len() as f64 * ratio).round() as usize).max(1);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let x = i as f64 / ratio;
        let i0 = x.floor() as usize;
        let frac = (x - i0 as f64) as f32;
        let a = src.get(i0).copied().unwrap_or(0.0);
        let b = src.get(i0 + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
    out
}

// ---------------------------------------------------------------- theme

pub type Clip = Arc<Vec<f32>>;

/// The clips a theme provides, keyed by event. Several clips on one
/// event are variants: they are handed out in turn so that a repeated
/// action never sounds mechanical.
pub struct SoundTheme {
    clips: HashMap<Event, Vec<Clip>>,
    next: HashMap<Event, usize>,
}

impl SoundTheme {
    pub fn empty() -> Self {
        SoundTheme {
            clips: HashMap::new(),
            next: HashMap::new(),
        }
    }

    /// Loads the theme in `dir`, resampling every clip to `rate`.
    ///
    /// The meta file decides everything; a listed file that is missing or
    /// unreadable simply leaves its event silent, so a partial theme is
    /// perfectly valid.
    pub fn load(dir: &Path, rate: u32) -> Self {
        let mut theme = SoundTheme::empty();
        let Ok(text) = std::fs::read_to_string(dir.join("meta")) else {
            return theme;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let key = key.trim();
            let value = value.trim();
            // Description= is documentation for whoever opens the file;
            // the program has nowhere to show it and skips it like any
            // other key that names no event.
            let Some(event) = Event::from_key(key) else { continue };
            let mut clips = Vec::new();
            for name in value.split_whitespace() {
                // The meta file names files inside its own directory; a
                // path that tries to leave it is ignored.
                if name.contains('/') || name.contains('\\') || name == ".." {
                    continue;
                }
                let Ok(bytes) = std::fs::read(dir.join(name)) else { continue };
                let Some((samples, src_rate)) = decode_wav(&bytes) else { continue };
                clips.push(Arc::new(resample(&samples, src_rate, rate)));
            }
            if !clips.is_empty() {
                theme.clips.insert(event, clips);
            }
        }
        theme
    }

    /// The next clip for an event, rotating through its variants.
    pub fn clip(&mut self, e: Event) -> Option<Clip> {
        let clips = self.clips.get(&e)?;
        if clips.is_empty() {
            return None;
        }
        let i = self.next.entry(e).or_insert(0);
        let clip = clips.get(*i % clips.len()).cloned();
        *i = i.wrapping_add(1);
        clip
    }

    /// How many events the theme actually provides clips for.
    pub fn event_count(&self) -> usize {
        self.clips.values().filter(|c| !c.is_empty()).count()
    }
}

// --------------------------------------------------------------- mixer

/// Beyond this many overlapping sounds the oldest is dropped: a key held
/// down on repeat must not pile up unbounded.
const MAX_VOICES: usize = 24;

struct Voice {
    clip: Clip,
    pos: usize,
    gain: f32,
}

/// Renders playing clips into interleaved f32 frames. Pure arithmetic —
/// no device, no threads — so the platform layer only has to hand it an
/// output buffer.
pub struct Mixer {
    voices: Vec<Voice>,
    ambient: Option<Voice>,
    volume: f32,
}

impl Default for Mixer {
    fn default() -> Self {
        Mixer::new()
    }
}

impl Mixer {
    pub fn new() -> Self {
        Mixer {
            voices: Vec::new(),
            ambient: None,
            volume: 1.0,
        }
    }

    pub fn play(&mut self, clip: Clip, gain: f32) {
        if clip.is_empty() {
            return;
        }
        if self.voices.len() >= MAX_VOICES {
            self.voices.remove(0);
        }
        self.voices.push(Voice { clip, pos: 0, gain });
    }

    /// Starts, replaces or (with None) stops the looping background bed.
    pub fn set_ambient(&mut self, clip: Option<Clip>) {
        self.ambient = clip
            .filter(|c| !c.is_empty())
            .map(|clip| Voice { clip, pos: 0, gain: 1.0 });
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
    }

    /// Fills an interleaved output buffer. Every channel gets the same
    /// signal: these are mono UI sounds and panning them would only make
    /// the interface feel lopsided.
    pub fn fill(&mut self, out: &mut [f32], channels: usize) {
        for s in out.iter_mut() {
            *s = 0.0;
        }
        if channels == 0 {
            return;
        }
        let frames = out.len() / channels;
        let master = self.volume;

        for f in 0..frames {
            let mut acc = 0.0f32;
            for v in self.voices.iter_mut() {
                if let Some(s) = v.clip.get(v.pos) {
                    acc += s * v.gain;
                    v.pos += 1;
                }
            }
            if let Some(a) = self.ambient.as_mut() {
                if let Some(s) = a.clip.get(a.pos) {
                    acc += s * a.gain;
                    a.pos += 1;
                }
                // Seamless loop: the clip is authored to meet its own end.
                if a.pos >= a.clip.len() {
                    a.pos = 0;
                }
            }
            // Soft clip: many simultaneous sounds must not tear.
            let v = (acc * master).clamp(-1.0, 1.0);
            let base = f * channels;
            for c in 0..channels {
                if let Some(o) = out.get_mut(base + c) {
                    *o = v;
                }
            }
        }

        self.voices.retain(|v| v.pos < v.clip.len());
    }

    /// Whether a clip that will END is still being rendered.
    ///
    /// The ambient bed does not count, and that is the whole point of
    /// the question: it loops for as long as the program runs, so an
    /// answer that included it would never become false and anyone
    /// waiting on it would wait forever. What this answers is "has
    /// everything that was going to finish finished", which is what
    /// the farewell sound at shutdown needs to know.
    pub fn playing(&self) -> bool {
        !self.voices.is_empty()
    }
}

// -------------------------------------------------- the mixer, shared

/// A [`Mixer`] plus the signal that says the last finite voice has just
/// finished — the pair a device thread and the rest of the program hold
/// between them.
///
/// The signal could have been a bare `Condvar` beside the mutex at every
/// call site, but then raising it would be the obligation of whoever
/// calls [`Mixer::fill`], and the cost of forgetting is invisible:
/// nothing breaks, every wait simply runs to its timeout instead of
/// ending when the sound does. Filling THROUGH this type is what makes
/// the edge impossible to miss.
pub struct SharedMixer {
    mixer: Mutex<Mixer>,
    /// Raised on the playing -> silent EDGE only. Signalling on every
    /// period instead would spend a wake syscall per audio buffer —
    /// ~190 a second, for the whole session — on behalf of a waiter
    /// that exists twice in a run.
    drained: Condvar,
}

impl Default for SharedMixer {
    fn default() -> Self {
        SharedMixer::new()
    }
}

impl SharedMixer {
    pub fn new() -> SharedMixer {
        SharedMixer {
            mixer: Mutex::new(Mixer::new()),
            drained: Condvar::new(),
        }
    }

    /// The mixer itself, for the callers that only set something.
    ///
    /// A panic elsewhere must not silence the desktop for the rest of
    /// the session, so a poisoned lock is taken anyway: the worst a
    /// half-written `Mixer` holds is a voice at a wrong position, which
    /// is one wrong buffer of sound, and the alternative is no sound at
    /// all ever again.
    pub fn lock(&self) -> MutexGuard<'_, Mixer> {
        self.mixer.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Renders one output buffer and raises the drained edge if this was
    /// the buffer that emptied the mixer. The device thread's one call.
    pub fn fill(&self, out: &mut [f32], channels: usize) {
        let mut m = self.lock();
        let was = m.playing();
        m.fill(out, channels);
        if was && !m.playing() {
            self.drained.notify_all();
        }
    }

    /// Waits until nothing finite is playing any more, or until `cap`
    /// runs out; true means the sound finished, false that the cap did.
    ///
    /// The predicate is tested BEFORE the first wait, so a clip that
    /// finishes between being started and being waited on cannot lose
    /// its edge — a missed wakeup here would look exactly like the
    /// fixed-length sleep this replaced.
    pub fn wait_drained(&self, cap: Duration) -> bool {
        let deadline = Instant::now() + cap;
        let mut m = self.lock();
        while m.playing() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            let (guard, _) = self
                .drained
                .wait_timeout(m, left)
                .unwrap_or_else(|e| e.into_inner());
            m = guard;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav16(samples: &[i16], channels: u16, rate: u32) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes()); // PCM
        v.extend_from_slice(&channels.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        v.extend_from_slice(&(channels * 2).to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(&data);
        v
    }

    #[test]
    fn decodes_mono_and_downmixes_stereo() {
        let (mono, rate) = decode_wav(&wav16(&[0, 16384, -16384], 1, 48000)).unwrap();
        assert_eq!(rate, 48000);
        assert_eq!(mono.len(), 3);
        assert!((mono[1] - 0.5).abs() < 0.001);

        // Stereo hard-panned left averages to half amplitude.
        let (m, _) = decode_wav(&wav16(&[16384, 0, 16384, 0], 2, 44100)).unwrap();
        assert_eq!(m.len(), 2);
        assert!((m[0] - 0.25).abs() < 0.001);
    }

    #[test]
    fn malformed_files_return_none_without_panicking() {
        assert!(decode_wav(b"").is_none());
        assert!(decode_wav(b"RIFF").is_none());
        assert!(decode_wav(b"RIFF\x00\x00\x00\x00WAVE").is_none());
        // A chunk claiming a huge size must not run off the end.
        let mut v = wav16(&[1, 2, 3], 1, 48000);
        v[16] = 0xFF;
        v[17] = 0xFF;
        v[18] = 0xFF;
        v[19] = 0xFF;
        let _ = decode_wav(&v);
        // Zero-length chunk id must not spin forever.
        let mut z = Vec::from(*b"RIFF\x00\x00\x00\x00WAVE");
        z.extend_from_slice(b"junk\x00\x00\x00\x00");
        assert!(decode_wav(&z).is_none());
    }

    #[test]
    fn resampling_scales_length() {
        let src = vec![0.0f32; 100];
        assert_eq!(resample(&src, 48000, 24000).len(), 50);
        assert_eq!(resample(&src, 48000, 48000).len(), 100);
    }

    #[test]
    fn mixer_plays_loops_and_retires_voices() {
        let mut m = Mixer::new();
        m.play(Arc::new(vec![1.0, 1.0]), 0.5);
        let mut out = vec![0.0f32; 8]; // 4 frames, stereo
        m.fill(&mut out, 2);
        // Both channels carry the same signal.
        assert!((out[0] - 0.5).abs() < 0.001);
        assert!((out[1] - 0.5).abs() < 0.001);
        // The clip was 2 frames long; frame 3 is silent and the voice is gone.
        assert_eq!(out[4], 0.0);
        m.fill(&mut out, 2);
        assert!(out.iter().all(|s| *s == 0.0));

        // Ambient wraps instead of ending.
        m.set_ambient(Some(Arc::new(vec![1.0])));
        m.fill(&mut out, 2);
        assert!(out.iter().all(|s| (*s - 1.0).abs() < 0.001));
    }

    #[test]
    fn theme_rotates_variants_and_queue_drains() {
        let mut t = SoundTheme::empty();
        t.clips.insert(
            Event::Key,
            vec![Arc::new(vec![1.0]), Arc::new(vec![2.0])],
        );
        assert_eq!(t.clip(Event::Key).unwrap()[0], 1.0);
        assert_eq!(t.clip(Event::Key).unwrap()[0], 2.0);
        assert_eq!(t.clip(Event::Key).unwrap()[0], 1.0);
        assert!(t.clip(Event::Boot).is_none());

        assert_eq!(Event::from_key("panelopen"), Some(Event::PanelOpen));
        assert!(Event::from_key("Nonsense").is_none());

        let mut got = Vec::new();
        emit(Event::Click);
        emit(Event::Save);
        drain(&mut got);
        assert_eq!(got, vec![Event::Click, Event::Save]);
        drain(&mut got);
        assert!(got.is_empty());
    }

    /// A stand-in for the device thread: renders the shared mixer in
    /// period-sized buffers until it is asked to stop, exactly like the
    /// ALSA writer in the desktop does, and at no particular speed —
    /// the whole point of the mechanism under test is that the waiter
    /// does not care how fast the card runs.
    fn spin_device(
        mixer: Arc<SharedMixer>,
        stop: Arc<std::sync::atomic::AtomicBool>,
        period: usize,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut buf = vec![0.0f32; period * 2];
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                mixer.fill(&mut buf, 2);
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    }

    /// THE EVENT ENDS THE WAIT, NOT THE CLOCK. The cap here is minutes;
    /// the clip is a few hundred frames. If the wait were a sleep of any
    /// fixed length derived from the cap, this test would sit here for
    /// that long and then fail on the elapsed assertion.
    #[test]
    fn waiting_for_the_farewell_ends_when_the_sound_does() {
        let mixer = Arc::new(SharedMixer::new());
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dev = spin_device(mixer.clone(), stop.clone(), 64);

        mixer.lock().play(Arc::new(vec![0.5f32; 256]), 1.0);
        let t0 = Instant::now();
        let drained = mixer.wait_drained(Duration::from_secs(120));
        let waited = t0.elapsed();

        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        dev.join().unwrap();

        assert!(drained, "the wait must end on the sound, not on the cap");
        assert!(
            waited < Duration::from_secs(5),
            "waited {waited:?} for a clip the device drains in milliseconds"
        );
        assert!(!mixer.lock().playing());
    }

    /// The other half of the contract: a device that never renders must
    /// not be able to hold the exit open. The cap is what bounds it, and
    /// the answer says which of the two ended the wait.
    #[test]
    fn a_silent_device_cannot_hold_the_exit_open() {
        let mixer = Arc::new(SharedMixer::new());
        mixer.lock().play(Arc::new(vec![0.5f32; 48_000]), 1.0);

        let t0 = Instant::now();
        let drained = mixer.wait_drained(Duration::from_millis(60));
        let waited = t0.elapsed();

        assert!(!drained, "nothing rendered, so nothing can have drained");
        assert!(waited >= Duration::from_millis(60));
        assert!(
            waited < Duration::from_secs(5),
            "the cap must end the wait promptly, waited {waited:?}"
        );
    }

    /// The ambient bed loops for the life of the program. Counting it as
    /// "playing" would mean every exit with sound enabled paid the full
    /// cap instead of the length of the farewell — the failure mode is
    /// silent, so it gets a test of its own.
    #[test]
    fn the_looping_bed_does_not_count_as_playing() {
        let mixer = Arc::new(SharedMixer::new());
        mixer.lock().set_ambient(Some(Arc::new(vec![0.25f32; 64])));

        let t0 = Instant::now();
        assert!(mixer.wait_drained(Duration::from_secs(30)));
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "an endless bed must not be waited on"
        );
        // And it really is still running: this is not "no sound at all".
        let mut out = vec![0.0f32; 8];
        mixer.fill(&mut out, 2);
        assert!(out.iter().all(|s| (*s - 0.25).abs() < 0.001));
    }
}
