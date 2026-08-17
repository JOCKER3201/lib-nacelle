//! What an addon's user asked of it — read by the HOST, parsed by the
//! addon.
//!
//! Until this module existed a widget was told two things and no third:
//! what the theme says, through the token entries, and what a neighbour
//! published, through [`crate::channel`]. Nothing carried the one fact
//! that is neither a colour nor a message — the value its own user set.
//! An addon with a setting had to bake it in, and "configurable" stopped
//! at whatever the author guessed.
//!
//! # Who opens the file
//!
//! The host does, always. A plugin names an ADDON and a FILE; the host
//! turns that pair into a path, searches the settings directories in
//! order and answers the text.
//!
//! This is the clipboard's boundary drawn again (przynależność, the
//! clipboard verdict): a plugin reaches host state through a call that
//! NAMES what it wants, never through a handle or a path that would move
//! the decision to the plugin. Hand a plugin a path and the plugin picks
//! the file; hand it a name out of a namespace the host controls and the
//! worst a wrong name can do is miss. The search order — the user's
//! directory before the system's — then exists once, in the host, rather
//! than sixteen times in sixteen addons that would each drift.
//!
//! One consequence stated rather than left to be discovered: an addon
//! may name ANOTHER addon and read its settings. That is deliberate.
//! What is being defended is the filesystem, not the settings namespace:
//! these files are the user's own, sitting in the user's own directory,
//! readable in any editor. A widget reading `addons/clock.ron` learns
//! what the clock shows, which is a fact it could have read off the
//! screen; a widget reading `~/.ssh/id_rsa` is a different kind of
//! event, and the name rules below make it impossible rather than
//! unlikely.
//!
//! # What crosses: the source text, not resolved values
//!
//! This was the contract decision, and it stays decided.
//!
//! The rejected shape was a typed key lookup — `setting_f32("rows")`,
//! `setting_str("format")` — the host parsing and answering values one
//! key at a time. It is the shape the theme entries already have, so it
//! looks like the house style. It is wrong here for four reasons:
//!
//! 1. **It would carry a FINITE type set across a boundary meant for an
//!    infinite one.** The theme is a closed vocabulary — a colour, a
//!    length, a flag, an enum index — mastered in one file this project
//!    owns. Addon settings are the opposite: their shapes are unknown
//!    by definition, because the next addon is not written yet. Every
//!    new shape would be a new ABI entry, and every ABI growth is a
//!    migration of every addon, which is the expensive part.
//! 2. **It would flatten exactly what RON was chosen for.** TOML was
//!    considered and rejected for the addon half specifically, because
//!    RON carries enum variants, tuples and named structs natively.
//!    Those survive only if the TYPED parse happens where the type is —
//!    in the addon. A key lookup would deliver the leaves of a tree and
//!    lose the tree, and the format choice would have bought nothing.
//! 3. **`#[serde(default)]` is the answer to RON's one real risk, and
//!    only the owner of the type can write it.** A `Key=Value` file
//!    lost one line per mistake; a RON document is all-or-nothing. The
//!    defence is a derived parser where every field has a default, so a
//!    partial, an old or an empty document still yields a whole value.
//!    A host-side lookup has no type to put the attribute on.
//! 4. **The parser is already there.** Every plugin links this crate,
//!    so `ron` and the one-line [`load`] arrive with it. The claim that
//!    "each addon would need a parser" is true of the design that
//!    HANDS OUT VALUES too: something must turn `"rows"` into a number
//!    and a struct, and doing it by hand out of stringly-typed lookups
//!    is a parser with the type checking removed.
//!
//! So the boundary carries UTF-8 text and the addon deserialises it into
//! its own struct. The host reads, the addon interprets — which is also
//! the only split under which each side does the part it alone can.
//!
//! # A parse failure is shown, never swallowed
//!
//! The failure this module refuses to have is the silent one: a
//! mistyped bracket, a widget that quietly looks factory-fresh, and a
//! user with no reason to connect the two. It would be strictly worse
//! than the state before RON, where one bad line cost one setting.
//!
//! So a document that does not parse is reported three ways, and the
//! addon's defaults are used only after it has been:
//!
//! * once on stderr, with the file's path and the line and column;
//! * retained in [`problems`], for the settings window to show — the
//!   host is the only side holding the path, so it is the only side
//!   that can say anything useful;
//! * to the addon itself, as [`Origin::Malformed`], which is a
//!   different answer from [`Origin::Absent`] precisely so that "you
//!   have no settings file" and "you have one and it is being ignored"
//!   cannot be confused by the one piece of code able to draw the
//!   difference.
//!
//! All three begin with somebody ASKING for a file, which leaves out
//! the one file nobody will ever ask for: the name that is not a plain
//! name. Nothing here can find it, because finding it means walking a
//! directory and this module hands out no paths and reads none of its
//! own. A host that walks its own settings directory can, and [`report`]
//! is where what it finds joins the same list.
//!
//! # Where the files are
//!
//! The toolkit does not know, and does not guess. As with
//! [`crate::assets`], only the embedder knows its own name, so the
//! embedder installs the directories:
//!
//! ```ignore
//! nacelle::settings::install(nacelle::assets::AssetRoots::xdg_config("nacelle"));
//! ```
//!
//! which searches `$XDG_CONFIG_HOME/nacelle` (`~/.config/nacelle`) and
//! then every `$XDG_CONFIG_DIRS` entry (`/etc/xdg/nacelle`), and writes
//! only to the first. Inside, the arrangement is the owner's decision of
//! 2026-08-12: one file per addon, a directory once an addon needs two.
//!
//! An embedder that forgets that one line has every addon running on
//! baked-in defaults and every settings file on the machine ignored, so
//! the omission is not allowed to be quiet: the first read says so on
//! stderr, and [`installed`] answers it at any time, for a settings
//! window that would rather show it than print it.
//!
//! ```text
//! <config>/addons/shell.ron           settings("shell", "")
//! <config>/addons/search/engines.ron  settings("search", "engines")
//! ```
//!
//! # The nearest file wins WHOLE
//!
//! The program's own configuration cascades key by key: a key the user
//! never set is answered by the system file. Addon settings do not, and
//! the difference is not an oversight.
//!
//! Merging two RON documents means a per-field policy — replace, union,
//! recurse — that only the owner of the type could state, and the host
//! parsing generically has no way to know which. So the first file found
//! is the whole answer. Nothing is lost by it: `#[serde(default)]` gives
//! every absent field a defined value, so a user file that sets one key
//! is complete and correct, and the packaged file's job is to be the
//! whole answer for a user who has written none — which is exactly what
//! the XDG decision asked for, a user directory that stays empty until
//! the user changes something.

use crate::assets::AssetRoots;
use crate::runtime::{
    self, SETTINGS_ABSENT, SETTINGS_FILE_MAX, SETTINGS_MALFORMED, SETTINGS_NAME_MAX,
    SETTINGS_OK, SETTINGS_REFUSED,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::RwLock;

/// Where the text a caller got came from, and how much to trust it.
///
/// Two of the four end in "use your defaults", and they are separate
/// values because they mean opposite things to a user: [`Absent`] is the
/// ordinary state of a fresh install and deserves no mention anywhere,
/// while [`Malformed`] means a file the user wrote is being ignored and
/// deserves to be said out loud.
///
/// [`Absent`]: Origin::Absent
/// [`Malformed`]: Origin::Malformed
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    /// A file was found and the host's own parse of it succeeded.
    File,
    /// No such file in any settings directory. Not an error.
    Absent,
    /// A file was found and the host could not parse it. The text is
    /// still delivered — the host's parse is generic and the caller's
    /// is not, so the caller has the last word about its own document.
    Malformed,
    /// The name was not a plain name, or no settings directories are
    /// installed at all. A programming error, not a user's.
    Refused,
}

impl Origin {
    /// The number this crosses the boundary as.
    pub fn code(self) -> u32 {
        match self {
            Origin::File => SETTINGS_OK,
            Origin::Absent => SETTINGS_ABSENT,
            Origin::Malformed => SETTINGS_MALFORMED,
            Origin::Refused => SETTINGS_REFUSED,
        }
    }

    /// The origin a number means. A status this build does not know is
    /// a NEWER host describing something this one has no word for, and
    /// it reads as [`Origin::Malformed`]: "there is a file and I cannot
    /// vouch for it" is the conservative reading, and it still lets the
    /// caller try its own parse on text that did arrive.
    pub fn from_code(code: u32) -> Origin {
        match code {
            SETTINGS_OK => Origin::File,
            SETTINGS_ABSENT => Origin::Absent,
            SETTINGS_REFUSED => Origin::Refused,
            _ => Origin::Malformed,
        }
    }
}

/// One settings file the host could not use, kept for the settings
/// window to show. The path is in it because the path is the only part
/// the user can act on, and the host is the only side that has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The addon the file belongs to — empty when the file belongs to
    /// no addon at all, which is what [`report`] is for: a file whose
    /// NAME is not a name, so no addon can ever ask for it.
    pub addon: String,
    /// The member file, or empty for an addon's single file.
    pub file: String,
    pub path: PathBuf,
    /// Ready to show: what went wrong, and where in the file.
    pub message: String,
}

/// One cached file. Absent files are cached too — a widget polling a
/// file nobody ever wrote is the common case, and it must not become a
/// `stat` per frame.
struct Cached {
    addon: String,
    file: String,
    text: String,
    origin: Origin,
}

/// One file [`store`] itself wrote, kept so that the backup it takes is
/// never a copy of this program's own output.
struct Written {
    addon: String,
    file: String,
    text: String,
}

/// How many distinct (addon, file) pairs are remembered. A program has
/// sixteen addons; the bound is against a caller asking for made-up
/// names in a loop, and past it reads still ANSWER, they just stop being
/// remembered — a cache that refuses is a cache that changes behaviour.
const CACHE_MAX: usize = 128;

static ROOTS: RwLock<Option<AssetRoots>> = RwLock::new(None);
static CACHE: RwLock<Vec<Cached>> = RwLock::new(Vec::new());
static PROBLEMS: RwLock<Vec<Problem>> = RwLock::new(Vec::new());
/// What [`store`] last put at each pair's path — see [`ours`], which is
/// the whole reason it is kept.
static WRITTEN: RwLock<Vec<Written>> = RwLock::new(Vec::new());
/// Starts at 1 so a caller may keep 0 for "I have never read".
static EPOCH: AtomicU32 = AtomicU32::new(1);

/// A plain name: not a path fragment, not a pattern, not empty.
///
/// Lower-case ASCII, digits, `_` and `-`. Deliberately narrower than
/// [`crate::assets::safe_component`], which allows a dot because a
/// `.layaut` file needs one: here the `.ron` suffix belongs to the host,
/// so refusing the dot outright means `..` cannot be SPELLED rather than
/// being filtered afterwards — no escape to reason about, at any depth.
/// Case is fixed for the same class of reason: two names differing only
/// in case are two files here and one file on a case-folding filesystem,
/// and a settings file that loads on one machine and not another is a
/// bug report nobody can reproduce.
fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= SETTINGS_NAME_MAX
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// The path within the settings directory, relative to `addons/`.
fn relative(addon: &str, file: &str) -> Option<String> {
    if !name_ok(addon) {
        return None;
    }
    if file.is_empty() {
        return Some(format!("{addon}.ron"));
    }
    if !name_ok(file) {
        return None;
    }
    Some(format!("{addon}/{file}.ron"))
}

/// Installs the directories settings are read from and written to.
/// Called once at startup, before any widget exists. Until it is, every
/// read answers [`Origin::Refused`] — a toolkit that invented a
/// directory name would be guessing at the embedder's identity, which
/// is the mistake [`crate::assets`] exists to avoid.
pub fn install(roots: AssetRoots) {
    if !runtime::is_host() {
        warn_attached("settings::install");
        return;
    }
    if let Ok(mut r) = ROOTS.write() {
        *r = Some(roots);
    }
    bump();
}

/// Forgets every cached file and steps the epoch, so the next read goes
/// to disk. For a host that edited the files behind the toolkit's back
/// — a reload command, a watch on the directory.
pub fn reload() {
    if !runtime::is_host() {
        warn_attached("settings::reload");
        return;
    }
    bump();
}

/// Drops the caches and steps the epoch. Problems are dropped with the
/// cache: they describe files that are about to be read again, and a
/// stale one would have the settings window reporting a mistake the
/// user has already fixed.
///
/// [`WRITTEN`] is deliberately NOT dropped with them, and the difference
/// is load-bearing rather than an oversight. The other two describe what
/// is on disk NOW and are worthless the moment it may have changed;
/// [`WRITTEN`] records what this program itself put there, which no
/// re-read can re-establish. Clearing it would make the save after every
/// reload — and [`store`] ends in a bump, so that is every SECOND save —
/// copy this program's own output over the user's document, which is the
/// exact loss [`ours`] exists to prevent.
fn bump() {
    if let Ok(mut c) = CACHE.write() {
        c.clear();
    }
    if let Ok(mut p) = PROBLEMS.write() {
        p.clear();
    }
    // The NUMBER means nothing — only that it differs from the one a
    // caller remembers — so a step is all this has to be, and it is one
    // operation rather than a read and a write that two hosts could
    // interleave into a single step.
    EPOCH.fetch_add(1, Ordering::Release);
}

fn warn_attached(what: &str) {
    crate::ui::warn_once(
        "settings.attached",
        &format!(
            "{what} was called by a plugin's copy of the toolkit — the settings \
             directories are the host's. The call is being dropped."
        ),
    );
}

/// Whether an embedder ever called [`install`]. False means every read
/// answers [`Origin::Refused`] and every settings file on the machine is
/// being ignored.
///
/// It is public because the embedder is the only side that can fix it
/// and the only side that knows whether it meant to: a host with a
/// settings window can say so where the user is looking, and a host that
/// deliberately runs without settings — a test, a headless render — can
/// ask instead of reading the warning as a defect. There is deliberately
/// no way to ask which directories: that would hand out a path, which is
/// the one thing this module does not do.
pub fn installed() -> bool {
    ROOTS.read().map(|r| r.is_some()).unwrap_or(false)
}

/// Said once when a read arrives before [`install`] did.
///
/// This is the failure mode the module exists to refuse, wearing the
/// embedder's clothes rather than the user's: with no directories every
/// read is [`Origin::Refused`], every addon quietly uses the values
/// baked into it, and a user who wrote `addons/search.ron` sees a widget
/// that looks factory-fresh with nothing anywhere connecting the two.
/// A wrong NAME already said so out loud; nothing said this, which made
/// the larger mistake the quieter one.
fn warn_not_installed() {
    crate::ui::warn_once(
        "settings.roots",
        "no settings directories are installed — `nacelle::settings::install` was \
         never called, so EVERY addon is running on the values baked into it and \
         every settings file on this machine is being ignored",
    );
}

/// Every file the host could not use, in the order it found them. What
/// the settings window shows; empty is the ordinary answer.
///
/// A host that never called [`install`] has no bad FILES and still has
/// the larger problem — see [`installed`], which a settings window must
/// ask separately, because the two are answered by different things.
pub fn problems() -> Vec<Problem> {
    PROBLEMS.read().map(|p| p.clone()).unwrap_or_default()
}

/// The settings window's half of a report, without the stderr half.
///
/// Split out because the two halves are keyed apart on purpose: a
/// document the host could not parse AND an addon could not fit is two
/// separate lines on stderr, each keyed so neither silences the other,
/// but it is ONE bad file and the window must not list it twice.
fn push_problem(addon: &str, file: &str, path: PathBuf, message: String) {
    if let Ok(mut p) = PROBLEMS.write() {
        // The cache holds one entry per pair, so one problem per pair
        // is the matching bound; a duplicate would mean the same file
        // twice in the settings window. The FIRST report wins, which is
        // the host's own — "this is not RON" is what a user acts on,
        // where "it does not fit" is what that failure looks like from
        // the far side of it.
        if !p.iter().any(|q| q.addon == addon && q.file == file) {
            p.push(Problem {
                addon: addon.to_string(),
                file: file.to_string(),
                path,
                message,
            });
        }
    }
}

/// A file in the settings directories that NOTHING will ever read, put
/// in front of the user by the host that found it.
///
/// The one thing this module cannot see for itself. Every other problem
/// here is discovered by somebody ASKING for a file, and the whole of
/// [`Origin::Refused`] is that a name which is not a plain name is a
/// programming error — an addon asking for `../etc/shadow` deserves a
/// line on stderr and nothing more. But a host that WALKS the directory
/// meets the same refusal wearing the user's clothes: `My Addon.ron` is
/// a file somebody wrote, sitting where settings files go, and no addon
/// can ever ask for that name, so no read will ever reach it and no
/// report will ever mention it. Left to stderr alone it is the exact
/// failure the module's own [`load`] refuses to have — a settings
/// window announcing that every file on the machine loads while two of
/// the user's do not — except permanent, because there is no repair
/// short of renaming the file.
///
/// Deduplicated by PATH rather than by name, because a name is what
/// this entry does not have: two directories may hold the same bad one.
pub fn report(path: PathBuf, message: String) {
    if !runtime::is_host() {
        warn_attached("settings::report");
        return;
    }
    if let Ok(mut p) = PROBLEMS.write() {
        if !p.iter().any(|q| q.path == path) {
            p.push(Problem { addon: String::new(), file: String::new(), path, message });
        }
    }
}

fn record(addon: &str, file: &str, path: PathBuf, message: String) {
    crate::ui::warn_once(
        &format!("settings.bad.{addon}.{file}"),
        &format!("{}: {message}", path.display()),
    );
    push_problem(addon, file, path, message);
}

/// Where a pair's file stands, for a caller that has ALREADY been given
/// a reason to name it.
///
/// It hands out no path to anybody: it is private, and the one caller
/// puts what it answers into a [`Problem`], which is the module's one
/// sanctioned way for a path to reach a screen. Silent about a missing
/// installation, unlike [`from_disk`] — it is asked about a read that
/// has already happened, so whatever that read had to say is said.
///
/// Answers `None` inside a plugin, and that is the whole of the
/// asymmetry below: a plugin's copy of this module has no roots (its
/// [`install`] is refused), so a plugin's misfit stays a line on stderr
/// exactly as before. Carrying it back over the boundary would be an
/// ABI entry, and the two addons this program ships are linked rather
/// than loaded, so the host's own copy is the one that sees them.
fn host_path(addon: &str, file: &str) -> Option<PathBuf> {
    let rel = relative(addon, file)?;
    let roots = ROOTS.read().ok()?;
    roots.as_ref()?.find("addons", &rel)
}

/// Reads one file off disk and says what it thinks of it. The host's
/// parse is [`ron::Value`] — the document, not anybody's type — because
/// this side has the path and the line numbers and nothing else.
fn from_disk(addon: &str, file: &str) -> (String, Origin) {
    let Some(rel) = relative(addon, file) else {
        crate::ui::warn_once(
            "settings.name",
            &format!(
                "an addon asked for settings under the name {addon:?}/{file:?}, which is \
                 not a plain name — the request was refused"
            ),
        );
        return (String::new(), Origin::Refused);
    };
    let found = {
        let Ok(roots) = ROOTS.read() else {
            warn_not_installed();
            return (String::new(), Origin::Refused);
        };
        let Some(roots) = roots.as_ref() else {
            warn_not_installed();
            return (String::new(), Origin::Refused);
        };
        roots.find("addons", &rel)
    };
    let Some(path) = found else {
        return (String::new(), Origin::Absent);
    };
    // Asked before reading, not after: a ceiling enforced by truncating
    // what is already in memory is not a ceiling on memory.
    match std::fs::metadata(&path) {
        Ok(m) if m.len() > SETTINGS_FILE_MAX as u64 => {
            record(
                addon,
                file,
                path,
                format!(
                    "this settings file is {} bytes, past the {SETTINGS_FILE_MAX}-byte \
                     limit — it was not read",
                    m.len()
                ),
            );
            return (String::new(), Origin::Malformed);
        }
        _ => {}
    }
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            record(addon, file, path, format!("could not be read: {e}"));
            return (String::new(), Origin::Malformed);
        }
    };
    let text = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(_) => {
            // Not delivered, unlike a parse failure: a caller receives
            // `&str`, and there is no prefix of invalid UTF-8 that is
            // worth handing anybody.
            record(addon, file, path, "is not UTF-8 text".to_string());
            return (String::new(), Origin::Malformed);
        }
    };
    match ron::from_str::<ron::Value>(&text) {
        Ok(_) => (text, Origin::File),
        Err(e) => {
            // A file holding only comments, or nothing at all, ends up
            // here as an end-of-input error, and it is named plainly:
            // "states no value" is a thing a person can act on, where
            // ron's own wording for it is not. It is NOT quietly turned
            // into "absent" — a file the user created and left empty is
            // a file whose author expected it to do something.
            let message = if e.code == ron::Error::Eof {
                "states no value — a settings file must contain one, `()` for none"
                    .to_string()
            } else {
                format!("is not valid RON — {e}")
            };
            record(addon, file, path, message);
            (text, Origin::Malformed)
        }
    }
}

/// The cached text for a pair, reading it once if this is the first ask.
fn cached(addon: &str, file: &str, f: impl FnOnce(&str, Origin) -> (usize, Origin)) -> (usize, Origin) {
    if let Ok(c) = CACHE.read() {
        if let Some(e) = c.iter().find(|e| e.addon == addon && e.file == file) {
            return f(&e.text, e.origin);
        }
    }
    let (text, origin) = from_disk(addon, file);
    let out = f(&text, origin);
    // A refusal is not remembered: it is a caller's mistake rather than
    // a fact about the disk, and caching it would fill the table from
    // whatever bad names a loop produces.
    if origin != Origin::Refused {
        if let Ok(mut c) = CACHE.write() {
            if c.len() < CACHE_MAX && !c.iter().any(|e| e.addon == addon && e.file == file) {
                c.push(Cached {
                    addon: addon.to_string(),
                    file: file.to_string(),
                    text,
                    origin,
                });
            }
        }
    }
    out
}

fn copy_out(text: &str, origin: Origin, buf: &mut [u8]) -> (usize, Origin) {
    let n = buf.len().min(text.len());
    buf[..n].copy_from_slice(&text.as_bytes()[..n]);
    (text.len(), origin)
}

/// Host-side read, for [`crate::plugin`]'s side of the boundary.
pub(crate) fn local_read_into(addon: &str, file: &str, buf: &mut [u8]) -> (usize, Origin) {
    cached(addon, file, |text, origin| copy_out(text, origin, buf))
}

pub(crate) fn local_epoch() -> u32 {
    EPOCH.load(Ordering::Acquire)
}

/// Copies an addon's settings text into `buf`, answering its FULL
/// length and where it came from.
///
/// The full length rather than what was written, for
/// [`crate::runtime::HostApi::settings_read`]'s reason: a prefix of a
/// document is a document that will not parse, so a caller must be able
/// to tell a truncation from a fit. Most callers want [`load`] instead.
pub fn read_into(addon: &str, file: &str, buf: &mut [u8]) -> (usize, Origin) {
    runtime::shared_with(
        "settings::read",
        |api| match api {
            None => local_read_into(addon, file, buf),
            Some(api) => {
                if !api.has_settings() {
                    warn_no_settings();
                    return (0, Origin::Absent);
                }
                let mut status = SETTINGS_ABSENT;
                let n = (api.settings_read)(
                    addon.as_ptr(),
                    addon.len() as u32,
                    file.as_ptr(),
                    file.len() as u32,
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    &mut status,
                );
                (n as usize, Origin::from_code(status))
            }
        },
        (0, Origin::Absent),
    )
}

/// Says once that this host predates the settings entries. A plugin
/// running on its own defaults is a reasonable state; a plugin running
/// on them while the user has written a file and been told nothing is
/// not, and from the plugin's side these look identical.
fn warn_no_settings() {
    crate::ui::warn_once(
        "settings.absent",
        "this host is older than the addon settings entries — every addon is \
         running on the values baked into it, and any settings file on this \
         machine is being ignored",
    );
}

/// Steps whenever a settings file may have changed. A caller parses
/// once, keeps the value and re-reads when this moves; parsing a
/// document per frame is not something a widget can afford, and never
/// re-reading means the settings window's Apply changes nothing on
/// screen.
pub fn epoch() -> u32 {
    runtime::shared(
        "settings::epoch",
        local_epoch,
        |api| {
            if !api.has_settings() {
                warn_no_settings();
                return 0;
            }
            (api.settings_epoch)()
        },
        0,
    )
}

/// The whole text of an addon's settings file, allocated.
///
/// `file` is empty for an addon with one settings file, and names the
/// member for an addon with a directory of them.
pub fn text(addon: &str, file: &str) -> (String, Origin) {
    // A settings file that fits this probe is read without a second
    // crossing, and the common answer — no file at all — allocates
    // nothing whatsoever.
    let mut probe = [0u8; 4096];
    let (len, origin) = read_into(addon, file, &mut probe);
    if len == 0 {
        return (String::new(), origin);
    }
    if len <= probe.len() {
        return (String::from_utf8_lossy(&probe[..len]).into_owned(), origin);
    }
    let mut buf = vec![0u8; len.min(SETTINGS_FILE_MAX)];
    let (len, origin) = read_into(addon, file, &mut buf);
    let n = len.min(buf.len());
    (String::from_utf8_lossy(&buf[..n]).into_owned(), origin)
}

/// An addon's settings, parsed into the addon's own type.
///
/// The one call an addon makes:
///
/// ```ignore
/// #[derive(serde::Deserialize, Default)]
/// #[serde(default)]                       // REQUIRED — see below
/// struct Config {
///     rows: u32,
///     format: String,
/// }
///
/// let (cfg, origin) = nacelle::settings::load::<Config>("clock", "");
/// ```
///
/// **Every field must be `#[serde(default)]`**, which the container
/// attribute above does in one line. It is not style. A RON document is
/// parsed all or nothing, so without defaults a file missing one field
/// — an old file, a file written before the addon grew a setting, a
/// file where the user set only the thing they cared about — fails
/// whole and the user loses every setting in it over the one that was
/// not there. With them, the document says what it says and the type
/// answers the rest.
///
/// A failed parse costs the addon's defaults and never silence: the
/// host has already reported a document that does not parse at all, and
/// a document that parses but does not FIT this type — the case only
/// this side can see — is reported here, once, naming the addon and
/// what serde objected to. It goes into [`problems`] as well as onto
/// stderr, because it is the likeliest mistake a settings file has in
/// it and a settings window that showed only the host's half would call
/// a file bad only when it was unreadable, never when it was merely
/// wrong.
pub fn load<T>(addon: &str, file: &str) -> (T, Origin)
where
    T: serde::de::DeserializeOwned + Default,
{
    let (text, origin) = text(addon, file);
    if text.is_empty() {
        return (T::default(), origin);
    }
    match ron::from_str::<T>(&text) {
        Ok(v) => (v, origin),
        Err(e) => {
            let message = format!(
                "does not fit what the addon expects — {e}. It is running on its \
                 own defaults."
            );
            // Reported even when the host already called the document
            // malformed: the two messages are about different things,
            // and the host's is about the file where this one is about
            // the fit. Keyed apart so neither silences the other.
            crate::ui::warn_once(
                &format!("settings.fit.{addon}.{file}"),
                &format!(
                    "the settings for addon {addon:?}{} {message}",
                    if file.is_empty() { String::new() } else { format!(" ({file})") }
                ),
            );
            // And into [`problems`], which is the ONLY thing a settings
            // window can show. Without this the window told a
            // half-truth on the likeliest mistake there is: a value of
            // the wrong type parses as RON, so the host has nothing to
            // say about it, and the one side that CAN see it was saying
            // it only to a stderr nobody has open. A user with
            // `(hidden: "yes")` in a file they wrote today had a widget
            // on factory values and a window reporting all clear.
            if let Some(path) = host_path(addon, file) {
                push_problem(addon, file, path, message);
            }
            (T::default(), Origin::Malformed)
        }
    }
}

/// Whether what stands at a pair's path is byte for byte what this
/// program last wrote there.
///
/// This one question is what makes the backup worth having, and getting
/// it wrong has a measured cost: a `.bak` refreshed on EVERY write is a
/// backup of the previous SAVE, not of the user's document, and two
/// saves are enough to lose the document entirely. The path to it is
/// short and ordinary. A settings window reads a file it cannot use —
/// a stray bracket, or a value of the wrong type, which [`load`] answers
/// with the addon's DEFAULTS by design — so its model is factory values
/// plus whatever the user is now dragging. Save once: `.bak` holds the
/// user's file. Save again — one more press of an arrow key on a slider
/// — and `.bak` holds the first save's factory text. Nothing anywhere
/// still has what the user wrote.
///
/// So the rule is not "keep the previous contents", which is what was
/// written down, but **keep what this program did not write**: a hand
/// edit is backed up the first time it is replaced and then left alone,
/// however many saves follow. It needs no generations, no timestamps and
/// no pruning — one file, holding the only version that was ever
/// irreplaceable.
///
/// Unremembered pairs answer false, so a program restarted between the
/// two saves takes one more backup than it strictly needs. That is the
/// side to be wrong on.
fn ours(addon: &str, file: &str, on_disk: &str) -> bool {
    WRITTEN
        .read()
        .map(|w| {
            w.iter()
                .any(|e| e.addon == addon && e.file == file && e.text == on_disk)
        })
        .unwrap_or(false)
}

/// Remembers the text [`store`] just wrote, for [`ours`].
fn remember(addon: &str, file: &str, text: &str) {
    if let Ok(mut w) = WRITTEN.write() {
        if let Some(e) = w.iter_mut().find(|e| e.addon == addon && e.file == file) {
            e.text = text.to_string();
        } else if w.len() < CACHE_MAX {
            // Past the bound nothing is remembered and [`ours`] answers
            // false, which costs a backup nobody needed rather than
            // skipping one somebody did.
            w.push(Written {
                addon: addon.to_string(),
                file: file.to_string(),
                text: text.to_string(),
            });
        }
    }
}

/// The new text, whole and on the medium, under a temporary name.
///
/// The `sync_all` is the half that was promised and missing. A rename is
/// atomic with respect to the DIRECTORY, so "the old file or the new
/// one" holds only once the new one's bytes have actually landed;
/// without it a crash can leave the entry pointing at a file that has
/// the length and none of the content, which is neither of the two
/// outcomes the caller was told to expect.
///
/// The file is the CALLER's, already claimed under a name of this
/// process's own — see [`claim_temp`], which is where the name that
/// nobody else can be holding comes from.
fn write_whole(mut f: std::fs::File, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    f.write_all(text.as_bytes())?;
    f.sync_all()
}

/// How many links deep [`through_links`] will follow before it answers
/// with the name it has reached. A chain longer than this is a loop,
/// and the write that follows fails with the system's own error rather
/// than walking forever.
const LINK_HOPS: u32 = 16;

/// The file a name finally leads to, following symbolic links.
///
/// A rename replaces the NAME, not the file: a settings file that is a
/// link into a dotfiles repository is a link until the first save, and
/// an ordinary file with the same contents afterwards. The values
/// survive that, which is why it reads as harmless, and the user's own
/// copy stops being written to and stops being read from the same
/// moment — every edit they make in the repository from then on goes
/// nowhere, with nothing said. So the temporary lands beside the
/// TARGET and is renamed over the target, and the link the user put
/// there is still a link afterwards.
///
/// A dangling link answers with the name it points at, which does not
/// exist: the write then creates the file the user asked for rather
/// than quietly replacing their link with one of ours.
fn through_links(path: &Path) -> PathBuf {
    let mut at = path.to_path_buf();
    for _ in 0..LINK_HOPS {
        // A path that is not a link at all fails here, and that is the
        // ordinary exit: what has been reached is the answer.
        let Ok(target) = std::fs::read_link(&at) else { return at };
        at = if target.is_absolute() {
            target
        } else {
            at.parent().unwrap_or(Path::new(".")).join(target)
        };
    }
    at
}

/// `<file name><suffix>`, which is not what `with_extension` does: a
/// settings file is `clock.ron`, and what is wanted beside it is
/// `clock.ron.bak` rather than `clock.bak`.
fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// How many temporary names one process will try before giving up.
/// Past one it is two threads of this same process saving at the same
/// instant, which is already unusual; the limit is against a directory
/// that answers "taken" forever.
const TEMP_TRIES: u32 = 64;

/// A temporary file beside `target` under a name no other writer can
/// be holding, CLAIMED rather than opened.
///
/// `File::create` on a shared name TRUNCATES what is there, and the
/// settings window and the running desktop are two processes — the
/// module already says so where it explains why the rescue copy uses
/// `create_new`. Two of them saving at once under one temporary name
/// is one writer emptying the other's file and the other renaming the
/// result into place: a settings file with half of one document and
/// half of another, which for an all-or-nothing format is no settings
/// file at all. The name carries the process id so the ordinary case
/// never collides, and `create_new` is what makes that a guarantee
/// rather than a likelihood.
///
/// The suffix is `.tmp` and the process id sits in front of it, so the
/// name never ends in `.ron`: a host walking the directory for settings
/// files must not find a half-written one and call it broken.
///
/// One left behind by a crash keeps its name and the claim steps past
/// it. Deleting a file this process did not create is the one thing a
/// claim cannot justify — the name may be a live save of the other
/// process, and telling those apart is the problem this avoids rather
/// than solves.
fn claim_temp(target: &Path) -> std::io::Result<(std::fs::File, PathBuf)> {
    use std::io::{Error, ErrorKind};
    let pid = std::process::id();
    for n in 0..TEMP_TRIES {
        let suffix =
            if n == 0 { format!(".{pid}.tmp") } else { format!(".{pid}.{n}.tmp") };
        let candidate = beside(target, &suffix);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(f) => return Ok((f, candidate)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(Error::new(ErrorKind::AlreadyExists, "no temporary name beside the settings file is free"))
}

/// Writes an addon's settings file, keeping a copy of what stood there.
///
/// The host's alone — the settings window, not a widget. Four things
/// happen in this order, and each is here rather than at a call site so
/// that there is exactly one of them:
///
/// * the text is PARSED before anything is touched, so a settings
///   window cannot write a file that will not load;
/// * the previous contents are copied to `<name>.ron.bak` UNLESS this
///   program wrote them, because a whole-document format loses
///   everything when a write goes wrong, where the line-oriented one it
///   replaces lost a line. The exception is the part that took a
///   measurement to get right: a copy refreshed on every save is a copy
///   of the previous SAVE, and two saves then leave nothing holding what
///   the user wrote. What is kept is what this program did NOT write —
///   see `ours` in this module for the whole of it;
/// * the new text lands through a temporary file, a `sync_all` and a
///   rename, so an interrupted write leaves the old file whole rather
///   than half the new one or an empty one. The temporary carries this
///   process's id and is CLAIMED, not opened — `claim_temp` — and the
///   rename lands on what the name leads to rather than on the name
///   itself, so a file kept as a link into somebody's dotfiles is still
///   a link afterwards — `through_links`;
/// * the directory entry is flushed too, because the rename is a change
///   to a directory and a directory is a file like any other.
pub fn store(addon: &str, file: &str, text: &str) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    if !runtime::is_host() {
        warn_attached("settings::store");
        return Err(Error::new(ErrorKind::PermissionDenied, "settings are the host's"));
    }
    let Some(rel) = relative(addon, file) else {
        return Err(Error::new(ErrorKind::InvalidInput, "not a plain addon name"));
    };
    if text.len() > SETTINGS_FILE_MAX {
        return Err(Error::new(ErrorKind::InvalidInput, "past the settings size limit"));
    }
    if let Err(e) = ron::from_str::<ron::Value>(text) {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("refusing to write settings that do not parse: {e}"),
        ));
    }
    let dir = {
        let roots = ROOTS.read().map_err(|_| Error::other("settings roots unavailable"))?;
        let roots = roots
            .as_ref()
            .ok_or_else(|| Error::other("no settings directories are installed"))?;
        roots.write_dir("addons")
    };
    let path = dir.join(&rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Read rather than `exists`, because the answer needed is not
    // whether something stands there but whose it is. A file that will
    // not come back as text — not UTF-8, or not readable at all — is
    // certainly not one this program wrote, so it is copied aside like
    // any other stranger's rather than falling through the check.
    let stranger = match std::fs::read_to_string(&path) {
        Ok(old) => !ours(addon, file, &old),
        Err(_) => path.exists(),
    };
    if stranger {
        // A failed backup does not stop the write: refusing to save
        // because the SAFETY copy failed would lose the user's edit for
        // certain in order to avoid losing it maybe. It is said out
        // loud, though — a backup silently not taken is how the next
        // document goes missing without anybody having been given a
        // chance to notice.
        //
        // Beside the NAME rather than beside whatever the name leads
        // to: the copy is this module's own litter, and a settings file
        // linked into somebody's dotfiles repository is not a place to
        // leave litter in.
        if let Err(e) = std::fs::copy(&path, beside(&path, ".bak")) {
            crate::ui::warn_once(
                &format!("settings.bak.{addon}.{file}"),
                &format!(
                    "{}: could not be copied aside before being overwritten ({e}) — \
                     the save is going ahead and the previous contents are not kept",
                    path.display()
                ),
            );
        }
    }
    // The file the name leads to, which is where the rename has to
    // land: renaming over the NAME would leave an ordinary file where
    // the user put a link. See `through_links`.
    let target = through_links(&path);
    // Named `open` and not `file`: the pair's member name is called
    // `file` here and is still wanted below, by `remember`.
    let (open, tmp) = claim_temp(&target)?;
    if let Err(e) = write_whole(open, text) {
        // A half-written temporary is litter, and this branch owns the
        // name it just claimed, so it is this branch that clears it.
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, &target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    // Best effort by necessity: not every platform lets a directory be
    // opened as a file, and one that does not is no reason to report a
    // write that succeeded as a failure.
    if let Some(parent) = target.parent() {
        if let Ok(d) = std::fs::File::open(parent) {
            let _ = d.sync_all();
        }
    }
    remember(addon, file, text);
    bump();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test, not eight: the roots, the cache and the epoch are
    /// process-wide on purpose, and separate `#[test]`s would race each
    /// other under the default harness — the channel's board has one
    /// test for the same reason.
    #[test]
    fn the_host_reads_the_file_and_the_addon_parses_it() {
        #[derive(serde::Deserialize, Default, PartialEq, Debug)]
        #[serde(default)]
        struct Cfg {
            rows: u32,
            format: String,
        }

        let base = std::env::temp_dir().join(format!("nacelle-settings-{}", std::process::id()));
        let (user, system) = (base.join("user"), base.join("system"));
        std::fs::create_dir_all(user.join("addons/search")).unwrap();
        std::fs::create_dir_all(system.join("addons")).unwrap();

        // Nothing installed: refused, and refused is not "absent" —
        // a toolkit with no directories has been misused, where a
        // toolkit with directories and no file has not. It is also a
        // state the embedder can ASK about rather than deduce from a
        // status that reads the same as a bad name.
        assert!(!installed(), "nothing is installed until an embedder says so");
        assert_eq!(load::<Cfg>("clock", "").1, Origin::Refused);

        install(AssetRoots::new(vec![user.clone(), system.clone()], user.clone()));
        assert!(installed());

        // No file: the type's own defaults, quietly.
        let (cfg, origin) = load::<Cfg>("clock", "");
        assert_eq!(origin, Origin::Absent);
        assert_eq!(cfg, Cfg::default());
        assert!(problems().is_empty(), "an absent file is not a problem");

        // A file in the SYSTEM directory answers when the user has
        // none — the whole point of the packaged end of the cascade.
        std::fs::write(system.join("addons/clock.ron"), "(rows: 2, format: \"24h\")").unwrap();
        reload();
        let (cfg, origin) = load::<Cfg>("clock", "");
        assert_eq!(origin, Origin::File);
        assert_eq!(cfg, Cfg { rows: 2, format: "24h".into() });

        // The user's file wins WHOLE, and a field it does not mention
        // falls to the TYPE's default rather than to the system file's
        // value — the documented difference from the program config.
        std::fs::write(user.join("addons/clock.ron"), "(rows: 9)").unwrap();
        reload();
        let (cfg, origin) = load::<Cfg>("clock", "");
        assert_eq!(origin, Origin::File);
        assert_eq!(cfg, Cfg { rows: 9, format: String::new() });

        // A directory member is addressed by its own name.
        std::fs::write(user.join("addons/search/engines.ron"), "(rows: 4)").unwrap();
        reload();
        assert_eq!(load::<Cfg>("search", "engines").0.rows, 4);
        // ...and is a different file from the addon's own.
        assert_eq!(load::<Cfg>("search", "").1, Origin::Absent);

        // A broken document: defaults, but LOUDLY — the status says
        // there is a file, and the settings window is handed the path
        // and the position.
        std::fs::write(user.join("addons/clock.ron"), "(rows: 9").unwrap();
        reload();
        let (cfg, origin) = load::<Cfg>("clock", "");
        assert_eq!(origin, Origin::Malformed);
        assert_eq!(cfg, Cfg::default());
        let p = problems();
        assert_eq!(p.len(), 1, "one bad file, one problem");
        assert_eq!(p[0].addon, "clock");
        assert_eq!(p[0].path, user.join("addons/clock.ron"));
        assert!(!p[0].message.is_empty());

        // An empty file is malformed rather than absent: somebody made
        // it, expecting it to do something.
        std::fs::write(user.join("addons/clock.ron"), "// only a comment\n").unwrap();
        reload();
        assert_eq!(load::<Cfg>("clock", "").1, Origin::Malformed);
        assert!(problems()[0].message.contains("states no value"));

        // A document that parses but does not FIT is the case only the
        // addon's side can see, and it still ends in defaults, never in
        // a wrong value.
        std::fs::write(user.join("addons/clock.ron"), "(rows: \"nine\")").unwrap();
        reload();
        let (cfg, origin) = load::<Cfg>("clock", "");
        assert_eq!(origin, Origin::Malformed);
        assert_eq!(cfg, Cfg::default());
        // ...and it reaches the SETTINGS WINDOW, which is the half that
        // was missing. The host's own parse accepted this document, so
        // nothing above it ever named the file: a user who typed a
        // string where a number goes had a widget on its defaults, a
        // line on a stderr nobody is reading, and a settings window
        // saying every file on the machine loads.
        let p = problems();
        assert_eq!(p.len(), 1, "a document that does not fit is still a bad file");
        assert_eq!(p[0].addon, "clock");
        assert_eq!(p[0].path, user.join("addons/clock.ron"));
        assert!(
            p[0].message.contains("does not fit"),
            "the window is not told WHICH of the two failures this is: {}",
            p[0].message
        );

        // Truncation is detectable: the FULL length is answered, not
        // what was written, so a caller never parses half a document.
        std::fs::write(user.join("addons/clock.ron"), "(rows: 9)").unwrap();
        reload();
        let mut small = [0u8; 4];
        let (len, origin) = read_into("clock", "", &mut small);
        assert_eq!(len, 9, "the full length, not the four bytes written");
        assert_eq!(origin, Origin::File);
        assert_eq!(&small, b"(row");

        // Writing keeps the old contents and refuses to write rubbish.
        let before = epoch();
        assert!(store("clock", "", "(rows: 1").is_err(), "unparseable is refused");
        assert!(store("clock", "", "(rows: 1)").is_ok());
        assert_ne!(epoch(), before, "a write invalidates every cached read");
        assert_eq!(
            std::fs::read_to_string(user.join("addons/clock.ron.bak")).unwrap(),
            "(rows: 9)",
            "what stood there is kept"
        );
        assert_eq!(load::<Cfg>("clock", "").0.rows, 1);
        // Nothing is left lying about under the name the next save
        // reuses.
        assert!(!user.join("addons/clock.ron.tmp").exists());

        // A SECOND save does not refresh the copy, and that is the
        // whole of it: what stands on disk now is this program's own
        // last write, so copying it aside would replace the user's
        // document with our output. One more press of an arrow key on a
        // slider is one more save.
        assert!(store("clock", "", "(rows: 2)").is_ok());
        assert_eq!(
            std::fs::read_to_string(user.join("addons/clock.ron.bak")).unwrap(),
            "(rows: 9)",
            "the copy kept is the USER's file, not the previous save"
        );
        assert_eq!(load::<Cfg>("clock", "").0.rows, 2, "the save itself still lands");

        // ...while a file the user edited by hand IS copied aside,
        // however many saves came before it. The rule is whose the
        // bytes are, not how many writes have happened.
        std::fs::write(user.join("addons/clock.ron"), "(rows: 7)").unwrap();
        reload();
        assert!(store("clock", "", "(rows: 3)").is_ok());
        assert_eq!(
            std::fs::read_to_string(user.join("addons/clock.ron.bak")).unwrap(),
            "(rows: 7)",
            "a hand edit is the next thing worth keeping"
        );

        // The scenario end to end, on the case that makes it dangerous:
        // a document that PARSES and does not fit, which `load` answers
        // with the type's defaults — so a settings window's model holds
        // no trace of what the user wrote, and every save it makes is
        // factory text. Four of them, and the user's own file is still
        // there to be recovered.
        let handwritten = "(rows: \"nine\", format: \"iso\") // mine\n";
        std::fs::write(user.join("addons/clock.ron"), handwritten).unwrap();
        reload();
        assert_eq!(load::<Cfg>("clock", "").1, Origin::Malformed);
        for rows in 0..4 {
            store("clock", "", &format!("(rows: {rows})")).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(user.join("addons/clock.ron.bak")).unwrap(),
            handwritten,
            "four saves later the user's own document is still recoverable"
        );

        // A file that will not come back as text is a stranger's too.
        // It has to be checked, because "did this program write it" is
        // asked by READING, and the read is the thing that fails.
        std::fs::write(user.join("addons/clock.ron"), [0x28, 0xff, 0xfe, 0x29]).unwrap();
        reload();
        assert!(store("clock", "", "(rows: 5)").is_ok());
        assert_eq!(
            std::fs::read(user.join("addons/clock.ron.bak")).unwrap(),
            [0x28, 0xff, 0xfe, 0x29],
            "unreadable is not the same as ours"
        );

        // A temporary NOBODY here claimed is not touched, whatever it
        // is called. `File::create` empties what it finds, and what it
        // can find under a name every process shares is the other
        // process's save in progress — the settings window and the
        // running desktop are two of them. Emptying it is half a
        // document, and renaming that into place is a settings file
        // with half of one save and half of another in it.
        //
        // Both names are planted, and they measure different halves.
        // `clock.ron.tmp` is the one a shared name looks like — a build
        // that opens it is the bug — and `clock.ron.<pid>.tmp` is this
        // process's own first candidate, so planting it forces the
        // claim to move along to the next rather than to give up.
        let mid_write = "(rows: 4"; // as far as the other writer had got
        let shared = user.join("addons/clock.ron.tmp");
        let mine = user.join(format!("addons/clock.ron.{}.tmp", std::process::id()));
        for squatter in [&shared, &mine] {
            std::fs::write(squatter, mid_write).unwrap();
        }
        assert!(store("clock", "", "(rows: 6)").is_ok());
        for squatter in [&shared, &mine] {
            assert_eq!(
                std::fs::read_to_string(squatter).ok().as_deref(),
                Some(mid_write),
                "{}: another writer's temporary may not be truncated",
                squatter.display()
            );
            std::fs::remove_file(squatter).unwrap();
        }
        assert_eq!(load::<Cfg>("clock", "").0.rows, 6, "and the save still lands");

        // A settings file the user keeps as a LINK stays a link, and
        // the file at the far end of it is the one that changes.
        #[cfg(unix)]
        a_link_is_followed_not_replaced(&base, &user);

        // A file no addon can ever ask for reaches the settings window,
        // because the host that walked the directory put it there. This
        // is the one thing this module cannot see for itself: nobody
        // will ever READ `My Addon.ron`, so nothing here would ever
        // have a word to say about it.
        reload();
        let unnamable = user.join("addons/My Addon.ron");
        std::fs::write(&unnamable, "(rows: 1)").unwrap();
        report(unnamable.clone(), "is not a name any addon can ask for".to_string());
        report(unnamable.clone(), "and saying it twice is saying it once".to_string());
        let p = problems();
        assert_eq!(p.len(), 1, "one file, one line in the window");
        assert_eq!(p[0].path, unnamable);
        assert!(p[0].addon.is_empty(), "it belongs to no addon: that is the point");

        // Names are names, not paths. Every one of these is refused,
        // and the refusal is what makes the host the only side holding
        // a path a meaningful statement.
        for (addon, file) in [
            ("..", ""),
            ("../../etc/shadow", ""),
            ("clock", ".."),
            ("clock/../search", ""),
            ("Clock", ""),
            ("my.addon", ""),
            ("", ""),
            ("clock", "a/b"),
        ] {
            assert_eq!(
                load::<Cfg>(addon, file).1,
                Origin::Refused,
                "{addon:?}/{file:?} must be refused"
            );
        }

        let _ = std::fs::remove_dir_all(base);
    }

    /// The measurement behind `through_links`, split out only because
    /// symbolic links are a Unix thing and the rest of the test is not.
    ///
    /// What is checked is not the values — those survive a link being
    /// replaced by a copy of itself, which is exactly why the loss is
    /// silent. It is that the LINK is still there afterwards: the user's
    /// file is in their own tree, they go on editing it there, and a
    /// save that quietly turned the link into an ordinary file would
    /// mean every one of those edits from then on goes nowhere.
    #[cfg(unix)]
    fn a_link_is_followed_not_replaced(base: &Path, user: &Path) {
        #[derive(serde::Deserialize, Default)]
        #[serde(default)]
        struct Cfg {
            rows: u32,
        }

        let vault = base.join("dotfiles");
        std::fs::create_dir_all(&vault).unwrap();
        let real = vault.join("clock-settings.ron");
        std::fs::write(&real, "(rows: 11) // kept in my own tree\n").unwrap();
        let link = user.join("addons/clock.ron");
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        reload();
        assert_eq!(load::<Cfg>("clock", "").0.rows, 11, "the link is read through");

        assert!(store("clock", "", "(rows: 12)").is_ok());
        assert!(
            std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(),
            "the save replaced the user's link with a file of ours"
        );
        assert_eq!(
            std::fs::read_to_string(&real).unwrap(),
            "(rows: 12)",
            "the file at the far end is the one that had to change"
        );
        // The user's own text is kept where this module keeps its
        // copies — in the settings directory, not in their repository.
        assert_eq!(
            std::fs::read_to_string(user.join("addons/clock.ron.bak")).unwrap(),
            "(rows: 11) // kept in my own tree\n",
            "the hand-written original is still recoverable"
        );
        // ...and the next edit the user makes in their own tree is read
        // again, which is the whole of what the link was for.
        std::fs::write(&real, "(rows: 13)").unwrap();
        reload();
        assert_eq!(load::<Cfg>("clock", "").0.rows, 13);
    }
}
