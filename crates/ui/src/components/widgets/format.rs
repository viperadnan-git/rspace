//! Pure data → string/sort helpers (no gpui), shared across the views.

use rspace_core::{SortField, SortOrder};
use rspace_rclone_rc::Entry;

// single source in rclone_rc::ops so path-joining can't diverge between crates
pub use rspace_rclone_rc::join as join_path;

pub fn sort_arrow(order: SortOrder) -> &'static str {
    match order {
        SortOrder::Asc => "↑",
        SortOrder::Desc => "↓",
    }
}

pub fn sort_entries(entries: &mut [Entry], field: SortField, order: SortOrder) {
    entries.sort_by(|a, b| {
        let within = match field {
            SortField::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortField::Size => a.size.cmp(&b.size),
            SortField::Modified => a.mod_time.cmp(&b.mod_time),
        };
        let within = match order {
            SortOrder::Asc => within,
            SortOrder::Desc => within.reverse(),
        };
        b.is_dir.cmp(&a.is_dir).then(within)
    });
}

pub fn rclone_cmd(verb: &str, args: &[&str]) -> String {
    let mut s = format!("rclone {verb}");
    for a in args {
        s.push_str(&format!(" \"{a}\""));
    }
    s
}

pub fn human_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let s = ms / 1000;
        format!("{}m{:02}s", s / 60, s % 60)
    }
}

pub fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// Best-effort `Mon D, YYYY  HH:MM` from rclone's RFC3339 mod time (UTC).
pub fn human_date(rfc3339: &str) -> String {
    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    if rfc3339.len() < 16 {
        return String::new();
    }
    let (date, time) = (&rfc3339[..10], &rfc3339[11..16]);
    let p: Vec<&str> = date.split('-').collect();
    let (Some(y), Some(m), Some(d)) = (p.first(), p.get(1).and_then(|s| s.parse::<usize>().ok()), p.get(2))
    else {
        return String::new();
    };
    let mon = MONTHS.get(m.wrapping_sub(1)).copied().unwrap_or("");
    format!("{mon} {}, {y}  {time}", d.trim_start_matches('0'))
}

pub fn file_kind(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => ext.to_ascii_uppercase(),
        _ => "File".to_string(),
    }
}

pub fn human_size(bytes: i64) -> String {
    if bytes < 0 {
        return "—".to_string();
    }
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}
