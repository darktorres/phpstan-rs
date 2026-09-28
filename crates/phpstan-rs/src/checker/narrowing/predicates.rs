use crate::checker::{Checker, Locals, literal_string_key, union_types};
use syntax::ast::*;
use types::Type;

/// The expression `current_narrowed_type`'s own
/// fallback should actually infer from, when no prior narrowing entry
/// exists yet. `narrowing_key`'s own unwrapping (`Grouping`, `Assign`,
/// the literal-`null`-coalesce case below) picks a *key* based on the
/// real underlying target, but inferring the type straight from the
/// original (still-wrapped) expression is only safe for some of those
/// shapes: `Grouping` is transparent (`infer_expr` already sees straight
/// through it), and `Assign` is deliberately *not* unwrapped here -
/// `infer_expr` on the whole `Assign` already gives exactly the
/// right-hand side's own type, which is what ends up stored under the
/// left-hand side too, so unwrapping it further would infer from the
/// *pre-assignment* value instead. A `??`-wrapped expression is
/// genuinely different: `infer_expr` on the whole coalesce already
/// strips `null` via the coalesce's own semantics before an `is_int`/
/// `is_string`/etc. predicate gets a chance to narrow anything from the
/// *real* left-hand type - inferring from the real target directly is
/// what lets `is_int($x['k'] ?? null)` see `$x['k']`'s own true type
/// (`int|null`), not the coalesce result's already-narrower one.
pub(crate) fn narrowing_infer_target(e: &Expr) -> &Expr {
    match &e.kind {
        ExprKind::Grouping(inner) | ExprKind::ErrorSuppress(inner) => narrowing_infer_target(inner),
        ExprKind::Binary(BinOp::Coalesce, lhs, rhs)
            if matches!(&rhs.kind, ExprKind::Literal(Literal::Null)) =>
        {
            narrowing_infer_target(lhs)
        }
        _ => e,
    }
}

/// A stable string key for the small set of expressions this checker can
/// narrow: a bare local variable, a property-access chain rooted in one
/// (`$a->b->c`), or a literal-key array-index chain rooted in one
/// (`$a['k1']['k2']`, `$a->b['k']`, any mix of the two). Lets `locals`
/// (otherwise keyed by plain variable name) also carry narrowed
/// property-chain/array-index types, reusing the exact same map rather
/// than a second, parallel one - safe because every synthetic key this
/// produces contains a character (`-` from `->`, or `[`/`]`) that can
/// never appear in a real PHP variable name (see `is_real_variable_key`,
/// the discriminator this invariant exists to support), so the key
/// spaces can't collide.
///
/// Without the `PropertyAccess` case, `apply_narrowing`'s `instanceof`
/// handling could only ever narrow a bare variable - `$arg->value
/// instanceof Variable && $arg->value->name` (a common AST-visitor
/// idiom - narrow a property, then read a property only the
/// narrowed-to subtype has) would have no way to remember that
/// narrowing, so the second half would be checked against
/// `$arg->value`'s un-narrowed declared type.
///
/// The identical gap applies to a literal array-index chain instead of
/// a property chain: `is_string($lang_info['jquery_code'])` must narrow
/// `$lang_info['jquery_code']` too, or a later read of the same
/// expression sees its un-narrowed, wider declared/inferred type. A
/// dynamic key (`$a[$k]`) can't be narrowed this way - the same key at another
/// point in the code isn't guaranteed to be the same runtime index - so
/// only a literal key participates, matching `literal_string_key`'s own
/// existing use for the analogous shape-growing case in
/// `bind_assignment_target`.
pub(crate) fn narrowing_key(e: &Expr) -> Option<String> {
    match &e.kind {
        // No prefix - `locals` is keyed by bare variable name everywhere
        // else in this checker (`ExprKind::Variable(name) => locals.get(name)`),
        // and this has to produce the exact same key for a bare variable
        // to stay a drop-in replacement at the Instanceof-narrowing call
        // site.
        ExprKind::Variable(name) => Some(name.to_string()),
        ExprKind::PropertyAccess(base, prop, _) => {
            narrowing_key(base).map(|base_key| format!("{base_key}->{prop}"))
        }
        // A static property (`self::$adminSessionCookies !== null`)
        // needs a narrowing key too, not just an instance property's
        // `->` chain. Scoped to a real static property specifically
        // (the parser's own `$`-prefixed `member` encoding,
        // distinguishing it from a class constant or `Foo::class`) -
        // both this arm and the generic write-side purge-and-update in
        // `bind_assignment_target` key off `narrowing_key` uniformly,
        // so a later `self::$prop = ...` correctly invalidates/replaces
        // this same entry with no additional wiring needed.
        ExprKind::StaticPropertyFetch(StaticTarget::Name(class_name, _), name) => {
            Some(format!("{class_name}::${name}"))
        }
        ExprKind::Index(base, Some(idx)) => {
            let base_key = narrowing_key(base)?;
            match literal_string_key(idx) {
                Some(key) => Some(format!("{base_key}[{key}]")),
                // `is_string($this->mirror[$key])`
                // where `$key` is a variable, not a literal - a `$`-
                // prefixed marker distinguishes "keyed by this variable's
                // current value" from a literal key's own text (which
                // `is_real_variable_key` already treats as synthetic
                // either way, so this can't be mistaken for a real bare-
                // variable key). Only sound alongside real invalidation
                // of this entry whenever `subscript_name` is reassigned -
                // see `purge_stale_narrowing_for`, wired in everywhere a
                // bare variable's value can change.
                None => match &idx.kind {
                    ExprKind::Variable(subscript_name) => {
                        Some(format!("{base_key}[${subscript_name}]"))
                    }
                    _ => None,
                },
            }
        }
        // `is_string($result = expr)` - an assignment nested
        // inside the narrowed expression itself, the common "assign-then-
        // check" idiom. An assignment's own runtime value is exactly
        // what ends up stored under its left-hand side, so whatever gets
        // checked here is equally true of `lhs`'s own key afterward -
        // narrowing key can borrow the underlying variable/property/index
        // key without needing its own dedicated storage.
        ExprKind::Assign(lhs, _) | ExprKind::AssignRef(lhs, _) => narrowing_key(lhs),
        // `($file = readdir($dir)) !== false` - the
        // "assign-then-check" idiom above, but wrapped in parens, which
        // real PHP requires for the statement's own top-level shape
        // (`$file = readdir($dir) !== false` would parse as `$file =
        // (readdir($dir) !== false)` instead - `=` binds looser than
        // `!==` there). Without this, `Assign`'s own arm above never
        // gets a chance to run for the single most common shape it
        // exists to cover. The comparison-operand position specifically
        // (`false !== $file = readdir($dir)`, no parens, no assignment
        // at the very top of the statement) doesn't need this arm at
        // all - `parse_assignment_tail` already builds the right
        // `Assign` node directly as the comparison's own operand - but
        // plenty of real code still writes the parens out of habit, so
        // both arms stay.
        ExprKind::Grouping(inner) | ExprKind::ErrorSuppress(inner) => narrowing_key(inner),
        // `is_int($decoded['status'] ?? null)` - a
        // common defensive-coding idiom (json_decode results,
        // `$_SERVER`/`$_GET` reads, ...). Scoped to a literal `null`
        // fallback specifically, not any coalesce: `is_int($x['k'] ??
        // null)` being true can only happen if `$x['k']` itself was
        // already `int` (the fallback `null` would fail `is_int`
        // otherwise, so `null` specifically can never be the reason the
        // predicate passed) - but `is_int($x['k'] ?? 0)` gives no such
        // guarantee, since `$x['k']` being absent/`null` would still make
        // the whole expression `is_int`-true via the fallback `0`,
        // wrongly narrowing `$x['k']` itself away from a real possibility
        // (`null`) it could still hold.
        ExprKind::Binary(BinOp::Coalesce, lhs, rhs)
            if matches!(&rhs.kind, ExprKind::Literal(Literal::Null)) =>
        {
            narrowing_key(lhs)
        }
        _ => None,
    }
}

/// Whether `key` (as found in a `Locals` map) could be a real PHP
/// variable name, as opposed to one of `narrowing_key`'s own synthetic
/// property-chain/array-index keys - real PHP variable names are
/// restricted to `[A-Za-z_][A-Za-z0-9_]*`, so any other character (`-`
/// from `->`, `[`, `]`, ...) proves a key is synthetic, regardless of
/// which of `narrowing_key`'s own formats produced it. A shared,
/// character-class-based check rather than hunting for one specific
/// separator (`contains('-')`, say) - robust against `narrowing_key`
/// gaining a third synthetic shape later without every caller needing
/// its own update to keep excluding it correctly.
pub(crate) fn is_real_variable_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Recognizes `cond` as testing whether a `preg_match` call found a
/// match - bare (`if (preg_match(...))`) or compared against a literal
/// `0`/`1` in a way that unambiguously distinguishes "matched" from "not
/// matched". Deliberately excludes `!== false`/`=== false`: `preg_match`'s
/// return is `int|false`, and `false` doesn't distinguish a real "no
/// match" (`0`) from an actual error - real PHP leaves `$matches`
/// completely unchanged from before the call on error (a malformed
/// pattern with a pre-existing `$m` leaves `$m` untouched), a third
/// state this project has no way to
/// represent (it isn't "the matched shape" or "empty," it's "whatever it
/// was before this statement") - the same imprecision real PHPStan's own
/// extensions accept, not something this closes either.
///
/// Only **strict** `===`/`!==` against exactly 0 or 1 are recognized -
/// PHP's *loose* `==`/`!=` would make `preg_match(...) == 0` ambiguous
/// between "no match" (`0`) and "error" (`false`, which loosely equals
/// `0`: `false == 0` is `true` in PHP). `== 1`/`!= 1`
/// happen to still be safe even loosely (`false == 1` is `false`), but
/// real code overwhelmingly uses strict comparison here already, so
/// dropping loose comparison recognition entirely costs no real
/// coverage.
///
/// Scoped to plain `preg_match` (not `preg_match_all`, which needs none
/// of this), to no-`$flags` callers (mirrors this same
/// scope in `checker/expr.rs`'s own wiring), and to **positional**
/// arguments only - `Arg` carries its own `name: Option<String>` for a
/// named argument, and a bare positional destructure alone could
/// *misidentify* which expression is which under reordered named
/// arguments (`preg_match(subject: $s, pattern: $p, matches: $m)` would
/// otherwise bind `pattern` to what's actually `$s`), so any named
/// argument bails rather than guessing.
///
/// Returns `(pattern_arg, matches_arg, branch_true_means_matched)`.
pub(crate) fn preg_match_condition(cond: &Expr) -> Option<(&Expr, &Expr, bool)> {
    fn as_preg_match_args(e: &Expr) -> Option<&[Arg]> {
        match &e.kind {
            ExprKind::Call(callee, args) => match &callee.kind {
                ExprKind::Name(name) if name.trim_start_matches('\\') == "preg_match" => Some(args),
                _ => None,
            },
            ExprKind::Grouping(inner) | ExprKind::ErrorSuppress(inner) => as_preg_match_args(inner),
            _ => None,
        }
    }
    // A real `fn`, not a closure - a closure can't express the
    // higher-ranked lifetime this needs (called once with `args`
    // borrowed from `cond` directly, once with `call_args` borrowed from
    // `l`/`r`; a closure infers one fixed lifetime that can't satisfy
    // both call sites).
    fn extract(args: &[Arg], matched: bool) -> Option<(&Expr, &Expr, bool)> {
        let [pattern, subject, matches] = args else {
            return None;
        };
        if pattern.name.is_some() || subject.name.is_some() || matches.name.is_some() {
            return None;
        }
        Some((&pattern.value, &matches.value, matched))
    }
    if let Some(args) = as_preg_match_args(cond) {
        return extract(args, true); // bare truthy: true branch = matched
    }
    let ExprKind::Binary(op, l, r) = &cond.kind else {
        return None;
    };
    let (call_args, lit_side) = match (as_preg_match_args(l), as_preg_match_args(r)) {
        (Some(a), _) => (a, r),
        (_, Some(a)) => (a, l),
        _ => return None,
    };
    let ExprKind::Literal(Literal::Int(lit)) = &lit_side.kind else {
        return None;
    };
    match (op, lit) {
        (BinOp::Identical, 1) => extract(call_args, true),
        (BinOp::NotIdentical, 1) => extract(call_args, false),
        (BinOp::Identical, 0) => extract(call_args, false),
        (BinOp::NotIdentical, 0) => extract(call_args, true),
        _ => None,
    }
}

/// Drops every `locals` entry that depended on `key`'s pre-reassignment
/// identity - either because `key` was itself the root a *deeper*
/// synthetic key was built from (`"key->prop"`, `"key[lit]"` - covers
/// both a bare-variable root and a full chain root, e.g. reassigning
/// `$foo->bar` invalidates a stale `"foo->bar->baz"`), or because `key`
/// (when it's a bare variable name) was used as a *variable subscript*
/// inside a different chain entirely (`"other[$key]"`, `narrowing_key`'s
/// own new key shape - never matches for a non-bare-variable `key`,
/// since a subscript marker only ever wraps a plain variable name).
///
/// Without this, nothing invalidates a stale
/// composite-key entry when a variable it depends on changes - not just
/// for a variable-subscript key, but for any composite key at all
/// (reassigning `$foo` would leave a stale `"foo->bar"` sitting in
/// `locals` forever). Called from every real call site that can give a
/// bare variable, property chain, or index chain a new value.
///
/// Iterates and removes by key rather than a `retain` call - `Locals`'s
/// own doc comment (`checker.rs`) only documents `insert`/`remove`/
/// `entry`/`get`/`contains_key`/`iter` as verified-compatible with
/// `std::collections::HashMap` on the underlying `imbl` persistent map,
/// not `retain`.
pub(crate) fn purge_stale_narrowing_for(locals: &mut Locals, key: &str) {
    let stale: Vec<String> = locals
        .iter()
        .filter(|(k, _)| k.as_str() != key && key_depends_on(k, key))
        .map(|(k, _)| k.clone())
        .collect();
    for k in stale {
        locals.remove(&k);
    }
    // `key`'s own per-field container facts (see `ContainerFieldFactsMap`'s
    // doc comment) are just as stale as a composite `narrowing_key` entry
    // rooted at `key` the moment `key` itself is genuinely rebound - this
    // is already called at every real call site that can give a bare
    // variable a new value outside `Locals::declare` (which purges it
    // itself), so riding along here covers all of them uniformly rather
    // than needing its own separate call at each one.
    locals.container_field_facts.remove(key);
    locals.purge_branch_implied_facts_depending_on(key);
    // Same reasoning for `key`'s own per-discriminant accumulator facts
    // (see `DiscriminantArrayFactsMap`'s doc comment) - a wholesale
    // rebind (or an unrecognized by-ref pass) starts `key`'s own value
    // over from scratch, so any accumulated per-branch history recorded
    // for it is stale too.
    locals
        .discriminant_array_facts
        .retain(|(container, _, _), _| container != key);
}

/// Whether narrowing key `k` names `key` itself, a chain rooted at it
/// (`"key->prop"`, `"key[lit]"`), or a chain using `key` as a variable
/// subscript (`"other[$key]"`) - i.e. whether `k`'s value can change
/// when `key` is rebound.
pub(crate) fn key_depends_on(k: &str, key: &str) -> bool {
    k == key
        || k.strip_prefix(key)
            .is_some_and(|rest| rest.starts_with("->") || rest.starts_with('['))
        || k.contains(&format!("[${key}]"))
}

/// Eagerly copies `source_container`'s own recorded per-field facts (see
/// `ContainerFieldFactsMap`'s doc comment) onto `target_name`'s own
/// `narrowing_key` synthetic keys, the moment `target_name` is bound from
/// a dynamic-keyed read of `source_container` (a `foreach` value, or a
/// plain `$x = $container[$dyn];`) - the two pre-existing `narrowing_key`-
/// consulting Index-read fast paths (`checker/expr.rs`'s `check_expr`/
/// `infer_expr`) then pick them up with no further code needed at the
/// read site itself. A no-op when `source_container` has no recorded
/// facts (the overwhelmingly common case).
pub(crate) fn propagate_container_field_facts(
    locals: &mut Locals,
    target_name: &str,
    source_container: &str,
) {
    let Some(facts) = locals.container_field_facts.get(source_container).cloned() else {
        return;
    };
    for (field, ty) in facts.iter() {
        locals.insert(format!("{target_name}[{field}]"), ty.clone());
    }
}

pub(crate) fn narrowing_type_for_predicate(name: &str) -> Option<Type> {
    match name {
        "is_string" => Some(Type::string()),
        "is_int" | "is_integer" | "is_long" => Some(Type::int()),
        "is_float" | "is_double" => Some(Type::float()),
        "is_bool" | "is_boolean" => Some(Type::bool()),
        // Real PHPStan's own `IsArrayFunctionTypeSpecifyingExtension`
        // narrows to `ArrayType(MixedType(true), MixedType(true))` -
        // explicit, not implicit: this is a deliberate "the narrowed type
        // really is array-of-anything" answer, not a "we don't know"
        // fallback (confirmed reading `vendor/phpstan-src` directly).
        "is_array" => Some(Type::array(
            Type::unknown_array_key(),
            Type::explicit_mixed(),
        )),
        // `is_countable($x)` is true for exactly
        // `array|Countable` - guards `count($x)` in a correct PHP 8+
        // idiom (`count()` on anything else is a `TypeError`).
        "is_countable" => Some(Type::union_of(vec![
            Type::array(Type::unknown_array_key(), Type::mixed()),
            Type::object("Countable".into(), Vec::new()),
        ])),
        // Real PHP's own native `iterable` type hint, defined
        // as exactly `array|Traversable` - the same target `types::
        // parse_type_string`'s own dedicated `"iterable"` arm resolves
        // the native type hint to, kept in sync deliberately (both are
        // "what does `iterable` actually mean" in one place each).
        "is_iterable" => Some(Type::union_of(vec![
            Type::array(Type::unknown_array_key(), Type::mixed()),
            Type::object("Traversable".into(), Vec::new()),
        ])),
        "is_null" => Some(Type::null()),
        "is_object" => Some(Type::object_without_class()),
        "is_callable" => Some(Type::callable(None)),
        // `proc_open()`/`fsockopen()` return
        // `resource|false`, commonly guarded with
        // `if (! is_resource($x)) { throw ...; }` before `fclose`/
        // `fwrite`/`fread`/`proc_close`/etc.
        "is_resource" => Some(Type::resource(false)),
        // `is_scalar($x)` is true for exactly `int|float|string|bool` -
        // unlike `is_numeric` (its own dedicated arm above, not a plain
        // table entry here, since a string only *sometimes* qualifies),
        // `is_scalar`'s real set is exactly these four concrete types
        // with no such conditional gap.
        "is_scalar" => Some(Type::union_of(vec![
            Type::int(),
            Type::float(),
            Type::string(),
            Type::bool(),
        ])),
        _ => None,
    }
}

/// If `cond` is an `Or`-tree (any depth, matching a left-associative
/// `A || B || C` chain) whose every leaf is a direct, single-argument
/// call to a name `narrowing_type_for_predicate` recognizes, all
/// narrowing the *identical* expression, returns that shared key, the
/// union of every leaf's own target type, and one representative leaf
/// expression (for `current_narrowed_type`'s own fallback, when the key
/// isn't already in `locals`) - `None` for anything else (a mixed bag of
/// keys, a non-simple-predicate leaf, `is_numeric`/`in_array`/
/// `array_key_exists`, none of which are `narrowing_type_for_predicate`
/// table entries, ...), the same conservative "give up" this project's
/// narrowing already falls back to everywhere else.
impl<'a> Checker<'a> {
    pub(crate) fn or_chain_narrowing_target<'e>(
        &self,
        cond: &'e Expr,
        locals: &Locals,
    ) -> Option<(String, Type, &'e Expr)> {
        match &cond.kind {
            ExprKind::Binary(BinOp::Or | BinOp::LogicalOr, l, r) => {
                let (lk, lt, le) = self.or_chain_narrowing_target(l, locals)?;
                let (rk, rt, _) = self.or_chain_narrowing_target(r, locals)?;
                (lk == rk).then(|| (lk, union_types(lt, rt), le))
            }
            ExprKind::Grouping(inner) | ExprKind::ErrorSuppress(inner) => {
                self.or_chain_narrowing_target(inner, locals)
            }
            ExprKind::Call(callee, args) => {
                let (ExprKind::Name(name), [single_arg]) = (&callee.kind, args.as_slice()) else {
                    return None;
                };
                let key = narrowing_key(&single_arg.value)?;
                let target_ty = narrowing_type_for_predicate(name.trim_start_matches('\\'))?;
                Some((key, target_ty, &single_arg.value))
            }
            // `$node instanceof A || $node instanceof B ? (string) $node
            // : ...` needs a leaf for `instanceof` here too, despite
            // single-condition `if ($x instanceof Foo)` narrowing
            // already working fine elsewhere. A dynamic target (`$x instanceof
            // $className`) still doesn't name a concrete class to narrow
            // to, same as the single-condition `ExprKind::Instanceof` arm
            // above already requires.
            ExprKind::Instanceof(inner, InstanceofTarget::Name(class_name, _)) => {
                let key = narrowing_key(inner)?;
                let target_ty = self.resolve_name_expr(class_name);
                Some((key, target_ty, inner))
            }
            // `$parentId === null || $parentId === 0` - an equality-
            // comparison leaf, the same shape `apply_equality_narrowing`
            // already recognizes for a single condition, now composable
            // into an arbitrary-depth `||` chain the identical way
            // `is_*()`/`instanceof` leaves already are. `Mixed` is
            // excluded the same way `apply_equality_narrowing` excludes
            // it - an unresolvable value proves nothing about which
            // member the union should include.
            ExprKind::Binary(BinOp::Identical | BinOp::Eq, l, r) => {
                let (key, narrowed_expr, target_ty) = match (narrowing_key(l), narrowing_key(r)) {
                    (Some(key), _) => (key, l, self.infer_expr(r, locals)),
                    (None, Some(key)) => (key, r, self.infer_expr(l, locals)),
                    (None, None) => return None,
                };
                if target_ty.is_mixed() {
                    return None;
                }
                Some((key, target_ty, narrowed_expr))
            }
            _ => None,
        }
    }
}

/// `and_chain_narrowing_target`'s own counterpart of `or_chain_
/// narrowing_target`, for the shape De Morgan turns *into* one:
/// `!(!is_int($v) && !is_string($v))` is `is_int($v) || is_string($v)`,
/// so every leaf here must itself be a negated predicate call
/// (`Unary(Not, ...)`) - un-negating it and handing the inner
/// expression straight to `or_chain_narrowing_target` is exactly that
/// double-negation, reusing its identical per-leaf predicate-table
/// lookup and same-key union logic rather than duplicating either.
impl<'a> Checker<'a> {
    pub(crate) fn and_chain_narrowing_target<'e>(
        &self,
        cond: &'e Expr,
        locals: &Locals,
    ) -> Option<(String, Type, &'e Expr)> {
        match &cond.kind {
            ExprKind::Binary(BinOp::And | BinOp::LogicalAnd, l, r) => {
                let (lk, lt, le) = self.and_chain_narrowing_target(l, locals)?;
                let (rk, rt, _) = self.and_chain_narrowing_target(r, locals)?;
                (lk == rk).then(|| (lk, union_types(lt, rt), le))
            }
            ExprKind::Grouping(inner) | ExprKind::ErrorSuppress(inner) => {
                self.and_chain_narrowing_target(inner, locals)
            }
            ExprKind::Unary(UnaryOp::Not, inner) => self.or_chain_narrowing_target(inner, locals),
            _ => None,
        }
    }
}
