//! What one widget tells another, carried by the HOST.
//!
//! A widget cannot name another widget, cannot be told that another one
//! exists, and cannot be woken by one. Everything it may ask for goes to
//! the application as an [`crate::Action`], and no action means "somebody
//! else should now show something different". That gap is real, and it is
//! the toolkit's: the launcher's category list steers the application
//! grid through a `static` in a crate both widgets link, which works for
//! exactly as long as both are linked into one binary and stops the day
//! they become two `.so` files opened `RTLD_LOCAL` — each with its own
//! copy of that crate, and therefore its own copy of the cell.
//!
//! This module is the host-side channel that replaces it. Four things
//! were required of it, and the shape below is the simplest one that
//! does all four:
//!
//! 1. **It crosses a `.so`.** The values live in the HOST's copy of the
//!    toolkit and are reached through [`crate::runtime`], the same way
//!    `sound::emit` reaches the host's queue. Plugins keep their private
//!    statics and still read one value.
//! 2. **The topic is text, the payload is bytes.** Neither side needs a
//!    type the other was compiled against, and neither has to be rebuilt
//!    when the other's payload grows a field.
//! 3. **Nobody has to be listening.** A published value is RETAINED: it
//!    stands under its topic until it is replaced, so a widget created
//!    after the publish still reads it, and load order stops mattering.
//! 4. **Publishing cannot block drawing.** There is no queue to fill, no
//!    reader to wait for and no wakeup to deliver: a publisher writes and
//!    returns, and a reader picks the value up on its next frame — at
//!    most one frame later, which is what every other fact in an
//!    immediate-mode interface already costs.
//!
//! So: a BOARD of named values, not a bus. A queue fails 4 (an undrained
//! one grows without end, and bounding it makes the publisher wait) and
//! part of 3 (a subscriber that starts late has missed everything). A
//! callback fails 4 too, and worse: the host would end up calling into
//! one plugin from another plugin's stack, in the middle of a frame.
//!
//! What a board deliberately does NOT provide is history — a reader sees
//! the LAST value, never the ones before it. For "which category is
//! shown", "which file is selected", "what the search box holds", that is
//! the whole truth anyway; a channel that needed every intermediate value
//! would be a different mechanism, and this one is not it.
//!
//! ```ignore
//! nacelle::channel::publish("launcher.category", b"Utility");
//! // ... in the other widget, on its next frame:
//! if let Some(m) = nacelle::channel::read("launcher.category") {
//!     let name = String::from_utf8_lossy(&m.data);
//! }
//! ```

use crate::runtime::{
    self, CHANNEL_TOPICS_MAX, CHANNEL_TOPIC_MAX, CHANNEL_VALUE_MAX,
};
use std::sync::RwLock;

/// A value read off the board, with the sequence number it was published
/// under.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Message {
    pub data: Vec<u8>,
    /// Counts from 1 for a topic's first value. A reader that remembers
    /// this can tell "unchanged" from "changed" without comparing
    /// payloads — and 0, which never appears here, is what
    /// [`seq`] answers for a topic nobody has ever published to.
    pub seq: u64,
}

/// One topic and what stands under it.
struct Slot {
    topic: String,
    data: Vec<u8>,
    seq: u64,
}

/// The board itself, in whichever copy of the toolkit is the host.
///
/// An `RwLock` rather than a `Mutex`: reads happen while drawing, from
/// however many widgets asked, and they must not queue behind each other.
/// A `Vec` rather than a map because a program has a handful of topics
/// and a linear scan over a handful is faster than hashing them — and
/// `Vec::new` is `const`, so the board needs no lazy initialisation.
static BOARD: RwLock<Vec<Slot>> = RwLock::new(Vec::new());

/// A topic a caller may not have meant: empty names and absurd ones. The
/// bound exists because a topic is a constant in somebody's source, never
/// user input, so anything long is a bug rather than a use.
fn topic_ok(topic: &str) -> bool {
    !topic.is_empty() && topic.len() <= CHANNEL_TOPIC_MAX
}

fn local_publish(topic: &str, data: &[u8]) -> u64 {
    if !topic_ok(topic) || data.len() > CHANNEL_VALUE_MAX {
        return 0;
    }
    // A poisoned lock is stepped over rather than propagated: a panic
    // somewhere else must not turn every widget that reads this board
    // into a blank panel.
    let mut board = BOARD.write().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = board.iter_mut().find(|s| s.topic == topic) {
        s.data.clear();
        s.data.extend_from_slice(data);
        // 0 is reserved for "never published", so the counter steps over
        // it if it ever wraps — which it will not, and the guard is one
        // line against a sentinel that would otherwise stop being one.
        s.seq = s.seq.checked_add(1).unwrap_or(1);
        return s.seq;
    }
    if board.len() >= CHANNEL_TOPICS_MAX {
        // Refused, never evicted: dropping somebody else's topic to make
        // room would make a widget quietly stop hearing its partner,
        // which is the exact failure this module exists to end.
        crate::ui::warn_once(
            "channel.full",
            "the widget channel is full — a new topic was refused; \
             nothing that was already published has been lost",
        );
        return 0;
    }
    board.push(Slot { topic: topic.to_string(), data: data.to_vec(), seq: 1 });
    1
}

fn local_read(topic: &str, buf: &mut [u8]) -> (usize, u64) {
    let board = BOARD.read().unwrap_or_else(|e| e.into_inner());
    let Some(s) = board.iter().find(|s| s.topic == topic) else {
        return (0, 0);
    };
    let n = buf.len().min(s.data.len());
    buf[..n].copy_from_slice(&s.data[..n]);
    (s.data.len(), s.seq)
}

/// Says once that this host is older than the channel. A plugin built
/// against it degrades — it shows what it shows when nothing has been
/// chosen — but silence here would look exactly like a partner that never
/// published.
fn warn_no_channel() {
    crate::ui::warn_once(
        "channel.absent",
        "this host is older than the widget channel — a widget's \
         published value reaches nobody and its reads find nothing",
    );
}

/// States `data` under `topic`, replacing whatever stood there, and
/// answers the topic's new sequence number. 0 means the call was refused:
/// an empty or over-long topic, a payload past [`CHANNEL_VALUE_MAX`], a
/// board already holding [`CHANNEL_TOPICS_MAX`] topics, or a host older
/// than this entry.
///
/// Returns as soon as the value is written. Nothing is woken and nothing
/// is waited for — see the module's condition 4.
pub fn publish(topic: &str, data: &[u8]) -> u64 {
    runtime::shared(
        "channel::publish",
        || local_publish(topic, data),
        |api| {
            if !api.has_channel() {
                warn_no_channel();
                return 0;
            }
            (api.channel_publish)(
                topic.as_ptr(),
                topic.len() as u32,
                data.as_ptr(),
                data.len() as u32,
            )
        },
        0,
    )
}

/// Copies the value under `topic` into `buf`, answering its FULL length
/// and the topic's sequence number.
///
/// The full length rather than what was written, so a caller can tell a
/// truncation from a fit: a payload's prefix is a broken message, and a
/// broken message read as a whole one is worse than no message. A `seq`
/// of 0 means nothing was ever published under this topic — which is how
/// an absent value and an empty one stay different things.
pub fn read_into(topic: &str, buf: &mut [u8]) -> (usize, u64) {
    runtime::shared_with(
        "channel::read",
        |api| match api {
            None => local_read(topic, buf),
            Some(api) => {
                if !api.has_channel() {
                    warn_no_channel();
                    return (0, 0);
                }
                let mut seq = 0u64;
                let n = (api.channel_read)(
                    topic.as_ptr(),
                    topic.len() as u32,
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    &mut seq,
                );
                (n as usize, seq)
            }
        },
        (0, 0),
    )
}

/// The sequence number standing under `topic`, and nothing else — 0 when
/// nothing was ever published there. The cheap half of [`read_into`]:
/// a reader that only wants to know whether anything CHANGED asks this,
/// and reads the value in the frames where it did.
pub fn seq(topic: &str) -> u64 {
    read_into(topic, &mut []).1
}

/// The value under `topic`, allocated. None when nothing was ever
/// published there.
pub fn read(topic: &str) -> Option<Message> {
    // A stack probe first: every fact this channel was built for — a
    // category name, a path, a small list — fits in it, so the ordinary
    // read allocates once (for the answer) and the ordinary MISS, which
    // is what a reader polling an unpublished topic does every frame,
    // allocates not at all.
    let mut probe = [0u8; 256];
    let (len, seq) = read_into(topic, &mut probe);
    if seq == 0 {
        return None;
    }
    if len <= probe.len() {
        return Some(Message { data: probe[..len].to_vec(), seq });
    }
    // Longer than the probe. The value may be REPLACED between learning
    // its size and copying it, so the read repeats until an answer fits
    // the room it was given; the bound is what keeps a publisher
    // rewriting the topic in a tight loop from spinning here, and giving
    // up beats returning half a message.
    let mut buf = vec![0u8; len.min(CHANNEL_VALUE_MAX)];
    for _ in 0..4 {
        let (len, seq) = read_into(topic, &mut buf);
        if seq == 0 {
            return None;
        }
        if len <= buf.len() {
            buf.truncate(len);
            return Some(Message { data: buf, seq });
        }
        buf = vec![0u8; len.min(CHANNEL_VALUE_MAX)];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test for the board, not six: it is process-wide state, and
    /// separate tests writing it would race each other under the default
    /// harness — the reason `launcher-core`'s own cell has one test too.
    #[test]
    fn a_value_stands_under_its_topic_until_it_is_replaced() {
        // Nothing published: absent, and absent is not "empty".
        assert_eq!(read("test.never"), None);
        assert_eq!(seq("test.never"), 0);

        // The first value numbers from 1, which is what makes 0 usable
        // as "nobody has ever spoken here".
        assert_eq!(publish("test.pick", b"Utility"), 1);
        let m = read("test.pick").expect("a published value stands");
        assert_eq!(m.data, b"Utility");
        assert_eq!(m.seq, 1);

        // A reader that did not exist at publication time still reads it
        // — that is what RETAINED means, and condition 3.
        assert_eq!(read("test.pick").map(|m| m.data), Some(b"Utility".to_vec()));

        // Replacing steps the sequence, so a reader can skip work
        // without comparing payloads.
        assert_eq!(publish("test.pick", b"Games"), 2);
        assert_eq!(seq("test.pick"), 2);
        assert_eq!(read("test.pick").unwrap().data, b"Games");

        // An EMPTY payload is a value, distinct from no value at all: a
        // widget saying "nothing is selected now" must be able to.
        assert_eq!(publish("test.pick", b""), 3);
        let m = read("test.pick").expect("an empty value is still a value");
        assert!(m.data.is_empty());
        assert_eq!(m.seq, 3);

        // Topics do not run into each other.
        publish("test.other", b"x");
        assert_eq!(read("test.pick").unwrap().data, Vec::<u8>::new());

        // A short buffer is told how much it MISSED, not how much it
        // got, so truncation is detectable.
        publish("test.long", b"0123456789");
        let mut small = [0u8; 4];
        let (len, s) = read_into("test.long", &mut small);
        assert_eq!(len, 10, "the full length, not the four bytes written");
        assert_eq!(s, 1);
        assert_eq!(&small, b"0123");
        // And `read` gets the whole of a value past the stack probe.
        let big = vec![b'z'; 1000];
        publish("test.long", &big);
        assert_eq!(read("test.long").unwrap().data, big);

        // Refusals answer 0 and change nothing.
        assert_eq!(publish("", b"x"), 0);
        assert_eq!(publish(&"t".repeat(CHANNEL_TOPIC_MAX + 1), b"x"), 0);
        assert_eq!(publish("test.huge", &vec![0u8; CHANNEL_VALUE_MAX + 1]), 0);
        assert_eq!(read("test.huge"), None);
    }
}
