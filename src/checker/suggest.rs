//! "Did you mean?" name suggestions for undefined-symbol diagnostics.
//! Plain Levenshtein edit distance, capped by `MAX_DISTANCE` so an
//! unrelated name never gets suggested just because it happens to be
//! the least-bad option among equally-wrong candidates.

const MAX_DISTANCE: usize = 3;

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// The closest candidate to `target` under case-insensitive Levenshtein
/// distance, within `MAX_DISTANCE`, or `None` if nothing is close enough
/// to be worth suggesting. Ties keep whichever candidate the iterator
/// yields first.
pub(crate) fn suggest_similar_name<'a>(
    target: &str,
    candidates: impl Iterator<Item = &'a str>,
) -> Option<&'a str> {
    let target_lower = target.to_ascii_lowercase();
    candidates
        .filter(|c| !c.is_empty())
        .map(|c| (c, levenshtein(&target_lower, &c.to_ascii_lowercase())))
        .filter(|(c, dist)| *dist <= MAX_DISTANCE && !c.eq_ignore_ascii_case(target))
        .min_by_key(|(_, dist)| *dist)
        .map(|(c, _)| c)
}

#[cfg(test)]
mod tests {
    use super::suggest_similar_name;

    #[test]
    fn suggests_a_close_typo() {
        let candidates = ["getName", "setName", "toString"];
        assert_eq!(
            suggest_similar_name("getNam", candidates.into_iter()),
            Some("getName")
        );
    }

    #[test]
    fn declines_when_nothing_is_close() {
        let candidates = ["getName", "setName", "toString"];
        assert_eq!(
            suggest_similar_name("frobnicate", candidates.into_iter()),
            None
        );
    }

    #[test]
    fn ignores_an_exact_case_insensitive_match() {
        let candidates = ["getname"];
        assert_eq!(
            suggest_similar_name("getName", candidates.into_iter()),
            None
        );
    }
}
