//! Color theme — "tron" from eDEX-UI by default.
//! Any eDEX-UI theme file can be loaded via NACELLE_THEME=/path/to/tron.json

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Color {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }

    pub fn from_hex(hex: &str) -> Option<Self> {
        let h = hex.trim().trim_start_matches('#');
        // The byte-slices below require exactly 6 ASCII bytes; a 6-byte
        // multibyte value (e.g. "€€") would otherwise slice mid-character
        // and panic.
        if h.len() != 6 || !h.is_ascii() {
            return None;
        }
        let r = u8::from_str_radix(&h[0..2], 16).ok()?;
        let g = u8::from_str_radix(&h[2..4], 16).ok()?;
        let b = u8::from_str_radix(&h[4..6], 16).ok()?;
        Some(Self::rgb8(r, g, b))
    }

    /// Same color with a different alpha.
    pub fn alpha(self, a: f32) -> Self {
        Color { a, ..self }
    }

    /// Interpolation towards black (dimming).
    pub fn dim(self, f: f32) -> Self {
        Color {
            r: self.r * f,
            g: self.g * f,
            b: self.b * f,
            a: self.a,
        }
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

pub struct Theme {
    /// Main accent color (in eDEX: rgb(r,g,b) from the theme).
    pub base: Color,
    /// Background of the whole UI (light_black).
    pub bg: Color,
    /// Grid/grey color (grey).
    pub grey: Color,
    pub term_fg: Color,
    pub term_bg: Color,
    pub cursor: Color,
    /// 16-color ANSI palette for the terminal.
    pub ansi: [Color; 16],
}

impl Theme {
    /// The "tron" theme — the default eDEX-UI theme.
    pub fn tron() -> Self {
        let base = Color::rgb8(170, 207, 209);
        Theme {
            base,
            bg: Color::from_hex("#05080d").unwrap(),
            grey: Color::from_hex("#262828").unwrap(),
            term_fg: base,
            term_bg: Color::from_hex("#05080d").unwrap(),
            cursor: base,
            ansi: default_ansi(),
        }
    }

    /// Loads a theme in the eDEX-UI format (themes/*.json).
    pub fn from_edex_json(text: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let colors = v.get("colors")?;
        let base = Color::rgb8(
            colors.get("r")?.as_u64()? as u8,
            colors.get("g")?.as_u64()? as u8,
            colors.get("b")?.as_u64()? as u8,
        );
        let bg = colors
            .get("light_black")
            .and_then(|s| s.as_str())
            .and_then(Color::from_hex)
            .unwrap_or(Color::from_hex("#05080d").unwrap());
        let grey = colors
            .get("grey")
            .and_then(|s| s.as_str())
            .and_then(Color::from_hex)
            .unwrap_or(Color::from_hex("#262828").unwrap());
        let term = v.get("terminal");
        let getc = |key: &str, def: Color| -> Color {
            term.and_then(|t| t.get(key))
                .and_then(|s| s.as_str())
                .and_then(Color::from_hex)
                .unwrap_or(def)
        };
        let mut ansi = default_ansi();
        // Some eDEX themes define a full palette in terminal.colors
        if let Some(map) = term.and_then(|t| t.get("colors")).and_then(|c| c.as_object()) {
            let names = [
                "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
                "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue",
                "brightMagenta", "brightCyan", "brightWhite",
            ];
            for (i, n) in names.iter().enumerate() {
                if let Some(c) = map.get(*n).and_then(|s| s.as_str()).and_then(Color::from_hex) {
                    ansi[i] = c;
                }
            }
        }
        Some(Theme {
            base,
            bg,
            grey,
            term_fg: getc("foreground", base),
            term_bg: getc("background", bg),
            cursor: getc("cursor", base),
            ansi,
        })
    }

    pub fn load() -> Self {
        if let Ok(path) = std::env::var("NACELLE_THEME") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Some(t) = Self::from_edex_json(&text) {
                    return t;
                }
                eprintln!("nacelle-desktop: failed to parse theme {path}, using 'tron'");
            }
        }
        Self::tron()
    }
}

/// Standard xterm palette.
fn default_ansi() -> [Color; 16] {
    [
        Color::rgb8(0, 0, 0),
        Color::rgb8(205, 49, 49),
        Color::rgb8(13, 188, 121),
        Color::rgb8(229, 229, 16),
        Color::rgb8(36, 114, 200),
        Color::rgb8(188, 63, 188),
        Color::rgb8(17, 168, 205),
        Color::rgb8(229, 229, 229),
        Color::rgb8(102, 102, 102),
        Color::rgb8(241, 76, 76),
        Color::rgb8(35, 209, 139),
        Color::rgb8(245, 245, 67),
        Color::rgb8(59, 142, 234),
        Color::rgb8(214, 112, 214),
        Color::rgb8(41, 184, 219),
        Color::rgb8(255, 255, 255),
    ]
}

/// Color from the 256-color xterm palette index.
pub fn xterm_256(idx: u8, ansi: &[Color; 16]) -> Color {
    match idx {
        0..=15 => ansi[idx as usize],
        16..=231 => {
            let i = idx as u32 - 16;
            let steps = [0u8, 95, 135, 175, 215, 255];
            let r = steps[(i / 36) as usize];
            let g = steps[((i / 6) % 6) as usize];
            let b = steps[(i % 6) as usize];
            Color::rgb8(r, g, b)
        }
        232..=255 => {
            let v = 8 + (idx as u32 - 232) * 10;
            Color::rgb8(v as u8, v as u8, v as u8)
        }
    }
}

#[cfg(test)]
mod hex_tests {
    use super::Color;
    #[test]
    fn from_hex_rejects_multibyte_without_panic() {
        // 6 BYTES but 2 chars — must not panic on a mid-char slice.
        assert!(Color::from_hex("#\u{20ac}\u{20ac}").is_none());
        assert!(Color::from_hex("\u{20ac}\u{20ac}").is_none());
        // Valid still works; too-short/invalid still None.
        assert!(Color::from_hex("#aabbcc").is_some());
        assert!(Color::from_hex("#zzzzzz").is_none());
        assert!(Color::from_hex("#fff").is_none());
    }
}
