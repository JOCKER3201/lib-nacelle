//! §5.24's `when` — the one trigger a theme is allowed to write down.
//!
//! A mood is a pre-resolved sibling theme; picking one is an index swap
//! ([`super::set_mood`]). What was missing was the sentence that says WHEN:
//! `[mood.alert]` ships `when = "severity >= critical"` and nothing in the
//! program ever read it, so the alarm skin was resolved, baked, cached — and
//! unreachable except by an explicit call nobody made.
//!
//! # The language is four sentences, and this file is why it stays that way
//!
//! §5.24 fixes the vocabulary: **four predicate forms, parsed at load into an
//! enum, evaluated once per second against the telemetry snapshot the host
//! already keeps**. They are, exactly:
//!
//! | form | reads |
//! |---|---|
//! | `severity >= <role>` | the severities the interface is reporting now |
//! | `count(severity == <role>) >= <n>` | how many of them say the same thing |
//! | `battery < <n>` | charge, in percent |
//! | `temp > <n>` | the package temperature, in degrees Celsius |
//!
//! The comparator belongs to the form, not to a grammar: `battery > 10` is
//! not a predicate, it is a typo, and it warns like one. There is no
//! scripting here on purpose — a theme that could evaluate expressions could
//! decide the machine is on fire, and §5.24 gives that judgement to the host.
//!
//! # Evaluation is the host's, and so is the clock
//!
//! This module parses and answers `true`/`false`. Nothing here reads a clock,
//! latches a mood or talks to the engine: the cadence, the falling-edge
//! hysteresis and the precedence between a host's choice and a theme's rule
//! are the host's, because only the host knows what its telemetry means and
//! when it last changed.

use crate::theme::parse::{Diagnostic, Span};
use crate::ui::SEVERITY_ROLES;

/// How many of §5.10's seven roles form a RISING LADDER.
///
/// The closed set is in the master's declaration order — `ok`, `info`,
/// `warning`, `critical`, `contained`, `offline`, `unknown` — and only the
/// first four rise. The last three are states, not levels: `offline` is not
/// worse than `critical`, and `unknown` is what a MISSPELLED severity word
/// resolves to (§5.10). Comparing by raw index would let one typo in one
/// widget strobe the whole interface into its alarm skin, which is a worse
/// failure than the alarm never arriving.
const LADDER: u16 = 4;

/// The rung a severity stands on, or `None` for a role that is not on the
/// ladder at all.
fn rank(role: u16) -> Option<u16> {
    (role < LADDER).then_some(role)
}

/// The index of a severity role in §5.10's closed set.
fn role_index(name: &str) -> Option<u16> {
    SEVERITY_ROLES.iter().position(|r| *r == name).map(|i| i as u16)
}

/// The telemetry one evaluation sees (§5.24: "the telemetry snapshot that
/// already ticks at 1 Hz").
///
/// Every field is what the host already collects for its own reasons. A
/// field the host cannot answer is `None`, and a predicate over a `None` is
/// FALSE rather than a guess: a machine with no battery is not a machine at
/// nine percent.
pub struct MoodInput<'a> {
    /// Every severity the interface is reporting right now, as indices into
    /// [`SEVERITY_ROLES`]. Order carries nothing; the two severity forms ask
    /// "is any of these at least X" and "how many of these are X".
    pub severities: &'a [u16],
    /// Charge in percent.
    pub battery: Option<f32>,
    /// The package temperature in degrees Celsius.
    pub temp_c: Option<f32>,
}

/// §5.24's four predicate forms, parsed. `Never` is the fifth answer and not
/// a form: it is what `when = ""` means (host-selected only), and what an
/// unparseable `when` degrades to — a theme that misspells its trigger loses
/// the trigger, never the mood.
#[derive(Clone, Debug, PartialEq)]
pub enum MoodWhen {
    /// `""` — only the host may select this mood.
    Never,
    /// `severity >= <role>`, holding the role's rung on the ladder.
    SeverityAtLeast(u16),
    /// `count(severity == <role>) >= <n>`, holding the role's index in the
    /// closed set and the count that satisfies it.
    Count { role: u16, at_least: u32 },
    /// `battery < <n>`, in percent.
    BatteryBelow(f32),
    /// `temp > <n>`, in degrees Celsius.
    TempAbove(f32),
}

impl MoodWhen {
    /// Does this rule hold for this second's telemetry?
    ///
    /// Total and allocation-free: it runs once per second per declared mood,
    /// against a handful of severities, and it must never be the reason a
    /// host skips an evaluation.
    pub fn holds(&self, input: &MoodInput) -> bool {
        match *self {
            MoodWhen::Never => false,
            MoodWhen::SeverityAtLeast(rung) => input
                .severities
                .iter()
                .filter_map(|s| rank(*s))
                .any(|r| r >= rung),
            MoodWhen::Count { role, at_least } => {
                input.severities.iter().filter(|s| **s == role).count() as u32 >= at_least
            }
            MoodWhen::BatteryBelow(pct) => input.battery.is_some_and(|b| b < pct),
            MoodWhen::TempAbove(c) => input.temp_c.is_some_and(|t| t > c),
        }
    }
}

/// One mood's name and its declarative trigger, in the order the theme
/// declares them.
#[derive(Clone, Debug, PartialEq)]
pub struct MoodRule {
    pub name: String,
    pub when: MoodWhen,
}

/// The four forms, spelled the way a diagnostic should spell them.
const FORMS: &str = "severity >= <role> | count(severity == <role>) >= <n> | \
                     battery < <n> | temp > <n>";

/// Parses one `when` value (§5.24). Never fails: an unrecognised form warns
/// and leaves the mood host-selected, because §4.2 says a theme always loads.
///
/// Whitespace carries nothing, so it is removed before matching rather than
/// tokenised: `count(severity == critical) >= 3` and
/// `count( severity==critical )>=3` are the same sentence, and neither a
/// role name nor a number contains a space.
pub fn parse_when(mood: &str, text: &str, span: Span, out: &mut Vec<Diagnostic>) -> MoodWhen {
    let flat: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if flat.is_empty() {
        return MoodWhen::Never;
    }
    let refuse = |out: &mut Vec<Diagnostic>, why: String| {
        out.push(Diagnostic::warn(
            span,
            format!("mood \"{mood}\": when = \"{text}\" {why} — the mood stays host-selected"),
        ));
        MoodWhen::Never
    };

    if let Some(role) = flat.strip_prefix("severity>=") {
        let Some(i) = role_index(role) else {
            return refuse(out, format!("names no severity role ({})", SEVERITY_ROLES.join(" | ")));
        };
        let Some(rung) = rank(i) else {
            return refuse(
                out,
                format!(
                    "compares against \"{role}\", which is a severity STATE and not a rung \
                     (>= is defined over {})",
                    SEVERITY_ROLES[..LADDER as usize].join(" | ")
                ),
            );
        };
        return MoodWhen::SeverityAtLeast(rung);
    }

    if let Some(rest) = flat.strip_prefix("count(severity==") {
        let Some((role, n)) = rest.split_once(")>=") else {
            return refuse(out, format!("is not {FORMS}"));
        };
        let Some(role) = role_index(role) else {
            return refuse(out, format!("names no severity role ({})", SEVERITY_ROLES.join(" | ")));
        };
        let Ok(at_least) = n.parse::<u32>() else {
            return refuse(out, format!("counts \"{n}\", which is not a whole number"));
        };
        return MoodWhen::Count { role, at_least };
    }

    if let Some(n) = flat.strip_prefix("battery<") {
        return match n.parse::<f32>() {
            Ok(pct) if pct.is_finite() => MoodWhen::BatteryBelow(pct),
            _ => refuse(out, format!("reads \"{n}\" as a percentage")),
        };
    }

    if let Some(n) = flat.strip_prefix("temp>") {
        return match n.parse::<f32>() {
            Ok(c) if c.is_finite() => MoodWhen::TempAbove(c),
            _ => refuse(out, format!("reads \"{n}\" as degrees Celsius")),
        };
    }

    refuse(out, format!("is not one of §5.24's four predicate forms ({FORMS})"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::parse::{self, Sources};

    fn parsed(text: &str) -> (MoodWhen, Vec<String>) {
        let mut out = Vec::new();
        let w = parse_when("test", text, Span::default(), &mut out);
        (w, out.into_iter().map(|d| d.message).collect())
    }

    #[test]
    fn the_four_forms_parse_and_nothing_else_does() {
        assert_eq!(parsed("").0, MoodWhen::Never);
        assert_eq!(parsed("severity >= critical").0, MoodWhen::SeverityAtLeast(3));
        assert_eq!(
            parsed("count(severity == critical) >= 3").0,
            MoodWhen::Count { role: 3, at_least: 3 }
        );
        assert_eq!(parsed("battery < 10").0, MoodWhen::BatteryBelow(10.0));
        assert_eq!(parsed("temp > 90").0, MoodWhen::TempAbove(90.0));
        // Spacing is not part of the sentence.
        assert_eq!(parsed("count( severity==critical )>=3").0, MoodWhen::Count { role: 3, at_least: 3 });
    }

    /// The comparator belongs to the form. Inverting one is a typo, and a
    /// typo that silently became a working rule would be an alarm nobody
    /// asked for — or, worse, one that never comes.
    #[test]
    fn a_form_the_engine_does_not_ship_warns_instead_of_being_invented() {
        for text in [
            "battery > 10",
            "temp >= 90",
            "severity > critical",
            "cpu > 90",
            "count(severity == critical) > 3",
            "severity >= molten",
            "battery < a lot",
        ] {
            let (when, warnings) = parsed(text);
            assert_eq!(when, MoodWhen::Never, "{text} should not have parsed");
            assert_eq!(warnings.len(), 1, "{text} should warn exactly once");
            assert!(warnings[0].contains("host-selected"), "{text}: {}", warnings[0]);
        }
    }

    /// `unknown` is the fallback for a misspelled severity word (§5.10), so
    /// letting it sit above `critical` on a ladder it is not on would make
    /// one typo in one widget a screen-wide alarm.
    #[test]
    fn the_three_states_are_not_rungs() {
        for role in ["contained", "offline", "unknown"] {
            let (when, warnings) = parsed(&format!("severity >= {role}"));
            assert_eq!(when, MoodWhen::Never);
            assert!(warnings[0].contains("STATE"), "{}", warnings[0]);
        }
        // They remain perfectly good things to COUNT: "three panels can no
        // longer be read" is a fact, it is just not a rung.
        assert_eq!(
            parsed("count(severity == offline) >= 3").0,
            MoodWhen::Count { role: 5, at_least: 3 }
        );
    }

    #[test]
    fn a_predicate_over_telemetry_the_host_does_not_have_is_false_not_a_guess() {
        let none = MoodInput { severities: &[], battery: None, temp_c: None };
        assert!(!MoodWhen::BatteryBelow(10.0).holds(&none));
        assert!(!MoodWhen::TempAbove(90.0).holds(&none));
        assert!(!MoodWhen::SeverityAtLeast(3).holds(&none));
        assert!(!MoodWhen::Count { role: 3, at_least: 1 }.holds(&none));
    }

    #[test]
    fn each_form_answers_the_telemetry_it_names() {
        let hot = MoodInput { severities: &[], battery: Some(9.0), temp_c: Some(91.0) };
        assert!(MoodWhen::BatteryBelow(10.0).holds(&hot));
        assert!(MoodWhen::TempAbove(90.0).holds(&hot));
        let warm = MoodInput { severities: &[], battery: Some(10.0), temp_c: Some(90.0) };
        // Both comparators are strict: the threshold itself is not the alarm.
        assert!(!MoodWhen::BatteryBelow(10.0).holds(&warm));
        assert!(!MoodWhen::TempAbove(90.0).holds(&warm));

        // ok(0) info(1) warning(2) critical(3), and one unknown(6) that must
        // not count as anything.
        let panels = MoodInput { severities: &[0, 2, 3, 6], battery: None, temp_c: None };
        assert!(MoodWhen::SeverityAtLeast(3).holds(&panels));
        assert!(MoodWhen::SeverityAtLeast(2).holds(&panels));
        assert!(!MoodWhen::Count { role: 3, at_least: 2 }.holds(&panels));
        assert!(MoodWhen::Count { role: 3, at_least: 1 }.holds(&panels));
        let calm = MoodInput { severities: &[0, 1, 6], battery: None, temp_c: None };
        assert!(!MoodWhen::SeverityAtLeast(3).holds(&calm));
    }

    /// The master is the specification's own example, so it is also the
    /// regression test: alert reads the interface, lockdown is the host's.
    #[test]
    fn the_masters_own_moods_parse_without_a_word_of_complaint() {
        let mut out = Vec::new();
        let mut src = Sources::new();
        let f = src.add("default.theme", super::super::DEFAULT_THEME);
        let doc = parse::parse(&mut src, f, None, &mut out);
        out.clear();

        let rules = super::super::cascade::mood_rules(&doc, &mut out);
        let by_name = |n: &str| {
            rules.iter().find(|r: &&MoodRule| r.name == n).map(|r| r.when.clone())
        };
        assert_eq!(by_name("normal"), Some(MoodWhen::Never));
        assert_eq!(by_name("alert"), Some(MoodWhen::SeverityAtLeast(3)));
        assert_eq!(by_name("lockdown"), Some(MoodWhen::Never));
        let rendered: String = out.iter().map(|d| d.render(&src)).collect();
        assert!(out.is_empty(), "the master's moods are not clean:\n{rendered}");
    }
}
