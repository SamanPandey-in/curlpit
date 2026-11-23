//! Tiny, dependency-free fuzzy subsequence matcher for the command palette.
//!
//! Case-insensitive (ASCII). A higher score means a better, more
//! human-plausible match. Word-boundary "initials", prefix matches and
//! contiguous runs are rewarded; gaps and long unmatched tails are mildly
//! penalized.

/// Base reward for every matched character.
const MATCH: i32 = 1;
/// Extra reward when the first matched character is the very start of the candidate.
const START: i32 = 5;
/// Reward for a match at a word boundary (start, after a separator, or lower->Upper).
const BOUNDARY: i32 = 10;
/// Reward for a match that is contiguous with the previous match.
const CONTIG: i32 = 15;
/// Penalty per skipped character in a gap between two matches.
const GAP: i32 = 1;
/// Penalty per unmatched trailing character after the last match.
const TRAILING: i32 = 1;

/// Returns true if position `i` in `cand` starts a new "word": the very start,
/// a character right after a separator, or a lowercase->uppercase transition.
fn is_boundary(cand: &[char], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let prev = cand[i - 1];
    let cur = cand[i];
    matches!(prev, ' ' | '-' | '_' | '/') || (prev.is_ascii_lowercase() && cur.is_ascii_uppercase())
}

/// Case-insensitive subsequence match. Returns None if `query`'s characters are not
/// a subsequence of `candidate`; otherwise Some(score) where a HIGHER score is a better
/// match. An empty query returns Some(0) (matches everything).
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }

    let q: Vec<char> = query.chars().map(|c| c.to_ascii_lowercase()).collect();
    let cand: Vec<char> = candidate.chars().collect();

    let mut qi = 0usize;
    let mut score: i32 = 0;
    let mut prev_match: Option<usize> = None;
    let mut last_match = 0usize;

    for (i, &ch) in cand.iter().enumerate() {
        if qi >= q.len() {
            break;
        }
        if ch.to_ascii_lowercase() != q[qi] {
            continue;
        }

        score += MATCH;
        let boundary = is_boundary(&cand, i);
        match prev_match {
            None => {
                if i == 0 {
                    score += START;
                }
                if boundary {
                    score += BOUNDARY;
                }
            }
            Some(p) => {
                if i == p + 1 {
                    score += CONTIG;
                } else {
                    let gap = (i - p - 1) as i32;
                    score -= gap * GAP;
                    if boundary {
                        score += BOUNDARY;
                    }
                }
            }
        }

        prev_match = Some(i);
        last_match = i;
        qi += 1;
    }

    if qi < q.len() {
        return None;
    }

    let trailing = (cand.len() - 1 - last_match) as i32;
    score -= trailing * TRAILING;

    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(fuzzy_score("", "New Request"), Some(0));
        assert_eq!(fuzzy_score("", ""), Some(0));
    }

    #[test]
    fn non_subsequence_is_none() {
        assert_eq!(fuzzy_score("xyz", "New Request"), None);
    }

    #[test]
    fn case_insensitive() {
        assert!(fuzzy_score("NR", "New Request").is_some());
        assert!(fuzzy_score("nr", "new request").is_some());
    }

    #[test]
    fn word_boundary_initials_beat_scattered_midword() {
        let initials = fuzzy_score("nr", "New Request");
        let scattered = fuzzy_score("nr", "Entry Number");
        assert!(initials.is_some());
        assert!(scattered.is_some());
        assert!(initials > scattered);
    }

    #[test]
    fn contiguous_beats_scattered() {
        let contiguous = fuzzy_score("req", "Send Request");
        let scattered = fuzzy_score("req", "Rename Quickly Equals");
        assert!(contiguous.is_some());
        assert!(scattered.is_some());
        assert!(contiguous > scattered);
    }

    #[test]
    fn prefix_beats_later_match() {
        let prefix = fuzzy_score("se", "Send");
        let later = fuzzy_score("se", "Unselect");
        assert!(prefix.is_some());
        assert!(later.is_some());
        assert!(prefix > later);
    }

    #[test]
    fn exact_full_match_is_at_least_any_partial() {
        let full = fuzzy_score("send", "Send").unwrap();
        let partial = fuzzy_score("se", "Send").unwrap();
        assert!(full >= partial);
    }
}
