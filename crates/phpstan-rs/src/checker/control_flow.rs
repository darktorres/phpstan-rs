use crate::checker::keys::{KeyData, LocalKey};
use crate::checker::{Checker, Discriminant, Locals};
use types::{self, Relation, Type};

mod boolean_conditions;
mod generators;

pub(crate) use boolean_conditions::is_constant_condition_family_identifier;
pub(crate) use generators::{
    generator_missing_return_type, generator_template_param, type_allows_generator_return,
};

/// `converge_loop_body`'s own "could this loop have run zero times"
/// parameter.
pub(crate) enum LoopZeroIterations<'a> {
    /// `do`-`while`/`for (;;)` - the body always runs at least once by
    /// construction (no condition is ever checked *before* the first
    /// pass), so the returned exit state is exactly the kept pass's own
    /// `body_locals` (`continue_states` folded in) - no merge with
    /// `entry` at all, for any variable.
    Impossible,
    /// `while`/`for`-with-a-condition/`foreach` - the condition/subject
    /// is checked *before* the first iteration, so the exit state merges
    /// with `entry` (a name only in `body_locals` is dropped, one in
    /// both is unioned - `merge_scopes`'s own existing rule, reused
    /// as-is). `subject_exempt` is a `foreach`'s own iterated subject,
    /// when it's a bare `$variable` name - excluded from that merge
    /// specifically (see `converge_loop_body`'s own doc comment on this
    /// field for why a self-mutating `foreach` needs it kept out).
    /// `subject_key` is that `foreach`'s bare `$key` variable, by value:
    /// every element the loop visits ends as the body leaves
    /// `$subject[$key]`.
    Possible {
        subject_exempt: Option<&'a str>,
        subject_key: Option<&'a str>,
    },
}

impl<'a> Checker<'a> {
    /// Fixed-point convergence for a loop body (`While`/`DoWhile`/`For`/
    /// `Foreach` in `checker/stmt.rs`) - without this, a variable
    /// reassigned inside the loop body keeps its pre-loop-entry type for
    /// the entire single-pass check, which `check_block`'s own doc
    /// comment traces the exact false-positive for.
    ///
    /// Mirrors real PHPStan's own approach (`NodeScopeResolver::
    /// LOOP_SCOPE_ITERATIONS = 3`): run the body against a candidate entry state,
    /// see whether unioning the result back with the true pre-loop `entry`
    /// changes anything: if not, the state has converged and this pass's
    /// diagnostics are correct as reported; if so, its diagnostics were
    /// computed against a stale entry and must be discarded (the existing
    /// `diagnostics.truncate` pattern `check_program` already uses for
    /// `saw_dynamic_var_construct`), and one more pass runs with the
    /// unioned entry. Capped at 3 total passes - always keeps whichever
    /// pass is last, converged or not, rather than looping forever on a
    /// body that can't stabilize.
    ///
    /// `loop_break_states` is pushed/popped once per pass here (not by the
    /// caller) so a pre-convergence pass's own break states never leak;
    /// only the returned, kept pass's break states matter to the caller.
    ///
    /// `loop_continue_states` gets the identical per-pass push/pop, but is
    /// used completely differently: a `continue` jumps back to the top of
    /// *this* loop, so whatever state it reached is exactly as much a real
    /// entry to the next iteration as the body's own ordinary fall-through
    /// exit (`body_locals`) is - folded into `next_entry` alongside it,
    /// every pass, not just carried forward for the final kept one the way
    /// `break_states` is. Without this, a variable reassigned *only* on a
    /// path that ends in `continue` never contributed to `next_entry` at
    /// all (the fall-through-only `merge_scopes(entry, &body_locals)`
    /// below never reflects it), so convergence would wrongly conclude
    /// "nothing changed" after a single pass and keep that pass's own
    /// diagnostics - computed against the stale, pre-reassignment entry.
    /// A variable still changing from one pass of a loop to the next
    /// (a counter, a string built up, an array grown key by key) is widened
    /// so the next pass can converge: an int only on the side it is moving,
    /// anything else to its general type. Only a growing value widens; one
    /// that shrank back inside an earlier widening narrows to what the pass
    /// gave.
    pub(crate) fn widen_towards_entry(&self, pass_entry: &Locals, next_entry: &mut Locals) {
        let relation = self.relation();
        // A binding the pass did not change stays; only an added or changed
        // one can widen.
        let changing: Vec<(LocalKey, Type)> = pass_entry
            .types_diff(next_entry)
            .filter_map(|item| match item {
                imbl::ordmap::DiffItem::Add(name, ty) => Some((*name, None, ty)),
                imbl::ordmap::DiffItem::Update { old, new } => Some((*new.0, Some(old.1), new.1)),
                imbl::ordmap::DiffItem::Remove(..) => None,
            })
            .filter(|(_, before, ty)| match before {
                Some(before) => !relation.is_super_type_of(before, ty).yes(),
                None => true,
            })
            .map(|(name, before, ty)| {
                let widened = match before {
                    Some(before) => before.widen_towards(ty),
                    None => ty.generalize(),
                };
                (name, widened)
            })
            .collect();
        for (name, ty) in changing {
            next_entry.insert(name, ty);
        }
    }

    pub(crate) fn converge_loop_body(
        &mut self,
        entry: &Locals,
        declared_return: Option<&Type>,
        label: &str,
        // `while`/`for`/`foreach` can all genuinely run zero times (the
        // condition/subject is checked *before* the first iteration) -
        // `Possible` merges the exit state with `entry` accordingly (see
        // the loop's own comment below for exactly how). A `do`-`while`/
        // `for (;;)` is the one real exception: its own body always runs
        // at least once by construction (no condition is ever checked
        // before the first pass), so `check_do_while`/`check_for`'s own
        // no-condition arm pass `Impossible` to skip that merge entirely -
        // otherwise a body-only local (`do { $changed = false; ...; }
        // while ($changed);`) that must stay defined afterward would be
        // merged away as if the loop might never have run, wrongly
        // flagging `variable.undefined`.
        zero_iterations: LoopZeroIterations<'_>,
        mut run_pass: impl FnMut(&mut Self, &mut Locals, Option<&Type>, &str, usize),
    ) -> (Locals, Vec<Locals>) {
        // A variable still changing after `WIDEN_FROM_PASS` passes (a
        // counter, a string built up, an array grown key by key) is
        // widened so the next pass can converge - an int only on the side
        // it's moving, anything else to its general type; its literal values
        // would otherwise run out the passes and leave the exit state
        // short of what later iterations produce.
        const MAX_PASSES: usize = 6;
        const WIDEN_FROM_PASS: usize = 2;
        let mut pass_entry = entry.clone();
        for pass in 0..MAX_PASSES {
            let is_last_allowed_pass = pass == MAX_PASSES - 1;
            let mut body_locals = pass_entry.clone();
            self.loop_break_states.push(Vec::new());
            self.loop_continue_states.push(Vec::new());
            let mark = self.pass_mark();
            run_pass(self, &mut body_locals, declared_return, label, pass);
            let break_states = self.loop_break_states.pop().unwrap_or_default();
            let continue_states = self.loop_continue_states.pop().unwrap_or_default();
            let mut next_entry = merge_scopes(&self.relation(), entry, &body_locals);
            for state in &continue_states {
                next_entry = merge_scopes(&self.relation(), &next_entry, state);
            }
            if pass >= WIDEN_FROM_PASS && next_entry != pass_entry {
                self.widen_towards_entry(&pass_entry, &mut next_entry);
            }
            if next_entry == pass_entry || is_last_allowed_pass {
                // The kept pass's own `body_locals`, returned to the
                // caller as "the loop's exit state," is the bare
                // fallthrough-only state on its own - `continue_states`
                // must be folded in here too, or a variable only ever
                // assigned on the non-`continue` path (`foreach (...) {
                // if ($x) { continue; } $flag = true; }`) would look
                // unconditionally assigned after the loop instead of
                // "maybe assigned, maybe not." On the loop's own last
                // logical iteration, a `continue` there is just as valid
                // an exit path as an ordinary fallthrough, the identical
                // fallthrough-vs-break distinction every caller already
                // folds `break_states` back in for afterward.
                let mut exit_locals = body_locals;
                for state in &continue_states {
                    exit_locals = merge_scopes(&self.relation(), &exit_locals, state);
                }
                // A variable only ever assigned inside the loop body but
                // genuinely tracked before it too (`$x = null; foreach
                // (...) { $x = ...; }`) needs the same "maybe never
                // touched" treatment as one introduced fresh inside the
                // body - both must union with whatever `entry` already
                // held for that name, not just adopt the body's own
                // post-iteration value outright. `next_entry` (computed
                // just above - `merge_scopes(entry, &body_locals)`,
                // `continue_states` already folded in) already does
                // exactly this for every name uniformly: union one
                // present in both, drop one present only in
                // `body_locals`.
                //
                // The one real exception - `LoopZeroIterations::Possible`'s
                // own `subject_exempt`, a `foreach`'s own iterated subject
                // when it's a bare `$variable` - is a `foreach` mutating
                // its own subject array in place (`foreach ($items as
                // $day => ...) { $items[$day]['derivative'] = ...; }`),
                // which must never have its already-transformed shape
                // re-widened back toward the stale pre-loop one `entry`
                // still holds for that same variable - every key really
                // iterated over gets the loop's own treatment, never the
                // old one. Restored from `exit_locals`'s own (pre-union)
                // value after the general union, rather than being
                // excluded from it.
                if let LoopZeroIterations::Possible {
                    subject_exempt,
                    subject_key,
                } = zero_iterations
                {
                    // Every element visited ends as the body leaves the
                    // element its key names, when the body's exit knows
                    // it; iterating zero times left the subject as it
                    // was, which for an array is empty.
                    let subject_override = subject_exempt.and_then(|name| {
                        let subject = exit_locals.find_var(name)?;
                        let iterated = exit_locals.get(subject).cloned()?;
                        let element_key = subject_key.and_then(|key| {
                            let subscript = exit_locals.find_var(key)?;
                            exit_locals.find(&KeyData::VarIndex {
                                base: subject,
                                subscript,
                            })
                        });
                        let iterated = match element_key.and_then(|key| exit_locals.get(key)) {
                            Some(element) if iterated.is_array().yes() => {
                                iterated.with_general_array_value(element)
                            }
                            _ => iterated,
                        };
                        Some(match entry.get(subject) {
                            Some(before) if before.is_array().yes() => {
                                if before.is_non_empty_array().yes() {
                                    iterated
                                } else {
                                    self.relation().union(
                                        iterated,
                                        Type::array_shape(types::Shape::sealed(Vec::new())),
                                    )
                                }
                            }
                            Some(before) => self.relation().union(iterated, before.clone()),
                            None => iterated,
                        })
                    });
                    exit_locals = next_entry;
                    if let (Some(name), Some(ty)) = (subject_exempt, subject_override) {
                        exit_locals.insert_var(name, ty);
                    }
                }
                return (exit_locals, break_states);
            }
            self.rewind_to(mark);
            pass_entry = next_entry;
        }
        unreachable!("the loop above always returns by its last iteration")
    }
}

/// Joins two branch-local scopes back into one after an `if`/`while`. A
/// variable present in both branches gets the union of its two types; one
/// present in only one branch was introduced there and can't be assumed to
/// exist afterward, so it's dropped rather than carried forward unsoundly.
///
/// `provenance` is merged the same way `types` is - the union of both
/// sides' known declaration spans for a name present in both, since
/// either branch could be the one that actually ran. This is the one
/// function every multi-way join in this checker already routes through
/// (`Switch`'s own `exit_states` fold, `Try`'s own `end_states` fold,
/// `converge_loop_body`'s own fixed-point loop) - fixing
/// provenance-merging here once is what makes every one of those inherit
/// real reaching-definitions precision, with no further change needed
/// at any of those call sites.
#[hotpath::measure]
/// Adds the fact that `name` is `ty` to `facts`. Two facts for one
/// variable under one discriminant come from different paths, so it is
/// either.
pub(crate) fn add_implied_fact(
    relation: &Relation,
    facts: &mut Vec<(LocalKey, Type)>,
    name: LocalKey,
    ty: &Type,
) {
    match facts.iter_mut().find(|(k, _)| *k == name) {
        Some((_, existing)) if existing == ty => {}
        Some((_, existing)) => *existing = relation.union(existing.clone(), ty.clone()),
        None => facts.push((name, ty.clone())),
    }
}

/// Both sides' facts under one discriminant. A variable only one side has
/// a fact for is, on the other side, whatever that side holds, since the
/// discriminant held there too.
fn merge_implied_facts(
    relation: &Relation,
    then_facts: &[(LocalKey, Type)],
    else_facts: &[(LocalKey, Type)],
    then_locals: &Locals,
    else_locals: &Locals,
) -> Option<std::rc::Rc<Vec<(LocalKey, Type)>>> {
    let mut combined: Vec<(LocalKey, Type)> = Vec::new();
    for (name, ty) in then_facts {
        match else_facts.iter().find(|(k, _)| k == name) {
            Some((_, other)) => {
                add_implied_fact(relation, &mut combined, *name, ty);
                add_implied_fact(relation, &mut combined, *name, other);
            }
            None => {
                if let Some(other) = else_locals.get(*name) {
                    add_implied_fact(relation, &mut combined, *name, ty);
                    add_implied_fact(relation, &mut combined, *name, other);
                }
            }
        }
    }
    for (name, ty) in else_facts {
        if then_facts.iter().any(|(k, _)| k == name) {
            continue;
        }
        if let Some(other) = then_locals.get(*name) {
            add_implied_fact(relation, &mut combined, *name, ty);
            add_implied_fact(relation, &mut combined, *name, other);
        }
    }
    (!combined.is_empty()).then(|| std::rc::Rc::new(combined))
}

/// One side's facts under `key`, merged with an `other` side that has none:
/// kept when the discriminant cannot hold there, and otherwise each widened
/// by what `other` holds for its variable.
fn widen_implied_facts(
    relation: &Relation,
    facts: &std::rc::Rc<Vec<(LocalKey, Type)>>,
    key: &Discriminant,
    other: &Locals,
) -> Option<std::rc::Rc<Vec<(LocalKey, Type)>>> {
    if !discriminant_may_hold(other, key) {
        return Some(facts.clone());
    }
    let mut widened: Vec<(LocalKey, Type)> = Vec::new();
    for (name, ty) in facts.iter() {
        if let Some(other_ty) = other.get(*name) {
            add_implied_fact(relation, &mut widened, *name, ty);
            add_implied_fact(relation, &mut widened, *name, other_ty);
        }
    }
    (!widened.is_empty()).then(|| std::rc::Rc::new(widened))
}

/// Whether `locals` allows the discriminant `key` (`subject==value`, see
/// `Discriminant`) to hold. A subject that isn't a local, or a
/// value this can't compare, may.
fn discriminant_may_hold(locals: &Locals, key: &Discriminant) -> bool {
    let value = &key.value;
    let Some(ty) = locals.get(key.subject) else {
        // An undefined variable reads as `null`.
        return value.is_false().yes() || !locals.is_real_variable(key.subject);
    };
    let members = ty.union_members();
    if value.is_true().yes() {
        members
            .iter()
            .any(|m| !(m.is_null().yes() || m.is_false().yes()))
    } else if value.is_false().yes() {
        members.iter().any(|m| !m.is_true().yes())
    } else {
        members.iter().any(|m| !m.is_single_value() || m == value)
    }
}

pub(crate) fn merge_scopes(
    relation: &Relation,
    then_locals: &Locals,
    else_locals: &Locals,
) -> Locals {
    // Two scopes with one version hold the same facts, so what joins them is
    // either of them: a branch that wrote nothing joins as it was.
    if then_locals.version() != 0 && then_locals.version() == else_locals.version() {
        return then_locals.clone();
    }
    // Without provenance to join key by key, the join starts as a copy of
    // the then-scope, which shares everything with it, and is changed only
    // where the two differ: most keys are the same in both.
    let patched = !then_locals.tracks_provenance();
    let mut merged = if patched {
        then_locals.clone()
    } else {
        then_locals.fresh_scope()
    };
    merged.set_any_variable_may_exist(
        then_locals.any_variable_may_exist() || else_locals.any_variable_may_exist(),
    );
    // A `function_exists()` guard only survives a merge
    // point when both incoming branches agree it's guaranteed - the same
    // reasoning every other narrowed fact here already gets (`Type::union`
    // above widens a type across both branches; a guard, being a boolean
    // fact rather than a type, "widens" to "not guaranteed" the moment
    // either branch doesn't have it). Since only the `function_exists(...)`
    // true-branch itself ever inserts a name (`apply_narrowing`), this
    // also correctly makes the fact vanish the instant execution falls
    // through a non-terminating `if` with no early exit on the other
    // side - `check_if`'s own existing `then_terminates`/`else_terminates`
    // branch-selection logic (not this function) is what already makes
    // the "unconditional early return in the negative branch persists the
    // fact past the `if`" idiom work, by handing back the whole surviving
    // branch's `Locals` (guard set included) instead of a merge at all.
    *merged.guarded_functions_mut() = then_locals
        .guarded_functions()
        .clone()
        .intersection(else_locals.guarded_functions().clone());
    *merged.guarded_constants_mut() = then_locals
        .guarded_constants()
        .clone()
        .intersection(else_locals.guarded_constants().clone());
    *merged.guarded_classes_mut() = then_locals
        .guarded_classes()
        .clone()
        .intersection(else_locals.guarded_classes().clone());
    // Identical intersection treatment for the
    // method_exists()/property_exists() guard sets - a member is only
    // still provably guarded after an if/else join when both branches
    // proved it, same as guarded_functions above.
    *merged.guarded_methods_mut() = then_locals
        .guarded_methods()
        .clone()
        .intersection(else_locals.guarded_methods().clone());
    *merged.guarded_properties_mut() = then_locals
        .guarded_properties()
        .clone()
        .intersection(else_locals.guarded_properties().clone());
    // Reinitialized on either path is possibly reinitialized.
    *merged.clone_reinitialized_mut() = then_locals
        .clone_reinitialized()
        .clone()
        .union(else_locals.clone_reinitialized().clone());
    merged.join_reference_links(else_locals);
    merged.join_reference_links(then_locals);
    // Exposed on either path is possibly exposed.
    *merged.exposed_mut() = then_locals
        .exposed()
        .clone()
        .union(else_locals.exposed().clone());
    // A condition alias only survives a merge point when both incoming
    // branches still
    // agree on the exact same stored condition ID - ID equality is a
    // correct, cheap test for this: it's exactly true whenever both
    // branches inherited the alias unchanged from before the split (the
    // overwhelmingly common case - the assignment happened before an
    // unrelated `if`/`else`), and correctly false whenever
    // either branch reassigned the aliased name or the alias's own
    // target in between (`Locals::declare`'s own purge already dropped
    // it on that side, so the key is simply absent there and this filter
    // drops it too) - no case needs comparing two independently-
    // constructed-but-textually-identical conditions from different
    // branches, so no structural equality is needed anywhere.
    *merged.condition_aliases_mut() = then_locals
        .condition_aliases()
        .iter()
        .filter(|(k, (_, id))| {
            else_locals
                .condition_aliases()
                .get(*k)
                .is_some_and(|(_, other_id)| id == other_id)
        })
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    // A native type survives only when both branches know one.
    for (key, then_native) in then_locals.natives() {
        match else_locals.native(key) {
            Some(else_native) => {
                // Two equal types union to the first, so the copy of the
                // then-scope already holds the merged native.
                if then_native.is_same_node(else_native) || then_native == else_native {
                    debug_assert!(
                        relation.union(then_native.clone(), else_native.clone()) == *then_native
                    );
                    if !patched {
                        merged.set_native(key, then_native.clone());
                    }
                    continue;
                }
                if patched {
                    // `set_native` keeps no `mixed`, so what the copy holds
                    // goes first.
                    merged.clear_native(key);
                }
                merged.set_native(
                    key,
                    relation.union(then_native.clone(), else_native.clone()),
                );
            }
            None if patched => merged.clear_native(key),
            None => {}
        }
    }
    // Same "both branches inherited the identical, unchanged alias"
    // dedup as `condition_aliases` above - a `QueryBuilder` alias has no
    // separate target to intersect against, just the one stored expression ID.
    *merged.query_builder_aliases_mut() = then_locals
        .query_builder_aliases()
        .iter()
        .filter(|(k, id)| {
            else_locals
                .query_builder_aliases()
                .get(*k)
                .is_some_and(|other_id| *id == other_id)
        })
        .map(|(k, v)| (*k, *v))
        .collect();
    // Same shape as `query_builder_aliases` above, for the identical
    // reason: a container's own per-field facts (see
    // `ContainerFieldFactsMap`'s doc comment) can be invalidated on just
    // *one* branch (a rebind, or a by-ref pass to an unrecognized
    // function - either purges the container's own entry, see
    // `purge_stale_narrowing_for`), so a merge must only keep an entry
    // both sides still carry *unchanged*, never resurrect one only one
    // side still has - unlike `branch_implied_facts` below, this is not a
    // durable structural fact safe to union.
    *merged.container_field_facts_mut() = then_locals
        .container_field_facts()
        .iter()
        .filter(|(k, rc)| {
            else_locals
                .container_field_facts()
                .get(*k)
                .is_some_and(|other_rc| std::rc::Rc::ptr_eq(rc, other_rc))
        })
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    // See `DiscriminantArrayFactsMap`'s own doc comment - unions rather
    // than intersects, like `branch_implied_facts` below: a
    // `(container, subject, true)` entry only ever gets written inside
    // the `subject`-true branch, so it's naturally absent from the
    // `subject`-false branch's own fork - that absence means "the other
    // branch had no reason to touch it," not "invalidated," so it must
    // survive the merge. A key present on both sides (the same
    // container/subject/value slot written independently on each side -
    // not expected for the single-`if` shape this mechanism currently
    // triggers on, but possible from nested control flow) is unioned,
    // never silently overwritten by one side.
    *merged.discriminant_array_facts_mut() = then_locals.discriminant_array_facts().clone();
    for (key, else_ty) in else_locals.discriminant_array_facts().iter() {
        match merged.discriminant_array_facts().get(key).cloned() {
            Some(then_ty) if then_ty == *else_ty => {}
            Some(then_ty) => {
                merged
                    .discriminant_array_facts_mut()
                    .insert(*key, relation.union(then_ty, else_ty.clone()));
            }
            None => {
                merged
                    .discriminant_array_facts_mut()
                    .insert(*key, else_ty.clone());
            }
        }
    }
    // Unlike `condition_aliases` above, a `branch_implied_facts` entry is
    // a durable structural fact about the code at one exact discriminant
    // (see that field's own doc comment) - true whenever that
    // discriminant holds again, regardless of which branch of *this*
    // merge point actually ran, so a merge unions both sides rather than
    // intersecting. `Rc::ptr_eq`-deduped when both sides inherited the
    // identical entry unchanged (the common case - the fact was recorded
    // before this split), otherwise both sides' own fact lists are kept
    // (a genuinely new fact recorded independently on each side, from
    // two different `if`s sharing one discriminant).
    *merged.branch_implied_facts_mut() = then_locals.branch_implied_facts().clone();
    for (key, else_facts) in else_locals.branch_implied_facts().iter() {
        let combined = match then_locals.branch_implied_facts().get(key) {
            Some(then_facts) if std::rc::Rc::ptr_eq(then_facts, else_facts) => continue,
            Some(then_facts) => {
                merge_implied_facts(relation, then_facts, else_facts, then_locals, else_locals)
            }
            None => widen_implied_facts(relation, else_facts, key, then_locals),
        };
        match combined {
            Some(facts) => merged.branch_implied_facts_mut().insert(key.clone(), facts),
            None => merged.branch_implied_facts_mut().remove(key),
        };
    }
    for (key, then_facts) in then_locals.branch_implied_facts().iter() {
        if else_locals.branch_implied_facts().contains_key(key) {
            continue;
        }
        match widen_implied_facts(relation, then_facts, key, else_locals) {
            Some(facts) => merged.branch_implied_facts_mut().insert(key.clone(), facts),
            None => merged.branch_implied_facts_mut().remove(key),
        };
    }
    if patched {
        // Only the bindings the two scopes differ in change the copy: one
        // only the then-scope holds is dropped, one both hold with another
        // type is joined; one only the else-scope holds is not in it.
        for item in then_locals.types_diff(else_locals) {
            match item {
                imbl::ordmap::DiffItem::Add(..) => {}
                imbl::ordmap::DiffItem::Remove(name, _) => {
                    merged.remove_type(*name);
                }
                imbl::ordmap::DiffItem::Update { old, new } => {
                    merged.insert(*old.0, relation.union(old.1.clone(), new.1.clone()));
                }
            }
        }
        return merged;
    }
    for (name, then_ty) in then_locals.iter() {
        if let Some(else_ty) = else_locals.get(name) {
            let mut provenance = then_locals
                .get_provenance(name)
                .cloned()
                .unwrap_or_default();
            if let Some(else_provenance) = else_locals.get_provenance(name) {
                provenance.extend(else_provenance.iter().copied());
            }
            // `Type::union` takes its operands by value, so equal types
            // (a variable neither branch reassigned, the common case) are
            // kept without cloning both.
            let merged_ty = if then_ty == else_ty {
                then_ty.clone()
            } else {
                relation.union(then_ty.clone(), else_ty.clone())
            };
            merged.insert_with_provenance(name, merged_ty, provenance);
        }
    }
    merged
}
