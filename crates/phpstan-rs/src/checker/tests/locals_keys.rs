//! `Locals`' set of synthetic keys: what a rebound key invalidates is found
//! among them, and is what looking through every local finds.
use super::super::control_flow::merge_scopes;
use super::super::narrowing::{key_depends_on, purge_stale_narrowing_for};
use super::super::*;

/// Keys of every shape `narrowing_key` makes, and plain variables, with
/// names that are prefixes of one another.
const KEYS: &[&str] = &[
    "a",
    "ab",
    "this",
    "a->b",
    "a->b->c",
    "a->bc",
    "ab->c",
    "a[0]",
    "a['k']",
    "a['k']->p",
    "m[$a]",
    "m[$ab]",
    "m[$a]->p",
    "m['x'][$a]",
    "n[$a->b]",
    "Foo::$bar",
    "a[$k][$a]",
    "k",
    "this->items[$k]",
];

fn locals_with(keys: &[&str]) -> Locals {
    let mut locals = Locals::default();
    for key in keys {
        locals.insert((*key).to_string(), Type::int());
    }
    locals
}

fn listed(locals: &Locals) -> Vec<String> {
    let mut keys: Vec<String> = locals.iter().map(|(k, _)| k.clone()).collect();
    keys.sort();
    keys
}

/// What `purge_stale_narrowing_for` removed before it kept the synthetic
/// keys apart: every local that depends on `key`, other than `key`.
fn removed_by_scanning_every_local(locals: &Locals, key: &str) -> Vec<String> {
    let mut gone: Vec<String> = locals
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| k != key && key_depends_on(k, key))
        .collect();
    gone.sort();
    gone
}

#[test]
fn purging_a_key_removes_what_scanning_every_local_finds() {
    for key in KEYS {
        let mut locals = locals_with(KEYS);
        let expected = removed_by_scanning_every_local(&locals, key);
        let before = listed(&locals);
        purge_stale_narrowing_for(&mut locals, key);
        let after = listed(&locals);
        let removed: Vec<String> = before
            .iter()
            .filter(|k| !after.contains(k))
            .cloned()
            .collect();
        assert_eq!(removed, expected, "purging {key}");
    }
}

#[test]
fn the_synthetic_keys_are_the_keys_that_are_not_a_variables_name() {
    let mut locals = locals_with(KEYS);
    let expected = |locals: &Locals| {
        let mut keys: Vec<String> = locals
            .iter()
            .map(|(k, _)| k.clone())
            .filter(|k| !narrowing::is_real_variable_key(k))
            .collect();
        keys.sort();
        keys
    };
    let synthetic = |locals: &Locals| {
        let mut keys: Vec<String> = locals.synthetic_keys().cloned().collect();
        keys.sort();
        keys
    };
    assert_eq!(synthetic(&locals), expected(&locals));
    // Through every way a key is added, removed or copied.
    locals.declare("x->y".to_string(), Type::string(), Span::default());
    locals.insert_with_provenance("z[1]".to_string(), Type::null(), Default::default());
    assert_eq!(synthetic(&locals), expected(&locals));
    locals.remove("a->b");
    locals.remove("a");
    assert_eq!(synthetic(&locals), expected(&locals));
    let copy = locals.clone();
    locals.remove("m[$a]");
    locals.insert("w->v".to_string(), Type::bool());
    assert_eq!(synthetic(&copy), expected(&copy));
    assert_eq!(synthetic(&locals), expected(&locals));
    let fresh = locals.fresh_scope();
    assert!(fresh.synthetic_keys().next().is_none());
}

#[test]
fn a_scope_with_no_synthetic_key_and_no_fact_has_nothing_to_forget() {
    let mut locals = locals_with(&["a", "b", "this"]);
    assert!(!locals.has_state_to_forget());
    assert!(locals.has_key_depending_on("a"));
    assert!(!locals.has_key_depending_on("c"));
    locals.insert("a->p".to_string(), Type::int());
    assert!(locals.has_state_to_forget());
    assert!(locals.has_key_depending_on("a"));
    locals.remove("a->p");
    assert!(!locals.has_state_to_forget());
}

#[test]
fn has_key_depending_on_is_the_scan_of_every_local() {
    let locals = locals_with(KEYS);
    for key in KEYS.iter().chain(&["none", "a->", "m", "Foo::$baz"]) {
        let by_scan = locals.iter().any(|(k, _)| key_depends_on(k, key));
        assert_eq!(locals.has_key_depending_on(key), by_scan, "{key}");
    }
}

/// The same scenario of writes, in a scope that tracks provenance (which
/// joins key by key) or in one that does not (which joins by patching a
/// copy of the then-scope).
fn scenario(tracked: bool, seed: u64) -> (Locals, Locals) {
    let mut state = seed;
    let mut next = move |bound: u64| {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) % bound
    };
    let types = [
        Type::int(),
        Type::string(),
        Type::null(),
        Type::bool(),
        Type::mixed(),
        Type::float(),
    ];
    let names = ["a", "b", "c", "d", "e", "this", "a->p", "b[0]", "c[$a]"];
    let mut base = Locals::default();
    base.set_track(tracked);
    let write = |locals: &mut Locals, n: &mut dyn FnMut(u64) -> u64| {
        let name = names[n(names.len() as u64) as usize].to_string();
        match n(5) {
            0 => {
                locals.remove(&name);
            }
            1 => {
                locals.set_native(&name, types[n(6) as usize].clone());
            }
            2 => locals.clear_native(&name),
            _ => {
                locals.insert(name, types[n(6) as usize].clone());
            }
        }
    };
    for _ in 0..next(8) {
        write(&mut base, &mut next);
    }
    let (mut then, mut other) = (base.clone(), base);
    for _ in 0..next(6) {
        write(&mut then, &mut next);
    }
    for _ in 0..next(6) {
        write(&mut other, &mut next);
    }
    (then, other)
}

#[test]
fn the_join_that_patches_a_copy_is_the_join_key_by_key() {
    let relation = types::Relation::structural();
    let relation = &relation;
    for seed in 0..3000u64 {
        let (then_p, else_p) = scenario(false, seed);
        let (then_t, else_t) = scenario(true, seed);
        let patched = merge_scopes(relation, &then_p, &else_p);
        let keyed = merge_scopes(relation, &then_t, &else_t);
        assert_eq!(listed(&patched), listed(&keyed), "keys, seed {seed}");
        for (key, ty) in keyed.iter() {
            assert_eq!(patched.get(key), Some(ty), "{key}, seed {seed}");
        }
        let natives = |l: &Locals| {
            let mut v: Vec<(String, Type)> =
                l.natives().map(|(k, t)| (k.clone(), t.clone())).collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        };
        assert_eq!(natives(&patched), natives(&keyed), "natives, seed {seed}");
        let synthetic = |l: &Locals| {
            let mut v: Vec<String> = l.synthetic_keys().cloned().collect();
            v.sort();
            v
        };
        assert_eq!(
            synthetic(&patched),
            synthetic(&keyed),
            "synthetic, seed {seed}"
        );
    }
}
