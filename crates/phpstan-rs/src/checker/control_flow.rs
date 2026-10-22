use crate::checker::keys::{KeyData, LocalKey};
use crate::checker::loop_counters::LoopCounters;
use crate::checker::{Checker, Discriminant, Locals};
use types::{self, Relation, Type, UnionAll as _};

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
    /// gave. A counter of the loop (`bands`) widens to its band instead:
    /// as far as the process's iterations can move it.
    pub(crate) fn widen_towards_entry(
        &self,
        pass_entry: &Locals,
        next_entry: &mut Locals,
        bands: &[(LocalKey, (i64, i64))],
    ) {
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
                let band = bands.iter().find(|(key, _)| *key == name).map(|(_, b)| *b);
                let widened = match (before, band) {
                    (Some(before), Some(band)) if before.is_int().yes() && ty.is_int().yes() => {
                        widen_counter(before, ty, band)
                    }
                    (Some(before), _) => before.widen_towards(ty),
                    (None, _) => ty.generalize(),
                };
                (name, widened)
            })
            .collect();
        // Widening changes a type, not whether the variable is defined.
        for (name, ty) in changing {
            let maybe = next_entry.is_maybe_undefined(name);
            next_entry.insert(name, ty);
            if maybe {
                next_entry.mark_maybe_undefined(name);
            }
        }
    }

    /// `ty` widened to every value the enclosing body writes to the variable
    /// `name` (`byref_writes`): what a by-reference capture of it may hold
    /// when its closure runs, wherever the closure is created.
    pub(crate) fn widen_to_byref_writes(&self, name: &str, ty: types::Type) -> types::Type {
        let Some(log) = &self.byref_writes else {
            return ty.unsealed_shapes();
        };
        let log = log.borrow();
        let relation = self.relation();
        // A write that `ty` already covers adds no value the capture can
        // hold, so the shape stays sealed; only a write outside `ty` (or
        // one to an unknown name) widens it.
        let outside_ty = log.any_name_written()
            || log
                .types_of(name)
                .iter()
                .any(|written| !relation.is_super_type_of(&ty, written).yes());
        if !outside_ty {
            return ty;
        }
        let mut types = vec![ty];
        types.extend(log.types_of(name).iter().cloned());
        if log.any_name_written() {
            types.push(types::Type::mixed());
        }
        types
            .into_iter()
            .union_all_with(&relation)
            .unwrap_or_else(types::Type::mixed)
            .unsealed_shapes()
    }

    /// `ty` at a call of a closure capturing `name` by reference, widened
    /// only when the body wrote `name` in a way its locals do not hold: an
    /// element or property write, or a reference binding to one, logged as
    /// mixed (`Locals::log_write`), or a write to an unknown name.
    pub(crate) fn widen_to_untracked_byref_writes(
        &self,
        name: &str,
        ty: types::Type,
    ) -> types::Type {
        let Some(log) = &self.byref_writes else {
            return ty.unsealed_shapes();
        };
        let log = log.borrow();
        let untracked =
            log.any_name_written() || log.types_of(name).iter().any(|written| written.is_mixed());
        if untracked { types::Type::mixed() } else { ty }
    }

    /// Runs `check` over a body's locals, repeating it while the body's
    /// write log still grows, when the body holds a by-reference capture
    /// (`use (&$x)`): a capture's closure starts from every value the body
    /// writes to the variable (`byref_writes`), and the writes its own check
    /// logs are what the next pass starts from. The log starts from the
    /// writes the enclosing body already logged, which a capture sees too.
    /// Every pass starts from `locals` as it was, and the log only grows
    /// across them, so the first
    /// pass that adds no write is the fixed point: its `locals` and
    /// diagnostics are kept, and any earlier pass is rewound the way a loop
    /// pass is. Bounded by `MAX_PASSES`; the last pass is kept when the
    /// writes still grow.
    pub(crate) fn check_body_to_writes_fixpoint<R>(
        &mut self,
        locals: &mut Locals,
        holds_byref_capture: bool,
        mut check: impl FnMut(&mut Self, &mut Locals) -> R,
    ) -> R {
        if !holds_byref_capture {
            return check(self, locals);
        }
        const MAX_PASSES: usize = 4;
        let log = locals.enable_write_log();
        if let Some(outer) = &self.byref_writes
            && !std::rc::Rc::ptr_eq(outer, &log)
        {
            log.borrow_mut().absorb(&outer.borrow());
        }
        let entry = locals.clone();
        let outer_writes = self.byref_writes.replace(std::rc::Rc::clone(&log));
        for pass in 0..MAX_PASSES {
            let mut pass_locals = entry.clone();
            pass_locals.renew_version();
            let before = log.borrow().clone();
            let snapshots = self.current_function_by_ref_exit_snapshots.clone();
            let unannotated = self.current_function_has_unannotated_call;
            let saw_return = self.current_function_saw_explicit_return;
            let mark = self.pass_mark();
            let value = check(self, &mut pass_locals);
            let settled = log.borrow().same_writes(&before);
            if settled || pass + 1 == MAX_PASSES {
                *locals = pass_locals;
                self.byref_writes = outer_writes;
                return value;
            }
            self.rewind_to(mark);
            self.current_function_by_ref_exit_snapshots = snapshots;
            self.current_function_has_unannotated_call = unannotated;
            self.current_function_saw_explicit_return = saw_return;
        }
        unreachable!("the last pass returns")
    }

    /// `converge_loop_passes` with the loop's `counters` widened to their
    /// bands: from the range a counter enters the loop with (or the band
    /// of an enclosing loop it is a counter of too), as far as the steps of
    /// the process's iterations take it, or of the loop's own
    /// `most_iterations` when it has a bound and the counter's steps run
    /// once an iteration, when that stays in the `int` range with a step
    /// to spare.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn converge_loop_body(
        &mut self,
        entry: &Locals,
        declared_return: Option<&Type>,
        label: &str,
        counters: &LoopCounters,
        most_iterations: Option<i64>,
        zero_iterations: LoopZeroIterations<'_>,
        run_pass: impl FnMut(&mut Self, &mut Locals, Option<&Type>, &str, usize),
    ) -> (Locals, Vec<Locals>) {
        let enclosing = self.loop_counter_bands.len();
        let run = self.settings.platform.loop_iterations;
        for name in counters.names() {
            let Some(steps) = counters.get(name) else {
                continue;
            };
            // The band bounds a counter where an iteration begins, which
            // a loop that can run zero times also leaves by, so after as
            // many iterations as it runs; one that can't is left after
            // its last iteration's body, past a beginning one fewer.
            let completed = most_iterations.filter(|_| steps.once).map(|most| {
                match zero_iterations {
                    LoopZeroIterations::Possible { .. } => most,
                    LoopZeroIterations::Impossible => most.saturating_sub(1).max(0),
                }
            });
            let iterations = match (run, completed) {
                (Some(run), Some(most)) => Some(run.min(most)),
                (run, most) => most.or(run),
            };
            let enclosing_band = self.loop_counter_bands[..enclosing]
                .iter()
                .rev()
                .find(|(n, _)| n == name)
                .map(|(_, band)| *band);
            let band = enclosing_band.or_else(|| {
                let ty = entry.get_var(name)?;
                if !ty.is_int().yes() {
                    return None;
                }
                let (Some(lo), Some(hi)) = ty.int_hull()? else {
                    return None;
                };
                crate::checker::loop_counters::counter_band((lo, hi), steps, iterations?)
            });
            if let Some(band) = band {
                self.loop_counter_bands.push((name.to_string(), band));
            }
        }
        let log_start = self.byref_written_names.borrow().len();
        let out = self.converge_loop_passes(
            entry,
            declared_return,
            label,
            CounterScope {
                enclosing,
                log_start,
            },
            zero_iterations,
            run_pass,
        );
        self.loop_counter_bands.truncate(enclosing);
        if self.loop_counter_bands.is_empty() {
            self.byref_written_names.borrow_mut().clear();
        }
        out
    }

    /// This loop's counter bands, keyed in `scope`, less the counters a
    /// call has written by reference since the loop began.
    fn counter_bands_in(
        &self,
        scope: &Locals,
        counters: CounterScope,
    ) -> Vec<(LocalKey, (i64, i64))> {
        let log = self.byref_written_names.borrow();
        let written = &log[counters.log_start.min(log.len())..];
        self.loop_counter_bands[counters.enclosing..]
            .iter()
            .filter(|(name, _)| !written.contains(name))
            .filter_map(|(name, band)| Some((scope.find_var(name)?, *band)))
            .collect()
    }

    fn converge_loop_passes(
        &mut self,
        entry: &Locals,
        declared_return: Option<&Type>,
        label: &str,
        counters: CounterScope,
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
                let bands = self.counter_bands_in(&pass_entry, counters);
                self.widen_towards_entry(&pass_entry, &mut next_entry, &bands);
            }
            if next_entry.settled(&pass_entry) || is_last_allowed_pass {
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
/// Where a loop's counters are: its bands from `enclosing` on in
/// `Checker::loop_counter_bands`, and the by-reference writes logged from
/// `log_start` on.
#[derive(Clone, Copy)]
struct CounterScope {
    enclosing: usize,
    log_start: usize,
}

/// A loop counter's widening: an end that moved goes to its band's end,
/// one that stayed put stays, and no value leaves the band.
fn widen_counter(before: &Type, next: &Type, band: (i64, i64)) -> Type {
    let (Some((Some(lo_before), Some(hi_before))), Some((Some(lo_next), Some(hi_next)))) =
        (before.int_hull(), next.int_hull())
    else {
        return before.widen_towards(next);
    };
    let lo = if lo_next < lo_before {
        band.0
    } else {
        lo_before
    };
    let hi = if hi_next > hi_before {
        band.1
    } else {
        hi_before
    };
    Type::int_range(Some(lo.max(band.0)), Some(hi.min(band.1)))
}

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

/// Whether `key` is a local variable that a scope can lack, so that a scope
/// holding it alone leaves it maybe-defined. A superglobal is defined in every
/// scope, and what one branch wrote to it is not a fact of the other.
fn is_local_variable(scope: &Locals, key: LocalKey) -> bool {
    match scope.data(key) {
        KeyData::Var(name) => scope.is_real_variable(key) && !syntax::ast::is_superglobal(&name),
        _ => false,
    }
}

/// `scope` with each variable it holds as `never` taken from `other` instead:
/// the branch that scope comes from is dead for that variable, so only the
/// other branch says what the variable is at the join. `None` when `scope`
/// holds no such variable.
fn without_dead_variables(scope: &Locals, other: &Locals) -> Option<Locals> {
    let dead: Vec<LocalKey> = scope
        .iter()
        .filter(|(key, ty)| scope.is_real_variable(*key) && ty.is_never())
        .map(|(key, _)| key)
        .collect();
    if dead.is_empty() {
        return None;
    }
    let mut live = scope.clone();
    for key in dead {
        match other.get(key) {
            Some(ty) => {
                let maybe = other.is_maybe_undefined(key);
                live.insert(key, ty.clone());
                if maybe {
                    live.mark_maybe_undefined(key);
                }
            }
            None => {
                live.remove(key);
            }
        }
    }
    Some(live)
}

pub(crate) fn merge_scopes(
    relation: &Relation,
    then_locals: &Locals,
    else_locals: &Locals,
) -> Locals {
    let then_live = without_dead_variables(then_locals, else_locals);
    let else_live = without_dead_variables(else_locals, then_locals);
    let then_locals = then_live.as_ref().unwrap_or(then_locals);
    let else_locals = else_live.as_ref().unwrap_or(else_locals);
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
    merged.share_write_log(then_locals);
    // A reference binding survives the join only where both paths make it;
    // the names one path alone bound are widened once the values are merged.
    let unbound = merged.join_references(then_locals, else_locals);
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
    // A condition alias only survives a merge point when both incoming
    // branches still
    // agree on the exact same stored condition - `Rc::ptr_eq` (not a
    // deep `Expr` comparison) is a correct, cheap test for this: it's
    // exactly true whenever both branches inherited the alias unchanged
    // from before the split (the overwhelmingly common case - the
    // assignment happened before an unrelated `if`/`else`, both forks
    // are still the same `Rc` allocation), and correctly false whenever
    // either branch reassigned the aliased name or the alias's own
    // target in between (`Locals::declare`'s own purge already dropped
    // it on that side, so the key is simply absent there and this filter
    // drops it too) - no case needs comparing two independently-
    // constructed-but-textually-identical conditions from different
    // branches, so no `Expr: PartialEq` bound is needed anywhere.
    *merged.condition_aliases_mut() = then_locals
        .condition_aliases()
        .iter()
        .filter(|(k, (_, rc))| {
            else_locals
                .condition_aliases()
                .get(*k)
                .is_some_and(|(_, other_rc)| std::rc::Rc::ptr_eq(rc, other_rc))
        })
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    // A native type survives only when both branches know one.
    for (key, then_native) in then_locals.natives() {
        match else_locals.native(key) {
            Some(else_native) => {
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
    // separate target to intersect against, just the one stored `Rc<Expr>`.
    *merged.query_builder_aliases_mut() = then_locals
        .query_builder_aliases()
        .iter()
        .filter(|(k, rc)| {
            else_locals
                .query_builder_aliases()
                .get(*k)
                .is_some_and(|other_rc| std::rc::Rc::ptr_eq(rc, other_rc))
        })
        .map(|(k, v)| (*k, v.clone()))
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
        // Only the bindings the two scopes differ in change the copy: a
        // variable only one scope holds stays, as maybe-defined, and a
        // property chain or index only one holds is dropped; one both hold
        // with another type is joined.
        for item in then_locals.types_diff(else_locals) {
            match item {
                imbl::ordmap::DiffItem::Add(name, ty) => {
                    if is_local_variable(&merged, *name) {
                        merged.insert(*name, ty.clone());
                        merged.mark_maybe_undefined(*name);
                    }
                }
                imbl::ordmap::DiffItem::Remove(name, _) => {
                    if is_local_variable(&merged, *name) {
                        merged.mark_maybe_undefined(*name);
                    } else {
                        merged.remove_type(*name);
                    }
                }
                imbl::ordmap::DiffItem::Update { old, new } => {
                    let key = *old.0;
                    let maybe =
                        then_locals.is_maybe_undefined(key) || else_locals.is_maybe_undefined(key);
                    merged.insert(key, relation.union(old.1.clone(), new.1.clone()));
                    if maybe {
                        merged.mark_maybe_undefined(key);
                    }
                }
            }
        }
        // Nor is one whose types are equal and whose data-flow sets are
        // not: what the else-scope's value came from joins the copy's.
        for (key, else_ty) in else_locals.iter() {
            if !else_ty.has_flow() {
                continue;
            }
            if let Some(then_ty) = then_locals.get(key)
                && *then_ty == *else_ty
                && !then_ty.deep_flow().contains_all(&else_ty.deep_flow())
            {
                let maybe = merged.is_maybe_undefined(key);
                merged.insert(key, relation.union(then_ty.clone(), else_ty.clone()));
                if maybe {
                    merged.mark_maybe_undefined(key);
                }
            }
        }
        // A variable both scopes hold with the same type is not in the diff,
        // so one the else-scope holds as maybe-defined is marked here.
        for key in else_locals.maybe_undefined_keys() {
            merged.mark_maybe_undefined(*key);
        }
        return merged;
    }
    for (name, then_ty) in then_locals.iter() {
        let mut provenance = then_locals
            .get_provenance(name)
            .cloned()
            .unwrap_or_default();
        let merged_ty = if let Some(else_ty) = else_locals.get(name) {
            if let Some(else_provenance) = else_locals.get_provenance(name) {
                provenance.extend(else_provenance.iter().copied());
            }
            // `Type::union` takes its operands by value, so equal types
            // (a variable neither branch reassigned, the common case) are
            // kept without cloning both.
            if then_ty == else_ty && then_ty.deep_flow() == else_ty.deep_flow() {
                then_ty.clone()
            } else {
                relation.union(then_ty.clone(), else_ty.clone())
            }
        } else if is_local_variable(then_locals, name) {
            then_ty.clone()
        } else {
            continue;
        };
        let maybe = !else_locals.contains_key(name)
            || then_locals.is_maybe_undefined(name)
            || else_locals.is_maybe_undefined(name);
        merged.insert_with_provenance(name, merged_ty, provenance);
        if maybe {
            merged.mark_maybe_undefined(name);
        }
    }
    for (name, else_ty) in else_locals.iter() {
        if then_locals.contains_key(name) || !is_local_variable(else_locals, name) {
            continue;
        }
        let provenance = else_locals
            .get_provenance(name)
            .cloned()
            .unwrap_or_default();
        merged.insert_with_provenance(name, else_ty.clone(), provenance);
        merged.mark_maybe_undefined(name);
    }
    for key in unbound {
        if merged.contains_key(key) {
            merged.insert(key, types::Type::mixed());
        }
    }
    merged
}

/// Whether a body holds a by-reference capture: a closure in it, outside
/// any nested function or closure body, has a `use (&$x)`.
pub(crate) fn body_has_byref_capture(stmts: &[syntax::ast::Stmt]) -> bool {
    syntax::visit::any_expr_in_body(stmts, |e| match &e.kind {
        syntax::ast::ExprKind::Closure(closure) => closure.uses.iter().any(|u| u.by_ref),
        _ => false,
    })
}

impl Checker<'_> {
    /// Whether `stmts` may run code a by-reference capture could share: a call
    /// other than one of a builtin every parameter of which takes a scalar by
    /// value (`strlen`), and a method call, a static call or a `new`.
    pub(crate) fn body_may_run_code(&self, stmts: &[syntax::ast::Stmt]) -> bool {
        syntax::visit::any_expr_in_body(stmts, |e| match &e.kind {
            syntax::ast::ExprKind::Call(callee, _) => match &callee.kind {
                syntax::ast::ExprKind::Name(name) => !self.is_scalar_builtin(name),
                _ => true,
            },
            syntax::ast::ExprKind::MethodCall(..)
            | syntax::ast::ExprKind::DynamicMethodCall(..)
            | syntax::ast::ExprKind::StaticCall(..)
            | syntax::ast::ExprKind::DynamicStaticCall(..)
            | syntax::ast::ExprKind::New(..)
            | syntax::ast::ExprKind::NewAnonymousClass(..) => true,
            _ => false,
        })
    }

    fn is_scalar_builtin(&self, name: &str) -> bool {
        let Some((_, sig)) = self.registry.lookup_function_with_name(name) else {
            return false;
        };
        if !self.registry.is_builtin_function(name) {
            return false;
        }
        // Functions that write variables by name, or take a reference.
        let bare = name
            .rsplit('\\')
            .next()
            .unwrap_or(name)
            .to_ascii_lowercase();
        if matches!(
            bare.as_str(),
            "extract"
                | "compact"
                | "get_defined_vars"
                | "parse_str"
                | "func_get_args"
                | "func_get_arg"
        ) {
            return false;
        }
        sig.params().iter().all(|param| {
            !param.by_ref()
                && param.ty().union_members().iter().all(|member| {
                    member.is_string().yes()
                        || member.is_int().yes()
                        || member.is_float().yes()
                        || member.is_bool().yes()
                        || member.is_null().yes()
                })
        })
    }
}
