//! Fuzzy ranking shared by the TUI and by "did you mean" suggestions.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// Returns indices of `haystacks` matching `query`, best first. Empty query keeps order.
pub fn rank<S: AsRef<str>>(query: &str, haystacks: &[S]) -> Vec<usize> {
    if query.trim().is_empty() {
        return (0..haystacks.len()).collect();
    }
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, usize)> = haystacks
        .iter()
        .enumerate()
        .filter_map(|(i, h)| {
            pattern
                .score(Utf32Str::new(h.as_ref(), &mut buf), &mut matcher)
                .map(|s| (s, i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

/// Character positions of `haystack` matched by `query`, sorted and unique (for highlighting).
pub fn indices(query: &str, haystack: &str) -> Vec<usize> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut buf = Vec::new();
    let mut out: Vec<u32> = Vec::new();
    pattern.indices(Utf32Str::new(haystack, &mut buf), &mut matcher, &mut out);
    out.sort_unstable();
    out.dedup();
    out.into_iter().map(|i| i as usize).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn indices_point_at_matched_chars() {
        let h = "apps/web dev npm";
        let i = super::indices("web dv", h);
        let chars: Vec<char> = h.chars().collect();
        let picked: String = i.iter().map(|&k| chars[k]).collect();
        assert_eq!(picked, "webdv");
        assert!(super::indices("", h).is_empty());
    }

    #[test]
    fn multi_word_query_matches_project_and_task() {
        let items = ["apps/web dev", "apps/web build", "services/api test"];
        let r = super::rank("api te", &items);
        assert_eq!(r.first(), Some(&2));
    }
}
