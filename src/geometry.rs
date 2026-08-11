//! Where the interactive widgets put their controls.
//!
//! The application decides hover before it asks any widget anything: the
//! pointer has to become a hand over a button the same frame it arrives,
//! and the widget that owns the button has not been called yet. So the
//! few rectangles both sides need live here, in the toolkit both sides
//! already depend on, rather than in either of them.
//!
//! Every function derives its result from the WINDOW height, so a widget
//! computing the same rectangle inside a plugin — which is what the
//! shipped ones do — lands on identical pixels without having to be
//! asked. Removing that duplication means giving the plugin interface a
//! way to declare its hover regions, which is a change of its own.

use crate::Rect;

/// Control panel: the two stacked buttons.
pub mod control {
    use super::*;

    /// The buttons are sized against the window rather than the panel,
    /// so the controls stay the same size wherever the panel is put.
    pub fn button_rects(r: Rect, window_h: f32) -> [Rect; 2] {
        let h = window_h * 0.045;
        let gap = h * 0.35;
        let w = r.w * 0.86;
        let x = r.x + (r.w - w) / 2.0;
        let bottom = r.y + r.h - gap;
        [
            Rect::new(x, bottom - h * 2.0 - gap, w, h),
            Rect::new(x, bottom - h, w, h),
        ]
    }
}

/// Terminal: the session tab strip.
pub mod shell {
    use super::*;

    /// How many sessions the strip is divided into. The application
    /// sizes its session array from this.
    pub const TAB_COUNT: usize = 5;

    pub fn tab_rects(r: Rect, window_h: f32) -> [Rect; TAB_COUNT] {
        let pad = window_h / 100.0 * 0.74;
        let tab_h = window_h / 100.0 * 2.6;
        let tabs = Rect::new(r.x + pad, r.y + pad, r.w - 2.0 * pad, tab_h);
        let tw = tabs.w / TAB_COUNT as f32;
        std::array::from_fn(|i| Rect::new(tabs.x + tw * i as f32, tabs.y, tw, tabs.h))
    }
}
