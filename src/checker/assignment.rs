use crate::ast::*;
use crate::checker::{
    Checker, Locals, is_unsealed_shape, narrowing_key, purge_stale_narrowing_for, union_types,
};
use crate::types::{AccessoryKind, Type, is_subtype};

impl<'a> Checker<'a> {
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
            // check_expr's own Index arm would call check_shape_key_access
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
            // being read, so `check_shape_key_access` must not run on it
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
                    // Unlike `check_shape_key_access` (deliberately
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
                locals.declare(name.clone(), ty, lhs.span);
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
                            Some(ExprKind::Literal(Literal::String(s))) => Some(s.clone()),
                            Some(ExprKind::Literal(Literal::Int(n))) => Some(n.to_string()),
                            Some(_) => None,
                            None => Some(position.to_string()),
                        };
                        let element_ty = field_key
                            .and_then(|k| fields.iter().find(|(fk, _, _)| *fk == k))
                            .map(|(_, t, _)| t.clone())
                            .unwrap_or(Type::Mixed);
                        self.bind_assignment_target(&item.value, element_ty, locals);
                    }
                }
                // `Self::array_element_type` also sees through an
                // `Intersection`-wrapped `list<T>`/`non-empty-array<T>` -
                // without it, destructuring one (`[$a, $b] = $listTypedVar`)
                // would lose its real element type down to `Mixed`.
                Type::Array(..) | Type::Intersection(_) => {
                    let element_ty = Self::array_element_type(&ty).unwrap_or(Type::Mixed);
                    for item in items {
                        self.bind_assignment_target(&item.value, element_ty.clone(), locals);
                    }
                }
                _ => {
                    for item in items {
                        self.bind_assignment_target(&item.value, Type::Mixed, locals);
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
                        if let Some(new_ty) =
                            write_index_into(&current, idx.as_deref(), idx_ty.as_ref(), ty)
                        {
                            if was_defined || is_superglobal(name) {
                                locals.insert(name.clone(), new_ty);
                            } else {
                                locals.declare(name.clone(), new_ty, base.span);
                            }
                        }
                    }
                    ExprKind::Index(..) => {
                        let current_base_ty = self.infer_expr(base, locals);
                        if let Some(new_base_ty) =
                            write_index_into(&current_base_ty, idx.as_deref(), idx_ty.as_ref(), ty)
                        {
                            self.bind_assignment_target(base, new_base_ty, locals);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    pub(crate) fn bind_simple_target(&self, target: &Expr, ty: Type, locals: &mut Locals) {
        if let ExprKind::Variable(name) = &target.kind {
            purge_stale_narrowing_for(locals, name);
            // Real rebind - `target` is a `foreach` key's bare `$var`
            // expression (its only caller), a real declaration site.
            self.probe_hit_at(target.span, &ty);
            locals.declare(name.clone(), ty, target.span);
        }
    }

    /// Flags `$x['key']` when `$x`'s type is a known `ArrayShape` and 'key'
    /// isn't one of its declared fields - a real typo-catcher, not a guess:
    /// only fires when the shape is fully known and the key is a literal,
    /// so a dynamic key or an unknown array type is never flagged. Reading
    /// a genuinely *optional* field is not flagged either - that's a
    /// possibly-undefined-index warning, a different (and noisier) check
    /// this prototype doesn't attempt.
    ///
    /// Also handles a `Type::Union` of `ArrayShape`s (e.g. `preg_match`'s
    /// `$matches`, unioned with an empty shape for the no-match case) -
    /// present in any member is enough not to flag, generalizing the
    /// same per-shape `optional`-field leniency across the whole `Union`
    /// rather than only within one shape. A member that isn't an
    /// `ArrayShape` at all means "don't know" for at least part of the
    /// type, so the whole check backs off.
    pub(crate) fn check_shape_key_access(
        &mut self,
        base: &Expr,
        idx: &Expr,
        locals: &Locals,
        label: &str,
    ) {
        let Some(key) = literal_string_key(idx) else {
            return;
        };
        let ty = self.infer_expr(base, locals);
        let members: Vec<Type> = match ty {
            Type::Union(m) => m,
            other => vec![other],
        };
        let shapes: Vec<&Vec<(String, Type, bool)>> = members
            .iter()
            .filter_map(|m| match m {
                Type::ArrayShape(fields) => Some(fields),
                _ => None,
            })
            .collect();
        if shapes.is_empty() || shapes.len() != members.len() {
            return;
        }
        // An unsealed shape (`is_unsealed_shape`'s doc comment) may
        // carry the key even though it isn't one of its declared fields -
        // the same "any union member could plausibly have it" leniency
        // this check extends to a key genuinely declared on only one arm
        // of the union, just for a different reason.
        if shapes.iter().any(|fields| {
            fields.iter().any(|(name, _, _)| *name == key) || is_unsealed_shape(fields)
        }) {
            return;
        }
        self.report(
            label.to_string(),
            format!("array{{...}} has no key '{key}'"),
            idx.span,
            2,
            "shapeAccess.notFound",
        );
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
/// `array`, never `null`/`mixed`. Reusing the generic `Type::Mixed`
/// fallback here (as if a superglobal's pre-assignment type were
/// "unknown/could be anything") broke `$_SESSION ??= [];` specifically:
/// `widen_coalesce_result`'s own "keep the left side's real array type
/// instead of collapsing to the empty-shape fallback" rule only fires
/// for an already-known `Type::Array`. Scoped to exactly the real declared
/// type: `Array(Mixed)` for the always-array superglobals, `Int` for
/// `argc` (never `[]` in the stub at all).
pub(crate) fn superglobal_default_type(name: &str) -> Type {
    match name {
        "argc" => Type::Int,
        _ if is_superglobal(name) => {
            Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed))
        }
        _ => Type::Mixed,
    }
}

/// Extracts a shape-lookup key from a literal index expression - `'name'`
/// or a bare int (PHP array keys are always stringified/int-normalized
/// internally, so both forms are compared the same way here). A
/// non-literal index (a variable, a call, ...) returns `None`, since it
/// can't be resolved against a shape's field names statically.
pub(crate) fn literal_string_key(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.clone()),
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
/// Maps `idx_ty` to a definite array key type for a write - `Type::Int`/
/// `Type::String` when `idx_ty` is confidently one or the other, otherwise
/// `unknown_array_key()` rather than a wrong guess.
fn dynamic_write_key_type(idx_ty: &Type) -> Type {
    if is_subtype(idx_ty, &Type::Int) {
        Type::Int
    } else if is_subtype(idx_ty, &Type::String) {
        Type::String
    } else {
        Type::unknown_array_key()
    }
}

fn write_index_into(
    container_ty: &Type,
    idx: Option<&Expr>,
    idx_ty: Option<&Type>,
    value_ty: Type,
) -> Option<Type> {
    match container_ty {
        // `Null` is the same "not really an array yet" case a top-level
        // undefined-variable write already auto-vivifies from a fresh
        // empty `ArrayShape` (real PHP auto-vivifies a null-valued offset
        // too) - delegate rather than bail, so a `Union` member that's
        // provably `Null` doesn't sink the whole write via `Union`'s
        // "all or nothing" `?`.
        Type::Null => write_index_into(&Type::ArrayShape(vec![]), idx, idx_ty, value_ty),
        Type::ArrayShape(fields) => match idx.and_then(literal_string_key) {
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
        // precisely anymore, so a write at any key merges into the
        // shared element type instead of being ignored. The container's
        // existing key type is unioned with a confidently-narrowed
        // dynamic key's type rather than kept unconditionally unchanged;
        // `union_types(existing_key, unknown_array_key())` collapses
        // back to `unknown_array_key()` when there's nothing to narrow.
        Type::Array(key, inner) => {
            let key_ty = match idx.and(idx_ty).map(dynamic_write_key_type) {
                Some(new_key_ty) => union_types((**key).clone(), new_key_ty),
                None => (**key).clone(),
            };
            Some(Type::Array(
                Box::new(key_ty),
                Box::new(union_types(value_ty, (**inner).clone())),
            ))
        }
        // A container typed as a `Union` (e.g. differently-shaped
        // `ArrayShape`s merged across an `if`/`else`) must update every
        // member the same way, bailing via `?` if any one can't be -
        // otherwise a later read through the same `Union` sees a key
        // written one line earlier as still missing.
        Type::Union(members) => {
            let mut updated = Vec::with_capacity(members.len());
            for m in members {
                updated.push(write_index_into(m, idx, idx_ty, value_ty.clone())?);
            }
            Some(
                updated
                    .into_iter()
                    .reduce(union_types)
                    .unwrap_or(Type::Mixed),
            )
        }
        // A `list<T>`/`non-empty-array<T>` container is an
        // `Intersection([Array(T), Accessory(_)])`. A dynamic-key write
        // drops any `Accessory` marker - an arbitrary key can't preserve
        // a list's sequential-key guarantee - but a real append (`idx`
        // absent) onto an already-list-marked container keeps it a list,
        // now provably non-empty.
        Type::Intersection(members) => {
            let inner = members.iter().find_map(Checker::array_element_type)?;
            let widened = union_types(value_ty, inner);
            let key_ty = idx
                .and(idx_ty)
                .map(dynamic_write_key_type)
                .unwrap_or_else(Type::unknown_array_key);
            if idx.is_none() && members.contains(&Type::Accessory(AccessoryKind::ArrayList)) {
                Some(Type::Intersection(vec![
                    Type::Array(Box::new(key_ty), Box::new(widened)),
                    Type::Accessory(AccessoryKind::ArrayList),
                    Type::Accessory(AccessoryKind::NonEmptyArray),
                ]))
            } else {
                Some(Type::Array(Box::new(key_ty), Box::new(widened)))
            }
        }
        _ => None,
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
        ExprKind::Literal(Literal::String(s)) => Some(std::borrow::Cow::Borrowed(s.as_str())),
        ExprKind::Grouping(inner) => literal_string_value(inner),
        ExprKind::Binary(BinOp::Concat, lhs, rhs) => {
            let l = literal_string_value(lhs)?;
            let r = literal_string_value(rhs)?;
            Some(std::borrow::Cow::Owned(format!("{l}{r}")))
        }
        _ => None,
    }
}
