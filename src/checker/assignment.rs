use crate::ast::*;
use crate::checker::{Checker, Locals};
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
            // name; anything else (a property chain, a computed base, a
            // variable with a non-shape or not-yet-known type) is left
            // alone, matching this project's usual "don't guess" scope.
            ExprKind::Index(base, Some(idx)) => {
                if let ExprKind::Variable(name) = &base.kind
                    && let Some(key) = literal_string_key(idx)
                    && let Some(Type::ArrayShape(fields)) = locals.get(name)
                {
                    let mut fields = fields.clone();
                    match fields.iter_mut().find(|(k, _, _)| *k == key) {
                        Some(field) => field.1 = ty,
                        None => fields.push((key, ty, false)),
                    }
                    locals.insert(name.clone(), Type::ArrayShape(fields));
                }
            }
            _ => {}
        }
    }

    pub(crate) fn bind_simple_target(&self, target: &Expr, ty: Type, locals: &mut Locals) {
        if let ExprKind::Variable(name) = &target.kind {
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
    pub(crate) fn check_shape_key_access(
        &mut self,
        base: &Expr,
        idx: &Expr,
        locals: &Locals,
        label: &str,
    ) {
        let Type::ArrayShape(fields) = self.infer_expr(base, locals) else {
            return;
        };
        let Some(key) = literal_string_key(idx) else {
            return;
        };
        if fields.iter().any(|(name, _, _)| *name == key) {
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
