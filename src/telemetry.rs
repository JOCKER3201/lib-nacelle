//! Telemetry data model shared by every platform: the Snapshot the
//! widgets render, and the byte/rate/uptime formatting helpers.
//! Each platform fills the Snapshot with its own collectors.

#[derive(Clone)]
pub struct ProcEntry {
    pub pid: u32,
    pub name: String,
    pub cpu: f32,
    pub mem_pct: f32,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    /// Counts the rewrites. A collector bumps it whenever it replaces
    /// the snapshot, which is what lets a reader tell "the same data
    /// again" from "new data" without comparing any of it — the
    /// scripting layer builds its expensive views once per rewrite
    /// rather than once per frame.
    pub generation: u64,
    pub cpu_name: String,
    pub cpu_per_core: Vec<f32>,
    pub load_avg: [f64; 3],
    pub temp_c: Option<f32>,
    pub mem_total: u64,
    pub mem_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub uptime: u64,
    pub top: Vec<ProcEntry>,
    pub proc_count: usize,
    pub net_up_rate: f64,
    pub net_down_rate: f64,
    pub iface: String,
    pub ipv4: Option<String>,
    pub ping_ms: Option<u32>,
    pub online: bool,
    pub battery: Option<(u8, bool)>,
    pub manufacturer: String,
    pub model: String,
    pub chassis: String,
    pub hostname: String,
    pub username: String,
    pub os_name: String,
    pub kernel: String,
}

/// Byte formatting like eDEX (GiB/MiB/KiB), written down under the
/// theme's number policy.
///
/// The MAGNITUDE ladder is arithmetic and stays here: which power of 1024
/// a count falls in is a fact about the count. Everything else was a look
/// decided in this file — `{:.2}` and `{:.1}` and `{:.0}` chose the
/// precision, the `.` chose the decimal mark, the literal space chose the
/// joint, and none of the three could be moved by a theme. §5.17 offers
/// two precisions and this maps the ladder onto them the way the ladder
/// itself was reaching for: the large units carry a fraction
/// (`num.decimals`), the small ones are already whole at their own scale
/// (`num.decimals_compact`).
pub fn fmt_bytes(b: u64) -> String {
    const G: f64 = 1024.0 * 1024.0 * 1024.0;
    const M: f64 = 1024.0 * 1024.0;
    const K: f64 = 1024.0;
    let b = b as f64;
    if b >= G {
        crate::num::Reading::new(b / G, "GiB").text()
    } else if b >= M {
        crate::num::Reading::new(b / M, "MiB").text()
    } else if b >= K {
        crate::num::Reading::compact(b / K, "KiB").text()
    } else {
        crate::num::Reading::compact(b, "B").text()
    }
}

/// A throughput, under the same policy and the same split.
pub fn fmt_rate(b: f64) -> String {
    const M: f64 = 1024.0 * 1024.0;
    const K: f64 = 1024.0;
    if b >= M {
        crate::num::Reading::new(b / M, "MB/s").text()
    } else if b >= K {
        crate::num::Reading::new(b / K, "kB/s").text()
    } else {
        crate::num::Reading::compact(b, "B/s").text()
    }
}

pub fn fmt_uptime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}
