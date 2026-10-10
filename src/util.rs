//! Small helpers shared by the screens.

/// Bytes as the panels show them: `512 B`, `3.4 KiB`, `15.2 GiB`.
pub fn fmt_bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["KiB", "MiB", "GiB", "TiB", "PiB"];
    if b < 1024 {
        return format!("{b} B");
    }
    let mut v = b as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    if v >= 100.0 { format!("{v:.0} {}", UNITS[unit]) } else { format!("{v:.1} {}", UNITS[unit]) }
}

/// A duration in seconds as `3d 4h`, `4h 12m`, `12m 5s`.
pub fn fmt_uptime(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m {s}s")
    }
}

/// A clock time from seconds since the epoch, in UTC: `2026-10-10 09:15`.
/// The process start in the detail column; good enough without a time zone
/// crate, and labelled UTC where it is shown.
pub fn fmt_epoch(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // Howard Hinnant's days-to-civil.
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem / 60 % 60)
}

/// How many monospace cells `c` takes: two for the wide characters of
/// East Asian scripts (kana, kanji, hangul, full-width forms), else one. The
/// CJK font behind the monospace one draws them about twice as wide, so a
/// Japanese label is cut where it really ends.
pub fn cells(c: char) -> usize {
    let wide = matches!(c as u32,
        0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x20000..=0x3FFFD);
    if wide { 2 } else { 1 }
}

/// The cells `s` takes on screen (see [`cells`]).
pub fn width(s: &str) -> usize {
    s.chars().map(cells).sum()
}

/// A fresh directory of its own under the system's temp directory, so tests
/// never read the machine's real configuration.
#[cfg(test)]
pub fn test_dir(label: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("mimamori-test-{}-{label}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create a test directory");
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes_and_durations() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(3 * 1024 + 400), "3.4 KiB");
        assert_eq!(fmt_bytes(150 * 1024 * 1024), "150 MiB");
        assert_eq!(fmt_uptime(3 * 86400 + 4 * 3600 + 5), "3d 4h");
        assert_eq!(fmt_uptime(4 * 3600 + 12 * 60), "4h 12m");
        assert_eq!(fmt_uptime(725), "12m 5s");
    }

    #[test]
    fn wide_characters_take_two_cells() {
        assert_eq!(width("cpu"), 3);
        assert_eq!(width("メモリ"), 6);
        assert_eq!(width("CPU 使用率"), 10);
        assert_eq!(width("ＡＢ"), 4);
        assert_eq!(width("─·…"), 3);
    }

    #[test]
    fn formats_an_epoch_in_utc() {
        assert_eq!(fmt_epoch(0), "1970-01-01 00:00");
        assert_eq!(fmt_epoch(1_791_623_700), "2026-10-10 09:15");
    }
}
