use crate::checker::keys::{KeyData, LocalKey};
use crate::checker::{Checker, Locals};
use syntax::ast::*;
use types::Type;

/// The expression `current_narrowed_type`'s own
/// fallback should actually infer from, when no prior narrowing entry
/// exists yet. `key_of`'s own unwrapping (`Grouping`, `Assign`,
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
pub(crate) fn narrowing_infer_target(arenas: &NodeArenas, id: ExprId) -> Option<&ExprNode> {
    let id = arenas.peeled(id)?;
    syntax::stack::guard(|| {
        let expr = arenas.expr(id)?;
        match &expr.kind {
            ExprKind::Binary(BinOp::Coalesce, lhs, rhs)
                if arenas
                    .expr(*rhs)
                    .is_some_and(|rhs| matches!(&rhs.kind, ExprKind::Literal(Literal::Null))) =>
            {
                narrowing_infer_target(arenas, *lhs)
            }
            _ => Some(expr),
        }
    })
}

/// Drops every `locals` entry that depended on `key`'s pre-reassignment
/// identity - either because `key` was itself the root a *deeper*
/// synthetic key was built from (`key->prop`, `key[lit]` - covers both a
/// bare-variable root and a full chain root, e.g. reassigning `$foo->bar`
/// invalidates a stale `foo->bar->baz`), or because `key` (when it's a
/// variable) was used as a *variable subscript* inside a different chain
/// entirely (`other[$key]`). What depends on a key is what the key table
/// recorded was built on it.
///
/// Without this, nothing invalidates a stale composite-key entry when a
/// variable it depends on changes - not just for a variable-subscript key,
/// but for any composite key at all (reassigning `$foo` would leave a stale
/// `foo->bar` sitting in `locals` forever). Called from every real call
/// site that can give a bare variable, property chain, or index chain a new
/// value.
pub(crate) fn purge_stale_narrowing_for(locals: &mut Locals, key: LocalKey) {
    // Only a key that isn't a variable can depend on another.
    if locals.may_have_member_narrowing() {
        let dependents = locals.keys().borrow().all_dependents(key);
        for dependent in dependents {
            if locals.is_synthetic(dependent) {
                locals.remove(dependent);
            }
        }
    }
    // `key`'s own per-field container facts (see `ContainerFieldFactsMap`'s
    // doc comment) are just as stale as a composite key rooted at `key` the
    // moment `key` itself is genuinely rebound - this is already called at
    // every real call site that can give a bare variable a new value
    // outside `Locals::declare` (which purges it itself), so riding along
    // here covers all of them uniformly rather than needing its own
    // separate call at each one.
    locals.container_field_facts_mut().remove(&key);
    locals.purge_branch_implied_facts_depending_on(key);
    // Same reasoning for the members `key` was proved to have and for its
    // own per-discriminant accumulator facts (see
    // `DiscriminantArrayFactsMap`'s doc comment) - a wholesale rebind (or
    // an unrecognized by-ref pass) starts `key`'s own value over from
    // scratch, so any accumulated per-branch history recorded for it is
    // stale too.
    locals.purge_member_facts_about(key);
}

/// `purge_stale_narrowing_for`, for the variable `name`.
pub(crate) fn purge_stale_narrowing_for_var(locals: &mut Locals, name: &str) {
    let key = locals.var_key(name);
    purge_stale_narrowing_for(locals, key);
}

/// Eagerly copies `source_container`'s own recorded per-field facts (see
/// `ContainerFieldFactsMap`'s doc comment) onto the index keys of `target`,
/// the moment `target` is bound from a dynamic-keyed read of
/// `source_container` (a `foreach` value, or a plain
/// `$x = $container[$dyn];`): the two pre-existing key-consulting
/// Index-read fast paths (`checker/expr.rs`'s `check_expr`/`infer_expr`)
/// then pick them up with no further code needed at the read site itself.
/// A no-op when `source_container` has no recorded facts (the overwhelmingly
/// common case).
pub(crate) fn propagate_container_field_facts(
    locals: &mut Locals,
    target: LocalKey,
    source_container: LocalKey,
) {
    let Some(facts) = locals
        .container_field_facts()
        .get(&source_container)
        .cloned()
    else {
        return;
    };
    for (field, ty) in facts.iter() {
        let key = locals.intern(KeyData::Index {
            base: target,
            key: field.as_str().into(),
        });
        locals.insert(key, ty.clone());
    }
}

// Joins a few known types, far below the union cap.
#[allow(clippy::disallowed_methods)]
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
        "is_iterable" => Some(types::parse_type_string("iterable")),
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
    pub(crate) fn or_chain_narrowing_target<'b>(
        &'b self,
        cond: &ExprNode,
        locals: &Locals,
    ) -> Option<(LocalKey, Type, &'b ExprNode)> {
        syntax::stack::guard(|| {
            let arenas = self.syntax_arenas()?;
            let cond = arenas.expr(arenas.peeled(cond.id)?)?;
            match &cond.kind {
                ExprKind::Binary(BinOp::Or | BinOp::LogicalOr, l, r) => {
                    let (lk, lt, le) = self.or_chain_narrowing_target(arenas.expr(*l)?, locals)?;
                    let (rk, rt, _) = self.or_chain_narrowing_target(arenas.expr(*r)?, locals)?;
                    (lk == rk).then(|| (lk, self.relation().union(lt, rt), le))
                }
                ExprKind::Call(callee, args) => {
                    let callee = arenas.expr(*callee)?;
                    let (ExprKind::Name(name), [single_arg]) =
                        (&callee.kind, arenas.args(args.clone()))
                    else {
                        return None;
                    };
                    let name = arenas.qualified_name(name)?;
                    let value = arenas.expr(single_arg.value)?;
                    let key = self.key_of_expr(locals, value)?;
                    let target_ty = narrowing_type_for_predicate(name.trim_start_matches('\\'))?;
                    Some((key, target_ty, value))
                }
                // `$node instanceof A || $node instanceof B ? (string) $node
                // : ...` needs a leaf for `instanceof` here too, despite
                // single-condition `if ($x instanceof Foo)` narrowing
                // already working fine elsewhere. A dynamic target (`$x instanceof
                // $className`) still doesn't name a concrete class to narrow
                // to, same as the single-condition `ExprKind::Instanceof` arm
                // above already requires.
                ExprKind::Instanceof(inner, InstanceofTarget::Name(class_name)) => {
                    let inner = arenas.expr(*inner)?;
                    let key = self.key_of_expr(locals, inner)?;
                    let class_name = arenas.qualified_name(class_name)?;
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
                    let (l, r) = (arenas.expr(*l)?, arenas.expr(*r)?);
                    let (key, narrowed_expr, target_ty) =
                        match (self.key_of_expr(locals, l), self.key_of_expr(locals, r)) {
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
        })
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
    pub(crate) fn and_chain_narrowing_target<'b>(
        &'b self,
        cond: &ExprNode,
        locals: &Locals,
    ) -> Option<(LocalKey, Type, &'b ExprNode)> {
        syntax::stack::guard(|| {
            let arenas = self.syntax_arenas()?;
            let cond = arenas.expr(arenas.peeled(cond.id)?)?;
            match &cond.kind {
                ExprKind::Binary(BinOp::And | BinOp::LogicalAnd, l, r) => {
                    let (lk, lt, le) = self.and_chain_narrowing_target(arenas.expr(*l)?, locals)?;
                    let (rk, rt, _) = self.and_chain_narrowing_target(arenas.expr(*r)?, locals)?;
                    (lk == rk).then(|| (lk, self.relation().union(lt, rt), le))
                }
                ExprKind::Unary(UnaryOp::Not, inner) => {
                    self.or_chain_narrowing_target(arenas.expr(*inner)?, locals)
                }
                _ => None,
            }
        })
    }
}
