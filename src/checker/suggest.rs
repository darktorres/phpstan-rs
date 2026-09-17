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
/// to be worth suggesting. Ties are broken by the candidate's own
/// lexicographic order, not by which one `candidates` happens to yield
/// first - candidates are commonly collected from a `HashMap`'s own
/// `.keys()` (e.g. `Registry::all_method_names`), whose iteration order
/// depends on insertion order, which can itself vary run to run when the
/// map is built while `check-project` processes files in parallel via
/// rayon. Without this, a real tie (confirmed: `isOneToMany`/`isToOne`
/// against the same undefined-method target, in Piwigo's own vendored
/// Doctrine ORM stubs) makes this diagnostic's own suggested tip
/// nondeterministic across otherwise-identical runs.
pub(crate) fn suggest_similar_name<'a>(
    target: &str,
    candidates: impl Iterator<Item = &'a str>,
) -> Option<&'a str> {
    let target_lower = target.to_ascii_lowercase();
    candidates
        .filter(|c| !c.is_empty())
        .map(|c| (c, levenshtein(&target_lower, &c.to_ascii_lowercase())))
        .filter(|(c, dist)| *dist <= MAX_DISTANCE && !c.eq_ignore_ascii_case(target))
        .min_by_key(|(c, dist)| (*dist, *c))
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
    fn ties_break_by_lexicographic_order_regardless_of_iteration_order() {
        // Both "axx" and "bxx" are Levenshtein distance 1 from "xxx" - a
        // genuine tie. The result must be the same regardless of which
        // order the candidate iterator happens to yield them in (real
        // candidates commonly come from a HashMap's own `.keys()`, whose
        // order isn't guaranteed to be stable across runs).
        assert_eq!(
            suggest_similar_name("xxx", ["axx", "bxx"].into_iter()),
            Some("axx")
        );
        assert_eq!(
            suggest_similar_name("xxx", ["bxx", "axx"].into_iter()),
            Some("axx")
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
