//! The answers `is_super_type_of` has given, by the pair of types: the
//! same pairs are asked again and again, by a union's absorption and its
//! merge of shapes, and by the comparison of their fields' values.
//!
//! An answer depends on the pair and on the class hierarchy alone. The
//! relation that knows no classes keeps its answers for good; one over a
//! registry keeps them while a `RelationMemoScope` holds the registry
//! unchanged.

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;

use rustc_hash::FxHashMap;

use crate::{ClassHierarchy, TrinaryLogic, Type};

/// Past this many pairs a thread's table is emptied.
const LIMIT: usize = 1 << 17;

/// The pair's nodes are held with the answer, which keeps their addresses
/// from being taken by other types while the entry stands.
type Entries = FxHashMap<(usize, usize), (Type, Type, TrinaryLogic)>;

thread_local! {
    static STRUCTURAL: RefCell<Entries> = RefCell::new(FxHashMap::default());
    static SCOPED: RefCell<Entries> = RefCell::new(FxHashMap::default());
    /// The address of the hierarchy `SCOPED` holds answers for, 0 for none.
    static SCOPE: Cell<usize> = const { Cell::new(0) };
}

fn address(hierarchy: &dyn ClassHierarchy) -> usize {
    (hierarchy as *const dyn ClassHierarchy).cast::<()>() as usize
}

/// While this lives, the relations over `hierarchy` keep their answers
/// for the thread. The borrow is what holds the hierarchy unchanged.
pub struct RelationMemoScope<'h> {
    outer: usize,
    hierarchy: PhantomData<&'h dyn ClassHierarchy>,
}

/// Keeps the answers of the relations over `hierarchy` until the scope is
/// dropped.
pub fn relation_memo_scope(hierarchy: &dyn ClassHierarchy) -> RelationMemoScope<'_> {
    let outer = SCOPE.with(|s| s.replace(address(hierarchy)));
    if outer != address(hierarchy) {
        SCOPED.with(|a| a.borrow_mut().clear());
    }
    RelationMemoScope {
        outer,
        hierarchy: PhantomData,
    }
}

impl Drop for RelationMemoScope<'_> {
    fn drop(&mut self) {
        let inner = SCOPE.with(|s| s.replace(self.outer));
        if inner != self.outer {
            SCOPED.with(|a| a.borrow_mut().clear());
        }
    }
}

pub(super) fn answer(
    hierarchy: &dyn ClassHierarchy,
    sup: &Type,
    sub: &Type,
    compute: impl FnOnce() -> TrinaryLogic,
) -> TrinaryLogic {
    let table = if hierarchy.is_structural() {
        &STRUCTURAL
    } else if SCOPE.with(Cell::get) == address(hierarchy) {
        &SCOPED
    } else {
        return compute();
    };
    let key = (sup.node_address(), sub.node_address());
    if let Some(known) = table.with(|a| a.borrow().get(&key).map(|e| e.2)) {
        return known;
    }
    let answer = compute();
    table.with(|a| {
        let mut entries = a.borrow_mut();
        if entries.len() >= LIMIT {
            entries.clear();
        }
        entries.insert(key, (sup.clone(), sub.clone(), answer));
    });
    answer
}
