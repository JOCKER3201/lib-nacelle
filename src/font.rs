//! Font loading and the glyph atlas (single-channel, R8).
//!
//! eDEX-UI uses "United Sans" (UI) and "Fira Mono" (terminal). The .woff2
//! files from the eDEX repository can be converted to .ttf and dropped into
//! ./fonts — they are picked up automatically. Otherwise we look for
//! similar system fonts.

use fontdue::{Font, FontSettings};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const ATLAS_W: usize = 2048;
pub const ATLAS_H: usize = 2048;
/// The mask band: the atlas's last rows, reserved for procedural R8 masks —
/// the soft-disk sprite glow and shadows are built from. The shelf packer
/// never allocates from it and reset_atlas() never clears it (r1 §4, M0).
pub const MASK_BAND_H: usize = 128;
/// First row of the band; glyphs may only ever pack above this line.
pub const MASK_BAND_Y: usize = ATLAS_H - MASK_BAND_H;
/// The soft disk: a 64x64 radial gaussian falloff at the band's origin.
/// Drawn as a nine-slice it is a rounded soft rectangle at any size — one
/// sprite serves every glow, shadow and soft box in the program.
pub const MASK_SOFT: (usize, usize, usize, usize) = (0, MASK_BAND_Y, 64, 64);

pub const FONT_UI: u8 = 0;
pub const FONT_MONO: u8 = 1;
/// How many fonts there are. A font id arriving from outside this crate
/// — from a plugin, say — is an index into a fixed array, so it has to
/// be checked against this rather than trusted.
pub const FONT_COUNT: u8 = 2;

#[derive(Clone, Copy)]
pub struct Glyph {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub w: f32,
    pub h: f32,
    /// Offset of the bitmap's left edge relative to the pen.
    pub xmin: f32,
    /// Offset of the bitmap's bottom edge relative to the baseline (Y axis up).
    pub ymin: f32,
    pub advance: f32,
}

pub struct FontSystem {
    fonts: [Font; 2],
    pub atlas: Vec<u8>,
    /// The rows touched since the last take_dirty_rows(): (lo, hi exclusive).
    /// Uploading only these is what keeps a glyph-churn frame at microseconds
    /// instead of re-copying four megabytes (r1's mandatory rider on M0).
    dirty_rows: Option<(usize, usize)>,
    pub atlas_dirty: bool,
    cache: HashMap<(u8, u32, char), Option<Glyph>>,
    // simple shelf packer
    cur_x: usize,
    cur_y: usize,
    row_h: usize,
    /// The atlas filled up mid-frame; reset it at the next frame start so
    /// glyphs already emitted into the current draw list keep valid UVs.
    reset_pending: bool,
}

impl FontSystem {
    pub fn new() -> Self {
        let (ui, mono) = load_fonts();
        let mut fs = FontSystem {
            fonts: [ui, mono],
            atlas: vec![0u8; ATLAS_W * ATLAS_H],
            dirty_rows: Some((0, ATLAS_H)),
            atlas_dirty: true,
            cache: HashMap::new(),
            cur_x: 2,
            cur_y: 2,
            row_h: 0,
            reset_pending: false,
        };
        // White pixel (0,0..2x2) for solid fills.
        for y in 0..2 {
            for x in 0..2 {
                fs.atlas[y * ATLAS_W + x] = 255;
            }
        }
        fs.bake_masks();
        fs
    }

    /// Replaces the terminal font (settings change); resets the atlas.
    pub fn set_mono(&mut self, font: Font) {
        self.fonts[FONT_MONO as usize] = font;
        self.reset_atlas();
    }

    /// Replaces the interface font (settings change); resets the atlas.
    pub fn set_ui(&mut self, font: Font) {
        self.fonts[FONT_UI as usize] = font;
        self.reset_atlas();
    }

    /// UV of the white pixel — used by solid shapes.
    pub fn white_uv() -> (f32, f32) {
        (0.5 / ATLAS_W as f32, 0.5 / ATLAS_H as f32)
    }

    /// Clears the atlas and cache (e.g. when full after many resizes).
    /// Call once at the start of each frame, before any glyph() calls:
    /// performs a deferred atlas reset requested when the atlas filled
    /// during the previous frame. Resetting here (never mid-frame) keeps
    /// the UVs of glyphs already in the draw list valid.
    fn mark_rows(&mut self, y0: usize, y1: usize) {
        let (lo, hi) = self.dirty_rows.unwrap_or((y0, y1));
        self.dirty_rows = Some((lo.min(y0), hi.max(y1)));
        self.atlas_dirty = true;
    }

    /// The rows the renderer must re-upload, and the reset of the tracker.
    /// None = nothing changed since the last call.
    pub fn take_dirty_rows(&mut self) -> Option<(u32, u32)> {
        let r = self.dirty_rows.take();
        self.atlas_dirty = false;
        r.map(|(lo, hi)| (lo as u32, (hi - lo) as u32))
    }

    /// Bakes the procedural masks into the reserved band. Once, at startup —
    /// the band survives every atlas reset, so the bake never re-runs.
    fn bake_masks(&mut self) {
        let (mx, my, mw, mh) = MASK_SOFT;
        let (cx, cy) = (mw as f32 / 2.0 - 0.5, mh as f32 / 2.0 - 0.5);
        // Gaussian falloff, sigma at a third of the radius: reads as light,
        // not as a hard-edged disk, and the nine-slice keeps the profile.
        let r = mw as f32 / 2.0;
        let sigma = r / 3.0;
        for y in 0..mh {
            for x in 0..mw {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let d = (dx * dx + dy * dy).sqrt();
                let v = if d >= r {
                    0.0
                } else {
                    (-(d * d) / (2.0 * sigma * sigma)).exp()
                };
                self.atlas[(my + y) * ATLAS_W + (mx + x)] = (v * 255.0) as u8;
            }
        }
        self.mark_rows(my, my + mh);
    }

    /// The soft-disk mask's uv rect, for the draw list's sprite emitters.
    pub fn mask_soft_uv() -> (f32, f32, f32, f32) {
        let (x, y, w, h) = MASK_SOFT;
        (
            x as f32 / ATLAS_W as f32,
            y as f32 / ATLAS_H as f32,
            (x + w) as f32 / ATLAS_W as f32,
            (y + h) as f32 / ATLAS_H as f32,
        )
    }

    pub fn begin_frame(&mut self) {
        if self.reset_pending {
            self.reset_atlas();
            self.reset_pending = false;
        }
    }

    fn reset_atlas(&mut self) {
        // Clear the glyph shelves only — the mask band below MASK_BAND_Y is
        // baked once and survives every reset.
        self.atlas[..MASK_BAND_Y * ATLAS_W].iter_mut().for_each(|p| *p = 0);
        for y in 0..2 {
            for x in 0..2 {
                self.atlas[y * ATLAS_W + x] = 255;
            }
        }
        self.cache.clear();
        self.cur_x = 2;
        self.cur_y = 2;
        self.row_h = 0;
        self.mark_rows(0, MASK_BAND_Y);
    }

    pub fn glyph(&mut self, font: u8, px: f32, ch: char) -> Option<Glyph> {
        let key = (font, (px * 4.0).round() as u32, ch);
        if let Some(g) = self.cache.get(&key) {
            return *g;
        }
        let f = &self.fonts[font as usize];
        let (metrics, bitmap) = f.rasterize(ch, px);
        if metrics.width == 0 || metrics.height == 0 {
            let g = Some(Glyph {
                u0: 0.0,
                v0: 0.0,
                u1: 0.0,
                v1: 0.0,
                w: 0.0,
                h: 0.0,
                xmin: 0.0,
                ymin: 0.0,
                advance: metrics.advance_width,
            });
            self.cache.insert(key, g);
            return g;
        }
        let (w, h) = (metrics.width, metrics.height);
        if self.cur_x + w + 2 > ATLAS_W {
            self.cur_x = 2;
            self.cur_y += self.row_h + 2;
            self.row_h = 0;
        }
        if self.cur_y + h + 2 > MASK_BAND_Y {
            // Atlas full — defer the reset to the next frame (begin_frame)
            // instead of zeroing it mid-frame under the current draw list.
            // This glyph is skipped for one frame, then rendered cleanly.
            self.reset_pending = true;
            return None;
        }
        let (ax, ay) = (self.cur_x, self.cur_y);
        for row in 0..h {
            let dst = (ay + row) * ATLAS_W + ax;
            self.atlas[dst..dst + w].copy_from_slice(&bitmap[row * w..row * w + w]);
        }
        self.cur_x += w + 2;
        self.row_h = self.row_h.max(h);
        self.mark_rows(ay, ay + h);

        let g = Some(Glyph {
            u0: ax as f32 / ATLAS_W as f32,
            v0: ay as f32 / ATLAS_H as f32,
            u1: (ax + w) as f32 / ATLAS_W as f32,
            v1: (ay + h) as f32 / ATLAS_H as f32,
            w: w as f32,
            h: h as f32,
            xmin: metrics.xmin as f32,
            ymin: metrics.ymin as f32,
            advance: metrics.advance_width,
        });
        self.cache.insert(key, g);
        g
    }

    /// Line metrics: (ascent, line height).
    pub fn line_metrics(&self, font: u8, px: f32) -> (f32, f32) {
        if let Some(m) = self.fonts[font as usize].horizontal_line_metrics(px) {
            (m.ascent, m.ascent - m.descent + m.line_gap)
        } else {
            (px * 0.8, px * 1.2)
        }
    }

    /// Cell width for the monospace font.
    pub fn mono_advance(&mut self, px: f32) -> f32 {
        self.glyph(FONT_MONO, px, 'M').map(|g| g.advance).unwrap_or(px * 0.6)
    }

    pub fn measure(&mut self, font: u8, px: f32, text: &str, letter_spacing: f32) -> f32 {
        let mut w = 0.0;
        for ch in text.chars() {
            if let Some(g) = self.glyph(font, px, ch) {
                w += g.advance + letter_spacing;
            }
        }
        w
    }
}

fn try_load(path: &Path) -> Option<Font> {
    let data = std::fs::read(path).ok()?;
    Font::from_bytes(data, FontSettings::default()).ok()
}

/// Recursive search for a font file whose name (case-insensitive,
/// separators stripped) contains one of the patterns.
fn find_font(dirs: &[PathBuf], patterns: &[&str]) -> Option<PathBuf> {
    fn walk(dir: &Path, patterns: &[&str], depth: u32, out: &mut Option<PathBuf>) {
        if depth > 4 || out.is_some() {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for entry in rd.flatten() {
            if out.is_some() {
                return;
            }
            let p = entry.path();
            if p.is_dir() {
                walk(&p, patterns, depth + 1, out);
            } else {
                let name: String = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_lowercase()
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .collect();
                if !(name.ends_with("ttf") || name.ends_with("otf")) {
                    continue;
                }
                // Avoid italic variants; bold only when explicitly requested.
                if name.contains("italic") || name.contains("oblique") {
                    continue;
                }
                for pat in patterns {
                    if name.contains(pat) {
                        if name.contains("bold") && !pat.contains("bold") {
                            continue;
                        }
                        *out = Some(p.clone());
                        break;
                    }
                }
            }
        }
    }
    for &pat in patterns {
        let mut found = None;
        for d in dirs {
            walk(d, &[pat], 0, &mut found);
            if found.is_some() {
                return found;
            }
        }
    }
    None
}

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("fonts")];
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(format!("{home}/.local/share/fonts")));
        dirs.push(PathBuf::from(format!("{home}/.fonts")));
    }
    dirs.push(PathBuf::from("/usr/share/fonts"));
    dirs.push(PathBuf::from("/usr/local/share/fonts"));
    dirs
}

/// Curated monospace families for the settings dropdown
/// (display name, normalized filename pattern).
const MONO_FAMILIES: [(&str, &str); 12] = [
    ("Fira Mono", "firamono"),
    ("Fira Code", "firacode"),
    ("JetBrains Mono", "jetbrainsmono"),
    ("DejaVu Sans Mono", "dejavusansmono"),
    ("Liberation Mono", "liberationmono"),
    ("Noto Sans Mono", "notosansmono"),
    ("Ubuntu Mono", "ubuntumono"),
    ("Source Code Pro", "sourcecodepro"),
    ("Hack", "hack"),
    ("IBM Plex Mono", "ibmplexmono"),
    ("Cascadia Code", "cascadiacode"),
    ("Inconsolata", "inconsolata"),
];

/// Curated interface (UI) families (display name, filename pattern).
const UI_FAMILIES: [(&str, &str); 7] = [
    ("United Sans", "unitedsans"),
    ("Oxanium", "oxanium"),
    ("Rajdhani", "rajdhani"),
    ("Exo 2", "exo2"),
    ("Orbitron", "orbitron"),
    ("Saira Condensed", "sairacondensed"),
    ("Saira", "saira"),
];

fn pattern_for(display: &str) -> Option<&'static str> {
    MONO_FAMILIES
        .iter()
        .chain(UI_FAMILIES.iter())
        .find(|(name, _)| *name == display)
        .map(|(_, pat)| *pat)
}

fn available_from(table: &[(&str, &str)]) -> Vec<String> {
    let dirs = font_dirs();
    table
        .iter()
        .filter(|(_, pat)| find_font(&dirs, &[pat]).is_some())
        .map(|(name, _)| name.to_string())
        .collect()
}

/// Monospace families actually available on this system (terminal font).
pub fn available_mono_families() -> Vec<String> {
    available_from(&MONO_FAMILIES)
}

/// Interface families available on this system (UI list first, then mono).
pub fn available_ui_families() -> Vec<String> {
    let mut out = available_from(&UI_FAMILIES);
    out.extend(available_from(&MONO_FAMILIES));
    out
}

/// Default search patterns used when no family is selected.
const DEFAULT_MONO_PATTERNS: [&str; 10] = [
    "firamonoregular", "firamono", "firacoderegular", "firacode",
    "jetbrainsmonoregular", "jetbrainsmono", "dejavusansmono",
    "liberationmonoregular", "liberationmono", "notosansmono",
];
const DEFAULT_UI_PATTERNS: [&str; 8] = [
    "unitedsansmedium", "unitedsans", "oxanium", "rajdhani",
    "exo2", "orbitron", "sairacondensed", "saira",
];

/// Loads a font by family display name and weight
/// (Light/Regular/Medium/SemiBold/Bold). With no family selected the
/// weight is searched across the default families of the given kind.
pub fn load_variant_for(
    family: Option<&str>,
    weight: Option<&str>,
    ui: bool,
) -> Option<Font> {
    let dirs = font_dirs();
    let w = weight.unwrap_or("Regular").to_lowercase().replace(' ', "");
    let base: Vec<&str> = match family.and_then(pattern_for) {
        Some(p) => vec![p],
        None => {
            if ui {
                DEFAULT_UI_PATTERNS.to_vec()
            } else {
                DEFAULT_MONO_PATTERNS.to_vec()
            }
        }
    };
    // The requested weight first, across all candidate families. For the
    // default UI font the weighted search also covers the mono families,
    // because United Sans ships in a single weight only.
    let mut weighted = base.clone();
    if ui && family.is_none() {
        weighted.extend(DEFAULT_MONO_PATTERNS);
    }
    if w != "regular" {
        for pat in &weighted {
            let c = format!("{pat}{w}");
            if let Some(p) = find_font(&dirs, &[c.as_str()]) {
                if let Some(f) = try_load(&p) {
                    return Some(f);
                }
            }
        }
    }
    // ...then the regular variants.
    for pat in &base {
        for c in [format!("{pat}regular"), pat.to_string()] {
            if let Some(p) = find_font(&dirs, &[c.as_str()]) {
                if let Some(f) = try_load(&p) {
                    return Some(f);
                }
            }
        }
    }
    None
}

/// Loads the default terminal font (Fira Mono like eDEX, then fallbacks).
pub fn load_default_mono() -> Font {
    let dirs = font_dirs();
    let mono_path = std::env::var("NACELLE_FONT_MONO").ok().map(PathBuf::from).or_else(|| {
        find_font(&dirs, &DEFAULT_MONO_PATTERNS)
    });
    mono_path.as_deref().and_then(try_load).unwrap_or_else(|| {
        panic!(
            "nacelle-desktop: no monospace font (.ttf/.otf) found.\n\
             Point NACELLE_FONT_MONO at one or drop it into ./fonts"
        )
    })
}

/// Loads the default interface font (United Sans like eDEX, then similar
/// "technical" typefaces; falls back to the monospace font).
pub fn load_default_ui() -> Font {
    let dirs = font_dirs();
    let ui_path = std::env::var("NACELLE_FONT_UI").ok().map(PathBuf::from).or_else(|| {
        find_font(&dirs, &DEFAULT_UI_PATTERNS)
    });
    ui_path.as_deref().and_then(try_load).unwrap_or_else(|| {
        eprintln!("nacelle-desktop: no UI font (United Sans) — using the monospace font");
        load_default_mono()
    })
}

fn load_fonts() -> (Font, Font) {
    (load_default_ui(), load_default_mono())
}
