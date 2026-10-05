use super::DebounceQueue;
use lsp_types::Uri;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{Duration, Instant};

fn uri() -> Uri {
    Uri::from_str("file:///a.php").unwrap()
}

#[test]
fn two_edits_before_the_deadline_coalesce_into_one_entry_with_the_newer_text_and_a_bumped_generation()
 {
    let mut q = DebounceQueue::new(Duration::from_millis(200));
    let now = Instant::now();
    let path = PathBuf::from("a.php");

    let g1 = q.record_edit(path.clone(), uri(), "first".to_string(), 1, now);
    let g2 = q.record_edit(
        path.clone(),
        uri(),
        "second".to_string(),
        2,
        now + Duration::from_millis(50),
    );
    assert_eq!(g1, 1);
    assert_eq!(g2, 2);

    let drained = q.drain_expired(now + Duration::from_millis(300));
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].1.text, "second");
    assert_eq!(drained[0].1.generation, 2);
}

#[test]
fn next_deadline_reflects_the_earliest_of_several_independent_paths() {
    let mut q = DebounceQueue::new(Duration::from_millis(200));
    let now = Instant::now();
    q.record_edit(PathBuf::from("a.php"), uri(), "a".to_string(), 1, now);
    q.record_edit(
        PathBuf::from("b.php"),
        uri(),
        "b".to_string(),
        1,
        now + Duration::from_millis(10),
    );

    assert_eq!(q.next_deadline(), Some(now + Duration::from_millis(200)));

    // An edit to an unrelated third path doesn't move the earliest
    // deadline backward.
    q.record_edit(
        PathBuf::from("c.php"),
        uri(),
        "c".to_string(),
        1,
        now + Duration::from_millis(500),
    );
    assert_eq!(q.next_deadline(), Some(now + Duration::from_millis(200)));
}

#[test]
fn drain_expired_fires_nothing_before_the_deadline_and_exactly_the_entry_once_it_passes() {
    let mut q = DebounceQueue::new(Duration::from_millis(200));
    let now = Instant::now();
    q.record_edit(PathBuf::from("a.php"), uri(), "a".to_string(), 1, now);

    assert!(q.drain_expired(now + Duration::from_millis(100)).is_empty());
    let drained = q.drain_expired(now + Duration::from_millis(200));
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].0, PathBuf::from("a.php"));
}

#[test]
fn current_generation_survives_a_drain_and_a_later_edit_bumps_it_further() {
    let mut q = DebounceQueue::new(Duration::from_millis(200));
    let now = Instant::now();
    let path = PathBuf::from("a.php");
    q.record_edit(path.clone(), uri(), "a".to_string(), 1, now);
    q.drain_expired(now + Duration::from_millis(200));

    assert_eq!(q.current_generation(&path), 1);

    q.record_edit(
        path.clone(),
        uri(),
        "b".to_string(),
        2,
        now + Duration::from_millis(300),
    );
    assert_eq!(q.current_generation(&path), 2);
}

#[test]
fn cancel_removes_a_pending_entry_and_zeroes_its_generation() {
    let mut q = DebounceQueue::new(Duration::from_millis(200));
    let now = Instant::now();
    let path = PathBuf::from("a.php");
    q.record_edit(path.clone(), uri(), "a".to_string(), 1, now);

    q.cancel(&path);

    assert!(q.drain_expired(now + Duration::from_millis(200)).is_empty());
    assert!(q.drain_all().is_empty());
    assert_eq!(
        q.current_generation(&path),
        0,
        "a captured generation >= 1 must no longer match after cancel - the mechanism didClose needs"
    );
}

#[test]
fn drain_all_returns_an_unexpired_entry_that_drain_expired_would_not_have() {
    let mut q = DebounceQueue::new(Duration::from_millis(200));
    let now = Instant::now();
    q.record_edit(PathBuf::from("a.php"), uri(), "a".to_string(), 1, now);

    assert!(q.drain_expired(now).is_empty());
    let drained = q.drain_all();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].1.text, "a");
}

#[test]
fn the_whole_program_checks_wait_for_the_edits_pending_and_the_window_after_a_save() {
    let mut q = DebounceQueue::new(Duration::from_millis(200));
    let now = Instant::now();
    assert!(!q.take_whole_program(now));

    q.request_whole_program(now);
    assert_eq!(q.next_deadline(), Some(now + Duration::from_millis(200)));
    assert!(!q.take_whole_program(now + Duration::from_millis(100)));

    // An edit pending holds them back, and its deadline is the one waited
    // for.
    q.record_edit(
        PathBuf::from("a.php"),
        uri(),
        "a".to_string(),
        1,
        now + Duration::from_millis(150),
    );
    assert_eq!(q.next_deadline(), Some(now + Duration::from_millis(350)));
    assert!(!q.take_whole_program(now + Duration::from_millis(300)));

    q.drain_expired(now + Duration::from_millis(400));
    assert!(q.take_whole_program(now + Duration::from_millis(400)));
    assert!(!q.take_whole_program(now + Duration::from_millis(500)));
    assert_eq!(q.next_deadline(), None);
}

#[test]
fn a_flush_takes_the_whole_program_checks_requested_whatever_their_deadline() {
    let mut q = DebounceQueue::new(Duration::from_millis(200));
    assert!(!q.take_whole_program_now());
    q.request_whole_program(Instant::now());
    assert!(q.take_whole_program_now());
    assert!(!q.take_whole_program_now());
}
