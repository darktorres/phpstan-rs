use crate::ast::*;
use crate::checker::{
    Checker, DiagnosticBuilder, Locals, is_unsealed_shape, narrowing_key,
    purge_stale_narrowing_for, union_types, unwrap_grouping,
};
use crate::types::{AccessoryKind, Type, is_subtype};

/// Real PHPStan's `NullsafeCheck::containsNullSafe` - whether an
/// assignment target's own access chain contains a nullsafe `?->`
/// anywhere along it (`$a?->b = 1`, or nested deeper: `$a?->b->c = 1`).
/// Doesn't see through `DynamicPropertyAccess`/`DynamicMethodCall`
/// (`$a?->{$expr}`) - that shape doesn't carry its own nullsafe flag at
/// all (see `ExprKind::DynamicPropertyAccess`'s own doc comment), a
/// real, disclosed gap matching how rare it is in practice.
pub(crate) fn contains_nullsafe(expr: &Expr) -> bool {
    match &unwrap_grouping(expr).kind {
        ExprKind::PropertyAccess(base, _, nullsafe) => *nullsafe || contains_nullsafe(base),
        ExprKind::MethodCall(base, _, _, nullsafe) => *nullsafe || contains_nullsafe(base),
        ExprKind::Index(base, _) => contains_nullsafe(base),
        ExprKind::StaticCall(StaticTarget::Expr(base), _, _) => contains_nullsafe(base),
        ExprKind::ArrayLiteral(items) => items.iter().any(|item| {
            item.key.as_ref().is_some_and(contains_nullsafe) || contains_nullsafe(&item.value)
        }),
        _ => false,
    }
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
    match &unwrap_grouping(expr).kind {
        ExprKind::Variable(_)
        | ExprKind::PropertyAccess(..)
        | ExprKind::DynamicPropertyAccess(..)
        | ExprKind::Index(..)
        | ExprKind::StaticAccess(..) => false,
        // A skipped destructuring slot (`[, $x]`/`[$a, , $c]`) has no
        // item of its own in real PHPStan's AST at all (`$item ===
        // null`, skipped outright) - this project's own parser instead
        // represents that same empty slot as a real item whose `value`
        // is a bare `Literal::Null` (confirmed via `destructuring_with_
        // skipped_slot_parses`), which must get the identical "skip,
        // never non-assignable" treatment or every real, common skipped-
        // slot destructuring in a real corpus false-positives here
        // (caught via a live Piwigo corpus measurement: `[, $extension]
        // = explode(...)`).
        ExprKind::ArrayLiteral(items) => items.iter().any(|item| {
            !matches!(&item.value.kind, ExprKind::Literal(Literal::Null))
                && is_non_assignable_target(&item.value)
        }),
        _ => true,
    }
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
    pub(crate) fn check_nullsafe_assignment_target(&mut self, target: &Expr, label: &str) {
        if !contains_nullsafe(target) {
            return;
        }
        self.report_builder(
            DiagnosticBuilder::new(
                label.to_string(),
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
    pub(crate) fn check_nullsafe_byref_target(&mut self, target: &Expr, label: &str) {
        if !contains_nullsafe(target) {
            return;
        }
        self.report_builder(
            DiagnosticBuilder::new(
                label.to_string(),
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
    pub(crate) fn check_assignable_target(&mut self, target: &Expr, label: &str) {
        if !is_non_assignable_target(target) {
            return;
        }
        self.report_builder(
            DiagnosticBuilder::new(
                label.to_string(),
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
        match &target.kind {
            ExprKind::Variable(_) => {}
            ExprKind::ArrayLiteral(items) => {
                for item in items {
                    self.check_assignment_target(&item.value, locals, label);
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
                    self.check_dim_fetch_key_type(base, i, locals, label);
                }
            }
            _ => {
                self.check_expr(target, locals, label);
            }
        }
    }

    /// Sibling recursion to `bind_assignment_target`'s own `ArrayLiteral`
    /// arm below - a by-ref parameter destructured out of an array
    /// (`[$byRef, $b] = $arr;`/`list($byRef) = $arr;`, any nesting depth)
    /// needs the same `parameterByRef.type` check a bare `$byRef = ...`
    /// assignment already gets from `check_param_out_assignment` - a
    /// no-op for anything that isn't a destructuring target at all (a
    /// bare `Variable`/`PropertyAccess`/`Index` lhs is already fully
    /// covered by the direct `check_param_out_assignment` call at this
    /// method's own call site). Mirrors `bind_assignment_target`'s
    /// per-position/per-key element-type resolution exactly, so a
    /// destructured `Type::ArrayShape` source still checks each element
    /// against its own real per-slot type, not a blanket `Mixed`.
    pub(crate) fn check_by_ref_destructuring(&mut self, lhs: &Expr, ty: &Type, label: &str) {
        let ExprKind::ArrayLiteral(items) = &lhs.kind else {
            return;
        };
        match ty {
            Type::ArrayShape(fields) => {
                for (position, item) in items.iter().enumerate() {
                    let field_key = match item.key.as_ref().map(|k| &k.kind) {
                        Some(ExprKind::Literal(Literal::String(s))) => Some(s.to_string()),
                        Some(ExprKind::Literal(Literal::Int(n))) => Some(n.to_string()),
                        Some(_) => None,
                        None => Some(position.to_string()),
                    };
                    let element_ty = field_key
                        .and_then(|k| fields.iter().find(|(fk, _, _)| *fk == k))
                        .map(|(_, t, _)| t.clone())
                        .unwrap_or(Type::Mixed(false));
                    self.check_param_out_assignment(&item.value, &element_ty, label);
                    self.check_by_ref_destructuring(&item.value, &element_ty, label);
                }
            }
            Type::Array(..) | Type::Intersection(_) => {
                let element_ty = Self::array_element_type(ty).unwrap_or(Type::Mixed(false));
                for item in items {
                    self.check_param_out_assignment(&item.value, &element_ty, label);
                    self.check_by_ref_destructuring(&item.value, &element_ty, label);
                }
            }
            _ => {
                for item in items {
                    self.check_param_out_assignment(&item.value, &Type::Mixed(false), label);
                    self.check_by_ref_destructuring(&item.value, &Type::Mixed(false), label);
                }
            }
        }
    }

    /// Binds an assignment's (or a `foreach` value's) left-hand side to
    /// `ty`, the type of whatever was just assigned/iterated - recursing
    /// into a nested `ArrayLiteral` target (`[$a, $b] = $arr;` and
    /// `list($a, $b) = $arr;` both parse this way, including a nested
    /// `[[$a, $b], $c] = $nested;`) rather than only handling one level
    /// deep. Per-key precision (`['name' => $n] = $shapedArray`) isn't
    /// resolved for a plain `Type::Array` source - every target gets the
    /// source's homogeneous element type, or Mixed for anything else.
    pub(crate) fn bind_assignment_target(&self, lhs: &Expr, ty: Type, locals: &mut Locals) {
        // Keep any existing `narrowing_key` entry for this exact
        // property-chain/literal-index-chain expression in sync with a
        // real assignment - `apply_narrowing`'s synthetic key format is
        // a separate slot in the same `Locals` map (see `narrowing_key`'s
        // doc comment): without this, reassigning a property/element a
        // prior condition already narrowed would leave the old narrowed
        // value sitting under that key forever (`if ($this->entries ===
        // null) { $this->entries = [...]; } return $this->entries;`,
        // where `infer_expr`'s "check narrowed key first" fast path
        // would read back the stale `null` instead of the newly-assigned
        // array). Redundant with (but harmless alongside) the `Variable`
        // arm below, which does the same insert under the identical key
        // for that one case.
        if let Some(key) = narrowing_key(lhs) {
            // Also drops any deeper stale entry rooted at this exact
            // key (`"key->prop"`, `"key[lit]"`) or that used `key` as a
            // variable subscript elsewhere (`"other[$key]"`) - real
            // invalidation-on-reassignment, generalized over any
            // `narrowing_key` shape (bare variable, property chain, or
            // index chain), not just a bare variable.
            purge_stale_narrowing_for(locals, &key);
            locals.insert(key, ty.clone());
        }
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
                locals.declare(name.to_string(), ty, lhs.span);
            }
            ExprKind::ArrayLiteral(items) => match &ty {
                // A known array shape (from a `@return array{0: int, 1:
                // string}`-typed source, most commonly) carries real
                // per-position/per-key types - a plain homogeneous
                // `Type::Array` fallback would silently discard that
                // precision. A keyed destructuring element (`['key' =>
                // $x] = $shaped`) looks up its own literal key directly;
                // a positional element (`[$a, $b] = $shaped`, the common
                // tuple-destructuring case) looks up the field whose key
                // is that position's own index ("0", "1", ...).
                Type::ArrayShape(fields) => {
                    for (position, item) in items.iter().enumerate() {
                        let field_key = match item.key.as_ref().map(|k| &k.kind) {
                            Some(ExprKind::Literal(Literal::String(s))) => Some(s.to_string()),
                            Some(ExprKind::Literal(Literal::Int(n))) => Some(n.to_string()),
                            Some(_) => None,
                            None => Some(position.to_string()),
                        };
                        let element_ty = field_key
                            .and_then(|k| fields.iter().find(|(fk, _, _)| *fk == k))
                            .map(|(_, t, _)| t.clone())
                            .unwrap_or(Type::Mixed(false));
                        self.bind_assignment_target(&item.value, element_ty, locals);
                    }
                }
                // `Self::array_element_type` also sees through an
                // `Intersection`-wrapped `list<T>`/`non-empty-array<T>` -
                // without it, destructuring one (`[$a, $b] = $listTypedVar`)
                // would lose its real element type down to `Mixed`.
                Type::Array(..) | Type::Intersection(_) => {
                    let element_ty = Self::array_element_type(&ty).unwrap_or(Type::Mixed(false));
                    for item in items {
                        self.bind_assignment_target(&item.value, element_ty.clone(), locals);
                    }
                }
                _ => {
                    for item in items {
                        self.bind_assignment_target(&item.value, Type::Mixed(false), locals);
                    }
                }
            },
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
                        // locally. `superglobal_default_type` already
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
                        let was_defined = locals.contains_key(name);
                        let current = locals.get(name).cloned().unwrap_or_else(|| {
                            if is_superglobal(name) {
                                superglobal_default_type(name)
                            } else {
                                Type::ArrayShape(vec![])
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
                            && let Some((subject, value)) = self.active_discriminant.last().cloned()
                        {
                            let key = (name.to_string(), subject, value);
                            let prior = locals
                                .discriminant_array_facts
                                .get(&key)
                                .cloned()
                                .unwrap_or_else(|| Type::ArrayShape(vec![]));
                            if let Some(new_slot_ty) = write_index_into(
                                &prior,
                                None,
                                None,
                                ty.clone(),
                                false,
                                self.loop_depth > 0,
                            ) {
                                locals.discriminant_array_facts.insert(key, new_slot_ty);
                            }
                        }
                        // A dynamic-key write whose index *expression* is
                        // already known, by its own inferred type, to be
                        // one or more specific literal keys (`$key =
                        // 'id'; $row[$key] = ...;`, or a `foreach` over a
                        // small literal array of key names) is just as
                        // resolvable per-field as a literal AST token is -
                        // `narrowing_key(lhs)` at this function's own top
                        // only recognizes the latter (producing this
                        // write's own `"row[$key]"` dollar-prefixed
                        // "current value" marker instead). Each resolved
                        // literal key's own `narrowing_key`-format entry
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
                        // function `narrowing_key` block above.
                        if let Some(i) = idx.as_deref()
                            && literal_string_key(i).is_none()
                            && let Some(idx_ty) = idx_ty.as_ref()
                        {
                            for literal_key in literal_keys_from_type(idx_ty) {
                                let synthetic_key = format!("{name}[{literal_key}]");
                                let merged = match locals.get(&synthetic_key) {
                                    Some(prior) => union_types(prior.clone(), ty.clone()),
                                    None => ty.clone(),
                                };
                                locals.insert(synthetic_key, merged);
                            }
                        }
                        if let Some(new_ty) = write_index_into(
                            &current,
                            idx.as_deref(),
                            idx_ty.as_ref(),
                            ty,
                            in_range,
                            self.loop_depth > 0,
                        ) {
                            if was_defined || is_superglobal(name) {
                                locals.insert(name.to_string(), new_ty);
                            } else {
                                locals.declare(name.to_string(), new_ty, base.span);
                            }
                        }
                    }
                    ExprKind::Index(..) => {
                        let current_base_ty = self.nested_write_container_type(base, locals);
                        // A nested write's own container isn't a bare
                        // variable name `in_range_index_writes` could
                        // ever match against - out of scope for this
                        // narrow mechanism.
                        if let Some(new_base_ty) = write_index_into(
                            &current_base_ty,
                            idx.as_deref(),
                            idx_ty.as_ref(),
                            ty,
                            false,
                            self.loop_depth > 0,
                        ) {
                            self.bind_assignment_target(base, new_base_ty, locals);
                        }
                    }
                    // A write through a narrowed property (`$this->p = [];
                    // ... $this->p[$k] = $v;`) updates that narrowed type,
                    // or it would stay the stale pre-write one.
                    _ => {
                        if let Some(key) = narrowing_key(base)
                            && let Some(current) = locals.get(&key).cloned()
                            && let Some(new_ty) = write_index_into(
                                &current,
                                idx.as_deref(),
                                idx_ty.as_ref(),
                                ty,
                                false,
                                self.loop_depth > 0,
                            )
                        {
                            locals.insert(key, new_ty);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// The current type of a nested write's intermediate container
    /// (`$x['a']` in `$x['a']['b'] = 1`). An offset provably absent from
    /// its own container - an append, a literal key the shape doesn't
    /// have, or any key on an empty shape - is auto-vivified by PHP into
    /// a fresh array, at any depth, rather than read back as `mixed`.
    fn nested_write_container_type(&self, e: &Expr, locals: &Locals) -> Type {
        let ExprKind::Index(inner, idx) = &e.kind else {
            return self.infer_expr(e, locals);
        };
        let inner_ty = match &inner.kind {
            ExprKind::Index(..) => self.nested_write_container_type(inner, locals),
            _ => self.infer_expr(inner, locals),
        };
        let shapes: Vec<&Vec<(String, Type, bool)>> = match &inner_ty {
            Type::ArrayShape(fields) => vec![fields],
            Type::Union(members) => {
                let shapes: Vec<_> = members
                    .iter()
                    .filter_map(|m| match m {
                        Type::ArrayShape(fields) => Some(fields),
                        _ => None,
                    })
                    .collect();
                if shapes.len() != members.len() {
                    return self.infer_expr(e, locals);
                }
                shapes
            }
            _ => return self.infer_expr(e, locals),
        };
        if shapes.iter().any(|fields| is_unsealed_shape(fields)) {
            return self.infer_expr(e, locals);
        }
        let absent = match idx.as_deref() {
            None => true,
            Some(i) => match literal_string_key(i) {
                Some(key) => !shapes
                    .iter()
                    .any(|fields| fields.iter().any(|(name, _, _)| *name == key)),
                None => shapes.iter().all(|fields| fields.is_empty()),
            },
        };
        if absent {
            Type::ArrayShape(vec![])
        } else {
            self.infer_expr(e, locals)
        }
    }

    pub(crate) fn bind_simple_target(&self, target: &Expr, ty: Type, locals: &mut Locals) {
        if let ExprKind::Variable(name) = &target.kind {
            purge_stale_narrowing_for(locals, name);
            // Real rebind - `target` is a `foreach` key's bare `$var`
            // expression (its only caller), a real declaration site.
            self.probe_hit_at(target.span, &ty);
            locals.declare(name.to_string(), ty, target.span);
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
    pub(crate) fn check_offset_existence(
        &mut self,
        base: &Expr,
        idx: &Expr,
        locals: &Locals,
        label: &str,
    ) {
        let container = self.infer_expr(base, locals);
        let dim = self.infer_expr(idx, locals);
        self.report_missing_offset(&container, &dim, idx.span, label);
    }

    /// `ArrayDestructuringRule`'s per-key half: every destructured key
    /// (a position, or an explicit key) checked against the destructured
    /// value, recursing into a nested destructuring target.
    pub(crate) fn check_destructuring_offsets(
        &mut self,
        items: &[ArrayItem],
        container: &Type,
        locals: &Locals,
        label: &str,
    ) {
        let mut position: i64 = 0;
        for item in items {
            if item.spread {
                continue;
            }
            if item.key.is_none() && matches!(item.value.kind, ExprKind::Literal(Literal::Null)) {
                position += 1;
                continue;
            }
            let (dim, span) = match &item.key {
                Some(k) => (self.infer_expr(k, locals), k.span),
                None => {
                    position += 1;
                    (Type::IntLiteral(position - 1), item.value.span)
                }
            };
            if self.report_missing_offset(container, &dim, span, label) {
                continue;
            }
            if let ExprKind::ArrayLiteral(inner) = &item.value.kind {
                let key = normalized_literal_key(&dim).map(|(k, _)| k);
                let inner_container =
                    crate::checker::expr::index_result_type(container.clone(), key.as_deref());
                self.check_destructuring_offsets(inner, &inner_container, locals, label);
            }
        }
    }

    /// Reports `offsetAccess.notFound` when `container` provably lacks
    /// `dim`. Returns whether it did.
    pub(crate) fn report_missing_offset(
        &mut self,
        container: &Type,
        dim: &Type,
        span: crate::token::Span,
        label: &str,
    ) -> bool {
        if offset_verdict(container, dim) != Some(false) {
            return false;
        }
        self.report(
            label.to_string(),
            format!("Offset {dim} does not exist on {container}."),
            span,
            3,
            "offsetAccess.notFound",
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
    pub(crate) fn check_offset_accessibility(&mut self, base_ty: &Type, idx: &Expr, label: &str) {
        if self.type_confirmed_not_offset_accessible(base_ty) {
            self.report(
                label.to_string(),
                format!("Cannot access offset on {base_ty}."),
                idx.span,
                3,
                "offsetAccess.nonOffsetAccessible",
            );
        } else if let Some(level) = self.registry.strict_mixed_receiver_level(base_ty) {
            self.report(
                label.to_string(),
                "Cannot access offset on mixed.".to_string(),
                idx.span,
                level,
                "offsetAccess.nonOffsetAccessible",
            );
        }
    }

    /// Whether `ty` is confirmed to never support `[...]` read access -
    /// verified against real PHPStan's own `Type::isOffsetAccessible()`
    /// implementations, not assumed. `Null` is the one deliberate,
    /// source-verified exception: unlike `Bool`/`Int`/`Float` (each
    /// `use NonOffsetAccessibleTypeTrait`), `NullType::isOffsetAccessible()`
    /// (`NullType.php:196`) returns "yes" - real PHPStan never flags
    /// `$x = null; echo $x['a'];`, only the legal auto-vivification write
    /// explains why `Null` needs this exemption while still being closed
    /// for `method.nonObject`/`property.nonObject`/`foreach.nonIterable`.
    /// `String`/`StringLiteral`/`ClassString` are accessible too - real
    /// PHP's own `$str[0]` character read.
    pub(crate) fn type_confirmed_not_offset_accessible(&self, ty: &Type) -> bool {
        match ty {
            Type::Array(..)
            | Type::ArrayShape(_)
            | Type::HasOffsetValue(..)
            | Type::String
            | Type::StringLiteral(_)
            | Type::ClassString(_)
            | Type::Null => false,
            Type::Void
            | Type::Bool
            | Type::True
            | Type::False
            | Type::Int
            | Type::IntLiteral(_)
            | Type::IntRange(..)
            | Type::Float
            | Type::FloatLiteral(_) => true,
            Type::Object(class, _) | Type::EnumCase(class, _) => {
                self.class_confirmed_not(class, "ArrayAccess")
            }
            Type::Union(members) => members
                .iter()
                .any(|m| self.type_confirmed_not_offset_accessible(m)),
            Type::Intersection(members) => members
                .iter()
                .all(|m| self.type_confirmed_not_offset_accessible(m)),
            Type::TemplateParam(param) => self.type_confirmed_not_offset_accessible(&param.bound),
            Type::Conditional {
                if_true, if_false, ..
            } => {
                self.type_confirmed_not_offset_accessible(if_true)
                    || self.type_confirmed_not_offset_accessible(if_false)
            }
            _ => false,
        }
    }

    /// Shared by `type_confirmed_not_offset_accessible`/
    /// `type_confirmed_not_destructurable` - `class` is confirmed to
    /// never satisfy `interface` only when it's a registered,
    /// non-interface class that doesn't `is_subclass_of` it. An
    /// interface-typed receiver, or an unregistered class, stays
    /// "maybe" - mirrors `iterable_element_type`'s own identical
    /// leniency for `Traversable` already established in this codebase.
    pub(crate) fn class_confirmed_not(&self, class: &str, interface: &str) -> bool {
        self.registry
            .classes
            .get(class)
            .is_some_and(|info| info.kind != ClassKind::Interface)
            && !self.registry.is_subclass_of(class, interface)
    }

    /// Whether `ty` is confirmed to never be a valid `foreach` subject -
    /// used by `foreach.nonIterable`. Real PHP does **not** fall back to
    /// "iterate the object's public properties" as a safe default for a
    /// plain, non-`Traversable` class the way intuition might suggest -
    /// real PHPStan's `ObjectType::isInstanceOf()` returns a definite
    /// "no" for a concrete, non-interface class that doesn't implement
    /// the target interface, which is why this fires on plain classes
    /// too, not just scalars. `Array`/`ArrayShape`/`HasOffsetValue` are
    /// `false` here for defensive completeness even though `check_foreach`
    /// never actually reaches this classifier for them -
    /// `array_element_type` claims that case first.
    pub(crate) fn type_confirmed_not_iterable(&self, ty: &Type) -> bool {
        match ty {
            Type::Array(..) | Type::ArrayShape(_) | Type::HasOffsetValue(..) => false,
            Type::Null
            | Type::Void
            | Type::Bool
            | Type::True
            | Type::False
            | Type::Int
            | Type::IntLiteral(_)
            | Type::IntRange(..)
            | Type::Float
            | Type::FloatLiteral(_)
            | Type::String
            | Type::StringLiteral(_)
            | Type::ClassString(_) => true,
            Type::Object(class, _) | Type::EnumCase(class, _) => {
                self.class_confirmed_not(class, "Traversable")
            }
            // `Null` is dropped before this evaluation - see
            // `type_can_never_be_object`'s own doc comment (`expr.rs`)
            // for why: real PHPStan strips `Null` out of any non-pure-Null
            // type before running this family of checks at all.
            Type::Union(members) => members
                .iter()
                .filter(|m| !matches!(m, Type::Null))
                .any(|m| self.type_confirmed_not_iterable(m)),
            Type::Intersection(members) => {
                members.iter().all(|m| self.type_confirmed_not_iterable(m))
            }
            Type::TemplateParam(param) => self.type_confirmed_not_iterable(&param.bound),
            Type::Conditional {
                if_true, if_false, ..
            } => {
                self.type_confirmed_not_iterable(if_true)
                    || self.type_confirmed_not_iterable(if_false)
            }
            _ => false,
        }
    }

    /// Whether `ty` is confirmed to never support array destructuring
    /// (`[$a, $b] = $x;`/`list($a, $b) = $x;`) - used by
    /// `offsetAccess.nonArray`. **Not** the same predicate as
    /// `type_confirmed_not_offset_accessible`, verified from real
    /// PHPStan's own `ArrayDestructuringRule` source: its criterion is
    /// `isArray()->yes() || ArrayAccess`-superset, with no exemption for
    /// `String` - unlike `$str[0]`, destructuring a string was never
    /// given PHP language support (`[$a, $b] = 'ab';` raises the
    /// identical `Cannot use string as array` warning real PHP gives
    /// every other non-array scalar, empirically confirmed).
    pub(crate) fn type_confirmed_not_destructurable(&self, ty: &Type) -> bool {
        match ty {
            Type::Array(..) | Type::ArrayShape(_) | Type::HasOffsetValue(..) => false,
            Type::Null
            | Type::Void
            | Type::Bool
            | Type::True
            | Type::False
            | Type::Int
            | Type::IntLiteral(_)
            | Type::IntRange(..)
            | Type::Float
            | Type::FloatLiteral(_)
            | Type::String
            | Type::StringLiteral(_)
            | Type::ClassString(_) => true,
            Type::Object(class, _) | Type::EnumCase(class, _) => {
                self.class_confirmed_not(class, "ArrayAccess")
            }
            Type::Union(members) => members
                .iter()
                .filter(|m| !matches!(m, Type::Null))
                .any(|m| self.type_confirmed_not_destructurable(m)),
            Type::Intersection(members) => members
                .iter()
                .all(|m| self.type_confirmed_not_destructurable(m)),
            Type::TemplateParam(param) => self.type_confirmed_not_destructurable(&param.bound),
            Type::Conditional {
                if_true, if_false, ..
            } => {
                self.type_confirmed_not_destructurable(if_true)
                    || self.type_confirmed_not_destructurable(if_false)
            }
            _ => false,
        }
    }

    /// Whether `ty` is confirmed to never be callable - used by
    /// `callable.nonCallable` for a dynamic call (`$x(...)`). Verified
    /// against which real `Type` implementations `use
    /// PHPStan\Type\Traits\NonCallableTypeTrait` directly: `BooleanType`/
    /// `IntegerType`/`FloatType`/`NullType`/`VoidType`. `String`/`Array`
    /// stay "maybe" (a string can name a real function/static method, an
    /// array can be a valid `[$obj, 'method']` callable) - resolving
    /// either precisely enough to ever say "no" needs machinery well
    /// beyond this check's scope, matching real PHPStan's own
    /// `MaybeCallableTypeTrait` default for these two type families.
    /// `resource` (this project's `Object("resource", [])` convention -
    /// `resource` is never a real registered class) gets its own
    /// explicit arm ahead of the general `Object` one, which would
    /// otherwise treat it as an unregistered-class "maybe" - the wrong
    /// answer for a well-understood, permanently-uninvokable pseudo-type.
    pub(crate) fn type_confirmed_not_callable(&self, ty: &Type) -> bool {
        match ty {
            Type::Null
            | Type::Void
            | Type::Bool
            | Type::True
            | Type::False
            | Type::Int
            | Type::IntLiteral(_)
            | Type::IntRange(..)
            | Type::Float
            | Type::FloatLiteral(_) => true,
            Type::Object(class, _) if class.as_str() == "resource" => true,
            Type::Object(class, _) | Type::EnumCase(class, _) => {
                class.as_str() != "Closure"
                    && self
                        .registry
                        .classes
                        .get(class.as_str())
                        .is_some_and(|info| info.kind != ClassKind::Interface)
                    && self.registry.lookup_method(class, "__invoke").is_none()
            }
            Type::Union(members) => members
                .iter()
                .filter(|m| !matches!(m, Type::Null))
                .any(|m| self.type_confirmed_not_callable(m)),
            Type::Intersection(members) => {
                members.iter().all(|m| self.type_confirmed_not_callable(m))
            }
            Type::TemplateParam(param) => self.type_confirmed_not_callable(&param.bound),
            Type::Conditional {
                if_true, if_false, ..
            } => {
                self.type_confirmed_not_callable(if_true)
                    || self.type_confirmed_not_callable(if_false)
            }
            _ => false,
        }
    }
}

pub(crate) fn is_superglobal(name: &str) -> bool {
    matches!(
        name,
        "GLOBALS"
            | "_GET"
            | "_POST"
            | "_REQUEST"
            | "_SERVER"
            | "_SESSION"
            | "_COOKIE"
            | "_FILES"
            | "_ENV"
            | "argv"
            | "argc"
    )
}

/// A superglobal's own type, read *before* any real assignment to it
/// appears in this function's own `locals` - real phpstorm-stubs' own
/// `superglobals/_superglobals.php` bootstrap file declares every one of
/// these (bar `argc`) with a literal `= []` assignment, which is exactly
/// what real PHPStan's own global-variable seeding generalizes to a bare
/// `array`, never `null`/`mixed`. Reusing the generic `Type::Mixed(false)`
/// fallback here (as if a superglobal's pre-assignment type were
/// "unknown/could be anything") broke `$_SESSION ??= [];` specifically:
/// `widen_coalesce_result`'s own "keep the left side's real array type
/// instead of collapsing to the empty-shape fallback" rule only fires
/// for an already-known `Type::Array`. Scoped to exactly the real declared
/// type: `Array(Mixed)` for the always-array superglobals, `Int` for
/// `argc` (never `[]` in the stub at all).
///
/// The always-array case mirrors real PHPStan's own `MutatingScope::
/// getVariableType`'s `isGlobalVariable` branch exactly (confirmed
/// reading `vendor/phpstan-src` directly): `ArrayType(BenevolentUnionType([
/// Int, String]), MixedType(true))` - a `Benevolent`-marked key (this
/// project's own `benevolent_array_key()`, the identical marker
/// `dynamic_write_key_type`'s own fallback already uses for the same
/// "I don't know the key" case) and an *explicit* Mixed value, not the
/// generic implicit fallback - a deliberate "this superglobal's value
/// really is unconstrained" answer, not "we don't know."
pub(crate) fn superglobal_default_type(name: &str) -> Type {
    match name {
        "argc" => Type::Int,
        _ if is_superglobal(name) => Type::Array(
            Box::new(Type::benevolent_array_key()),
            Box::new(Type::Mixed(true)),
        ),
        _ => Type::Mixed(false),
    }
}

/// Extracts a shape-lookup key from a literal index expression - `'name'`
/// or a bare int (PHP array keys are always stringified/int-normalized
/// internally, so both forms are compared the same way here). A
/// non-literal index (a variable, a call, ...) returns `None`, since it
/// can't be resolved against a shape's field names statically.
pub(crate) fn literal_string_key(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.to_string()),
        ExprKind::Literal(Literal::Int(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// Computes a container's new type after `container[idx] = value_ty` -
/// shared by `bind_assignment_target`'s `Index` arm for both a flat write
/// (`$x[...] = v`, `container_ty` is `$x`'s own current type) and a
/// nested one (`$a[$k1][$k2] = v`, called once per nesting level with
/// each level's own `idx`/`value_ty`, `container_ty` being that level's
/// own current type as computed by `infer_expr`). Returns `None` when
/// `container_ty` doesn't carry enough shape information to update
/// precisely - left alone, matching this project's usual "don't guess"
/// scope.
/// `literal_string_key`'s own type-based sibling - a non-literal index
/// *expression* (`$row[$key] = ...`) whose *inferred* type is already a
/// precise `Type::StringLiteral`/`Type::IntLiteral` (from `$key = 'id';`
/// alone, or any other narrowing that pins it down, no loop needed) is
/// just as resolvable against a shape's own field names as a literal AST
/// token is - `literal_string_key` only ever checks the syntax, never the
/// type its caller already computed and passed in as `idx_ty`. Kept as a
/// separate helper (checked only where `write_index_into`'s `ArrayShape`
/// arm needs it) rather than folded into `literal_string_key` itself, to
/// avoid changing that function's signature at its other, unrelated call
/// sites.
fn literal_key_from_type(idx_ty: Option<&Type>) -> Option<String> {
    match idx_ty {
        Some(Type::StringLiteral(s)) => Some(s.to_string()),
        Some(Type::IntLiteral(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// `literal_key_from_type`'s own plural form - every `StringLiteral`/
/// `IntLiteral` member reachable from `ty`, recursing through a `Union` (a
/// `foreach` over a small literal array of key names binds its own loop
/// variable to the union of every element, not one resolved literal per
/// pass). Empty for anything else, never a guess.
fn literal_keys_from_type(ty: &Type) -> Vec<String> {
    match ty {
        Type::StringLiteral(s) => vec![s.clone()],
        Type::IntLiteral(n) => vec![n.to_string()],
        Type::Union(members) => members.iter().flat_map(literal_keys_from_type).collect(),
        _ => Vec::new(),
    }
}

/// Maps `idx_ty` to a definite array key type for a write - `Type::Int`/
/// `Type::String` when `idx_ty` is confidently one or the other, otherwise
/// `unknown_array_key()` rather than a wrong guess. Also reused by
/// `checker::sql_reflection`'s own `fetchAllKeyValue`/
/// `fetchAllAssociativeIndexed` dispatch (sub-phase D) - the identical
/// "what does this uncertain-typed value become as a real PHP array
/// key" question, just asked about a fetched column's own value instead
/// of a write's own index expression.
pub(crate) fn dynamic_write_key_type(idx_ty: &Type) -> Type {
    if is_subtype(idx_ty, &Type::Int) {
        Type::Int
    } else if is_subtype(idx_ty, &Type::String) {
        Type::String
    } else {
        // Mirrors real PHPStan's own `MixedType::toArrayKey()`/
        // `UnionType::toArrayKey()` (confirmed reading `vendor/
        // phpstan-src` directly): the fallback for a dynamic write whose
        // own key expression can't be confidently proven `int` or
        // `string` is a *benevolent* union, not a plain one - accepted
        // later wherever *either* member would independently satisfy the
        // target (`Registry::is_assignable`'s existing Benevolent
        // handling, from Fix 1, already does the right thing once a
        // value's type actually carries the marker). Real corpus false
        // positive this fixes: a dynamic-key write keyed by a `Type::
        // Mixed` foreach variable, later read back and passed to a
        // `string`-typed parameter - real PHPStan doesn't flag it either.
        Type::benevolent_array_key()
    }
}

fn write_index_into(
    container_ty: &Type,
    idx: Option<&Expr>,
    idx_ty: Option<&Type>,
    value_ty: Type,
    in_range: bool,
    in_loop: bool,
) -> Option<Type> {
    match container_ty {
        // `Null` is the same "not really an array yet" case a top-level
        // undefined-variable write already auto-vivifies from a fresh
        // empty `ArrayShape` (real PHP auto-vivifies a null-valued offset
        // too) - delegate rather than bail, so a `Union` member that's
        // provably `Null` doesn't sink the whole write via `Union`'s
        // "all or nothing" `?`.
        Type::Null => write_index_into(
            &Type::ArrayShape(vec![]),
            idx,
            idx_ty,
            value_ty,
            in_range,
            in_loop,
        ),
        Type::ArrayShape(fields) => match idx
            .and_then(literal_string_key)
            .or_else(|| literal_key_from_type(idx_ty))
        {
            Some(key) => {
                let mut fields = fields.clone();
                match fields.iter_mut().find(|(k, _, _)| *k == key) {
                    Some(field) => field.1 = value_ty,
                    None => fields.push((key, value_ty, false)),
                }
                Some(Type::ArrayShape(fields))
            }
            None => {
                let widened = fields.iter().fold(value_ty, |acc, (_, field_ty, _)| {
                    union_types(acc, field_ty.clone())
                });
                // A real append (`idx` absent) onto an already list-like
                // container always assigns the next sequential int key,
                // so the result stays a list, now provably non-empty. A
                // dynamic key write falls through this same branch but
                // must not get the marker - an arbitrary key need not be
                // sequential.
                if idx.is_none() && Checker::is_list_like(container_ty) {
                    Some(Type::Intersection(vec![
                        Type::Array(Box::new(Type::unknown_array_key()), Box::new(widened)),
                        Type::Accessory(AccessoryKind::ArrayList),
                        Type::Accessory(AccessoryKind::NonEmptyArray),
                    ]))
                } else {
                    let key_ty = idx
                        .and(idx_ty)
                        .map(dynamic_write_key_type)
                        .unwrap_or_else(Type::unknown_array_key);
                    Some(Type::Array(Box::new(key_ty), Box::new(widened)))
                }
            }
        },
        // Already widened past a literal shape - no single key to grow
        // precisely anymore. A genuinely literal key still gets a real
        // `hasOffsetValue(...)` intersection member layered on top (see
        // `write_literal_key_aware`'s own doc comment) - the container's
        // shared element type still widens via `union_types` exactly as
        // before regardless, so a later dynamic-key read stays sound.
        Type::Array(key, inner) => Some(write_literal_key_aware(
            key,
            inner,
            idx,
            idx_ty,
            value_ty,
            in_loop,
            &[],
        )),
        // A container typed as a `Union` (e.g. differently-shaped
        // `ArrayShape`s merged across an `if`/`else`) must update every
        // member the same way, bailing via `?` if any one can't be -
        // otherwise a later read through the same `Union` sees a key
        // written one line earlier as still missing.
        Type::Union(members) => {
            let mut updated = Vec::with_capacity(members.len());
            for m in members {
                updated.push(write_index_into(
                    m,
                    idx,
                    idx_ty,
                    value_ty.clone(),
                    in_range,
                    in_loop,
                )?);
            }
            Some(
                updated
                    .into_iter()
                    .reduce(union_types)
                    .unwrap_or(Type::Mixed(false)),
            )
        }
        // A `list<T>`/`non-empty-array<T>` container is an
        // `Intersection([Array(T), Accessory(_)])`. A dynamic-key write
        // drops any `Accessory` marker - an arbitrary key can't preserve
        // a list's sequential-key guarantee - but a real append (`idx`
        // absent) onto an already-list-marked container keeps it a list,
        // now provably non-empty. `in_range` is the one further
        // exception: `Checker::in_range_index_writes`'s own doc comment -
        // a write whose index is syntactically the exact loop variable
        // bounded by `count()` of this exact container (`for ($i = 0; $i
        // < count($x); $i++) { $x[$i] = ...; }`) rewrites an existing
        // element in place, never growing past the end or skipping an
        // index, so real PHP list-ness genuinely survives it too -
        // confirmed via `dumpType()` against real PHPStan. Non-emptiness
        // is handled differently for the two cases: a real append always
        // proves it (the container just grew), but an in-range rewrite
        // doesn't change whether the container was already empty, so it
        // only carries `NonEmptyArray` forward when the container already
        // had it, never adds it fresh.
        //
        // A container that instead already carries one or more
        // `hasOffsetValue(...)` members (from an earlier literal-key
        // write this same function produced) is a completely separate
        // case from the list-marker one above - dispatched first, before
        // any list-specific handling, since the two never co-occur (a
        // list's keys are always sequential ints, so a literal-key write
        // onto one is a dynamic-key write in this function's own terms,
        // which the list arm below already drops the marker for).
        Type::Intersection(members)
            if members
                .iter()
                .any(|m| matches!(m, Type::HasOffsetValue(..))) =>
        {
            let (key, inner) = members.iter().find_map(|m| match m {
                Type::Array(k, v) => Some((k.as_ref(), v.as_ref())),
                _ => None,
            })?;
            let existing: Vec<Type> = members
                .iter()
                .filter(|m| matches!(m, Type::HasOffsetValue(..)))
                .cloned()
                .collect();
            Some(write_literal_key_aware(
                key, inner, idx, idx_ty, value_ty, in_loop, &existing,
            ))
        }
        Type::Intersection(members) => {
            let inner = members.iter().find_map(Checker::array_element_type)?;
            let widened = union_types(value_ty, inner);
            let key_ty = idx
                .and(idx_ty)
                .map(dynamic_write_key_type)
                .unwrap_or_else(Type::unknown_array_key);
            let is_append = idx.is_none();
            if (is_append || in_range)
                && members.contains(&Type::Accessory(AccessoryKind::ArrayList))
            {
                let mut new_members = vec![
                    Type::Array(Box::new(key_ty), Box::new(widened)),
                    Type::Accessory(AccessoryKind::ArrayList),
                ];
                if is_append || members.contains(&Type::Accessory(AccessoryKind::NonEmptyArray)) {
                    new_members.push(Type::Accessory(AccessoryKind::NonEmptyArray));
                }
                Some(Type::Intersection(new_members))
            } else {
                Some(Type::Array(Box::new(key_ty), Box::new(widened)))
            }
        }
        _ => None,
    }
}

/// Builds `Array(key, inner)[literal-or-dynamic key] = value_ty`'s result,
/// layering a `hasOffsetValue(...)` intersection member on top whenever
/// the write's own key resolves to a genuine compile-time literal and
/// this write isn't inside a loop body currently under analysis - shared
/// by `write_index_into`'s own `Type::Array` arm (`existing_has_offset_
/// values` empty, no prior tracked keys) and its `Type::Intersection`
/// arm for a container that already carries one or more from an earlier
/// literal-key write (accumulated here, replacing any entry for the same
/// key rather than duplicating it - real PHPStan does the same, confirmed
/// via `assertType()`: two straight-line literal-key writes to different
/// keys keep both as separate `hasOffsetValue(...)` members).
///
/// The `in_loop` guard mirrors real PHPStan's own observed behavior
/// exactly (confirmed via `assertType()`, not guessed): a loop variable
/// narrowed to a literal during one simulated iteration still generalizes
/// away before the merge that produces the loop's own exit type, so a
/// `for ($i = 0; ...) { $x[$i] = ...; }`-shaped write never produces
/// `hasOffsetValue` there either. This project's own multi-pass
/// fixed-point loop convergence (`converge_loop_body`) has no such
/// generalization step, so without this guard it would instead compose a
/// fresh, differently-keyed `hasOffsetValue` on almost every pass - which
/// doesn't collapse safely across passes the way plain union-widening
/// always has. Confirmed as a real, broad corpus regression (6 new false
/// positives, 0 fixes) before this guard was added; `self.loop_depth ==
/// 0` is this project's own established idiom for exactly this class of
/// "only sound on a single, non-looping pass" soundness issue (see
/// `ExprKind::MethodCall`'s nullsafe-on-non-nullable check in
/// `checker::expr` for the identical reasoning).
fn write_literal_key_aware(
    key: &Type,
    inner: &Type,
    idx: Option<&Expr>,
    idx_ty: Option<&Type>,
    value_ty: Type,
    in_loop: bool,
    existing_has_offset_values: &[Type],
) -> Type {
    let key_ty = match idx.and(idx_ty).map(dynamic_write_key_type) {
        Some(new_key_ty) => union_types(key.clone(), new_key_ty),
        None => key.clone(),
    };
    let base = Type::Array(
        Box::new(key_ty),
        Box::new(union_types(value_ty.clone(), inner.clone())),
    );
    let literal_key = idx
        .and_then(literal_string_key)
        .or_else(|| literal_key_from_type(idx_ty));
    match literal_key {
        Some(key) if !in_loop => {
            let mut members: Vec<Type> = existing_has_offset_values
                .iter()
                .filter(|m| !matches!(m, Type::HasOffsetValue(k, _) if *k == key))
                .cloned()
                .collect();
            members.push(Type::HasOffsetValue(key, Box::new(value_ty)));
            members.insert(0, base);
            Type::Intersection(members)
        }
        // A dynamic key, or a write now happening inside a loop, can't
        // extend the tracked-keys set precisely - same "give up
        // precision" fallback the plain `Type::Array` arm always took,
        // dropping every existing `hasOffsetValue` member too since none
        // of them can be trusted to still hold once an arbitrary key may
        // have overwritten them.
        _ => base,
    }
}

/// A literal string's text, or a statically-foldable `.`-concatenation of
/// nothing but literal strings (and parenthesized groupings of the same),
/// folded into one owned value. `None` for anything not fully foldable
/// this way (a variable anywhere in the chain, a non-string literal) -
/// unlike `literal_string_key`, this doesn't also accept an int literal,
/// since a format string is never legitimately written as a bare number.
/// Purely syntactic: a concatenation with a variable operand is never
/// resolved here even if a caller's type tracking knows it's a literal
/// value, since this function only sees the raw `Expr`.
pub(crate) fn literal_string_value(e: &Expr) -> Option<std::borrow::Cow<'_, str>> {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => Some(std::borrow::Cow::Borrowed(s.as_ref())),
        ExprKind::Grouping(inner) => literal_string_value(inner),
        ExprKind::Binary(BinOp::Concat, lhs, rhs) => {
            let l = literal_string_value(lhs)?;
            let r = literal_string_value(rhs)?;
            Some(std::borrow::Cow::Owned(format!("{l}{r}")))
        }
        _ => None,
    }
}

/// A literal array key's normalized form: an int key as its decimal
/// text, a numeric-integer string likewise, any other string as-is.
fn normalized_literal_key(dim: &Type) -> Option<(String, bool)> {
    match dim {
        Type::IntLiteral(n) => Some((n.to_string(), true)),
        Type::StringLiteral(s) if s.parse::<i64>().is_ok_and(|n| n.to_string() == *s) => {
            Some((s.clone(), true))
        }
        Type::StringLiteral(s) => Some((s.clone(), false)),
        Type::True => Some(("1".to_string(), true)),
        Type::False => Some(("0".to_string(), true)),
        Type::Null => Some((String::new(), false)),
        _ => None,
    }
}

/// Whether `container` definitely has (`Some(true)`), definitely lacks
/// (`Some(false)`), or might have (`None`) the offset `dim`.
pub(crate) fn offset_verdict(container: &Type, dim: &Type) -> Option<bool> {
    match container {
        Type::ArrayShape(fields) => {
            if is_unsealed_shape(fields) {
                return None;
            }
            if fields.is_empty() && !matches!(dim, Type::Mixed(_) | Type::Never) {
                return Some(false);
            }
            let (key, _) = normalized_literal_key(dim)?;
            match fields.iter().find(|(name, _, _)| *name == key) {
                Some((_, _, false)) => Some(true),
                Some((_, _, true)) => None,
                None => Some(false),
            }
        }
        Type::Array(key_ty, _) => general_array_offset_verdict(key_ty, false, dim),
        Type::Intersection(members) => {
            let is_list = members
                .iter()
                .any(|m| matches!(m, Type::Accessory(crate::types::AccessoryKind::ArrayList)));
            let key_ty = members.iter().find_map(|m| match m {
                Type::Array(k, _) => Some(k.as_ref()),
                _ => None,
            })?;
            general_array_offset_verdict(key_ty, is_list, dim)
        }
        Type::Union(members) => {
            let verdicts: Vec<Option<bool>> =
                members.iter().map(|m| offset_verdict(m, dim)).collect();
            if verdicts.iter().all(|v| *v == Some(false)) {
                Some(false)
            } else if verdicts.iter().all(|v| *v == Some(true)) {
                Some(true)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn general_array_offset_verdict(key_ty: &Type, is_list: bool, dim: &Type) -> Option<bool> {
    let (key, is_int_key) = normalized_literal_key(dim)?;
    let key_kinds = |t: &Type| -> (bool, bool) {
        match t {
            Type::Int | Type::IntLiteral(_) | Type::IntRange(..) => (true, false),
            Type::String | Type::StringLiteral(_) | Type::ClassString(_) => (false, true),
            _ => (true, true),
        }
    };
    let (may_be_int, may_be_string) = match key_ty {
        Type::Union(members) => members
            .iter()
            .map(key_kinds)
            .fold((false, false), |a, b| (a.0 || b.0, a.1 || b.1)),
        Type::Intersection(members) => members
            .iter()
            .map(key_kinds)
            .fold((true, true), |a, b| (a.0 && b.0, a.1 && b.1)),
        other => key_kinds(other),
    };
    if is_list {
        return (!is_int_key || key.starts_with('-')).then_some(false);
    }
    if is_int_key && !may_be_int {
        return Some(false);
    }
    if !is_int_key && !may_be_string {
        return Some(false);
    }
    None
}
