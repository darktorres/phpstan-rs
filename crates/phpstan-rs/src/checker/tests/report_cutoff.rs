//! A checker told the cutoff of the report leaves unmade exactly the
//! findings the cutoff would cut.
use super::helpers::all_diagnostics_for_with;
use diagnostics::LevelCutoff;

const SOURCES: &[&str] = &[
    "<?php\nfunction f(int $a): string { return $a; }\nf('x');\n$u = $undefined + 1;\nclass A { function m() { return $this->missing; } }\n",
    "<?php\nfunction g($a, $a) {}\nfunction h(array $x): int { return $x['k'] ?? null; }\nh(1, 2);\n",
    "<?php\n/** @param int $x */\nfunction i($x) { return strlen($x); }\ni(null);\nnew Nope();\n",
];

#[test]
fn a_checker_given_the_cutoff_makes_what_the_cutoff_keeps() {
    for source in SOURCES {
        let all = all_diagnostics_for_with(source, |_| {});
        assert!(
            all.iter().any(|d| d.level > 0),
            "the source has a finding above level 0: {source}"
        );
        for level in [0, 1, 3, 6, 9] {
            let cutoff = LevelCutoff::at(level);
            let expected: Vec<_> = all
                .iter()
                .filter(|d| cutoff.reports(d))
                .map(|d| (d.message.clone(), d.span, d.level))
                .collect();
            let made: Vec<_> = all_diagnostics_for_with(source, |s| s.report_cutoff = Some(cutoff))
                .iter()
                .filter(|d| cutoff.reports(d))
                .map(|d| (d.message.clone(), d.span, d.level))
                .collect();
            assert_eq!(made, expected, "level {level}: {source}");
            let unmade = all_diagnostics_for_with(source, |s| s.report_cutoff = Some(cutoff));
            assert!(
                unmade.iter().all(|d| cutoff.reports(d)),
                "level {level} made a finding the cutoff cuts: {source}"
            );
        }
    }
}
