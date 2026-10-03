//! `Locals`' set of synthetic keys: what a rebound key invalidates is found
//! among them, and is what looking through every key finds.
use super::super::control_flow::merge_scopes;
use super::super::keys::{KeyData, LocalKey};
use super::super::narrowing::purge_stale_narrowing_for;
use super::super::*;

/// Keys of every shape the table makes, and plain variables, with names
/// that are prefixes of one another.
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
    "Foo::$bar",
    "a[$k][$a]",
    "k",
    "this->items[$k]",
];

/// The key `text` spells, made in `locals`: a variable or static property,
/// then `->name`, `[$variable]` and `[literal]` steps.
fn key(locals: &Locals, text: &str) -> LocalKey {
    let mut rest = text;
    let root_end = rest.find(['-', '[']).unwrap_or(rest.len());
    let mut current = match rest[..root_end].split_once("::$") {
        Some((class, prop)) => locals.intern(KeyData::StaticProp {
            class: class.into(),
            prop: prop.into(),
        }),
        None => locals.var_key(&rest[..root_end]),
    };
    rest = &rest[root_end..];
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("->") {
            let end = after.find(['-', '[']).unwrap_or(after.len());
            current = locals.intern(KeyData::Prop {
                base: current,
                prop: after[..end].into(),
            });
            rest = &after[end..];
        } else {
            let after = rest.strip_prefix('[').expect("a step");
            let end = after.find(']').expect("a closed step");
            let inner = &after[..end];
            current = match inner.strip_prefix('$') {
                Some(variable) => {
                    let subscript = locals.var_key(variable);
                    locals.intern(KeyData::VarIndex {
                        base: current,
                        subscript,
                    })
                }
                None => locals.intern(KeyData::Index {
                    base: current,
                    key: inner.into(),
                }),
            };
            rest = &after[end + 1..];
        }
    }
    current
}

fn locals_with(keys: &[&str]) -> Locals {
    let mut locals = Locals::default();
    for text in keys {
        let key = key(&locals, text);
        locals.insert(key, Type::int());
    }
    locals
}

fn listed(locals: &Locals) -> Vec<String> {
    let mut keys: Vec<String> = locals.iter().map(|(k, _)| locals.render(k)).collect();
    keys.sort();
    keys
}

/// Whether the text `k` depends on the text `key`, as keys were compared
/// before they were made by structure: it is `key`, a chain rooted at it,
/// or a chain with `key` as a variable subscript.
fn text_depends_on(k: &str, key: &str) -> bool {
    k == key
        || k.strip_prefix(key)
            .is_some_and(|rest| rest.starts_with("->") || rest.starts_with('['))
        || (k.contains('[') && k.contains(&format!("[${key}]")))
}

/// What `purge_stale_narrowing_for` removes when every local is looked
/// through: every local that depends on `key`, other than `key`.
fn removed_by_scanning_every_local(locals: &Locals, key: &str) -> Vec<String> {
    let mut gone: Vec<String> = locals
        .iter()
        .map(|(k, _)| locals.render(k))
        .filter(|k| k != key && text_depends_on(k, key))
        .collect();
    gone.sort();
    gone
}

#[test]
fn purging_a_key_removes_what_scanning_every_local_finds() {
    for text in KEYS {
        let mut locals = locals_with(KEYS);
        let expected = removed_by_scanning_every_local(&locals, text);
        let before = listed(&locals);
        let purged = key(&locals, text);
        purge_stale_narrowing_for(&mut locals, purged);
        let after = listed(&locals);
        let removed: Vec<String> = before
            .iter()
            .filter(|k| !after.contains(k))
            .cloned()
            .collect();
        assert_eq!(removed, expected, "purging {text}");
    }
}

#[test]
fn the_synthetic_keys_are_the_keys_that_are_not_a_variables_name() {
    let mut locals = locals_with(KEYS);
    let expected = |locals: &Locals| {
        let mut keys: Vec<String> = locals
            .iter()
            .map(|(k, _)| k)
            .filter(|k| !locals.is_real_variable(*k))
            .map(|k| locals.render(k))
            .collect();
        keys.sort();
        keys
    };
    let synthetic = |locals: &Locals| {
        let mut keys: Vec<String> = locals.synthetic_keys().map(|k| locals.render(*k)).collect();
        keys.sort();
        keys
    };
    assert_eq!(synthetic(&locals), expected(&locals));
    // Through every way a key is added, removed or copied.
    let xy = key(&locals, "x->y");
    locals.declare(xy, Type::string(), Span::default());
    let z1 = key(&locals, "z[1]");
    locals.insert_with_provenance(z1, Type::null(), Default::default());
    assert_eq!(synthetic(&locals), expected(&locals));
    let ab = key(&locals, "a->b");
    let a = key(&locals, "a");
    locals.remove(ab);
    locals.remove(a);
    assert_eq!(synthetic(&locals), expected(&locals));
    let copy = locals.clone();
    let ma = key(&locals, "m[$a]");
    locals.remove(ma);
    let wv = key(&locals, "w->v");
    locals.insert(wv, Type::bool());
    assert_eq!(synthetic(&copy), expected(&copy));
    assert_eq!(synthetic(&locals), expected(&locals));
    let fresh = locals.fresh_scope();
    assert!(fresh.synthetic_keys().next().is_none());
}

#[test]
fn rebinding_a_receiver_forgets_the_members_it_was_proved_to_have() {
    let mut locals = locals_with(&["o", "p", "flag"]);
    let (o, p, flag) = (key(&locals, "o"), key(&locals, "p"), key(&locals, "flag"));
    locals.guarded_methods_mut().insert((o, "m".to_string()));
    locals.guarded_properties_mut().insert((o, "x".to_string()));
    locals.guarded_methods_mut().insert((p, "m".to_string()));
    locals
        .discriminant_array_facts_mut()
        .insert((p, flag, true), Type::int());
    locals.declare(o, Type::object("Other".into(), Vec::new()), Span::default());
    assert!(!locals.guarded_methods().contains(&(o, "m".to_string())));
    assert!(!locals.guarded_properties().contains(&(o, "x".to_string())));
    // Another receiver's stay, and so do facts about another subject.
    assert!(locals.guarded_methods().contains(&(p, "m".to_string())));
    assert_eq!(locals.discriminant_array_facts().len(), 1);
    // A fact is about its subject too.
    locals.declare(flag, Type::bool(), Span::default());
    assert!(locals.discriminant_array_facts().is_empty());
}

#[test]
fn a_scope_with_no_synthetic_key_and_no_fact_has_nothing_to_forget() {
    let mut locals = locals_with(&["a", "b", "this"]);
    assert!(!locals.has_state_to_forget());
    let a = key(&locals, "a");
    let c = key(&locals, "c");
    assert!(locals.has_key_depending_on(a));
    assert!(!locals.has_key_depending_on(c));
    let ap = key(&locals, "a->p");
    locals.insert(ap, Type::int());
    assert!(locals.has_state_to_forget());
    assert!(locals.has_key_depending_on(a));
    locals.remove(ap);
    assert!(!locals.has_state_to_forget());
}

#[test]
fn has_key_depending_on_is_the_scan_of_every_local() {
    let locals = locals_with(KEYS);
    for text in KEYS.iter().chain(&["none", "a->x", "m", "Foo::$baz"]) {
        let by_scan = locals
            .iter()
            .any(|(k, _)| text_depends_on(&locals.render(k), text));
        let probe = key(&locals, text);
        assert_eq!(locals.has_key_depending_on(probe), by_scan, "{text}");
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
        let name = key(locals, names[n(names.len() as u64) as usize]);
        match n(5) {
            0 => {
                locals.remove(name);
            }
            1 => {
                locals.set_native(name, types[n(6) as usize].clone());
            }
            2 => locals.clear_native(name),
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
            // The two scenarios made their own tables, whose keys come in
            // the same order: the writes are the same.
            let patched_key = patched.find(&keyed.data(key)).expect("a key of the other");
            assert_eq!(patched.get(patched_key), Some(ty), "{key:?}, seed {seed}");
        }
        let natives = |l: &Locals| {
            let mut v: Vec<(String, Type)> =
                l.natives().map(|(k, t)| (l.render(k), t.clone())).collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        };
        assert_eq!(natives(&patched), natives(&keyed), "natives, seed {seed}");
        let synthetic = |l: &Locals| {
            let mut v: Vec<String> = l.synthetic_keys().map(|k| l.render(*k)).collect();
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
