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

/// A declared non-final class without `__toString` may be a subclass that has one, so its
/// `argument.type` is at level 9 and the default report (level 6) leaves it unmade. A final
/// class without one is a certain error at the default level.
#[test]
fn a_plain_class_argument_is_reported_only_at_level_nine() {
    let plain = "<?php\nclass WeakPlain {}\nfunction f(WeakPlain $p): int { return strlen($p); }\n";
    let sealed =
        "<?php\nfinal class WeakSealed {}\nfunction f(WeakSealed $s): int { return strlen($s); }\n";
    let argument_types = |source: &str, level: u8| -> Vec<u8> {
        let cutoff = LevelCutoff::at(level);
        all_diagnostics_for_with(source, |s| s.report_cutoff = Some(cutoff))
            .iter()
            .filter(|d| d.identifier.as_str() == "argument.type")
            .map(|d| d.level)
            .collect()
    };
    assert!(argument_types(plain, 6).is_empty());
    assert_eq!(argument_types(plain, 9), vec![9]);
    assert_eq!(argument_types(sealed, 6), vec![5]);
}
