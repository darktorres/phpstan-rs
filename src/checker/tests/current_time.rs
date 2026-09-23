//! Implicit current-time dependence.

use super::helpers::*;

#[test]
fn implicit_clock_reads() {
    let src = r#"<?php
function f(int $ts, string $when): void {
    time();
    microtime(true);
    date('Y-m-d');
    date('Y-m-d', $ts);
    date('Y-m-d', null);
    getdate();
    mktime(0, 0, 0, 1, 1, 2024);
    mktime(0);
    strtotime('+1 day');
    strtotime('+1 day', $ts);
    strtotime('2024-01-01');
    strtotime($when);
    new \DateTimeImmutable();
    new \DateTimeImmutable('next monday');
    new \DateTimeImmutable('2024-01-01');
    new \DateTime('10:30');
    date_create();
    \DateTimeImmutable::createFromFormat('Y-m-d', '2024-01-01');
    \DateTimeImmutable::createFromFormat('!Y-m-d', '2024-01-01');
    \DateTimeImmutable::createFromFormat('U', '1700000000');
    $_SERVER['REQUEST_TIME'];
    new \ArrayObject();
}
"#;
    let mut found: Vec<String> =
        diagnostics_for_with(src, |r| r.check_current_time_dependence = true)
            .into_iter()
            .filter(|d| d.identifier.starts_with("currentTime."))
            .map(|d| format!("{} {}", d.span.start.line, d.identifier))
            .collect();
    found.sort_by_key(|l| l.split(' ').next().unwrap().parse::<u32>().unwrap());
    assert_eq!(
        found,
        vec![
            "3 currentTime.time",
            "4 currentTime.time",
            "5 currentTime.defaultTimestamp",
            "7 currentTime.defaultTimestamp",
            "8 currentTime.defaultTimestamp",
            "10 currentTime.defaultTimestamp",
            "11 currentTime.relative",
            "15 currentTime.relative",
            "16 currentTime.relative",
            "18 currentTime.relative",
            "19 currentTime.relative",
            "20 currentTime.createFromFormat",
            "23 currentTime.requestTime",
        ]
    );
}
