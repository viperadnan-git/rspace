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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, size: i64, is_dir: bool) -> Entry {
        Entry { name: name.into(), path: name.into(), size, mod_time: String::new(), is_dir }
    }

    #[test]
    fn human_size_scales_by_unit() {
        assert_eq!(human_size(-1), "—");
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn human_duration_ranges() {
        assert_eq!(human_duration(500), "500ms");
        assert_eq!(human_duration(1500), "1.5s");
        assert_eq!(human_duration(65_000), "1m05s");
    }

    #[test]
    fn parent_of_strips_last_segment() {
        assert_eq!(parent_of("a/b/c"), "a/b");
        assert_eq!(parent_of("a"), "");
        assert_eq!(parent_of(""), "");
    }

    #[test]
    fn file_kind_from_extension() {
        assert_eq!(file_kind("foo.RS"), "RS");
        assert_eq!(file_kind("foo.tar.gz"), "GZ");
        assert_eq!(file_kind("README"), "File");
        assert_eq!(file_kind("trailing."), "File");
    }

    #[test]
    fn human_date_formats_rfc3339() {
        assert_eq!(human_date("2026-06-19T10:25:37Z"), "Jun 19, 2026  10:25");
        assert_eq!(human_date("short"), "");
    }

    #[test]
    fn sort_entries_dirs_first_then_field() {
        let mut v = vec![entry("b.txt", 10, false), entry("dir", 0, true), entry("a.txt", 30, false)];
        sort_entries(&mut v, SortField::Name, SortOrder::Asc);
        assert_eq!(v.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["dir", "a.txt", "b.txt"]);
        sort_entries(&mut v, SortField::Size, SortOrder::Desc);
        assert_eq!(v.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["dir", "a.txt", "b.txt"]);
    }

    #[test]
    fn sort_arrow_glyphs() {
        assert_eq!(sort_arrow(SortOrder::Asc), "↑");
        assert_eq!(sort_arrow(SortOrder::Desc), "↓");
    }
}
