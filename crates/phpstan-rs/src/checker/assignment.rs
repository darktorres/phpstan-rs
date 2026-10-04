use crate::checker::keys::{KeyData, LocalKey};
use crate::checker::{Checker, Locals, purge_stale_narrowing_for, purge_stale_narrowing_for_var};
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use types::{Shape, ShapeKey, Type};

/// Real PHPStan's `NullsafeCheck::containsNullSafe` - whether an
/// assignment target's own access chain contains a nullsafe `?->`
/// anywhere along it (`$a?->b = 1`, or nested deeper: `$a?->b->c = 1`).
/// Doesn't see through `DynamicPropertyAccess`/`DynamicMethodCall`
/// (`$a?->{$expr}`) - that shape doesn't carry its own nullsafe flag at
/// all (see `ExprKind::DynamicPropertyAccess`'s own doc comment), a
/// real, disclosed gap matching how rare it is in practice.
pub(crate) fn contains_nullsafe(expr: &Expr) -> bool {
    syntax::stack::guard(|| match &expr.peeled().kind {
        ExprKind::PropertyAccess(base, _, nullsafe) => {
            nullsafe.is_nullsafe() || contains_nullsafe(base)
        }
        ExprKind::MethodCall(base, _, _, nullsafe) => {
            nullsafe.is_nullsafe() || contains_nullsafe(base)
        }
        ExprKind::Index(base, _) => contains_nullsafe(base),
        ExprKind::StaticCall(StaticTarget::Expr(base), _, _)
        | ExprKind::DynamicStaticCall(StaticTarget::Expr(base), _, _)
        | ExprKind::StaticPropertyFetch(StaticTarget::Expr(base), _)
        | ExprKind::DynamicStaticPropertyFetch(StaticTarget::Expr(base), _) => {
            contains_nullsafe(base)
        }
        ExprKind::Destructure(items, _) => items.iter().any(|item| {
            item.key.as_ref().is_some_and(contains_nullsafe)
                || item.target.as_ref().is_some_and(contains_nullsafe)
        }),
        _ => false,
    })
}

/// Real PHPStan's `InvalidAssignVarRule::containsNonAssignableExpression`,
/// i.e. whether an assignment's own left-hand side is a real PHP lvalue
/// (`assign.invalidExpr` otherwise: `foo() = 1;`/`1 = 2;`, a real
/// unconditional fatal). Unlike real PHPStan's own AST (where a computed
/// property name is still just `PropertyFetch`, never a distinct node
/// type), this project's `DynamicPropertyAccess` (`$obj->{$expr} = 1;`)
/// needs its own explicit "assignable" arm to match that same semantics,
/// while `DynamicMethodCall`/`MethodCall` deliberately don't get one,
/// since neither is ever a valid assignment target in real PHP either.
pub(crate) fn is_non_assignable_target(expr: &Expr) -> bool {
    syntax::stack::guard(|| match &expr.peeled().kind {
        ExprKind::Variable(_)
        | ExprKind::PropertyAccess(..)
        | ExprKind::DynamicPropertyAccess(..)
        | ExprKind::Index(..)
        | ExprKind::ClassConstFetch(..)
        | ExprKind::ClassNameFetch(_)
        | ExprKind::StaticPropertyFetch(..)
        | ExprKind::DynamicStaticPropertyFetch(..) => false,
        ExprKind::Destructure(items, _) => items
            .iter()
            .any(|item| item.target.as_ref().is_some_and(is_non_assignable_target)),
        _ => true,
    })
}

impl<'a> Checker<'a> {
    /// Real PHPStan's `InvalidAssignVarRule`'s own nullsafe-assignment-
    /// target half (`nullsafe.assign`) - `$a?->b = 1;` is a real,
    /// unconditional PHP fatal ("Cannot assign to nullsafe operator"),
    /// not a narrowing question, so it's checked unconditionally rather
    /// than folded into `check_assignment_target`'s otherwise type-
    /// driven arms. The rule's other two halves are
    /// `check_nullsafe_byref_target` (nullsafe on an assign-by-ref's own
    /// RHS) and `check_assignable_target` (a non-assignable LHS),
    /// both just below.
    pub(crate) fn check_nullsafe_assignment_target(&mut self, target: &Expr) {
        if !contains_nullsafe(target) {
            return;
        }
        self.report_builder(
            DiagnosticBuilder::new(
                "Nullsafe operator cannot be on left side of assignment.".to_string(),
                target.span,
                0,
                "nullsafe.assign",
            )
            .non_ignorable(),
        );
    }

    /// Real PHPStan's `InvalidAssignVarRule`'s own nullsafe-by-ref-RHS
    /// half (`nullsafe.byRef`) - `$a = &$b?->c;` is a real, unconditional
    /// PHP fatal ("Cannot assign by reference to an overloaded object"/
    /// nullsafe-specific variant), the RHS-position mirror of
    /// `check_nullsafe_assignment_target` just above. Only ever called
    /// for `ExprKind::AssignRef`'s own RHS - a plain `Assign`'s RHS is an
    /// ordinary read, never checked this way.
    pub(crate) fn check_nullsafe_byref_target(&mut self, target: &Expr) {
        if !contains_nullsafe(target) {
            return;
        }
        self.report_builder(
            DiagnosticBuilder::new(
                "Nullsafe operator cannot be on right side of assignment by reference.".to_string(),
                target.span,
                0,
                "nullsafe.byRef",
            )
            .non_ignorable(),
        );
    }

    /// Real PHPStan's `InvalidAssignVarRule`'s own non-assignable-
    /// expression half (`assign.invalidExpr`) - unlike a real lvalue-
    /// grammar-enforcing parser, this project's own `parse_assignment_
    /// tail` accepts any expression as an assignment's left-hand side
    /// (needed for the guard-clause idiom documented on `parse_
    /// assignment_tail` itself), so `foo() = 1;`/`1 = 2;` genuinely
    /// reach here rather than already being a parse error.
    pub(crate) fn check_assignable_target(&mut self, target: &Expr) {
        if !is_non_assignable_target(target) {
            return;
        }
        self.report_builder(
            DiagnosticBuilder::new(
                "Expression on left side of assignment is not assignable.".to_string(),
                target.span,
                0,
                "assign.invalidExpr",
            )
            .non_ignorable(),
        );
    }

    /// The actual variable-binding side effect of an assignment - lives in
    /// `check_expr` (called from every place an assignment expression can
    /// appear: a bare expression statement, a `for` init/step, a `while`
    /// condition, ...) rather than a separate pass triggered only for
    /// top-level expression statements, so `for ($j = 0; ...)`'s `$j`
    /// gets bound.
    /// Checks an assignment's own left-hand side for whatever isn't a
    /// pure "definition site" - a plain variable (or nested variable,
    /// recursing through any level of `[$a, $b] = ...`-style
    /// destructuring) is never a read, but an `Index`'s own key
    /// expression and a `PropertyAccess`'s own base still are. Shared
    /// between the top-level assignment target and every nested
    /// destructuring element, so `[[$a, $b], $c] = $nested;` doesn't
    /// wrongly check `$a`/`$b` as reads of not-yet-defined variables.
    pub(crate) fn check_assignment_target(
        &mut self,
        target: &Expr,
        locals: &mut Locals,
        label: &str,
    ) {
        syntax::stack::guard(|| {
            match &target.kind {
                ExprKind::Variable(_) => {}
                ExprKind::Destructure(items, _) => {
                    for item in items {
                        if let Some(key) = &item.key {
                            self.check_expr(key, locals, label);
                        }
                        if let Some(target) = &item.target {
                            self.check_assignment_target(target, locals, label);
                        }
                    }
                }
                // A property being *assigned* isn't a read of that property -
                // only its base is (`$obj->prop = ...` reads $obj). Routing
                // the full PropertyAccess through check_expr here would
                // wrongly flag legitimate dynamic-property creation as
                // "undefined property".
                ExprKind::PropertyAccess(base, _, _) => {
                    self.check_expr(base, locals, label);
                }
                // Same reasoning as PropertyAccess just above, for the exact
                // same reason: `$x['newKey'] = ...` defines that key, it
                // doesn't read it - routing the full Index expression through
                // check_expr's own Index arm would call check_offset_existence
                // and wrongly flag the ordinary "start from an array literal,
                // build it up one key at a time" idiom as accessing a
                // nonexistent key of an already-fully-known shape (Finding
                // 48 made this a live, common-case bug: an array literal's
                // own inferred shape is only what's known so far, not a
                // sealed contract the array can never grow past).
                //
                // A nested write (`$piwigoInfos['updates'][] = [...]`,
                // target itself an `Index` whose base is another `Index`)
                // must recurse through `check_assignment_target` for that
                // inner base too, not `check_expr`: `$piwigoInfos['updates']`
                // is itself part of this same write's path, not a value
                // being read, so `check_offset_existence` must not run on it
                // either. Generalizes over any nesting depth, the same way
                // `bind_assignment_target`'s `Index` arm does for the write
                // side.
                ExprKind::Index(base, idx) => {
                    match &base.kind {
                        ExprKind::Index(..) => self.check_assignment_target(base, locals, label),
                        // Same reasoning as the top-level `Variable` arm
                        // above, one level down: `$x[] = ...`/`$x['key'] =
                        // ...` as `$x`'s very first use is real PHP's silent
                        // array auto-vivification, not a read of an
                        // already-defined `$x` - routing it through
                        // `check_expr` here would wrongly flag it as
                        // `variable.undefined`. Whether `$x` already holds a
                        // non-array-compatible type is a separate,
                        // not-yet-checked question, matching this project's
                        // "don't guess" leniency for the write side
                        // (`bind_assignment_target`'s identical `Index` arm).
                        ExprKind::Variable(_) => {}
                        _ => {
                            self.check_expr(base, locals, label);
                        }
                    }
                    if let Some(i) = idx {
                        self.check_expr(i, locals, label);
                        // Unlike `check_offset_existence` (deliberately
                        // skipped on the write side just above - a *missing*
                        // shape key isn't a bug on write, PHP auto-vivifies
                        // it), a key's own *illegal type* is exactly as real
                        // a `TypeError` on a write as a read - `$arr[new
                        // Foo()] = 1` fails before the array ever grows.
                        self.check_dim_fetch_key_type(base, i, locals);
                    }
                }
                _ => {
                    self.check_expr(target, locals, label);
                }
            }
        })
    }

    /// Sibling recursion to `bind_assignment_target`'s own `Destructure`
    /// arm below - a by-ref parameter destructured out of an array
    /// (`[$byRef, $b] = $arr;`/`list($byRef) = $arr;`, any nesting depth)
    /// needs the same `parameterByRef.type` check a bare `$byRef = ...`
    /// assignment already gets from `check_param_out_assignment` - a
    /// no-op for anything that isn't a destructuring target at all (a
    /// bare `Variable`/`PropertyAccess`/`Index` lhs is already fully
    /// covered by the direct `check_param_out_assignment` call at this
    /// method's own call site). Mirrors `bind_assignment_target`'s
    /// per-position/per-key element-type resolution exactly, so a
    /// destructured array shape source still checks each element
    /// against its own real per-slot type, not a blanket `Mixed`.
    pub(crate) fn check_by_ref_destructuring(&mut self, lhs: &Expr, ty: &Type) {
        syntax::stack::guard(|| {
            let ExprKind::Destructure(items, _) = &lhs.kind else {
                return;
            };
            for (position, item) in items.iter().enumerate() {
                let Some(target) = &item.target else {
                    continue;
                };
                let element_ty = ty
                    .offset_value_type(destructured_key(item, position).as_ref())
                    .unwrap_or(Type::mixed());
                self.check_param_out_assignment(target, &element_ty);
                self.check_by_ref_destructuring(target, &element_ty);
            }
        })
    }

    /// What a property reads as after `lhs = value`. A hooked property
    /// reads as its declared type, as its hooks decide what is stored and
    /// read. Otherwise `value` when it fits the property's native type, or
    /// the declared type, as PHP either coerces the value into it or
    /// rejects the write with a `TypeError`. A write that isn't to a known
    /// property is returned as is.
    fn property_value_after_write(&self, lhs: &Expr, value: &Type, locals: &Locals) -> Type {
        let ExprKind::PropertyAccess(base, prop, _) = &lhs.kind else {
            return value.clone();
        };
        let base_ty = self.infer_expr(base, locals);
        let Some((_, sig)) = self.find_property_owner(&base_ty, prop) else {
            return value.clone();
        };
        if sig.has_get_hook() || sig.has_set_hook() {
            return sig.ty().clone();
        }
        if !sig.has_native_type()
            || self
                .registry
                .relation()
                .is_super_type_of(sig.ty_native(), value)
                .yes()
        {
            return value.clone();
        }
        sig.ty().clone()
    }

    /// Binds an assignment's (or a `foreach` value's) left-hand side to
    /// `ty`, the type of whatever was just assigned/iterated, recursing
    /// into a destructuring target (`[$a, $b] = $arr;`, `list($a, $b) =
    /// $arr;`, nested `[[$a, $b], $c] = $nested;`): each element target
    /// gets the value at its key.
    pub(crate) fn bind_assignment_target(&self, lhs: &Expr, ty: Type, locals: &mut Locals) {
        syntax::stack::guard(|| {
            // Keep any existing `key_of` entry for this exact
            // property-chain/literal-index-chain expression in sync with a
            // real assignment - `apply_narrowing`'s synthetic key format is
            // a separate slot in the same `Locals` map (see `key_of`'s
            // doc comment): without this, reassigning a property/element a
            // prior condition already narrowed would leave the old narrowed
            // value sitting under that key forever (`if ($this->entries ===
            // null) { $this->entries = [...]; } return $this->entries;`,
            // where `infer_expr`'s "check narrowed key first" fast path
            // would read back the stale `null` instead of the newly-assigned
            // array). Redundant with (but harmless alongside) the `Variable`
            // arm below, which does the same insert under the identical key
            // for that one case.
            purge_aliased_index_entries(lhs, locals);
            let own_entry = locals.key_of(lhs).map(|key| {
                // Also drops any deeper stale entry rooted at this exact
                // key (`"key->prop"`, `"key[lit]"`) or that used `key` as a
                // variable subscript elsewhere (`"other[$key]"`) - real
                // invalidation-on-reassignment, generalized over any
                // `key_of` shape (bare variable, property chain, or
                // index chain), not just a bare variable.
                purge_stale_narrowing_for(locals, key);
                let stored = self.property_value_after_write(lhs, &ty, locals);
                locals.insert(key, stored.clone());
                (key, stored)
            });
            match &lhs.kind {
                // The one real "this variable is now bound to a fresh
                // value" site in this whole function - a plain assignment
                // (`$x = ...`) or, recursing, each leaf of a list-
                // destructuring target (`[$a, $b] = ...`) - `lhs.span` is
                // already exactly the right span either way, since this
                // recursion re-enters with `item.value` (each destructuring
                // leaf's own real `Expr`, own real span) for the latter
                // case.
                ExprKind::Variable(name) => {
                    // See `Checker::probe_hit_at`'s doc comment.
                    self.probe_hit_at(lhs.span, &ty);
                    locals.declare_var(name, ty, lhs.span);
                }
                // Each element takes the value read at its key, as a read
                // of it would: a keyed element's literal key, a positional
                // one's index.
                ExprKind::Destructure(items, _) => {
                    for (position, item) in items.iter().enumerate() {
                        let Some(target) = &item.target else {
                            continue;
                        };
                        let key_ty = match (destructured_key(item, position), &item.key) {
                            (Some(key), _) => key.to_type(),
                            (None, Some(key)) => self.infer_expr(key, locals).to_array_key(),
                            (None, None) => Type::unknown_array_key(),
                        };
                        let element_ty = self.offset_read_type(&ty, &key_ty);
                        self.bind_assignment_target(target, element_ty, locals);
                    }
                }
                // `$x['newKey'] = $value;` adds a real key to `$x`'s
                // tracked shape when `$x` already holds a known one (most
                // commonly, from an earlier `$x = []` or `$x = ['a' => 1]`
                // literal) - without this, an array literal's new precision
                // would make the common "start from a literal, build it up
                // one key at a time" idiom look like every later read was
                // accessing a key the shape could never have. Only updates a
                // known shape already sitting in `locals` under a bare
                // variable name; anything else (a property chain, a variable
                // with a not-yet-known type) is left alone.
                //
                // A nested write (`$items[$day]['derivative'] = $derivative;` -
                // `base` is itself an `Index`, not a bare `Variable`) is
                // handled by recursing: compute `base`'s current type, work
                // out its new type as if `base[idx] = ty` had just happened
                // to it, then bind that back onto `base` - which lands in
                // this same arm again if `base` itself is another `Index`,
                // correctly handling any nesting depth.
                ExprKind::Index(base, idx) => {
                    // Computed once here (not inside `write_index_into`
                    // itself, a free function with no access to `self`/
                    // `locals`) so a dynamic key already narrowed by a
                    // preceding `is_int()`/`is_string()`-family guard (e.g.
                    // `if (is_int($row['id'])) { $arr[$row['id']] = ... }`)
                    // can pick a definite `int`/`string` key type instead of
                    // always falling back to `unknown_array_key()`.
                    let idx_ty = idx.as_deref().map(|i| self.infer_expr(i, locals));
                    match &base.kind {
                        ExprKind::Variable(name) => {
                            // A write through an as-yet-undefined variable
                            // (`$x[] = 1;`/`$x['key'] = 1;` as `$x`'s very
                            // first use) is real PHP's array auto-
                            // vivification - silently creates a fresh `[]`
                            // and writes into it, no warning at any error-
                            // reporting level. Starting `write_index_into`
                            // from a fresh empty `ArrayShape` reuses its
                            // existing "a real append onto a provably
                            // list-like container (an empty shape included)
                            // stays a real, now-non-empty list" rule, so
                            // `$x[] = 1;` alone correctly infers
                            // `non-empty-list<int>`, not just `array`.
                            //
                            // A superglobal (`$_SERVER['PHP_SELF'] = ...;`)
                            // must NOT go through this same fresh-empty-shape
                            // path even though it's also typically absent
                            // from `locals` - unlike an ordinary local, a
                            // superglobal already has a real, much wider
                            // shape PHP populates before the script ever
                            // runs, so treating this write as if it were the
                            // array's first key would wrongly narrow it down
                            // to only the key(s) a test happens to write
                            // locally. `unassigned_variable_type` already
                            // exists for exactly this "read before any real
                            // assignment appears in locals" case - reused
                            // verbatim rather than re-deriving it.
                            //
                            // Was `name` genuinely undefined before this
                            // write? A superglobal never really is (PHP
                            // populates it before the script runs, even
                            // though it's typically absent from `locals`
                            // too) - only a real local's first `$x[...] = ...`
                            // write is this variable's true declaration site.
                            let was_defined = locals.contains_var(name);
                            let current = locals.get_var(name).cloned().unwrap_or_else(|| {
                                if is_superglobal(name) {
                                    unassigned_variable_type(name)
                                } else {
                                    Type::array_shape(Shape::sealed(Vec::new()))
                                }
                            });
                            // `Checker::in_range_index_writes`'s own doc
                            // comment - a write whose index is syntactically
                            // the exact loop variable this exact container's
                            // own `for (...; $i < count($container); ...)`
                            // bound is scoped to.
                            let in_range = idx
                                .as_deref()
                                .and_then(|i| match &i.kind {
                                    ExprKind::Variable(idx_name) => Some(idx_name.as_ref()),
                                    _ => None,
                                })
                                .is_some_and(|idx_name| {
                                    self.in_range_index_writes
                                        .iter()
                                        .any(|(c, i)| c.as_str() == name.as_ref() && i == idx_name)
                                });
                            // `Checker::active_discriminant`'s own doc comment
                            // - a real append (`idx.is_none()`) onto a bare
                            // variable, checked while inside a recognized
                            // `if`/`else` branch, also grows that branch's
                            // own separately-tracked accumulator - computed
                            // from `discriminant_array_facts`'s own prior
                            // value for this exact `(container, subject,
                            // value)` slot (an empty shape the first time),
                            // *not* from `current` (which already carries the
                            // *other* branch's own history too, the exact
                            // cross-contamination this side-channel exists to
                            // avoid). Recorded *alongside*, never instead of,
                            // the ordinary write just below.
                            if idx.is_none()
                                && let Some((subject, value)) =
                                    self.active_discriminant.last().cloned()
                            {
                                let container = locals.var_key(name);
                                let key = (container, subject, value);
                                let prior = locals
                                    .discriminant_array_facts()
                                    .get(&key)
                                    .cloned()
                                    .unwrap_or_else(|| {
                                        Type::array_shape(Shape::sealed(Vec::new()))
                                    });
                                if let Some(new_slot_ty) = prior.set_offset_value_type(
                                    None,
                                    ty.clone(),
                                    false,
                                    self.loop_depth == 0,
                                ) {
                                    locals
                                        .discriminant_array_facts_mut()
                                        .insert(key, new_slot_ty);
                                }
                            }
                            // A dynamic-key write whose index *expression* is
                            // already known, by its own inferred type, to be
                            // one or more specific literal keys (`$key =
                            // 'id'; $row[$key] = ...;`, or a `foreach` over a
                            // small literal array of key names) is just as
                            // resolvable per-field as a literal AST token is -
                            // `key_of(lhs)` at this function's own top
                            // only recognizes the latter (producing this
                            // write's own `"row[$key]"` dollar-prefixed
                            // "current value" marker instead). Each resolved
                            // literal key's own `key_of`-format entry
                            // is updated directly here, unioned with whatever
                            // the read side already had (a `foreach` over
                            // several literal keys hits this once per key,
                            // each write needing to survive the others' own
                            // updates) - real corpus shape:
                            // `CategoryAvailableListController`'s own
                            // `foreach (['id', ...] as $key) { $row[$key] =
                            // ...; }`. Only fires for a genuinely non-literal
                            // index expression - a literal one (`$row['id']
                            // = ...`) is already covered by the top-of-
                            // function `key_of` block above.
                            if let Some(i) = idx.as_deref()
                                && literal_string_key(i).is_none()
                                && let Some(idx_ty) = idx_ty.as_ref()
                            {
                                for literal_key in literal_keys_from_type(idx_ty) {
                                    let container = locals.var_key(name);
                                    let element = locals.intern(KeyData::Index {
                                        base: container,
                                        key: literal_key.as_str().into(),
                                    });
                                    let merged = match locals.get(element) {
                                        Some(prior) => {
                                            self.relation().union(prior.clone(), ty.clone())
                                        }
                                        None => ty.clone(),
                                    };
                                    locals.insert(element, merged);
                                }
                            }
                            // A string takes the write as a string does.
                            let written = if current.is_string().yes() {
                                Some(current.string_offset_write(
                                    idx_ty.as_ref(),
                                    &ty,
                                    self.negative_string_offsets(),
                                ))
                            } else {
                                current.set_offset_value_type(
                                    idx_ty.as_ref(),
                                    ty,
                                    in_range,
                                    self.loop_depth == 0,
                                )
                            };
                            if let Some(new_ty) = written {
                                if was_defined || is_superglobal(name) {
                                    locals.insert_var(name, new_ty);
                                    if let Some(native) = locals.native_var(name) {
                                        let native = native_after_offset_change(native, true);
                                        locals.set_native_var(name, native);
                                    }
                                } else {
                                    locals.declare_var(name, new_ty, base.span);
                                }
                            }
                        }
                        ExprKind::Index(..) => {
                            let current_base_ty = self.nested_write_container_type(base, locals);
                            // A nested write's own container isn't a bare
                            // variable name `in_range_index_writes` could
                            // ever match against - out of scope for this
                            // narrow mechanism.
                            if let Some(new_base_ty) = current_base_ty.set_offset_value_type(
                                idx_ty.as_ref(),
                                ty,
                                false,
                                self.loop_depth == 0,
                            ) {
                                self.bind_assignment_target(base, new_base_ty, locals);
                            }
                        }
                        // A write through a narrowed property (`$this->p = [];
                        // ... $this->p[$k] = $v;`) updates that narrowed type,
                        // or it would stay the stale pre-write one.
                        _ => {
                            if let Some(key) = locals.key_of(base)
                                && let Some(current) = locals.get(key).cloned()
                                && let Some(new_ty) = current.set_offset_value_type(
                                    idx_ty.as_ref(),
                                    ty,
                                    false,
                                    self.loop_depth == 0,
                                )
                            {
                                locals.insert(key, new_ty);
                            }
                        }
                    }
                }
                _ => {}
            }
            // Rebinding an element's base forgets what depends on the base,
            // this very element among them - which is exactly what was just
            // written.
            if matches!(lhs.kind, ExprKind::Index(..))
                && let Some((key, stored)) = own_entry
            {
                locals.insert(key, stored);
            }
        })
    }

    /// `unset(target)`: a variable leaves scope, an offset leaves its
    /// array, and a property's narrowed value is forgotten. The target is
    /// read as quietly as `isset()` reads it - unsetting what doesn't
    /// exist is no error.
    pub(crate) fn check_unset_target(&mut self, target: &Expr, locals: &mut Locals, label: &str) {
        self.check_expr_quiet_root(target, locals, label);
        // `unset($o[$k])` calls `offsetUnset`; a statement has no
        // expression of its own to throw from, so its throw goes to the
        // enclosing `try` here.
        self.implicit_array_access_probe(target, &["offsetUnset"], target.span, locals);
        if let Some(Some(thrown)) = self.call_throw_types.remove(&target.span) {
            self.record_throw_state(thrown, locals);
        }
        if let Some(key) = locals.key_of(target) {
            purge_stale_narrowing_for(locals, key);
            locals.remove(key);
        }
        purge_aliased_index_entries(target, locals);
        let ExprKind::Index(base, Some(idx)) = &target.kind else {
            return;
        };
        if let ExprKind::Variable(name) = &base.kind
            && !locals.contains_var(name)
        {
            return;
        }
        let idx_ty = self.infer_expr(idx, locals);
        let base_ty = self.infer_expr(base, locals);
        let after = base_ty.unset_offset(&idx_ty);
        if after == base_ty {
            return;
        }
        match &base.kind {
            // Narrowed siblings (`$a['y']`) and every-element facts still
            // hold with one element gone; facts about the whole array
            // don't.
            ExprKind::Variable(name) => {
                let variable = locals.var_key(name);
                locals.purge_branch_implied_facts_depending_on(variable);
                locals
                    .discriminant_array_facts_mut()
                    .retain(|(container, _, _), _| *container != variable);
                locals.insert_var(name, after);
                if let Some(native) = locals.native_var(name) {
                    let native = native_after_offset_change(native, false);
                    locals.set_native_var(name, native);
                }
            }
            _ => self.bind_assignment_target(base, after, locals),
        }
    }

    /// The current type of a nested write's intermediate container
    /// (`$x['a']` in `$x['a']['b'] = 1`), at any depth: where the offset
    /// may be absent or holds `null`, PHP creates a fresh array, so the
    /// container includes `array{}` there.
    fn nested_write_container_type(&self, e: &Expr, locals: &Locals) -> Type {
        syntax::stack::guard(|| {
            let ExprKind::Index(inner, idx) = &e.kind else {
                return self.infer_expr(e, locals);
            };
            let inner_ty = match &inner.kind {
                ExprKind::Index(..) => self.nested_write_container_type(inner, locals),
                _ => self.infer_expr(inner, locals),
            };
            // An element narrowed by an earlier write or check is present,
            // with that type.
            if let Some(key) = locals.find_key(e)
                && let Some(narrowed) = locals.get(key)
            {
                return narrowed.clone();
            }
            let idx_ty = idx.as_deref().map(|i| self.infer_expr(i, locals));
            inner_ty
                .nested_write_container(idx_ty.as_ref())
                .unwrap_or_else(|| self.infer_expr(e, locals))
        })
    }

    pub(crate) fn bind_simple_target(&self, target: &Expr, ty: Type, locals: &mut Locals) {
        if let ExprKind::Variable(name) = &target.kind {
            purge_stale_narrowing_for_var(locals, name);
            // Real rebind - `target` is a `foreach` key's bare `$var`
            // expression (its only caller), a real declaration site.
            self.probe_hit_at(target.span, &ty);
            locals.declare_var(name, ty, target.span);
        }
    }

    /// `NonexistentOffsetInArrayDimFetchCheck`'s definite half
    /// (`offsetAccess.notFound`): a read of an offset `$x`'s type proves
    /// can never exist - a literal key missing from a known sealed shape
    /// (a union of shapes only when every member lacks it; an optional
    /// field counts as present), or a literal key whose kind a general
    /// array's key type rules out (a
    /// non-numeric string on int keys or a list, an int on string keys,
    /// a negative int on a list).
    ///
    /// `read` is the whole read (`$x['k']`): an element an earlier check
    /// narrowed to a type without `null` is there.
    pub(crate) fn check_offset_existence(
        &mut self,
        read: &Expr,
        base: &Expr,
        idx: &Expr,
        locals: &Locals,
    ) {
        if locals.may_have_member_narrowing()
            && let Some(key) = locals.find_key(read)
            && locals
                .get(key)
                .is_some_and(|narrowed| narrowed.is_null().no())
        {
            return;
        }
        let container = self.infer_expr(base, locals);
        let dim = self.infer_expr(idx, locals);
        self.report_missing_offset(&container, &dim, idx.span);
    }

    /// `ArrayDestructuringRule`'s per-key half: every destructured key
    /// (a position, or an explicit key) checked against the destructured
    /// value, recursing into a nested destructuring target.
    pub(crate) fn check_destructuring_offsets(
        &mut self,
        items: &[DestructureItem],
        container: &Type,
        locals: &Locals,
    ) {
        syntax::stack::guard(|| {
            for (position, item) in items.iter().enumerate() {
                let Some(target) = &item.target else {
                    continue;
                };
                let (dim, span) = match &item.key {
                    Some(k) => (self.infer_expr(k, locals), k.span),
                    None => (Type::int_literal(position as i64), target.span),
                };
                if self.report_missing_offset(container, &dim, span) {
                    continue;
                }
                if let ExprKind::Destructure(inner, _) = &target.kind {
                    let inner_container = container
                        .offset_value_type_at(&dim.to_array_key())
                        .unwrap_or(Type::mixed());
                    self.check_destructuring_offsets(inner, &inner_container, locals);
                }
            }
        })
    }

    /// Reports `offsetAccess.notFound` when `container` provably lacks
    /// `dim`, and `offsetAccess.mightNotExist` (level 7) when its array
    /// members say `dim` may be missing: an optional field, or a member
    /// without it. A general array's (or a list's) key is taken as
    /// present unless
    /// `rules.report_possibly_nonexistent_general_array_offset` is on.
    /// Returns whether it reported.
    pub(crate) fn report_missing_offset(
        &mut self,
        container: &Type,
        dim: &Type,
        span: syntax::span::Span,
    ) -> bool {
        if container.has_offset_value_type(dim).no() {
            self.report(
                format!("Offset {dim} does not exist on {container}."),
                span,
                3,
                "offsetAccess.notFound",
            );
            return true;
        }
        let arrays: Vec<Type> = container
            .union_members()
            .iter()
            .filter(|m| m.is_array().yes())
            .cloned()
            .collect();
        let strings: Vec<Type> = container
            .union_members()
            .iter()
            .filter(|m| m.is_string().yes())
            .cloned()
            .collect();
        let string_presence = (!strings.is_empty()).then(|| {
            self.relation()
                .union_of(strings)
                .string_offset_read(dim, self.negative_string_offsets())
                .1
        });
        if string_presence == Some(types::OffsetPresence::Absent) && arrays.is_empty() {
            self.report(
                format!("Offset {dim} does not exist on {container}."),
                span,
                3,
                "offsetAccess.notFound",
            );
            return true;
        }
        let array_presence = self
            .relation()
            .union_of(arrays)
            .array_offset_read(dim)
            .map(|(_, p)| p);
        let presence = match (array_presence, string_presence) {
            (Some(a), Some(s)) => a.join(s),
            (Some(p), None) | (None, Some(p)) => p,
            (None, None) => return false,
        };
        let might_not_exist = match presence {
            types::OffsetPresence::MayBeAbsent | types::OffsetPresence::Absent => true,
            types::OffsetPresence::Unknown => {
                self.settings
                    .rules
                    .report_possibly_nonexistent_general_array_offset
            }
            types::OffsetPresence::Present => false,
        };
        if !might_not_exist {
            return false;
        }
        self.report(
            format!("Offset {dim} might not exist on {container}."),
            span,
            7,
            "offsetAccess.mightNotExist",
        );
        true
    }

    /// Flags `$x[...]` (a read) when `$x`'s type is confirmed to never be
    /// offset-accessible at all (`offsetAccess.nonOffsetAccessible`) -
    /// mirrors real PHPStan's `Type::isOffsetAccessible()`. **Read-only**,
    /// mirroring `check_offset_existence`'s own explicit write-side
    /// exclusion: `$x = null; $x['a'] = 1;` legally auto-vivifies `$x`
    /// into an array on write, so this is only ever wired into a read
    /// site, never `check_assignment_target`'s.
    pub(crate) fn check_offset_accessibility(&mut self, base_ty: &Type, idx: &Expr) {
        // Reading an offset of `null` gives `null` with a warning (PHP
        // 7.4+), and is a bug on every version.
        if let Some(level) = self.failing_level(base_ty, 3, |m| {
            m.is_null().yes() || self.relation().is_offset_accessible(m).no()
        }) {
            self.report(
                format!("Cannot access offset on {base_ty}."),
                idx.span,
                level,
                "offsetAccess.nonOffsetAccessible",
            );
        } else if let Some(from) = types::strict_mixed_receiver_from(base_ty) {
            self.report_found_from(
                "Cannot access offset on mixed.".to_string(),
                idx.span,
                3,
                from,
                "offsetAccess.nonOffsetAccessible",
            );
        }
    }
}

/// What reading a variable no assignment has reached gives: a
/// superglobal's value (an array with a benevolent key and an explicit
/// `mixed` value, as PHPStan types it), `$argc`'s count, and for any
/// other name the error type - the read is reported undefined, so what's
/// built on it doesn't report again.
pub(crate) fn unassigned_variable_type(name: &str) -> Type {
    match name {
        "argc" => Type::int(),
        _ if is_superglobal(name) => {
            Type::array(Type::benevolent_array_key(), Type::explicit_mixed())
        }
        _ => Type::error(),
    }
}

/// Extracts a shape-lookup key from a literal index expression - `'name'`
/// or a bare int (PHP array keys are always stringified/int-normalized
/// internally, so both forms are compared the same way here). A
/// non-literal index (a variable, a call, ...) returns `None`, since it
/// can't be resolved against a shape's field names statically.
/// An index write can change entries keyed differently than its own
/// narrowing key: a write through a non-literal key (`$r[$k] = ...`, or
/// `$r[$k]['x'] = ...`) may land on any existing key of that container,
/// and a literal-key write (`$r['qs'] = ...`) may land on whatever a
/// variable-subscript entry (`$r[$k]`) currently reads. Drops every
/// such entry at each level of `lhs`'s index chain.
fn purge_aliased_index_entries(lhs: &Expr, locals: &mut Locals) {
    syntax::stack::guard(|| {
        let ExprKind::Index(base, Some(idx)) = &lhs.kind else {
            if let ExprKind::Index(base, None) = &lhs.kind {
                purge_aliased_index_entries(base, locals);
            }
            return;
        };
        purge_aliased_index_entries(base, locals);
        let Some(base_key) = locals.key_of(base) else {
            return;
        };
        // The written element itself isn't an alias of itself: the write
        // rewrites its entry, and a deeper write reads it as its container.
        let own = locals.key_of(lhs);
        // What is built on an element of `base`: any element for a write
        // through a non-literal key, a variable-subscript one for a literal.
        let literal = literal_string_key(idx).is_some();
        let stale: Vec<LocalKey> = {
            let keys = locals.keys().borrow();
            let mut stale = Vec::new();
            for &element in keys.dependents(base_key) {
                let aliased = match keys.data(element) {
                    KeyData::VarIndex { base, .. } => *base == base_key,
                    KeyData::Index { base, .. } => !literal && *base == base_key,
                    _ => false,
                };
                if !aliased {
                    continue;
                }
                stale.push(element);
                stale.extend(keys.all_dependents(element));
            }
            stale
                .into_iter()
                .filter(|k| locals.contains_key(*k) && Some(*k) != own)
                .collect()
        };
        for k in stale {
            locals.remove(k);
        }
    })
}

/// The array key a literal subscript names, by its bytes: `'5'` is the
/// int key 5, `"\xff"` its own string key.
pub(crate) fn literal_shape_key(e: &Expr) -> Option<ShapeKey> {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => Some(ShapeKey::from_bytes(s.as_bytes())),
        ExprKind::Literal(Literal::Int(n)) => Some(ShapeKey::Int(*n)),
        _ => None,
    }
}

pub(crate) fn literal_string_key(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.to_string()),
        ExprKind::Literal(Literal::Int(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// A variable's native type after a write (`written`) or an `unset` at one
/// of its offsets: an array is a general array, non-empty after a write;
/// `null` becomes an array on a write; objects and strings stay what they
/// are.
// Joins a few known types, far below the union cap.
#[allow(clippy::disallowed_methods)]
fn native_after_offset_change(native: &Type, written: bool) -> Type {
    let general = Type::array(Type::unknown_array_key(), Type::mixed());
    let general = if written {
        general.to_non_empty_array()
    } else {
        general
    };
    Type::union_of(
        native
            .union_members()
            .iter()
            .map(|m| {
                if m.is_array().yes() || (written && m.is_null().yes()) {
                    general.clone()
                } else {
                    m.clone()
                }
            })
            .collect(),
    )
}

/// `literal_key_from_type`'s own plural form - every `StringLiteral`/
/// `IntLiteral` member reachable from `ty`, recursing through a `Union` (a
/// `foreach` over a small literal array of key names binds its own loop
/// variable to the union of every element, not one resolved literal per
/// pass). Empty for anything else, never a guess.
fn literal_keys_from_type(ty: &Type) -> Vec<String> {
    ty.union_members()
        .iter()
        .filter_map(|m| match m.int_literal_value() {
            Some(n) => Some(n.to_string()),
            None => m.constant_strings_text().first().map(|s| s.to_string()),
        })
        .collect()
}

/// The key a destructuring element reads: its literal key, its position
/// when it has none, `None` for a key only known at runtime.
pub(crate) fn destructured_key(item: &DestructureItem, position: usize) -> Option<ShapeKey> {
    match item.key.as_ref().map(|k| &k.kind) {
        Some(ExprKind::Literal(Literal::String(s))) => Some(ShapeKey::from_bytes(s.as_bytes())),
        Some(ExprKind::Literal(Literal::Int(n))) => Some(ShapeKey::Int(*n)),
        Some(_) => None,
        None => Some(ShapeKey::Int(position as i64)),
    }
}
