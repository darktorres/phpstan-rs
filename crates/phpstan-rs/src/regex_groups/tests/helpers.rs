use super::super::*;

pub(super) fn numbers_and_names(groups: &[RegexCaptureGroup]) -> Vec<(usize, Option<&str>, bool)> {
    groups
        .iter()
        .map(|g| (g.number, g.name.as_deref(), g.optional))
        .collect()
}

pub(super) fn literals(groups: &[RegexCaptureGroup]) -> Vec<Option<Vec<String>>> {
    groups.iter().map(|g| g.literal_values.clone()).collect()
}

pub(super) fn top_levels(groups: &[RegexCaptureGroup]) -> Vec<bool> {
    groups.iter().map(|g| g.top_level).collect()
}

// --- Tri-state fallback ---

pub(super) fn fallbacks(groups: &[RegexCaptureGroup]) -> Vec<RegexGroupFallback> {
    groups.iter().map(|g| g.fallback).collect()
}
