use crate::checker::{Checker, Locals, narrowing_key, purge_stale_narrowing_for, union_types};
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
    match &unwrap_grouping(expr).kind {
        ExprKind::PropertyAccess(base, _, nullsafe) => *nullsafe || contains_nullsafe(base),
        ExprKind::MethodCall(base, _, _, nullsafe) => *nullsafe || contains_nullsafe(base),
        ExprKind::Index(base, _) => contains_nullsafe(base),
        ExprKind::StaticCall(StaticTarget::Expr(base), _, _)
        | ExprKind::DynamicStaticCall(StaticTarget::Expr(base), _, _)
        | ExprKind::StaticPropertyFetch(StaticTarget::Expr(base), _)
        | ExprKind::DynamicStaticPropertyFetch(StaticTarget::Expr(base), _) => {
            contains_nullsafe(base)
        }
        ExprKind::ArrayLiteral(items) | ExprKind::List(items) => items.iter().any(|item| {
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
        | ExprKind::ClassConstFetch(..)
        | ExprKind::ClassNameFetch(_)
        | ExprKind::StaticPropertyFetch(..)
        | ExprKind::DynamicStaticPropertyFetch(..) => false,
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
        ExprKind::ArrayLiteral(items) | ExprKind::List(items) => items.iter().any(|item| {
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
        match &target.kind {
            ExprKind::Variable(_) => {}
            ExprKind::ArrayLiteral(items) | ExprKind::List(items) => {
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
                    self.check_dim_fetch_key_type(base, i, locals);
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
    pub(crate) fn check_by_ref_destructuring(&mut self, lhs: &Expr, ty: &Type) {
        let (ExprKind::ArrayLiteral(items) | ExprKind::List(items)) = &lhs.kind else {
            return;
        };
        match ty {
            Type::ArrayShape(shape) => {
                for (position, item) in items.iter().enumerate() {
                    let field_key = match item.key.as_ref().map(|k| &k.kind) {
                        Some(ExprKind::Literal(Literal::String(s))) => {
                            Some(ShapeKey::from_name(&s.to_str_lossy()))
                        }
                        Some(ExprKind::Literal(Literal::Int(n))) => Some(ShapeKey::Int(*n)),
                        Some(_) => None,
                        None => Some(ShapeKey::Int(position as i64)),
                    };
                    let element_ty = field_key
                        .and_then(|k| shape.field(&k))
                        .map(|f| f.ty.clone())
                        .unwrap_or(Type::mixed());
                    self.check_param_out_assignment(&item.value, &element_ty);
                    self.check_by_ref_destructuring(&item.value, &element_ty);
                }
            }
            Type::Array(..) | Type::Intersection(_) => {
                let element_ty = ty.array_value_type().unwrap_or(Type::mixed());
                for item in items {
                    self.check_param_out_assignment(&item.value, &element_ty);
                    self.check_by_ref_destructuring(&item.value, &element_ty);
                }
            }
            _ => {
                for item in items {
                    self.check_param_out_assignment(&item.value, &Type::mixed());
                    self.check_by_ref_destructuring(&item.value, &Type::mixed());
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
        purge_aliased_index_entries(lhs, locals);
        if let Some(key) = narrowing_key(lhs) {
            // Also drops any deeper stale entry rooted at this exact
            // key (`"key->prop"`, `"key[lit]"`) or that used `key` as a
            // variable subscript elsewhere (`"other[$key]"`) - real
            // invalidation-on-reassignment, generalized over any
            // `narrowing_key` shape (bare variable, property chain, or
            // index chain), not just a bare variable.
            purge_stale_narrowing_for(locals, &key);
            let stored = self.property_value_after_write(lhs, &ty, locals);
            locals.insert(key, stored);
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
            ExprKind::ArrayLiteral(items) | ExprKind::List(items) => match &ty {
                // A known array shape (from a `@return array{0: int, 1:
                // string}`-typed source, most commonly) carries real
                // per-position/per-key types - a plain homogeneous
                // `Type::Array` fallback would silently discard that
                // precision. A keyed destructuring element (`['key' =>
                // $x] = $shaped`) looks up its own literal key directly;
                // a positional element (`[$a, $b] = $shaped`, the common
                // tuple-destructuring case) looks up the field whose key
                // is that position's own index ("0", "1", ...).
                Type::ArrayShape(shape) => {
                    for (position, item) in items.iter().enumerate() {
                        let field_key = match item.key.as_ref().map(|k| &k.kind) {
                            Some(ExprKind::Literal(Literal::String(s))) => {
                                Some(ShapeKey::from_name(&s.to_str_lossy()))
                            }
                            Some(ExprKind::Literal(Literal::Int(n))) => Some(ShapeKey::Int(*n)),
                            Some(_) => None,
                            None => Some(ShapeKey::Int(position as i64)),
                        };
                        let element_ty = field_key
                            .and_then(|k| shape.field(&k))
                            .map(|f| f.ty.clone())
                            .unwrap_or(Type::mixed());
                        self.bind_assignment_target(&item.value, element_ty, locals);
                    }
                }
                // `Type::array_value_type` also sees through an
                // `Intersection`-wrapped `list<T>`/`non-empty-array<T>` -
                // without it, destructuring one (`[$a, $b] = $listTypedVar`)
                // would lose its real element type down to `Mixed`.
                Type::Array(..) | Type::Intersection(_) => {
                    let element_ty = ty.array_value_type().unwrap_or(Type::mixed());
                    for item in items {
                        self.bind_assignment_target(&item.value, element_ty.clone(), locals);
                    }
                }
                _ => {
                    for item in items {
                        self.bind_assignment_target(&item.value, Type::mixed(), locals);
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
                            && let Some((subject, value)) = self.active_discriminant.last().cloned()
                        {
                            let key = (name.to_string(), subject, value);
                            let prior = locals
                                .discriminant_array_facts
                                .get(&key)
                                .cloned()
                                .unwrap_or_else(|| Type::array_shape(Shape::sealed(Vec::new())));
                            if let Some(new_slot_ty) = prior.set_offset_value_type(
                                None,
                                ty.clone(),
                                false,
                                self.loop_depth == 0,
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
                        if let Some(new_ty) = current.set_offset_value_type(
                            idx_ty.as_ref(),
                            ty,
                            in_range,
                            self.loop_depth == 0,
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
                        if let Some(key) = narrowing_key(base)
                            && let Some(current) = locals.get(&key).cloned()
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
        let shapes: Vec<&Shape> = match &inner_ty {
            Type::ArrayShape(shape) => vec![shape],
            Type::Union(members) => {
                let shapes: Vec<_> = members
                    .iter()
                    .filter_map(|m| match m {
                        Type::ArrayShape(shape) => Some(shape),
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
        if shapes.iter().any(|shape| !shape.is_sealed()) {
            return self.infer_expr(e, locals);
        }
        let absent = match idx.as_deref() {
            None => true,
            Some(i) => match literal_string_key(i) {
                Some(key) => {
                    let key = ShapeKey::from_name(&key);
                    !shapes.iter().any(|shape| shape.field(&key).is_some())
                }
                None => shapes.iter().all(|shape| shape.fields.is_empty()),
            },
        };
        if absent {
            Type::array_shape(Shape::sealed(Vec::new()))
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
    pub(crate) fn check_offset_existence(&mut self, base: &Expr, idx: &Expr, locals: &Locals) {
        let container = self.infer_expr(base, locals);
        let dim = self.infer_expr(idx, locals);
        self.report_missing_offset(&container, &dim, idx.span);
    }

    /// `ArrayDestructuringRule`'s per-key half: every destructured key
    /// (a position, or an explicit key) checked against the destructured
    /// value, recursing into a nested destructuring target.
    pub(crate) fn check_destructuring_offsets(
        &mut self,
        items: &[ArrayItem],
        container: &Type,
        locals: &Locals,
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
                    (Type::int_literal(position - 1), item.value.span)
                }
            };
            if self.report_missing_offset(container, &dim, span) {
                continue;
            }
            if let ExprKind::ArrayLiteral(inner) | ExprKind::List(inner) = &item.value.kind {
                let array_key = dim.to_array_key();
                let key = array_key
                    .int_literal_value()
                    .map(|n| n.to_string())
                    .or_else(|| match array_key.constant_strings()[..] {
                        [s] => Some(s.to_string()),
                        _ => None,
                    });
                let inner_container =
                    crate::checker::expr::index_result_type(container.clone(), key.as_deref());
                self.check_destructuring_offsets(inner, &inner_container, locals);
            }
        }
    }

    /// Reports `offsetAccess.notFound` when `container` provably lacks
    /// `dim`. Returns whether it did.
    pub(crate) fn report_missing_offset(
        &mut self,
        container: &Type,
        dim: &Type,
        span: syntax::token::Span,
    ) -> bool {
        if !container.has_offset_value_type(dim).no() {
            return false;
        }
        self.report(
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
    pub(crate) fn check_offset_accessibility(&mut self, base_ty: &Type, idx: &Expr) {
        if self.relation().is_offset_accessible(base_ty).no() {
            self.report(
                format!("Cannot access offset on {base_ty}."),
                idx.span,
                3,
                "offsetAccess.nonOffsetAccessible",
            );
        } else if let Some(level) = self.strict_mixed_receiver_level(base_ty) {
            self.report(
                "Cannot access offset on mixed.".to_string(),
                idx.span,
                level,
                "offsetAccess.nonOffsetAccessible",
            );
        }
    }
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
        "argc" => Type::int(),
        _ if is_superglobal(name) => {
            Type::array(Type::benevolent_array_key(), Type::explicit_mixed())
        }
        _ => Type::mixed(),
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
    let ExprKind::Index(base, Some(idx)) = &lhs.kind else {
        if let ExprKind::Index(base, None) = &lhs.kind {
            purge_aliased_index_entries(base, locals);
        }
        return;
    };
    purge_aliased_index_entries(base, locals);
    let Some(base_key) = narrowing_key(base) else {
        return;
    };
    let siblings = format!("{base_key}[");
    let variable_siblings = format!("{base_key}[$");
    let prefix = if literal_string_key(idx).is_some() {
        &variable_siblings
    } else {
        &siblings
    };
    let stale: Vec<String> = locals
        .iter()
        .filter(|(k, _)| k.starts_with(prefix.as_str()))
        .map(|(k, _)| k.clone())
        .collect();
    for k in stale {
        locals.remove(&k);
    }
}

pub(crate) fn literal_string_key(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.to_string()),
        ExprKind::Literal(Literal::Int(n)) => Some(n.to_string()),
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
        Type::StringLiteral(s) => vec![s.to_string()],
        Type::IntLiteral(n) => vec![n.to_string()],
        Type::Union(members) => members.iter().flat_map(literal_keys_from_type).collect(),
        _ => Vec::new(),
    }
}
