//! ONE number policy, read from `[num]`.
//!
//! The master's §5.17 block states a complete policy for how a reading is
//! written down: which character parts the integer from the fraction,
//! which one parts the thousands, how long an integer has to be before it
//! is grouped at all, how many places a value carries normally and how
//! many where the room is tight, and the typography of the unit that
//! follows it. Its own comment on `decimal_sep` says why the block exists
//! at all — **"THE THEME DECIDES, not a locale guess"**, so that one theme
//! renders the same reading on every machine.
//!
//! Until this module existed the block had two readers (`tabular_set` and
//! `tabular_punct`, both about the figure BOX and neither about the
//! number) and every reading in the program was written by a `format!`
//! at its own call site: `format!("{v:.0}%")` for a gauge, `{:.2} GiB`
//! for a byte count, `format!("{n:.p$}")` for a script's `round`. An
//! instrument theme asking for `12 345,67 TB` could not get it, and the
//! three call sites could not have agreed even if it could.
//!
//! What is NOT here is any fallback: a theme whose `decimal_sep` is empty
//! writes its numbers without one, which is the raw look the governing
//! principle asks for and not a design this file invented.

use crate::theme::{self, TokenId};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

fn tok(cell: &'static OnceLock<TokenId>, name: &'static str) -> TokenId {
    *cell.get_or_init(|| theme::id(name).unwrap_or(TokenId::MISSING))
}

/// A TEXT token, memoised per theme epoch.
///
/// Text tokens live in the cold-path diagnostics — they are not in
/// `ResolvedTheme` — and are found there by a linear scan of every text
/// token the theme declares. The scan must therefore happen once per
/// theme and never per number, which is what the epoch key buys; the
/// `Rc<str>` is the same bargain [`crate::ui`] strikes for
/// `num.tabular_set`, since a reading is composed on a draw path.
///
/// [`theme::content_epoch`] and NOT [`theme::epoch`], for the reason that
/// counter was added: `epoch` answers "which BAKE is published", and a
/// desktop whose two monitors are unequal heights publishes two of them in
/// turn, so its value alternates every frame forever. A one-slot cache
/// keyed on it misses every time — which is how the font system came to
/// re-parse every face sixty times a second. A text token is CONTENT: it
/// does not move when the viewport does, so the content counter is both
/// the correct key and the one that holds.
fn text_token(slot: &'static std::thread::LocalKey<RefCell<Option<(u32, Rc<str>)>>>, name: &str) -> Rc<str> {
    let epoch = theme::content_epoch();
    slot.with(|s| {
        let mut s = s.borrow_mut();
        if let Some((e, v)) = s.as_ref() {
            if *e == epoch {
                return v.clone();
            }
        }
        let v: Rc<str> = theme::diagnostics().text(name).unwrap_or_default().into();
        *s = Some((epoch, v.clone()));
        v
    })
}

thread_local! {
    static DECIMAL_SEP: RefCell<Option<(u32, Rc<str>)>> = const { RefCell::new(None) };
    static GROUP_SEP: RefCell<Option<(u32, Rc<str>)>> = const { RefCell::new(None) };
    static UNIT_TEXT_GAP: RefCell<Option<(u32, Rc<str>)>> = const { RefCell::new(None) };
}

/// `num.decimal_sep` — the character between integer and fraction.
pub fn decimal_sep() -> Rc<str> {
    text_token(&DECIMAL_SEP, "num.decimal_sep")
}

/// `num.group_sep` — the character between thousands groups.
pub fn group_sep() -> Rc<str> {
    text_token(&GROUP_SEP, "num.group_sep")
}

/// `num.unit.text_gap` — what stands between a value and its unit when the
/// two are ONE STRING rather than two drawn runs.
///
/// `num.unit.gap` is an em, and an em is a distance a text path lays out;
/// a `String` handed to a script, a table cell or a tooltip carries no
/// distances at all, only characters. Both keys describe the same joint
/// and neither can do the other's work, so the master declares both and
/// says so at each.
pub fn unit_text_gap() -> Rc<str> {
    text_token(&UNIT_TEXT_GAP, "num.unit.text_gap")
}

/// `num.decimals` — the places a value carries.
pub fn decimals() -> usize {
    static ID: OnceLock<TokenId> = OnceLock::new();
    theme::resolved().px(tok(&ID, "num.decimals")).clamp(0.0, 6.0) as usize
}

/// `num.decimals_compact` — the places a value carries where the room is
/// tight: a gauge readout, a temperature.
pub fn decimals_compact() -> usize {
    static ID: OnceLock<TokenId> = OnceLock::new();
    theme::resolved().px(tok(&ID, "num.decimals_compact")).clamp(0.0, 6.0) as usize
}

/// `num.group` and `num.group_min`: digits per group, and the shortest
/// integer that is grouped at all.
///
/// The master's range on `group` is 2..4 and a group of zero would be an
/// infinite loop, so the floor is arithmetic and not a look. `group_min`
/// has no floor of its own: a theme setting it to 0 groups everything,
/// which is a legible thing to ask for.
fn grouping() -> (usize, usize) {
    static GROUP: OnceLock<TokenId> = OnceLock::new();
    static GROUP_MIN: OnceLock<TokenId> = OnceLock::new();
    let t = theme::resolved();
    (
        t.px(tok(&GROUP, "num.group")).max(1.0) as usize,
        t.px(tok(&GROUP_MIN, "num.group_min")).max(0.0) as usize,
    )
}

/// A value written down under the theme's number policy, to `places`
/// decimals.
///
/// The rounding is Rust's, done first and read back as digits: grouping a
/// string that has not been rounded yet would put separators into a run
/// the rounding then shortens.
pub fn format(v: f64, places: usize) -> String {
    let plain = format!("{v:.places$}");
    let (sign, body) = match plain.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", plain.as_str()),
    };
    let (int, frac) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    let mut out = String::with_capacity(plain.len() + 4);
    out.push_str(sign);
    out.push_str(&grouped(int));
    if let Some(frac) = frac {
        out.push_str(&decimal_sep());
        out.push_str(frac);
    }
    out
}

/// The integer part with `num.group_sep` every `num.group` digits, once
/// it is at least `num.group_min` digits long.
///
/// Counted from the RIGHT, which is what a thousands group is: `12345`
/// under a group of 3 is `12 345` and not `123 45`.
fn grouped(int: &str) -> String {
    let (size, min) = grouping();
    let sep = group_sep();
    if sep.is_empty() || int.len() < min || int.len() <= size {
        return int.to_string();
    }
    let digits: Vec<char> = int.chars().collect();
    let mut out = String::with_capacity(int.len() * 2);
    for (i, c) in digits.iter().enumerate() {
        // The gap goes BEFORE a digit whose distance from the right edge
        // is a whole number of groups — and never before the first, which
        // would open the number with a separator.
        if i > 0 && (digits.len() - i) % size == 0 {
            out.push_str(&sep);
        }
        out.push(*c);
    }
    out
}

/// A value at `num.decimals`.
pub fn value(v: f64) -> String {
    format(v, decimals())
}

/// A value at `num.decimals_compact` — the form a gauge readout takes.
pub fn value_compact(v: f64) -> String {
    format(v, decimals_compact())
}

/// `num.unit.case` applied to a unit suffix.
///
/// Through the toolkit's one applier, so the unit reads by the same rule
/// the panel band and the window title do — including the rule for a word
/// the list does not hold, which used to be silent capitals here as
/// everywhere else. The master's own note on this key says a theme that
/// wants shouty units "still writes `upper` and gets it everywhere at
/// once"; a typo now gets nothing at all, and says so.
pub fn unit(s: &str) -> String {
    static ID: OnceLock<TokenId> = OnceLock::new();
    crate::ui::recase(crate::ui::case_of(tok(&ID, "num.unit.case")), s).into_owned()
}

/// `num.unit.percent_attached` — whether the gap before a percent sign is
/// suppressed. `85%` with no gap; `60 ps` keeps its.
pub fn percent_attached() -> bool {
    static ID: OnceLock<TokenId> = OnceLock::new();
    theme::resolved().flag(tok(&ID, "num.unit.percent_attached"))
}

/// A number and the unit that follows it, kept apart because the master
/// gives the unit six keys of its own: its own size, gap, tracking, case,
/// colour and baseline. A drawing path sets the two as two runs; a path
/// that can only carry characters joins them with [`Reading::text`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    /// The value, already grouped and pointed by [`format`].
    pub number: String,
    /// The suffix, already cased by [`unit`]. Empty for a bare number.
    pub unit: String,
}

impl Reading {
    /// A reading at `num.decimals`.
    pub fn new(v: f64, unit_text: &str) -> Reading {
        Reading { number: value(v), unit: unit(unit_text) }
    }

    /// A reading at `num.decimals_compact` — where the room is tight.
    pub fn compact(v: f64, unit_text: &str) -> Reading {
        Reading { number: value_compact(v), unit: unit(unit_text) }
    }

    /// Whether the joint between the two runs is closed up: a percent
    /// sign under `num.unit.percent_attached`, and nothing else.
    ///
    /// A theme whose gap is nothing closes every joint too, but it does so
    /// through the gap itself — `num.unit.gap` on the drawn path and
    /// `num.unit.text_gap` on the string one — and those are two separate
    /// keys measured in two different things. Reading either of them here
    /// would put one of the two joints under the other's key.
    pub fn attached(&self) -> bool {
        self.unit.starts_with('%') && percent_attached()
    }

    /// The pair as one string, for every consumer that carries characters
    /// and not runs — a script's cell, a table's value, a tooltip.
    pub fn text(&self) -> String {
        if self.unit.is_empty() {
            return self.number.clone();
        }
        let mut out = String::with_capacity(self.number.len() + self.unit.len() + 1);
        out.push_str(&self.number);
        if !self.attached() {
            out.push_str(&unit_text_gap());
        }
        out.push_str(&self.unit);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The grouping rule itself, against the shipped master: a group of
    /// three from the right, and nothing below `group_min` digits.
    ///
    /// Spelled against `grouped` rather than `format` so that the claim is
    /// about the digits alone and does not also depend on the separator
    /// the master happens to ship.
    #[test]
    fn a_group_is_counted_from_the_right() {
        let _ = theme::load();
        let sep = group_sep();
        let (size, min) = grouping();
        assert!(size >= 2, "the master declares 2..4 digits per group");
        assert!(min >= size, "a minimum shorter than one group could never fire");

        // One digit under the minimum, and one at it.
        let short = "1".repeat(min - 1);
        assert_eq!(grouped(&short), short);
        let at_min = "1".repeat(min);
        assert!(at_min.len() > size, "the fixture must be long enough to split");
        assert!(
            grouped(&at_min).contains(&*sep),
            "an integer of {min} digits is grouped: {}",
            grouped(&at_min)
        );
    }

    /// Grouping decorates a number; it never adds or drops a digit.
    #[test]
    fn grouping_keeps_every_digit_it_was_given() {
        let _ = theme::load();
        for v in [0.0, 7.0, 1234.0, 123_456_789.0, -98_765.0] {
            let written = format(v, 0);
            let plain = format!("{v:.0}");
            assert_eq!(
                written.chars().filter(|c| c.is_ascii_digit()).count(),
                plain.chars().filter(|c| c.is_ascii_digit()).count(),
                "{v}: {written}"
            );
            assert_eq!(written.starts_with('-'), plain.starts_with('-'), "{v}");
        }
    }
}
