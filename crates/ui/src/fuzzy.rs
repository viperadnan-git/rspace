//! Tiny subsequence fuzzy matcher for command/remote/path lists — no async, no
//! deps. Case-insensitive (ASCII fold); rewards word-boundary and consecutive
//! matches, and shorter / earlier matches. Good enough for a few hundred items.

/// A scored match. `positions` are char indices into the candidate that matched,
/// for highlighting. Higher `score` is a better match.
pub struct Match {
    pub score: i32,
    pub positions: Vec<usize>,
}

/// Match `query` against `text` (ASCII case-insensitive). Returns `None` unless
/// every query char appears in order. An empty query matches everything (score 0).
pub fn fuzzy_match(query: &str, text: &str) -> Option<Match> {
    if query.is_empty() {
        return Some(Match { score: 0, positions: Vec::new() });
    }
    let q: Vec<char> = query.chars().map(|c| c.to_ascii_lowercase()).collect();
    let chars: Vec<char> = text.chars().collect();
    let mut qi = 0;
    let mut positions = Vec::with_capacity(q.len());
    let mut score = 0;
    for (i, &ch) in chars.iter().enumerate() {
        if qi >= q.len() {
            break;
        }
        if ch.to_ascii_lowercase() == q[qi] {
            if i == 0 || !chars[i - 1].is_alphanumeric() {
                score += 8; // start of a word
            }
            if positions.last() == Some(&i.wrapping_sub(1)) {
                score += 6; // consecutive run
            }
            score += 1;
            positions.push(i);
            qi += 1;
        }
    }
    if qi != q.len() {
        return None;
    }
    // Prefer shorter candidates and earlier first matches.
    score -= chars.len() as i32 / 8;
    score -= *positions.first().unwrap_or(&0) as i32 / 4;
    Some(Match { score, positions })
}

#[cfg(test)]
mod tests {
    use super::fuzzy_match;

    #[test]
    fn requires_in_order_subsequence() {
        assert!(fuzzy_match("fb", "foobar").is_some());
        assert!(fuzzy_match("bf", "foobar").is_none());
        assert!(fuzzy_match("xyz", "foobar").is_none());
    }

    #[test]
    fn reports_match_positions() {
        let m = fuzzy_match("fb", "foobar").unwrap();
        assert_eq!(m.positions, vec![0, 3]);
    }

    #[test]
    fn empty_query_matches_everything() {
        let m = fuzzy_match("", "foobar").unwrap();
        assert_eq!(m.score, 0);
        assert!(m.positions.is_empty());
    }

    #[test]
    fn case_insensitive() {
        assert!(fuzzy_match("FOO", "foobar").is_some());
    }
}
