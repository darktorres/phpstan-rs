//! Relation answers for one immutable hierarchy generation. Each thread
//! keeps two bounded segments per kind of answer, filed under the
//! generation, so an answer of an earlier generation is never found for a
//! query of a later one and a change of generation clears nothing.

use std::cell::RefCell;
use std::hash::Hash;
use std::sync::OnceLock;

use rustc_hash::FxHashMap;

use crate::{TrinaryLogic, Type};

const SEGMENT_LIMIT: usize = 1 << 16;

/// The entries filed since the last rotation and those before it.
struct Segments<K, V> {
    previous: FxHashMap<K, V>,
    current: FxHashMap<K, V>,
}

impl<K: Hash + Eq, V> Segments<K, V> {
    const fn new() -> Self {
        Segments {
            previous: FxHashMap::with_hasher(rustc_hash::FxBuildHasher),
            current: FxHashMap::with_hasher(rustc_hash::FxBuildHasher),
        }
    }

    fn get(&self, key: &K) -> Option<&V> {
        self.current.get(key).or_else(|| self.previous.get(key))
    }

    fn insert(&mut self, key: K, value: V) {
        if self.current.len() >= SEGMENT_LIMIT {
            self.previous = std::mem::take(&mut self.current);
        }
        self.current.insert(key, value);
    }
}

/// The generation, then the node addresses of the types asked about. The
/// types themselves are kept in the entry so that the addresses stay theirs.
type Pair = (u64, usize, usize);

struct Memo {
    answers: Segments<Pair, (Type, Type, TrinaryLogic)>,
    /// What `rejected_from` found, under the two relation settings it reads.
    rejections: Segments<(Pair, u8), (Type, Type, Option<u8>)>,
    strings: Segments<(u64, usize), (Type, Option<Type>)>,
    /// One flag per evaluation in progress: whether it read an answer that
    /// is only provisional.
    active: Vec<bool>,
}

thread_local! {
    static MEMO: RefCell<Memo> = const {
        RefCell::new(Memo {
            answers: Segments::new(),
            rejections: Segments::new(),
            strings: Segments::new(),
            active: Vec::new(),
        })
    };
    #[cfg(test)]
    static ENABLED_FOR_TESTS: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

struct EvaluationGuard;

impl Drop for EvaluationGuard {
    fn drop(&mut self) {
        MEMO.with(|memo| {
            memo.borrow_mut().active.pop();
        });
    }
}

static ENABLED: OnceLock<bool> = OnceLock::new();

fn enabled() -> bool {
    #[cfg(test)]
    if let Some(enabled) = ENABLED_FOR_TESTS.with(std::cell::Cell::get) {
        return enabled;
    }
    *ENABLED.get_or_init(|| std::env::var_os("PHPSTAN_RS_NO_RELATION_MEMO").is_none())
}

/// Every evaluation in progress has read something that may still change.
pub(crate) fn mark_provisional() {
    MEMO.with(|memo| {
        for provisional in memo.borrow_mut().active.iter_mut() {
            *provisional = true;
        }
    });
}

/// `compute`d unless `lookup` knows the answer; stored by `store` unless an
/// answer it read was provisional.
fn evaluate<R: Clone>(
    lookup: impl FnOnce(&Memo) -> Option<R>,
    compute: impl FnOnce() -> R,
    store: impl FnOnce(&mut Memo, &R),
) -> R {
    if !enabled() {
        return compute();
    }
    if let Some(known) = MEMO.with(|memo| lookup(&memo.borrow())) {
        return known;
    }
    MEMO.with(|memo| memo.borrow_mut().active.push(false));
    let evaluation = EvaluationGuard;
    let answer = compute();
    let provisional = MEMO.with(|memo| memo.borrow().active.last().copied().unwrap_or(true));
    drop(evaluation);
    if !provisional {
        MEMO.with(|memo| store(&mut memo.borrow_mut(), &answer));
    }
    answer
}

pub(crate) fn answer(
    generation: u64,
    sup: &Type,
    sub: &Type,
    compute: impl FnOnce() -> TrinaryLogic,
) -> TrinaryLogic {
    let key = (generation, sup.node_address(), sub.node_address());
    evaluate(
        |memo| memo.answers.get(&key).map(|entry| entry.2),
        compute,
        |memo, answer| {
            memo.answers
                .insert(key, (sup.clone(), sub.clone(), *answer));
        },
    )
}

/// `Relation::rejected_from` of `accepted` against `accepting`, which also
/// depends on the relation's two settings, `settings`.
pub(crate) fn rejection(
    generation: u64,
    accepting: &Type,
    accepted: &Type,
    settings: u8,
    compute: impl FnOnce() -> Option<u8>,
) -> Option<u8> {
    let key = (
        (generation, accepting.node_address(), accepted.node_address()),
        settings,
    );
    evaluate(
        |memo| memo.rejections.get(&key).map(|entry| entry.2),
        compute,
        |memo, found| {
            memo.rejections
                .insert(key, (accepting.clone(), accepted.clone(), *found));
        },
    )
}

/// `Type::to_string_type` of `ty`.
pub(crate) fn string_type(
    generation: u64,
    ty: &Type,
    compute: impl FnOnce() -> Option<Type>,
) -> Option<Type> {
    let key = (generation, ty.node_address());
    evaluate(
        |memo| memo.strings.get(&key).map(|entry| entry.1.clone()),
        compute,
        |memo, string| {
            memo.strings.insert(key, (ty.clone(), string.clone()));
        },
    )
}

#[cfg(test)]
#[test]
fn answers_are_generation_scoped_and_the_memo_can_be_disabled() {
    use crate::TrinaryLogic::{No, Yes};
    use std::cell::Cell;

    let sup = Type::int();
    let sub = Type::int();
    let calls = Cell::new(0);
    ENABLED_FOR_TESTS.with(|enabled| enabled.set(Some(true)));

    assert_eq!(
        answer(71, &sup, &sub, || {
            calls.set(calls.get() + 1);
            Yes
        }),
        Yes
    );
    assert_eq!(
        answer(71, &sup, &sub, || {
            calls.set(calls.get() + 1);
            No
        }),
        Yes
    );
    assert_eq!(
        answer(72, &sup, &sub, || {
            calls.set(calls.get() + 1);
            No
        }),
        No
    );
    assert_eq!(calls.get(), 2);

    // Going back to an earlier generation finds its answer again.
    assert_eq!(
        answer(71, &sup, &sub, || {
            calls.set(calls.get() + 1);
            No
        }),
        Yes
    );
    assert_eq!(
        answer(72, &sup, &sub, || {
            calls.set(calls.get() + 1);
            Yes
        }),
        No
    );
    assert_eq!(calls.get(), 2);

    assert_eq!(
        answer(73, &sup, &sub, || {
            mark_provisional();
            Yes
        }),
        Yes
    );
    assert_eq!(
        answer(73, &sup, &sub, || {
            calls.set(calls.get() + 1);
            No
        }),
        No
    );
    assert_eq!(calls.get(), 3);

    ENABLED_FOR_TESTS.with(|enabled| enabled.set(Some(false)));
    assert_eq!(answer(74, &sup, &sub, || Yes), Yes);
    assert_eq!(answer(74, &sup, &sub, || No), No);
    ENABLED_FOR_TESTS.with(|enabled| enabled.set(None));
}
