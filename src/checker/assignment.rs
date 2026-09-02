use crate::ast::*;
use crate::checker::{Checker, Locals, narrowing_key, purge_stale_narrowing_for, union_types};
use crate::types::{self, Type};

impl<'a> Checker<'a> {
    /// The actual variable-binding side effect of an assignment - lives in
    /// `check_expr` (called from *every* place an assignment expression can
    /// appear: a bare expression statement, a `for` init/step, a `while`
    /// condition, ...) rather than a separate pass triggered only for
    /// top-level expression statements, which is exactly what let `for ($j
    /// = 0; ...)`'s `$j` go unbound (and then misfire as "undefined") until
    /// this was caught.
    /// Checks an assignment's own left-hand side for whatever isn't a
    /// pure "definition site" - a plain variable (or nested variable,
    /// recursing through any level of `[$a, $b] = ...`-style
    /// destructuring) is never a read, but an `Index`'s own key
    /// expression and a `PropertyAccess`'s own base still are. Shared
    /// between the top-level assignment target and every nested
    /// destructuring element, so `[[$a, $b], $c] = $nested;` doesn't
    /// wrongly check `$a`/`$b` as reads of not-yet-defined variables the
    /// way only handling one level of nesting used to.
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
            ExprKind::Index(base, idx) => {
                self.check_expr(base, locals, label);
                if let Some(i) = idx {
                    self.check_expr(i, locals, label);
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
        // Phase 22: keep any existing `narrowing_key` entry for this
        // exact property-chain/literal-index-chain expression in sync
        // with a real assignment - `apply_narrowing`'s own synthetic key
        // format is a separate slot in the same `Locals` map (see
        // `narrowing_key`'s own doc comment), and without this,
        // reassigning a property/element a prior condition already
        // narrowed left the *old* narrowed value sitting under that key
        // forever - a real, common idiom this broke: `if ($this->entries
        // === null) { $this->entries = [...]; } return $this->entries;`,
        // where `infer_expr`'s own "check narrowed key first" fast path
        // kept reading back the stale `null` from the condition check
        // instead of the real, newly-assigned array. Redundant with (but
        // harmless alongside) the `Variable` arm below, which does the
        // same insert under the identical key for that one case.
        if let Some(key) = narrowing_key(lhs) {
            // Phase 33 Finding 5: also drop any *deeper* stale entry
            // rooted at this exact key (`"key->prop"`, `"key[lit]"`) or
            // that used `key` as a variable subscript elsewhere
            // (`"other[$key]"`) - real invalidation-on-reassignment,
            // generalized over any `narrowing_key` shape (bare variable,
            // property chain, or index chain), not just a bare variable.
            purge_stale_narrowing_for(locals, &key);
            locals.insert(key, ty.clone());
        }
        match &lhs.kind {
            ExprKind::Variable(name) => {
                locals.insert(name.clone(), ty);
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
                Type::Array(inner) => {
                    let element_ty = (**inner).clone();
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
            // Finding 48's own follow-on: `$x['newKey'] = $value;` adds a
            // real key to `$x`'s own tracked shape when `$x` already
            // holds a known one (most commonly, from an earlier `$x = []`
            // or `$x = ['a' => 1]` literal) - without this, an array
            // literal's own new precision made the extremely common
            // "start from a literal, build it up one key at a time"
            // idiom look like every later read was accessing a key the
            // shape could never have, since the shape itself never grew
            // past its own starting literal. Only updates a *known*
            // shape already sitting in `locals` under a bare variable
            // name; anything else (a property chain, a variable with a
            // not-yet-known type) is left alone, matching this project's
            // usual "don't guess" scope.
            //
            // Phase 22 Finding 2's own follow-on: a *nested* write
            // (`$items[$day]['derivative'] = $derivative;` - `base` is
            // itself an `Index`, not a bare `Variable`) used to be
            // silently ignored entirely, which interacted badly with
            // Finding 2's own widening - a real corpus example
            // (`CalendarMonthly::buildMonthCalendar`) builds `$items` via
            // `$items[(int) $row->period] = [...]` (Finding 2 widens this
            // to `Type::Array`), then adds more fields per-element via
            // `$items[$day]['derivative'] = ...`/`['file'] = ...` -
            // ignoring those nested writes meant `$items[$day]`'s
            // inferred element type stayed frozen at the *initial*
            // literal's own fields, so reading `$items[$day]['derivative']`
            // afterward was flagged as a genuinely new false positive,
            // not present before Finding 2's own widening made `$items`
            // precise enough to check in the first place. Handled by
            // recursing: compute `base`'s own current type, work out its
            // new type as if `base[idx] = ty` had just happened to it,
            // then bind *that* back onto `base` - which lands in this
            // same arm again if `base` itself is another `Index`,
            // correctly handling any nesting depth.
            ExprKind::Index(base, idx) => match &base.kind {
                ExprKind::Variable(name) => {
                    let Some(current) = locals.get(name).cloned() else {
                        return;
                    };
                    if let Some(new_ty) = write_index_into(&current, idx.as_deref(), ty) {
                        locals.insert(name.clone(), new_ty);
                    }
                }
                ExprKind::Index(..) => {
                    let current_base_ty = self.infer_expr(base, locals);
                    if let Some(new_base_ty) =
                        write_index_into(&current_base_ty, idx.as_deref(), ty)
                    {
                        self.bind_assignment_target(base, new_base_ty, locals);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    pub(crate) fn bind_simple_target(&self, target: &Expr, ty: Type, locals: &mut Locals) {
        if let ExprKind::Variable(name) = &target.kind {
            purge_stale_narrowing_for(locals, name);
            locals.insert(name.clone(), ty);
        }
    }

    pub(crate) fn check_property_assignment(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        locals: &Locals,
        label: &str,
    ) {
        let ExprKind::PropertyAccess(base, prop, _) = &lhs.kind else {
            return;
        };
        let Type::Object(class, type_args) = self.infer_expr(base, locals) else {
            return;
        };
        let Some((owner, prop_sig)) = self.registry.lookup_property_with_owner(&class, prop) else {
            // Not a declared property: could be a dynamic property, which
            // is legal (if deprecated) PHP - not flagged here, only wrong
            // types on properties we do know about.
            return;
        };
        // Finding 47: a `readonly` property may only be initialized once,
        // from within the scope of the class that declares it - in
        // practice, almost always its own constructor. Pragmatic first
        // cut (per the plan's own text): flag every write outside the
        // literal syntactic body of the declaring class's own
        // constructor, rather than attempting real "has this exact
        // property already been initialized" dataflow - a promoted
        // constructor parameter never reaches this check at all (no
        // `ExprKind::Assign` node exists for it in source), so it counts
        // as the allowed first write automatically.
        if prop_sig.is_readonly
            && (self.current_class.as_deref() != Some(owner.as_str())
                || self.current_method.as_deref() != Some("__construct"))
        {
            self.report(
                label.to_string(),
                format!("{owner}::${prop} is readonly and cannot be modified here"),
                lhs.span,
                0,
            );
        }
        let bindings = self.class_template_bindings(&class, &type_args, &owner);
        let expected = types::substitute_template(&prop_sig.ty, &bindings);
        let expected = self.resolve_pseudo(&expected, &class);
        let actual = self.infer_expr(rhs, locals);
        if actual == Type::Mixed || self.registry.is_assignable(&actual, &expected) {
            return;
        }
        self.report(
            label.to_string(),
            format!("{class}::${prop} expects {expected}, {actual} given"),
            rhs.span,
            3,
        );
    }

    /// Flags `$x['key']` when `$x`'s type is a known `ArrayShape` and 'key'
    /// isn't one of its declared fields - a real typo-catcher, not a guess:
    /// only fires when the shape is fully known and the key is a literal,
    /// so a dynamic key or an unknown array type is never flagged. Reading
    /// a genuinely *optional* field is not flagged either - that's a
    /// possibly-undefined-index warning, a different (and noisier) check
    /// this prototype doesn't attempt.
    ///
    /// Phase 12 Finding E: also handles a `Type::Union` of `ArrayShape`s
    /// (e.g. `preg_match`'s own `$matches`, unioned with an empty shape
    /// for the no-match case) - present in *any* member is enough not to
    /// flag, generalizing the same per-shape `optional`-field leniency
    /// above across the whole `Union` rather than only within one shape.
    /// A member that isn't an `ArrayShape` at all (`Mixed`, or anything
    /// else) means "don't know" for at least part of the type, so the
    /// whole check backs off, the same leniency already granted a single
    /// unknown array type.
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
        if shapes
            .iter()
            .any(|fields| fields.iter().any(|(name, _, _)| *name == key))
        {
            return;
        }
        self.report(
            label.to_string(),
            format!("array{{...}} has no key '{key}'"),
            idx.span,
            2,
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
fn write_index_into(container_ty: &Type, idx: Option<&Expr>, value_ty: Type) -> Option<Type> {
    match container_ty {
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
                Some(Type::Array(Box::new(widened)))
            }
        },
        // Already widened past a literal shape (by an earlier append/
        // dynamic-key write) - no single key to grow precisely anymore,
        // so a write at *any* key (literal or not: once homogeneous,
        // every element shares one type regardless) merges into the
        // shared element type instead of being ignored outright. Without
        // this, a nested write through an already-widened container
        // (`$items[(int) $row->period] = [...]` widening `$items`, then
        // `$items[$day]['derivative'] = ...` writing through it) stayed
        // silently untracked, leaving `$items[$day]`'s inferred type
        // frozen at its *initial* shape and producing a real false
        // positive once that initial shape was read back with a field
        // only ever added by the untracked nested write.
        Type::Array(inner) => Some(Type::Array(Box::new(union_types(
            value_ty,
            (**inner).clone(),
        )))),
        _ => None,
    }
}

/// A literal string's own text, or `None` for anything else (a variable,
/// a concatenation, ...) - unlike `literal_string_key`, this doesn't also
/// accept/stringify an int literal, since a format string is never
/// legitimately written as a bare number.
pub(crate) fn literal_string_value(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.as_str()),
        _ => None,
    }
}
