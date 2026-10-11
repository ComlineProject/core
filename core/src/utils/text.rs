//! Small string helpers shared by diagnostics that want to suggest what
//! the author probably meant.
//!
//! These lived in `schema::ir::validation::validator`, which owned them
//! only because it was the first module to need one. They are generic
//! string utilities with callers in validation, import resolution and
//! settings-key checking, so they belong here rather than behind a
//! `pub(crate)` re-export from a validation pass.

/// Closest candidate to `target` within a "plausibly a typo" edit distance
/// (half the target's length, min 1) — `None` if nothing is close.
pub fn closest<'a>(target: &str, candidates: impl Iterator<Item = &'a str>) -> Option<String> {
    let len = target.chars().count();
    let max_distance = ((len + 1) / 2).max(1);
    candidates
        .filter(|&c| c != target)
        .map(|c| (c, levenshtein_distance(target, c)))
        .filter(|&(_, d)| d <= max_distance)
        .min_by_key(|&(_, d)| d)
        .map(|(c, _)| c.to_string())
}

/// Standard Levenshtein edit distance (single-character insert/delete/
/// substitute) between two strings, used to power "did you mean?"
/// suggestions.
pub fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let b_len = b.len();

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0usize; b_len + 1];

    for (i, &a_ch) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &b_ch) in b.iter().enumerate() {
            let cost = if a_ch == b_ch { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_single_character_typo() {
        assert_eq!(closest("allowed", ["allowed_x", "alowed"].into_iter()), Some("alowed".to_string()));
    }

    #[test]
    fn ignores_an_exact_match() {
        // The caller only asks when the written form already failed, so an
        // identical candidate is never a useful suggestion.
        assert_eq!(closest("allowed", ["allowed"].into_iter()), None);
    }

    #[test]
    fn gives_up_when_nothing_is_plausibly_a_typo() {
        assert_eq!(closest("abc", ["xyzzy_entirely_different"].into_iter()), None);
    }

    #[test]
    fn distance_is_symmetric_and_zero_for_equal_strings() {
        assert_eq!(levenshtein_distance("kitten", "kitten"), 0);
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(levenshtein_distance("sitting", "kitten"), 3);
    }

    #[test]
    fn handles_empty_strings() {
        assert_eq!(levenshtein_distance("", ""), 0);
        assert_eq!(levenshtein_distance("", "abc"), 3);
        assert_eq!(levenshtein_distance("abc", ""), 3);
    }
}
