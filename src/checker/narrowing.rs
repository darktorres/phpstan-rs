use crate::ast::*;
use crate::checker::{
    Checker, Locals, build_matched_shape, literal_string_key, literal_string_value, union_types,
};
use crate::regex_groups;
use crate::registry::Registry;
use crate::types::{AccessoryKind, Type, is_subtype};

impl<'a> Checker<'a> {
    /// Narrows `locals` in place based on `cond` being true (`branch_true`)
    /// or false, along one branch of an `if`/`while`. Only handles the
    /// common, safe patterns - equality against a known-type value
    /// (`$x === null`, `null !== $x`), `is_*()` guards, and `instanceof` -
    /// anything else leaves `locals` untouched rather than guess.
    pub(crate) fn apply_narrowing(&self, cond: &Expr, locals: &mut Locals, branch_true: bool) {
        // Phase 12 Finding E: `preg_match(...)` (bare, or compared
        // against a literal 0/1) doesn't otherwise match any existing
        // narrowing shape below, so this doesn't shadow anything -
        // verified directly, not assumed: the `Binary(op, l, r)` arm
        // further down *does* also run afterward for a comparison form,
        // dispatching to `apply_equality_narrowing`, but that function's
        // own `narrowing_key(l)`/`narrowing_key(r)` calls have no arm for
        // `ExprKind::Call` at all, so it's a genuine no-op for this
        // shape, not a silent double-narrowing.
        if let Some((pattern_arg, matches_arg, true_means_matched)) = preg_match_condition(cond)
            && let ExprKind::Variable(matches_name) = &matches_arg.kind
            && let Some(pattern_str) = literal_string_value(pattern_arg)
            && let Some(parsed) = regex_groups::parse_capture_groups(&pattern_str)
        {
            let narrowed = if branch_true == true_means_matched {
                build_matched_shape(
                    &parsed.groups,
                    &parsed.alternation_branch_counts,
                    &parsed.alternation_wrapped_in_optional_quantifier,
                    &Type::String,
                    false,
                )
            } else {
                Type::ArrayShape(vec![])
            };
            purge_stale_narrowing_for(locals, matches_name);
            locals.insert(matches_name.clone(), narrowed);
        }
        match &cond.kind {
            // `&&` narrows the "both true" branch by composing each side
            // in order (the right operand's own narrowing can rely on the
            // left's, same as check_expr's short-circuit-aware handling)
            // - the false branch would need "at least one is false, but
            // not sure which", which isn't a narrowing this can express,
            // so it's left alone, same conservative choice `||` mirrors
            // for its own unrepresentable branch below.
            ExprKind::Binary(BinOp::And, l, r) if branch_true => {
                self.apply_narrowing(l, locals, true);
                self.apply_narrowing(r, locals, true);
            }
            ExprKind::Binary(BinOp::Or, l, r) if !branch_true => {
                self.apply_narrowing(l, locals, false);
                self.apply_narrowing(r, locals, false);
            }
            // Phase 34 Finding B: "at least one is true, but not sure
            // which" isn't representable *in general* - but when every
            // operand is a direct predicate-table check on the identical
            // expression, it narrows precisely to the union of what each
            // operand would give alone (`is_int($x) || is_string($x)`
            // being true means `$x` is exactly `int|string`, not "give
            // up"). `or_chain_narrowing_target` is the one doing the
            // actual restriction-checking; this arm only acts when it
            // succeeds.
            ExprKind::Binary(BinOp::Or, ..) if branch_true => {
                if let Some((key, target_ty, repr_expr)) = self.or_chain_narrowing_target(cond) {
                    let current = self.current_narrowed_type(repr_expr, &key, locals);
                    locals.insert(key, narrow_type_to(self.registry, &current, &target_ty));
                }
            }
            // A round of Piwigo-corpus bucketing found a real, common
            // idiom this project's own "Known limitations" already
            // named as needing "its own sizable narrowing-engine
            // feature": `if ((! is_int($a) && ! is_string($a)) || (!
            // is_int($b) && ! is_string($b)) || ...) { continue; }`
            // (real shape, `CommentRepository::findAllWithConditions`/
            // `findList`, 7 sites) - narrowing several *different*
            // variables at once through one combined condition. Turned
            // out not to need that: the `Or ... if !branch_true` arm
            // just above already decomposes an arbitrarily-long `||`
            // chain into independent per-leaf `apply_narrowing` calls
            // (De Morgan on the *outer* `||`) - each landing here, on
            // exactly one `&&`-of-negated-predicates leaf, one variable
            // at a time. The only missing piece was teaching `&&`'s own
            // false branch to finish the same De Morgan step one level
            // down: `!(!is_int($v) && !is_string($v))` is `is_int($v)
            // || is_string($v)`, which `and_chain_narrowing_target`
            // computes by reusing `or_chain_narrowing_target` on each
            // un-negated leaf directly (double negation), then this
            // arm narrows exactly the way the sibling `Or ... if
            // branch_true` arm above already does for a literal `||`.
            // Every leaf here is independent (a different key each),
            // so no "different keys in one condition" support was
            // needed at this level at all - each And-term simply lands
            // on its own call to this same arm, mutating the one
            // shared `locals` for its own key only.
            ExprKind::Binary(BinOp::And, ..) if !branch_true => {
                if let Some((key, target_ty, repr_expr)) = self.and_chain_narrowing_target(cond) {
                    let current = self.current_narrowed_type(repr_expr, &key, locals);
                    locals.insert(key, narrow_type_to(self.registry, &current, &target_ty));
                }
            }
            // Deliberately `===`/`!==` only, not `==`/`!=` - a loose
            // comparison matches PHP's own type-juggling table (`0 ==
            // false`, `0 == null`, `"0" == 0`, ...), a much wider set of
            // values than the literal compared-to one, so narrowing a
            // variable down to exactly that value on a loose match would
            // discard every other value `==` could also have matched.
            ExprKind::Binary(op, l, r) => {
                let is_eq = matches!(op, BinOp::Identical);
                let is_neq = matches!(op, BinOp::NotIdentical);
                if is_eq || is_neq {
                    let matches_when_true = if is_eq { branch_true } else { !branch_true };
                    self.apply_equality_narrowing(l, r, locals, matches_when_true);
                } else if branch_true
                    && let ExprKind::Call(callee, args) = &l.kind
                    && let ExprKind::Name(name) = &callee.kind
                    && name.trim_start_matches('\\') == "count"
                    && let [single_arg] = args.as_slice()
                    && let Some(key) = narrowing_key(&single_arg.value)
                    && let ExprKind::Literal(Literal::Int(n)) = &r.kind
                    && ((matches!(op, BinOp::Gt) && *n == 0)
                        || (matches!(op, BinOp::GtEq) && *n == 1))
                {
                    // Phase 14: `count($x) > 0`/`count($x) >= 1` proves
                    // `$x` non-empty - scoped to exactly this shape with
                    // `count(...)` on the left (the verified real idiom);
                    // the reversed form (`0 < count($x)`) isn't handled.
                    let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                    locals.insert(key, upgrade_array_to_non_empty(&current));
                }
            }
            ExprKind::Unary(UnaryOp::Not, inner) => {
                self.apply_narrowing(inner, locals, !branch_true);
            }
            ExprKind::Grouping(inner) => self.apply_narrowing(inner, locals, branch_true),
            // Phase 26: `(bool) $x`'s own truthiness is exactly `$x`'s -
            // PHP's cast doesn't change which values are truthy/falsy, it
            // just makes the check explicit (a common, idiomatic style:
            // `if (! (bool) $fp) { throw ...; }` after `fopen()`) - so
            // this narrows precisely as if the cast weren't there at all.
            // Any other cast (`(int)`, `(string)`, ...) genuinely changes
            // the value and isn't narrowable this way, so it's left alone.
            ExprKind::Cast(kind, inner) if kind == "bool" => {
                self.apply_narrowing(inner, locals, branch_true);
            }
            ExprKind::Call(callee, args) => {
                // Phase 22: `narrowing_key`, not a bare `ExprKind::
                // Variable` match - without this, `is_string($arr['key'])`
                // (or `empty($arr['key'])`) never narrowed `$arr['key']`
                // at all, the array-index counterpart of the exact gap
                // the `instanceof` arm below already covers for property
                // chains (`$arg->value instanceof Variable`).
                if let (ExprKind::Name(name), [single_arg]) = (&callee.kind, args.as_slice())
                    && let Some(key) = narrowing_key(&single_arg.value)
                {
                    // Phase 55: a fully-qualified call (`\is_array($x)`,
                    // `\count($x)`, ...) keeps its leading backslash in
                    // `ExprKind::Name` - unlike ordinary call resolution
                    // (`name_ctx::resolve_function_candidates`, used for
                    // every real argument/arity check), every name
                    // comparison in this whole match arm was a bare
                    // string equality with no normalization, so a real,
                    // common defensive-global-namespace style (pervasive
                    // in Rector-prefixed vendor code, real shape:
                    // `\is_array($value)` nested inside an
                    // `is_iterable($value)` guard) silently skipped
                    // narrowing entirely.
                    let name = name.trim_start_matches('\\');
                    if name == "empty" {
                        // `empty($x)` is PHP's own falsy check - only the
                        // branch where it evaluated `false` (`$x` proven
                        // truthy) is safely narrowable, the same asymmetry
                        // as a bare truthiness check.
                        if !branch_true {
                            let current =
                                self.current_narrowed_type(&single_arg.value, &key, locals);
                            locals.insert(key, narrow_type_excluding_falsy(&current));
                        }
                    } else if name == "isset" {
                        // Phase 23: `isset($x)`'s truthy branch means
                        // specifically "not null" - it's true for `0`,
                        // `''`, `false`, an empty array, every falsy-but-
                        // non-null value - so this narrows only `Null`
                        // away, unlike `empty()`'s full falsy exclusion
                        // above. The false branch isn't safely narrowable
                        // to anything more specific, the same asymmetry
                        // `empty`/the truthy fallback below already accept.
                        if branch_true {
                            let current =
                                self.current_narrowed_type(&single_arg.value, &key, locals);
                            locals.insert(key, narrow_type_excluding(&current, &Type::Null));
                        }
                    } else if name == "count" && branch_true {
                        // Phase 55: a bare `if (count($x))`/`while
                        // (count($x))` truthiness check, not the `> 0`/
                        // `>= 1` comparison form the `Binary` arm above
                        // already handles - `count()` is never negative,
                        // so a truthy result means exactly the same
                        // "provably non-empty" fact. The false branch
                        // (`count($x) === 0`) isn't narrowed here, the
                        // same asymmetry the comparison form's own arm
                        // already accepts (`upgrade_array_to_non_empty`
                        // has no "prove empty" counterpart).
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        locals.insert(key, upgrade_array_to_non_empty(&current));
                    } else if name == "array_is_list" && branch_true {
                        // Phase 55's own still-open list: a real, named
                        // gap - webmozart/assert's own `Assert::isList()`
                        // (a real, common assertion-library idiom) proves
                        // list-ness via `is_array($x) && array_is_list
                        // ($x)`, the same shape `count($x)`'s own
                        // truthiness upgrade just above already covers
                        // for non-emptiness. Only the true branch is
                        // narrowable - failing `array_is_list()` proves
                        // nothing more specific than "not proven a list",
                        // the same asymmetry `count`/`empty` already
                        // accept above.
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        locals.insert(key, upgrade_array_to_list(&current));
                    } else if name == "is_numeric" {
                        // Phase 32 Finding G: deliberately its own arm,
                        // not a `narrowing_type_for_predicate` table entry
                        // - that table's single target type feeds *both*
                        // directions through the same dispatch below,
                        // which is sound for `is_scalar`/`is_string`/etc.
                        // (a string is unconditionally scalar/a-string
                        // either way) but not for `is_numeric`, where a
                        // string only *sometimes* qualifies: excluding a
                        // shared "int|float|string" target on the false
                        // branch would wrongly drop `String` entirely,
                        // even though a genuinely non-numeric string
                        // (`"hello"`) is exactly the ordinary case that
                        // makes `!is_numeric` true. The two directions
                        // need different exclusion sets instead.
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        let narrowed = if branch_true {
                            // Bool/True/False/Null can never satisfy
                            // is_numeric - unconditionally safe to drop.
                            // A kept String (bare, or inside the
                            // resulting Union) is upgraded to
                            // numeric-string (Phase 14 - verified
                            // against real PHPStan's own dumpType():
                            // `string $x` under is_numeric($x) narrows
                            // to plain numeric-string, `mixed $x`
                            // narrows to int|float|numeric-string).
                            let excluded = [Type::Bool, Type::True, Type::False, Type::Null]
                                .into_iter()
                                .fold(current, |acc, excluded| {
                                    narrow_type_excluding(&acc, &excluded)
                                });
                            upgrade_string_to_numeric(&excluded)
                        } else {
                            // Int/Float are unconditionally numeric, so
                            // failing is_numeric proves the value wasn't
                            // one - nothing else can be dropped this way.
                            [Type::Int, Type::Float]
                                .into_iter()
                                .fold(current, |acc, excluded| {
                                    narrow_type_excluding(&acc, &excluded)
                                })
                        };
                        locals.insert(key, narrowed);
                    } else if let Some(target_ty) = narrowing_type_for_predicate(name) {
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        let narrowed = if branch_true {
                            narrow_type_to(self.registry, &current, &target_ty)
                        } else {
                            narrow_type_excluding(&current, &target_ty)
                        };
                        locals.insert(key, narrowed);
                    }
                } else if let ExprKind::Name(name) = &callee.kind
                    && name.trim_start_matches('\\') == "in_array"
                    && let [needle, haystack, strict] = args.as_slice()
                    // Phase 33 Finding 2: only the strict, literal-array
                    // form real PHPStan's own `InArrayFunctionTypeSpecifyingExtension`
                    // narrows - a loose comparison's type-juggling would
                    // make the real safe target wider than the literal
                    // values listed here.
                    && matches!(&strict.value.kind, ExprKind::Literal(Literal::Bool(true)))
                    && let ExprKind::ArrayLiteral(items) = &haystack.value.kind
                    && let Some(target_ty) = self.in_array_target_type(items, locals)
                    && let Some(key) = narrowing_key(&needle.value)
                {
                    let current = self.current_narrowed_type(&needle.value, &key, locals);
                    let narrowed = if branch_true {
                        narrow_type_to(self.registry, &current, &target_ty)
                    } else {
                        narrow_type_excluding(&current, &target_ty)
                    };
                    locals.insert(key, narrowed);
                } else if let ExprKind::Name(name) = &callee.kind
                    && name.trim_start_matches('\\') == "array_key_exists"
                    && let [key_arg, arr_arg] = args.as_slice()
                    && let ExprKind::Literal(Literal::String(lit_key)) = &key_arg.value.kind
                    && let Some(arr_key) = narrowing_key(&arr_arg.value)
                {
                    // Phase 33 Finding 4: the checked expression here is
                    // the array argument, not the key - proving/disproving
                    // whether a specific offset is present lets a `Union`
                    // member that could never make the call come out this
                    // way be dropped, the same "keep/drop a Union member
                    // based on a provable fact" shape `instanceof`'s own
                    // member resolution already uses.
                    let current = self.current_narrowed_type(&arr_arg.value, &arr_key, locals);
                    let narrowed =
                        narrow_array_shape_by_key_presence(&current, lit_key, branch_true);
                    locals.insert(arr_key, narrowed);
                } else if let ExprKind::Name(name) = &callee.kind
                    && matches!(
                        name.trim_start_matches('\\'),
                        "str_contains" | "str_starts_with" | "str_ends_with"
                    )
                    && branch_true
                    && let [haystack, needle] = args.as_slice()
                    && let Some(key) = narrowing_key(&haystack.value)
                {
                    // Phase 55's own still-open list: real PHPStan's own
                    // `StrContainingTypeSpecifyingExtension` - a haystack
                    // that provably contains/starts/ends with a non-empty
                    // literal needle can't itself be empty. Only fires for
                    // a needle this checker can prove is a non-empty
                    // string literal (matching real PHPStan's own
                    // `isNonEmptyString()` gate) - a variable/computed
                    // needle proves nothing. Only the true branch is
                    // narrowable (mirrors `isFunctionSupported`'s own
                    // `$context->true()` restriction) - failing any of
                    // these three proves nothing about the haystack's own
                    // emptiness. `strpos`/`stripos`/`fnmatch`/`mb_*`
                    // variants (also in real PHPStan's own table) are
                    // deliberately left out - their `int|false` (or
                    // `false`-on-no-match) return makes a bare truthy
                    // `if (strpos(...))` a real, separate PHP footgun
                    // (position `0` is falsy despite being a match) this
                    // checker doesn't otherwise model, unlike these three
                    // genuinely bool-returning functions.
                    if let Type::StringLiteral(needle_lit) = self.infer_expr(&needle.value, locals)
                        && !needle_lit.is_empty()
                    {
                        let kind = if needle_lit == "0" {
                            AccessoryKind::NonEmptyString
                        } else {
                            AccessoryKind::NonFalsyString
                        };
                        let current = self.current_narrowed_type(&haystack.value, &key, locals);
                        locals.insert(key, upgrade_string_with_accessory(&current, kind));
                    }
                } else if let ExprKind::Name(name) = &callee.kind {
                    // A plain function call tagged `@phpstan-assert-if-true`/
                    // `-if-false` on its own docblock - see `FunctionSig::
                    // assert_if_true`'s own doc comment for the full
                    // picture. No `$this`/receiver here, so only a
                    // `Param`-targeted assertion can ever resolve
                    // (`apply_assert_infos` silently skips the rest).
                    let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
                    if let Some(sig) = self.registry.lookup_function(&primary).or_else(|| {
                        fallback
                            .as_deref()
                            .and_then(|g| self.registry.lookup_function(g))
                    }) {
                        let infos = if branch_true {
                            &sig.assert_if_true
                        } else {
                            &sig.assert_if_false
                        };
                        if !infos.is_empty() {
                            self.apply_assert_infos(infos, sig, None, args, locals);
                        }
                    }
                }
            }
            // `@phpstan-assert-if-true`/`-if-false` on a method's own
            // docblock (real corpus shape: PHPUnit's own accessor pair
            // `hasTestFilesFile(): bool` tagged `@phpstan-assert-if-true
            // !null $this->testFilesFile`) - see `FunctionSig::
            // assert_if_true`'s own doc comment for the full picture.
            // `recv_class` picks a *representative* member out of a
            // `Union`/`Intersection` receiver the exact same way
            // `checker/expr.rs`'s own `MethodCall` arm already does for
            // ordinary method-call checking - real corpus shape this
            // fixes: Doctrine ORM's own `AssociationMapping::
            // isToOneOwningSide(): bool` (`@phpstan-assert-if-true
            // OneToOneOwningSideMapping|ManyToOneAssociationMapping
            // $this`), called on a variable typed as the *union* of all
            // six concrete mapping subclasses - every one of which
            // inherits the identical method, so any single member
            // resolves the same `AssertInfo` the others would. Once
            // resolved, `apply_assert_infos`/`narrow_type_to` already
            // narrow the receiver's own *full* current type generically,
            // Union or not - only the lookup itself needed to stop
            // requiring a bare `Type::Object`.
            ExprKind::MethodCall(recv, method, args, _) => {
                let recv_ty = self.infer_expr(recv, locals);
                let class = match &recv_ty {
                    Type::Object(class, _) => Some(class.clone()),
                    Type::EnumCase(fqn, _) => Some(fqn.clone()),
                    Type::Union(members) | Type::Intersection(members) => {
                        members.iter().find_map(|m| match m {
                            Type::Object(class, _) => Some(class.clone()),
                            Type::EnumCase(fqn, _) => Some(fqn.clone()),
                            _ => None,
                        })
                    }
                    _ => None,
                };
                if let Some(class) = class
                    && let Some(sig) = self.registry.lookup_method(&class, method)
                {
                    let infos = if branch_true {
                        &sig.assert_if_true
                    } else {
                        &sig.assert_if_false
                    };
                    if !infos.is_empty() {
                        self.apply_assert_infos(infos, sig, Some(recv), args, locals);
                    }
                }
            }
            // Same idea as `MethodCall` just above, for a static method
            // call - no receiver expression exists here at all, so only
            // a `Param`-targeted assertion can ever resolve.
            ExprKind::StaticCall(class_name, method, args) => {
                if let Some(class) = self.resolve_static_class_name(class_name)
                    && let Some(sig) = self.registry.lookup_method(&class, method)
                {
                    let infos = if branch_true {
                        &sig.assert_if_true
                    } else {
                        &sig.assert_if_false
                    };
                    if !infos.is_empty() {
                        self.apply_assert_infos(infos, sig, None, args, locals);
                    }
                }
            }
            ExprKind::Instanceof(inner, target) => {
                // Phase 33 Finding 1: the false branch doesn't need a
                // "not this class" type to be representable - it only
                // needs to *drop* the one member just proven impossible
                // from an already-enumerated `Union`, exactly what
                // `narrow_type_excluding` already does soundly elsewhere.
                // A dynamic target (`$x instanceof $className`) still
                // doesn't name a concrete class to narrow to either way.
                if let Some(key) = narrowing_key(inner)
                    && let InstanceofTarget::Name(class_name, _) = target
                {
                    let resolved = self.resolve_name_expr(class_name);
                    if branch_true {
                        let current = self.current_narrowed_type(inner, &key, locals);
                        locals.insert(key, narrow_instanceof_true(&current, &resolved));
                    } else {
                        let current = self.current_narrowed_type(inner, &key, locals);
                        locals.insert(key, narrow_type_excluding(&current, &resolved));
                    }
                }
            }
            // An assignment used as a boolean sub-condition (`$a &&
            // ($x = f()) && $x->m()`, `!$a || !$x = f()`) evaluates
            // unconditionally whenever control actually reaches it -
            // the assigned value becomes `$x`'s real type regardless of
            // which way the *whole* enclosing condition ultimately goes,
            // so this must run before (and instead of) the generic
            // bare-truthiness catch-all just below, which `narrowing_
            // key`'s own `Assign` case would otherwise match too:
            // without this dedicated arm, that catch-all narrowed
            // falsiness out of the *pre*-assignment type it read via
            // `current_narrowed_type` (a no-op for most non-falsy
            // reassignments, silently discarding the assignment's real
            // effect) rather than binding the assigned type at all.
            // `check_expr`'s own `Binary(And/Or, ...)` handling re-derives
            // each operand's narrowing through this same function (see
            // its own doc comment) instead of reusing whatever it
            // already computed while checking each operand for
            // diagnostics, so a later &&/|| operand referencing the
            // freshly-assigned variable needs the assignment replayed
            // here too. Corpus-verified real shape (`Symfony\Component\
            // DependencyInjection\Compiler\AbstractRecursivePass::
            // getReflectionMethod()`): `$r->hasMethod('__call') && ($r =
            // $r->getMethod('__call')) && $r->isPublic()` wrongly flagged
            // `$r->isPublic()` as calling a nonexistent method on the
            // *pre*-reassignment `ReflectionClass`, never seeing that
            // `$r` had become a `ReflectionMethod` two conjuncts earlier
            // in the very same condition - a real, previously-invisible
            // gap unmasked only once `parse_not`'s own `!$var = expr`
            // fix stopped leaving reflection variables like this one
            // typed as `Mixed` (silencing every check through them)
            // whenever they were first bound via that idiom.
            //
            // The `branch_true` truthy case still needs its own extra
            // step on top of the plain bind: `if ($position = strpos($x,
            // "#")) { substr($x, 0, $position); }` (amphp's own
            // `Http2ConnectionProcessor`) relies on the *assignment's
            // own result* being truthy to reach the `if` body at all -
            // without also excluding falsy members from the
            // freshly-bound type here, `$position` kept `strpos()`'s raw
            // `int<0, max>|false` inside the truthy branch instead of a
            // real non-zero `int`, a corpus-verified false positive this
            // arm's own first version introduced (`substr()`'s 3rd
            // argument expects `int|null`, not `int<0, max>|false`).
            ExprKind::Assign(lhs, rhs) => {
                let rhs_ty = self.infer_expr(rhs, locals);
                self.bind_assignment_target(lhs, rhs_ty, locals);
                if branch_true
                    && let Some(key) = narrowing_key(lhs)
                    && let Some(current) = locals.get(&key).cloned()
                {
                    locals.insert(key, narrow_type_excluding_falsy(&current));
                }
            }
            // A bare truthiness check (`if ($x)` / `if (!$x)`, the latter
            // reaching here via the `Not` arm's recursion above) - only
            // the truthy branch is narrowable, by excluding the falsy
            // members this project can represent (`Null`/`False`); the
            // falsy branch isn't safely narrowable to anything more
            // specific than the current type.
            _ if branch_true && narrowing_key(cond).is_some() => {
                let key = narrowing_key(cond).expect("checked by guard above");
                let current = self.current_narrowed_type(cond, &key, locals);
                locals.insert(key, narrow_type_excluding_falsy(&current));
            }
            _ => {}
        }
    }

    /// The type to narrow *from* for a synthetic `narrowing_key` (a
    /// property chain or literal-key array-index chain, never a bare
    /// variable) - `locals.get(key)` alone, defaulting to `Type::Mixed`
    /// when absent, is wrong for these: unlike a bare variable (already
    /// tracked in `locals` by ordinary assignment/definition), a
    /// property/index chain's key only ever exists in `locals` as the
    /// result of an *earlier* narrowing - the common case, a chain never
    /// narrowed before, would silently discard its real, already-known
    /// declared/inferred type (from `infer_expr`) in favor of a bogus
    /// `Mixed`, which then gets *narrowed* (a no-op on `Mixed`) and
    /// written back, permanently overriding the correct type for the
    /// rest of the enclosing scope. A real corpus example this fix
    /// exists for: `if ($catListRequest->deleteId !== null) { ... }`
    /// narrowing `?int` down to `int` only works if the "current" type
    /// it narrows from is the real `?int`, not a wrongly-defaulted
    /// `Mixed`. A bare variable already goes through `locals.get(name)`
    /// correctly (it's always present by the time a condition checks
    /// it), so this only matters for non-`Variable` `narrowing_key`
    /// shapes - but computing it uniformly via `expr`/`infer_expr` is
    /// correct for a bare variable too (`infer_expr` on a `Variable`
    /// just reads the same `locals` entry), so every `apply_narrowing`
    /// call site uses this instead of a raw `locals.get(key).unwrap_or
    /// (Type::Mixed)`.
    fn current_narrowed_type(&self, expr: &Expr, key: &str, locals: &Locals) -> Type {
        locals
            .get(key)
            .cloned()
            .unwrap_or_else(|| self.infer_expr(narrowing_infer_target(expr), locals))
    }

    /// The union of `in_array()`'s own haystack literal values, if every
    /// element is a bare, unkeyed literal (`int`/`string`/`bool`/`null`) -
    /// `None` if any element is a spread, a keyed entry, or anything other
    /// than a syntactic literal, since this project has no way to prove a
    /// non-literal haystack element's value statically. Reuses `infer_expr`
    /// on each element rather than duplicating the `Literal -> Type`
    /// mapping that already exists, verbatim, in `check_expr`'s and
    /// `infer_expr`'s own `Literal` arms.
    fn in_array_target_type(&self, items: &[ArrayItem], locals: &Locals) -> Option<Type> {
        let mut member_types: Vec<Type> = Vec::new();
        for item in items {
            if item.spread || item.key.is_some() {
                return None;
            }
            if !matches!(item.value.kind, ExprKind::Literal(_)) {
                return None;
            }
            let ty = self.infer_expr(&item.value, locals);
            if !member_types.contains(&ty) {
                member_types.push(ty);
            }
        }
        match member_types.len() {
            0 => None,
            1 => member_types.into_iter().next(),
            _ => Some(Type::Union(member_types)),
        }
    }

    pub(crate) fn apply_equality_narrowing(
        &self,
        l: &Expr,
        r: &Expr,
        locals: &mut Locals,
        matches_when_true: bool,
    ) {
        // Phase 22: `narrowing_key` on each side, not a bare `ExprKind::
        // Variable` match - the same array-index gap the `Call` arm above
        // has, for the `$arr['key'] === null` idiom instead of an
        // `is_*()` predicate.
        let pair = match (narrowing_key(l), narrowing_key(r)) {
            (Some(key), _) => Some((key, l, self.infer_expr(r, locals))),
            (None, Some(key)) => Some((key, r, self.infer_expr(l, locals))),
            (None, None) => None,
        };
        let Some((key, narrowed_expr, target_ty)) = pair else {
            return;
        };
        if target_ty == Type::Mixed {
            return;
        }
        let current = self.current_narrowed_type(narrowed_expr, &key, locals);
        // A real, general unsoundness bug found via a corpus re-check
        // that turned out to have nothing to do with the two-file
        // cross-checking mystery it first looked like: `$x !== $y` was
        // narrowing `$x` by *excluding* `$y`'s own static type -
        // `Bool`/`Int`/`String`/... - not just a known single value.
        // `narrow_type_excluding` is only sound when `excluded`
        // represents one specific, already-proven value (`Null`,
        // `True`/`False`, a literal, an `is_*()` predicate's own
        // provably-exhaustive target) - every one of its other real
        // callers passes exactly that. A general type like plain `Bool`
        // covers *multiple* distinct values, so `$x !== $y` where `$y`'s
        // only known fact is "some `Bool`" proves nothing about which
        // specific value `$x` isn't - excluding the whole `Bool` type
        // from an already-`Bool`-typed `$x` wrongly collapsed it to
        // `Type::Never`. Confirmed live: `?bool $a` narrowed to `bool` by
        // `$a !== null`, then compared `$a !== $b` (`bool $b`) inside the
        // same `&&` - real corpus shape in
        // `CategoryUpdateController::__invoke`, minimized to `function
        // test(?bool $a, bool $b) { if ($a !== null && $a !== $b) { ... }
        // }`. `narrow_type_to` doesn't share this bug: for a non-`Union`
        // original it always returns `target` outright (matches iff both
        // sides are already type-compatible, since `===` between
        // incompatible static types is dead code either way) - safe
        // regardless of whether `target_ty` is a literal or a general
        // type, so only the exclusion branch needs gating here.
        let narrowed = if matches_when_true {
            narrow_type_to(self.registry, &current, &target_ty)
        } else if is_known_single_value(&target_ty) {
            narrow_type_excluding(&current, &target_ty)
        } else {
            current
        };
        locals.insert(key, narrowed);
    }

    /// Whether a method/property is confirmed missing from every value
    /// `ty` could hold - `has_member(class)` decides for one concrete
    /// class (already covering whatever fallback, like `__call`/`__get`,
    /// makes a class "have" the member without declaring it directly). A
    /// bare `Object` is the base case; a `Union` value could genuinely be
    /// any member, so missing is confirmed as soon as *any* resolvable
    /// member lacks it; a value satisfying an `Intersection` has every
    /// member's capabilities at once, so missing is only confirmed when
    /// *every* member lacks it. Anything else (a type this checker can't
    /// resolve into class members at all, or a non-`Object` member found
    /// while walking a `Union`/`Intersection`) never confirms "missing" -
    /// this project's consistent preference for fewer false positives
    /// over catching everything.
    pub(crate) fn member_confirmed_missing(
        &self,
        ty: &Type,
        has_member: impl Fn(&str) -> bool + Copy,
    ) -> bool {
        match ty {
            Type::Object(class, _) => {
                self.registry.classes.contains_key(class) && !has_member(class)
            }
            // Phase 79: a specific enum case is still a real instance of
            // its enum for member-lookup purposes - only its own case
            // identity (narrowing, `->name`/`->value`) cares which case,
            // never whether some *other* method/property exists.
            Type::EnumCase(fqn, _) => self.registry.classes.contains_key(fqn) && !has_member(fqn),
            Type::Union(members) => members
                .iter()
                .any(|m| self.member_confirmed_missing(m, has_member)),
            Type::Intersection(members) => members
                .iter()
                .all(|m| self.member_confirmed_missing(m, has_member)),
            _ => false,
        }
    }

    /// PHPUnit's own narrowing-relevant assertions (`self::assertIsString
    /// ($output)`, `$this->assertNotNull($x)`, ...) - a real, common
    /// idiom (real PHPStan explicitly supports it via its own
    /// `phpstan/phpstan-phpunit` extension, which this project doesn't
    /// depend on, so the narrowing is replicated directly here) - matches
    /// exactly the same "statement narrows the rest of the scope
    /// unconditionally" shape `assert($cond)` already gets in `check_stmt`'s
    /// `StmtKind::Expression` arm: if execution continues past a failing
    /// assertion, PHPUnit's own `AssertionFailedError` would have been
    /// thrown, so there's only one surviving path forward, no branch/merge
    /// needed. Matched purely on method name, not on confirming the
    /// receiver is really a `PHPUnit\Framework\TestCase` - these names are
    /// specific enough that a false match isn't a realistic concern,
    /// matching this project's existing "trust the name" precedent for
    /// `is_string`/`empty`/`isset` and friends.
    ///
    /// Deliberately excludes `assertIsNumeric` - real PHPStan narrows it to
    /// `int|float|numeric-string`, a type this project has no
    /// representation for; narrowing to a plain `int|float` would be
    /// actively wrong (and a new false negative) for the equally-common
    /// numeric-*string* case.
    pub(crate) fn apply_phpunit_assertion_narrowing(
        &self,
        method: &str,
        args: &[Arg],
        locals: &mut Locals,
    ) {
        let narrow_to = |locals: &mut Locals, idx: usize, target: &Type| {
            if let Some(arg) = args.get(idx)
                && let Some(key) = narrowing_key(&arg.value)
            {
                let current = self.current_narrowed_type(&arg.value, &key, locals);
                locals.insert(key, narrow_type_to(self.registry, &current, target));
            }
        };
        let narrow_excluding = |locals: &mut Locals, idx: usize, excluded: &Type| {
            if let Some(arg) = args.get(idx)
                && let Some(key) = narrowing_key(&arg.value)
            {
                let current = self.current_narrowed_type(&arg.value, &key, locals);
                locals.insert(key, narrow_type_excluding(&current, excluded));
            }
        };
        match method {
            "assertIsString" => narrow_to(locals, 0, &Type::String),
            "assertIsArray" => narrow_to(
                locals,
                0,
                &Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed)),
            ),
            "assertIsInt" | "assertIsInteger" => narrow_to(locals, 0, &Type::Int),
            "assertIsFloat" => narrow_to(locals, 0, &Type::Float),
            "assertIsBool" | "assertIsBoolean" => narrow_to(locals, 0, &Type::Bool),
            "assertIsResource" => {
                narrow_to(locals, 0, &Type::Object("resource".to_string(), Vec::new()));
            }
            "assertNull" => narrow_to(locals, 0, &Type::Null),
            "assertTrue" => narrow_to(locals, 0, &Type::True),
            "assertFalse" => narrow_to(locals, 0, &Type::False),
            "assertNotNull" => narrow_excluding(locals, 0, &Type::Null),
            "assertNotFalse" => narrow_excluding(locals, 0, &Type::False),
            "assertNotTrue" => narrow_excluding(locals, 0, &Type::True),
            // `assertInstanceOf(string $expected, mixed $actual)` - note
            // the argument order (expected class first, actual value
            // second), the reverse of `instanceof`'s own left-to-right
            // reading - `$expected` is conventionally `Foo::class`, a
            // `StaticAccess(class_name, "class")` expression.
            "assertInstanceOf" => {
                if let Some(class_arg) = args.first()
                    && let ExprKind::StaticAccess(class_name, member) = &class_arg.value.kind
                    && member == "class"
                    && let Some(actual_arg) = args.get(1)
                    && let Some(key) = narrowing_key(&actual_arg.value)
                {
                    locals.insert(key, self.resolve_name_expr(class_name));
                }
            }
            _ => {}
        }
    }

    /// Applies one `@phpstan-assert`-family list (`sig.assert_if_true`,
    /// `sig.assert_if_false`, or `sig.assert`) to `locals` - shared by
    /// both the conditional (`apply_narrowing`'s own `MethodCall`/`Call`/
    /// `StaticCall` arms, gated on which branch actually held) and
    /// unconditional (`check_stmt`'s own bare `@phpstan-assert` wiring,
    /// mirroring `assert($cond)`'s identical "narrows for the rest of
    /// scope, no branch/merge needed" shape) call sites. `recv` is the
    /// call's own receiver expression (`Some` for a method call, `None`
    /// for a plain function call) - a `ThisSelf`/`ThisProp` target is
    /// silently skipped when there isn't one, since there's no `$this`
    /// to narrow from a plain function call.
    pub(crate) fn apply_assert_infos(
        &self,
        infos: &[crate::registry::AssertInfo],
        sig: &crate::registry::FunctionSig,
        recv: Option<&Expr>,
        args: &[Arg],
        locals: &mut Locals,
    ) {
        for info in infos {
            let Some((key, current)) =
                self.assert_target_key_and_current(&info.target, sig, recv, args, locals)
            else {
                continue;
            };
            let narrowed = if info.negated {
                narrow_type_excluding(&current, &info.ty)
            } else {
                narrow_type_to(self.registry, &current, &info.ty)
            };
            locals.insert(key, narrowed);
        }
    }

    /// The `(narrowing key, current type)` pair a `@phpstan-assert`
    /// target resolves to at this call site - `None` when the target
    /// can't be resolved here at all (a `ThisSelf`/`ThisProp` target on a
    /// receiverless call, or a `Param` target this call didn't actually
    /// pass an argument for, e.g. an omitted optional parameter).
    /// `Param`/`ThisSelf` both name a *real* argument/receiver
    /// expression already in hand, so `current_narrowed_type` (which
    /// needs a real `Expr` for its own `infer_expr` fallback) applies
    /// directly; `ThisProp` has no such expression to offer (`$this->
    /// prop` is synthesized from docblock text, never a real parsed
    /// node) - its own "not already narrowed" fallback instead looks the
    /// property's declared type up directly from the receiver's own
    /// resolved class, sidestepping the need to fabricate one.
    fn assert_target_key_and_current(
        &self,
        target: &crate::registry::AssertTarget,
        sig: &crate::registry::FunctionSig,
        recv: Option<&Expr>,
        args: &[Arg],
        locals: &Locals,
    ) -> Option<(String, Type)> {
        use crate::registry::AssertTarget;
        match target {
            AssertTarget::Param(name) => {
                let idx = sig.params.iter().position(|p| &p.name == name)?;
                let arg = args.get(idx)?;
                let key = narrowing_key(&arg.value)?;
                let current = self.current_narrowed_type(&arg.value, &key, locals);
                Some((key, current))
            }
            AssertTarget::ThisSelf => {
                let recv = recv?;
                let key = narrowing_key(recv)?;
                let current = self.current_narrowed_type(recv, &key, locals);
                Some((key, current))
            }
            AssertTarget::ThisProp(prop) => {
                let recv = recv?;
                let base_key = narrowing_key(recv)?;
                let key = format!("{base_key}->{prop}");
                let current = match locals.get(&key) {
                    Some(t) => t.clone(),
                    // Real bug found and fixed this session: this used to
                    // match `Type::Object` directly, silently falling to
                    // `Type::Mixed` (discarding the property's own real
                    // type for narrowing purposes) for a `Union`/
                    // `Intersection` receiver. `property_access_type`
                    // (`checker/expr.rs`, shared with `ExprKind::
                    // PropertyAccess`) already recurses through both.
                    None => self.property_access_type(&self.infer_expr(recv, locals), prop),
                };
                Some((key, current))
            }
        }
    }
}

/// Phase 32 Finding F: the expression `current_narrowed_type`'s own
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
fn narrowing_infer_target(e: &Expr) -> &Expr {
    match &e.kind {
        ExprKind::Grouping(inner) => narrowing_infer_target(inner),
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
/// instanceof Variable && $arg->value->name` (an extremely common
/// AST-visitor idiom - narrow a property, then read a property only the
/// narrowed-to subtype has) had no way to remember that narrowing at
/// all, so the second half was checked against `$arg->value`'s
/// un-narrowed declared type. Found via real false positives once
/// undefined-property-on-read detection started actually exercising
/// this: nikic/php-parser AST-visitor code in this project's own
/// `tools/` does exactly this pattern.
///
/// Phase 22's own follow-on: the identical gap for a literal array-index
/// chain instead of a property chain - `is_string($lang_info
/// ['jquery_code'])` never narrowed `$lang_info['jquery_code']` at all
/// (every narrowing call site below required a bare `ExprKind::Variable`
/// directly, bypassing this function entirely, until Phase 22 changed
/// them to go through `narrowing_key` too), so a later read of the same
/// expression saw its un-narrowed, wider declared/inferred type - a real
/// corpus example (`Template::__construct`) fed that wider type into a
/// call whose declared parameter type no longer matched. A *dynamic*
/// key (`$a[$k]`) can't be narrowed this way - the same key at another
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
        ExprKind::Variable(name) => Some(name.clone()),
        ExprKind::PropertyAccess(base, prop, _) => {
            narrowing_key(base).map(|base_key| format!("{base_key}->{prop}"))
        }
        // A round of Piwigo-corpus bucketing found a static property
        // (`self::$adminSessionCookies !== null`) had no narrowing key at
        // all - only an instance property's `->` chain was handled above.
        // Scoped to a real static property specifically (the parser's own
        // `$`-prefixed `member` encoding, distinguishing it from a class
        // constant or `Foo::class`) - both this arm and the generic
        // write-side purge-and-update in `bind_assignment_target` key off
        // `narrowing_key` uniformly, so a later `self::$prop = ...`
        // correctly invalidates/replaces this same entry with no
        // additional wiring needed.
        ExprKind::StaticAccess(class_name, member) if member.starts_with('$') => {
            Some(format!("{class_name}::{member}"))
        }
        ExprKind::Index(base, Some(idx)) => {
            let base_key = narrowing_key(base)?;
            match literal_string_key(idx) {
                Some(key) => Some(format!("{base_key}[{key}]")),
                // Phase 33 Finding 5: `is_string($this->mirror[$key])`
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
        // Phase 23: `is_string($result = expr)` - an assignment nested
        // inside the narrowed expression itself, the common "assign-then-
        // check" idiom. An assignment's own runtime value is exactly
        // what ends up stored under its left-hand side, so whatever gets
        // checked here is equally true of `lhs`'s own key afterward -
        // narrowing key can borrow the underlying variable/property/index
        // key without needing its own dedicated storage.
        ExprKind::Assign(lhs, _) => narrowing_key(lhs),
        // Phase 26: `($file = readdir($dir)) !== false` - the
        // "assign-then-check" idiom above, but wrapped in parens, which
        // real PHP requires for the *statement's own* top-level shape
        // (`$file = readdir($dir) !== false` would parse as `$file =
        // (readdir($dir) !== false)` instead - `=` binds looser than
        // `!==` there). Without this, `Assign`'s own arm above never
        // even got a chance to run for the single most common shape it
        // exists to cover. Phase 85's parser fix means the *comparison-
        // operand* position specifically (`false !== $file = readdir($dir)`,
        // no parens, no assignment at the very top of the statement) no
        // longer needs this arm at all - `parse_assignment_tail` already
        // builds the right `Assign` node directly as the comparison's
        // own operand - but plenty of real code still writes the parens
        // out of habit, so both arms stay.
        ExprKind::Grouping(inner) => narrowing_key(inner),
        // Phase 32 Finding F: `is_int($decoded['status'] ?? null)` - a
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
/// match" (`0`) from an actual error - where real PHP leaves `$matches`
/// completely *unchanged* from before the call (confirmed directly
/// against a live interpreter: a malformed pattern with a pre-existing
/// `$m` left `$m` untouched), a third state this project has no way to
/// represent (it isn't "the matched shape" or "empty," it's "whatever it
/// was before this statement") - the same imprecision real PHPStan's own
/// extensions accept, not something this closes either.
///
/// Only **strict** `===`/`!==` against exactly 0 or 1 are recognized -
/// PHP's *loose* `==`/`!=` would make `preg_match(...) == 0` ambiguous
/// between "no match" (`0`) and "error" (`false`, which loosely equals
/// `0` - confirmed directly: `false == 0` is `true` in PHP). `== 1`/`!= 1`
/// happen to still be safe even loosely (`false == 1` is `false`), but a
/// real grep of every `preg_match(_all)?(...)` comparison across this
/// project's own verification corpus found zero uses of loose `==`/`!=`
/// at all - every real comparison is already strict - so dropping loose
/// comparison recognition entirely costs no real coverage.
///
/// Scoped to plain `preg_match` (not `preg_match_all`, which needs none
/// of this - see Finding D), to no-`$flags` callers (mirrors this same
/// scope in `checker/expr.rs`'s own wiring), and to **positional**
/// arguments only - `Arg` carries its own `name: Option<String>` for a
/// named argument, and a bare positional destructure alone could
/// *misidentify* which expression is which under reordered named
/// arguments (`preg_match(subject: $s, pattern: $p, matches: $m)` would
/// otherwise bind `pattern` to what's actually `$s`), so any named
/// argument bails rather than guessing.
///
/// Returns `(pattern_arg, matches_arg, branch_true_means_matched)`.
fn preg_match_condition(cond: &Expr) -> Option<(&Expr, &Expr, bool)> {
    fn as_preg_match_args(e: &Expr) -> Option<&[Arg]> {
        match &e.kind {
            ExprKind::Call(callee, args) => match &callee.kind {
                ExprKind::Name(name) if name.trim_start_matches('\\') == "preg_match" => Some(args),
                _ => None,
            },
            ExprKind::Grouping(inner) => as_preg_match_args(inner),
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
/// Phase 33 Finding 5: without this, nothing ever invalidated a stale
/// composite-key entry when a variable it depends on changed - not just
/// for a variable-subscript key, but for *any* composite key at all
/// (reassigning `$foo` left a stale `"foo->bar"` sitting in `locals`
/// forever). Called from every real call site that can give a bare
/// variable, property chain, or index chain a new value.
///
/// Iterates and removes by key rather than a `retain` call - `Locals`'s
/// own doc comment (`checker.rs`) only documents `insert`/`remove`/
/// `entry`/`get`/`contains_key`/`iter` as verified-compatible with
/// `std::collections::HashMap` on the underlying `imbl` persistent map,
/// not `retain`.
pub(crate) fn purge_stale_narrowing_for(locals: &mut Locals, key: &str) {
    let base_prop = format!("{key}->");
    let base_idx = format!("{key}[");
    let subscript_marker = format!("[${key}]");
    let stale: Vec<String> = locals
        .iter()
        .filter(|(k, _)| {
            k.starts_with(&base_prop) || k.starts_with(&base_idx) || k.contains(&subscript_marker)
        })
        .map(|(k, _)| k.clone())
        .collect();
    for k in stale {
        locals.remove(&k);
    }
}

pub(crate) fn narrowing_type_for_predicate(name: &str) -> Option<Type> {
    match name {
        "is_string" => Some(Type::String),
        "is_int" | "is_integer" | "is_long" => Some(Type::Int),
        "is_float" | "is_double" => Some(Type::Float),
        "is_bool" | "is_boolean" => Some(Type::Bool),
        "is_array" => Some(Type::Array(
            Box::new(Type::unknown_array_key()),
            Box::new(Type::Mixed),
        )),
        // Phase 13 Finding B: `is_countable($x)` is true for exactly
        // `array|Countable` - the only real corpus call site
        // (`SectionPopulator::computeMetaRobots`) guards `count($x)`
        // with it, a correct PHP 8+ idiom (`count()` on anything else is
        // a `TypeError`).
        "is_countable" => Some(Type::Union(vec![
            Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed)),
            Type::Object("Countable".to_string(), Vec::new()),
        ])),
        // Phase 55: real PHP's own native `iterable` type hint, defined
        // as exactly `array|Traversable` - the same target `types::
        // parse_type_string`'s own dedicated `"iterable"` arm resolves
        // the native type hint to, kept in sync deliberately (both are
        // "what does `iterable` actually mean" in one place each).
        "is_iterable" => Some(Type::Union(vec![
            Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed)),
            Type::Object("Traversable".to_string(), Vec::new()),
        ])),
        "is_null" => Some(Type::Null),
        "is_object" => Some(Type::Object("object".to_string(), Vec::new())),
        "is_callable" => Some(Type::Callable),
        // Phase 28, real shape: `proc_open()`/`fsockopen()` return
        // `resource|false`, guarded everywhere in this corpus with
        // `if (! is_resource($x)) { throw ...; }` before `fclose`/
        // `fwrite`/`fread`/`proc_close`/etc. - `resource` has no native
        // PHP type declaration, so it's represented the same
        // `Type::Object("resource", _)` way every other stub/checker
        // site in this project already does.
        "is_resource" => Some(Type::Object("resource".to_string(), Vec::new())),
        // `is_scalar($x)` is true for exactly `int|float|string|bool` -
        // unlike `is_numeric` (its own dedicated arm above, not a plain
        // table entry here, since a string only *sometimes* qualifies),
        // `is_scalar`'s real set is exactly these four concrete types
        // with no such conditional gap.
        "is_scalar" => Some(Type::Union(vec![
            Type::Int,
            Type::Float,
            Type::String,
            Type::Bool,
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
    fn or_chain_narrowing_target<'e>(&self, cond: &'e Expr) -> Option<(String, Type, &'e Expr)> {
        match &cond.kind {
            ExprKind::Binary(BinOp::Or, l, r) => {
                let (lk, lt, le) = self.or_chain_narrowing_target(l)?;
                let (rk, rt, _) = self.or_chain_narrowing_target(r)?;
                (lk == rk).then(|| (lk, union_types(lt, rt), le))
            }
            ExprKind::Grouping(inner) => self.or_chain_narrowing_target(inner),
            ExprKind::Call(callee, args) => {
                let (ExprKind::Name(name), [single_arg]) = (&callee.kind, args.as_slice()) else {
                    return None;
                };
                let key = narrowing_key(&single_arg.value)?;
                let target_ty = narrowing_type_for_predicate(name.trim_start_matches('\\'))?;
                Some((key, target_ty, &single_arg.value))
            }
            // A real, corpus-confirmed gap (Phase 51's own cast-validity
            // check surfaced it: `$node instanceof A || $node instanceof
            // B ? (string) $node : ...` cast-checked against the
            // un-narrowed base type, since this whole match had no leaf
            // for `instanceof` at all despite single-condition `if ($x
            // instanceof Foo)` narrowing already working fine elsewhere -
            // `latte/latte`'s own `PrintContext::objectProperty`/
            // `memberAsString`). A dynamic target (`$x instanceof
            // $className`) still doesn't name a concrete class to narrow
            // to, same as the single-condition `ExprKind::Instanceof` arm
            // above already requires.
            ExprKind::Instanceof(inner, InstanceofTarget::Name(class_name, _)) => {
                let key = narrowing_key(inner)?;
                let target_ty = self.resolve_name_expr(class_name);
                Some((key, target_ty, inner))
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
    fn and_chain_narrowing_target<'e>(&self, cond: &'e Expr) -> Option<(String, Type, &'e Expr)> {
        match &cond.kind {
            ExprKind::Binary(BinOp::And, l, r) => {
                let (lk, lt, le) = self.and_chain_narrowing_target(l)?;
                let (rk, rt, _) = self.and_chain_narrowing_target(r)?;
                (lk == rk).then(|| (lk, union_types(lt, rt), le))
            }
            ExprKind::Grouping(inner) => self.and_chain_narrowing_target(inner),
            ExprKind::Unary(UnaryOp::Not, inner) => self.or_chain_narrowing_target(inner),
            _ => None,
        }
    }
}

/// Narrows `original` to `target` along the branch where a check for
/// `target` succeeded: if `original` is a union, keep only the member(s)
/// that match; otherwise just trust the more specific `target`.
///
/// Phase 32 Finding L: a union member is kept when it's a *subtype* of
/// `target`, not just when it's structurally identical to it - exact
/// equality is the wrong tool for "is this the kind of thing being
/// narrowed": `is_array()`'s registered target is generic
/// `Type::Array(Mixed)`, so a specific-element `Array(String)` member
/// (from a docblock-refined param type) used to survive an `is_array()`
/// exclusion untouched, since `Array(String) != Array(Mixed)` even
/// though every value it describes plainly *is* an array. Deliberately
/// scoped to this `Union(members)` branch only, not the `_ => target.
/// clone()` fallback below it for a non-`Union` `original` - that
/// fallback already unconditionally trusts `target` regardless of
/// `original`'s own content, so there's nothing for `is_subtype` to
/// usefully change there.
/// `instanceof`'s own true-branch narrowing - deliberately not
/// `narrow_type_to` above, which unconditionally trusts `target` for a
/// non-`Union` `original`. That's correct for a scalar predicate
/// (`is_int($x)` on `int|string` needs nothing more than plain `int` -
/// mutually exclusive scalar types can't combine), but wrong for
/// `instanceof` against an unrelated *sibling* interface/class: `$view:
/// View` (an interface), `if ($view instanceof HasPageAssets)` (a
/// second, unrelated interface neither extending nor implementing the
/// other) needs `View&HasPageAssets`, not bare `HasPageAssets` -
/// discarding `View` entirely lost real information a caller elsewhere
/// still needed, a real corpus bug (`Template\Renderer::render`'s own
/// three independent, sequential `instanceof` checks on the same `View
/// $view` parameter). Only builds a real `Intersection` when neither
/// side is already a sub/supertype of the other - the common down-cast
/// (`Base` narrowed to a known `Child`) and nullable-narrowing
/// (`?Foo` narrowed to `Foo`) cases both still resolve to a single
/// class/interface exactly as before.
fn narrow_instanceof_true(current: &Type, target: &Type) -> Type {
    if is_subtype(target, current) {
        return target.clone();
    }
    if is_subtype(current, target) {
        return current.clone();
    }
    // A nullable `current` (`?Socket`, i.e. `Union([Object("Socket"),
    // Null])`) can never actually be `null` once a true `instanceof`
    // check has passed - `null instanceof AnyClass` is always `false` in
    // real PHP - so `null` is dropped before deciding whether what's
    // left is object-like enough to intersect with. Without this, a
    // nullable property/variable checked against an *unrelated*
    // interface (real corpus shape: amphp/http-client's own
    // `Http1Connection::watchIdleConnection()`, `?Socket` checked via
    // `instanceof ResourceStream`) fell all the way through to the
    // simpler "just trust the checked class" branch below - discarding
    // `Socket` entirely instead of intersecting down to `Socket&
    // ResourceStream` - so every one of `Socket`'s own members (real
    // shape: a later `$socket?->read()`) looked confirmed-missing on
    // the resulting bare `ResourceStream`.
    let non_null = narrow_type_excluding(current, &Type::Null);
    // Only combined into a real Intersection when every branch of
    // `current` (once stripped of any nullability, per above) is itself
    // provably object-like - anything else (`Mixed`, or a scalar/array
    // type that could never actually satisfy a true `instanceof` in the
    // first place, a sign this project's own upstream inference already
    // went wrong somewhere *else*) falls back to the simpler "just trust
    // the checked class" behavior instead of risking compounding an
    // unrelated bug into a nonsensical type like `null&ImageId`. Found
    // live: `$id = is_array($row) ? ($row['id'] ?? null) : null;` was
    // itself mis-inferred as bare `Null` (a real, separate gap, not
    // narrowed away here) - this guard is what kept this fix's own
    // regression check from compounding that into exactly this
    // nonsense. Recurses through `Union`/`Intersection` so a *second*,
    // independent `instanceof` check against the same variable (real
    // shape: `Template\Renderer::render`'s own three sequential checks)
    // still combines correctly once the first check's own merge has
    // already turned `current` into a real union of object-like
    // branches.
    if is_object_like(&non_null) {
        Type::Intersection(vec![non_null, target.clone()])
    } else {
        target.clone()
    }
}

fn is_object_like(ty: &Type) -> bool {
    match ty {
        Type::Object(..) => true,
        Type::Union(members) | Type::Intersection(members) => members.iter().all(is_object_like),
        _ => false,
    }
}

/// `registry` is needed for the `Union` arm's own member filter below -
/// plain `types::is_subtype` has no class-hierarchy awareness at all
/// (`types.rs` doesn't depend on `registry.rs`), so it only ever matched
/// a union member against `target` by exact structural equality. A real
/// corpus regression this missed: Doctrine ORM's own `@phpstan-assert-
/// if-true ToOneAssociationMapping $this` (an interface every concrete
/// mapping class implements only *transitively*, through an intermediate
/// abstract class) narrowed the full six-member union down to *zero*
/// matching members, falling through to the `target.clone()` case below
/// and discarding every member's own additional properties/methods
/// entirely - `Registry::is_assignable` is the identical hierarchy-aware
/// check `is_subtype` is documented as a non-hierarchy-aware subset of,
/// so using it here can only keep strictly more genuinely-compatible
/// members than before, never fewer.
pub(crate) fn narrow_type_to(registry: &Registry, original: &Type, target: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .filter(|m| registry.is_assignable(m, target))
                .cloned()
                .collect();
            match kept.len() {
                0 => target.clone(),
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        // Round 14 of the Piwigo corpus cleanup: a real, general
        // unsoundness bug - blindly returning `target` discarded
        // `original` even when `original` was already narrower, wrongly
        // *widening* it. Real corpus shape: `$image_id === $this->
        // sessionService->getRefererImageId()`, where `$image_id` is a
        // definite `int` and `getRefererImageId()` returns `int|null` -
        // the pre-existing code overwrote `$image_id` itself with the
        // broader `int|null` inside the `===` true branch, corrupting
        // every downstream use for the rest of the method. If `original`
        // is already at least as narrow as `target`, real equality
        // proves nothing new about it - keep it unchanged; only fall
        // back to `target` when it's the narrower side (the literal-
        // narrowing case this fallback exists for, `$x === 'foo'`).
        // Same hierarchy-aware upgrade as the `Union` arm above: a single
        // concrete class that only transitively implements `target` (an
        // interface inherited from an ancestor, not declared directly)
        // must still keep its own, more specific type here rather than
        // needlessly widening to the bare interface.
        _ if registry.is_assignable(original, target) => original.clone(),
        _ => target.clone(),
    }
}

/// Narrows `original` by removing `excluded` along the branch where a check
/// for it failed - the "not null" half of a `=== null` guard clause, the
/// most common narrowing pattern in real code.
///
/// Phase 32 Finding L: a union member is dropped when it's a *subtype* of
/// `excluded`, not just when it's structurally identical to it - the same
/// reasoning as `narrow_type_to` above, mirrored for exclusion (`is_array
/// (...)`'s *false*-branch exclusion of `Array(Mixed)` used to leave a
/// specific-element `Array(String)` member behind, since it was never
/// `==` to the generic target it should have matched as "an array").
///
/// Deliberately **not** applied to the non-`Union` fallback below
/// (`other if other == excluded => Type::Never`): `is_subtype`'s own
/// int-to-float argument-coercion rule (`Type::Int` is a subtype of
/// `Type::Float`) would make `narrow_type_excluding(Type::Int,
/// Type::Float)` wrongly collapse a plain, correctly-known `int` to
/// `Type::Never` - the value genuinely is an int, and its coercibility to
/// `float` for parameter-passing purposes has nothing to do with whether
/// it should be excluded here. Every existing caller of this branch
/// (`isset`'s `Null` exclusion, and similar plain-`Null` exclusions
/// elsewhere) is unaffected either way, since no other `Type` variant is
/// ever a subtype of `Null`.
/// Whether `ty` is proven to hold exactly one possible value - the only
/// kind of type it's sound to *exclude* from another type based on a
/// runtime `===`/`!==` comparison (`apply_equality_narrowing`'s own use).
/// `Null`/`True`/`False` and a literal are each their own single value;
/// everything else (`Bool`, `Int`, `String`, any other general/composite
/// type) covers multiple possible values, so knowing only "the other side
/// has this general type" - not which of its values it actually holds -
/// proves nothing about which value to rule out.
fn is_known_single_value(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Null
            | Type::True
            | Type::False
            | Type::IntLiteral(_)
            | Type::StringLiteral(_)
            // Phase 79: `Suit::Hearts` is exactly as single-valued as any
            // other literal - `$case !== Suit::Hearts` should exclude
            // that one case, the same way `$x !== 'a'` already excludes
            // that one string.
            | Type::EnumCase(_, _)
    ) ||
    // Found by a round of Piwigo-corpus bucketing: `$x !== []` (the same
    // idiom `$x !== ''` already gets, Phase 14) never excluded the empty
    // shape at all - an empty array literal is exactly as exhaustively
    // single-valued as `''`/a literal int, just structural rather than a
    // scalar `Type` variant.
    matches!(ty, Type::ArrayShape(fields) if fields.is_empty())
}

pub(crate) fn narrow_type_excluding(original: &Type, excluded: &Type) -> Type {
    match original {
        // Recurses into `narrow_type_excluding` for each kept member,
        // rather than just cloning it, so a `String`/`Array` member
        // found *inside* a `Union` gets the same Phase 14 accessory
        // upgrade a bare one does below - behavior-preserving for every
        // other existing caller, since a member that doesn't hit a new
        // arm still falls through to the same `other.clone()` it
        // reached directly before.
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .filter(|m| !is_subtype(m, excluded))
                .map(|m| narrow_type_excluding(m, excluded))
                .collect();
            match kept.len() {
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        // Phase 14: `$x !== ''` on a plain `string` - verified against
        // real PHPStan's own dumpType(). Deliberately not extended to an
        // already-Intersection-refined value (e.g. upgrading an existing
        // `non-empty-string` to `non-falsy-string` inside a nested
        // `!empty($x)` check) - a real gap, but a rare, redundant
        // double-guard pattern with no corpus evidence.
        Type::String if *excluded == Type::StringLiteral(String::new()) => {
            Type::Intersection(vec![
                Type::String,
                Type::Accessory(AccessoryKind::NonEmptyString),
            ])
        }
        // `$x !== []` on a plain `array`/`list` - the same idiom as the
        // `String` arm above, verified against real PHPStan's own
        // dumpType(): both a plain `array<T>` and an already-`list<T>`
        // upgrade to their own non-empty form. Shares `upgrade_array_to_
        // non_empty` with `!empty($x)`/`count($x) > 0` so the three
        // "prove this array is non-empty" paths can't drift apart.
        Type::Array(..) | Type::Intersection(_) if matches!(excluded, Type::ArrayShape(fields) if fields.is_empty()) => {
            upgrade_array_to_non_empty(original)
        }
        // Real PHP's own `Traversable` is a marker interface userland
        // code can never implement directly (`class Foo implements
        // Traversable {}` is a real fatal error, confirmed via `php -r`)
        // - only through one of its exactly two direct subinterfaces,
        // `Iterator` and `IteratorAggregate`. Excluding one therefore
        // structurally *proves* the other, unlike the general "we can't
        // disprove anything more precise than the type itself" fallback
        // below - real PHPStan special-cases this identical relationship.
        // Real corpus false positive this fixes: `Symfony\Component\
        // Messenger\MessageBus::dispatch()`'s own `while ($x instanceof
        // IteratorAggregate) { $x = $x->getIterator(); }` loop-exit type
        // staying bare `Traversable` (missing `rewind()`/`valid()`/
        // `current()`) instead of narrowing to the only other
        // structurally possible answer, `Iterator`.
        Type::Object(name, _)
            if name == "Traversable"
                && matches!(excluded, Type::Object(excl, _) if excl == "IteratorAggregate") =>
        {
            Type::Object("Iterator".to_string(), Vec::new())
        }
        Type::Object(name, _)
            if name == "Traversable"
                && matches!(excluded, Type::Object(excl, _) if excl == "Iterator") =>
        {
            Type::Object("IteratorAggregate".to_string(), Vec::new())
        }
        other if other == excluded => Type::Never,
        other => other.clone(),
    }
}

/// Narrows `ty` by whether `key` is (`present = true`) or isn't
/// (`present = false`) provably a real offset of it -
/// `array_key_exists($key, $ty)`'s own true/false branches. An
/// `ArrayShape` member declaring `key` (required or optional) can always
/// make the call `true`; only a member declaring it *required* can be
/// proven to make the call `false` (an optional field's absence is
/// exactly what "optional" means, so its being false there is not
/// impossible for that member specifically). A member omitting `key`
/// altogether can never make the call `true`. Recurses through `Union`
/// the same way `narrow_type_to`/`narrow_type_excluding` do; anything
/// that isn't an `ArrayShape` (or a `Union` of them) is left untouched,
/// this project's usual "don't guess" precedent.
pub(crate) fn narrow_array_shape_by_key_presence(ty: &Type, key: &str, present: bool) -> Type {
    match ty {
        Type::ArrayShape(fields) => match fields.iter().find(|(k, _, _)| k == key) {
            Some((_, _, optional)) => {
                if !present && !*optional {
                    Type::Never
                } else {
                    ty.clone()
                }
            }
            None => {
                if present {
                    Type::Never
                } else {
                    ty.clone()
                }
            }
        },
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .map(|m| narrow_array_shape_by_key_presence(m, key, present))
                .filter(|m| *m != Type::Never)
                .collect();
            match kept.len() {
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        _ => ty.clone(),
    }
}

/// Narrows `original` by excluding every falsy member this project can
/// represent - used for a bare truthiness check (`if ($x)`) and
/// `empty($x)`'s own truthy branch. Phase 14 extends the falsy set from
/// just `Null`/`False` to the full real set (`IntLiteral(0)`,
/// `StringLiteral("")`/`StringLiteral("0")`, an empty `ArrayShape`) and
/// upgrades a *kept* bare `String`/`Array` to its own accessory-refined
/// form (`non-falsy-string`/`non-empty-array` - `empty()` is false for
/// `'0'` too, not merely non-empty, verified against real PHPStan's own
/// dumpType()). Recurses through `Union` members and drops any that
/// resolve to `Never`, rather than pre-filtering and cloning separately -
/// simpler than maintaining two parallel "is this falsy" checks.
pub(crate) fn narrow_type_excluding_falsy(original: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .map(narrow_type_excluding_falsy)
                .filter(|t| *t != Type::Never)
                .collect();
            match kept.len() {
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        Type::Null | Type::False | Type::IntLiteral(0) => Type::Never,
        Type::StringLiteral(s) if s.is_empty() || s == "0" => Type::Never,
        Type::ArrayShape(fields) if fields.is_empty() => Type::Never,
        Type::String => Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::NonFalsyString),
        ]),
        Type::Array(_, _) => upgrade_array_to_non_empty(original),
        other => other.clone(),
    }
}

/// Shared by `narrow_type_excluding_falsy`'s own `Array` arm above and
/// the `count($x) > 0` narrowing below, so the two "prove this array is
/// non-empty" paths can't drift apart. Handles both a bare `Type::Array`
/// and an already-`Intersection([Array(_), Accessory(ArrayList)])` value
/// (a `list<T>`) - upgrading the latter to `non-empty-list<T>` rather
/// than leaving it unchanged - and avoids adding a duplicate
/// `NonEmptyArray` member if one is already present.
/// Phase 14: backs `is_numeric($x)`'s own true-branch narrowing above -
/// upgrades a kept `String`, bare or found inside a `Union`, to
/// `numeric-string`. The false branch is intentionally left alone (a
/// string that fails `is_numeric()` is still validly just `String`).
fn upgrade_string_to_numeric(ty: &Type) -> Type {
    upgrade_string_with_accessory(ty, AccessoryKind::NumericString)
}

/// Adds `kind` to a `String` value - bare, or already refined by some
/// other accessory kind (`non-empty-string`, `literal-string`, ... - any
/// `Intersection` whose base is `String`), recursing into a `Union`'s own
/// members the same way. Shared by `upgrade_string_to_numeric` (Phase 14,
/// `is_numeric($x)`) and the `str_contains`/`str_starts_with`/
/// `str_ends_with` haystack narrowing (Phase 55's own still-open list) -
/// both are "prove this string satisfies one more accessory fact" upgrades
/// that would otherwise duplicate the identical bare/intersection/union
/// recursion. Phase 52's own finding motivated the "already-refined
/// string" arm: a preg_match() capture group (`non-empty-string`)
/// surviving an `is_numeric()` guard is a real corpus shape
/// (`NumberComparator::__construct`), and silently falling through to
/// `other => other.clone()` left it un-upgraded.
fn upgrade_string_with_accessory(ty: &Type, kind: AccessoryKind) -> Type {
    match ty {
        Type::String => Type::Intersection(vec![Type::String, Type::Accessory(kind)]),
        Type::Intersection(members)
            if members.iter().any(|m| matches!(m, Type::String))
                && !members.contains(&Type::Accessory(kind)) =>
        {
            let mut members = members.clone();
            members.push(Type::Accessory(kind));
            Type::Intersection(members)
        }
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|m| upgrade_string_with_accessory(m, kind))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn upgrade_array_to_non_empty(ty: &Type) -> Type {
    match ty {
        Type::Array(key, inner) => Type::Intersection(vec![
            Type::Array(key.clone(), inner.clone()),
            Type::Accessory(AccessoryKind::NonEmptyArray),
        ]),
        Type::Intersection(members)
            if members.iter().any(|m| matches!(m, Type::Array(_, _)))
                && !members.contains(&Type::Accessory(AccessoryKind::NonEmptyArray)) =>
        {
            let mut members = members.clone();
            members.push(Type::Accessory(AccessoryKind::NonEmptyArray));
            Type::Intersection(members)
        }
        _ => ty.clone(),
    }
}

/// `array_is_list($x)`'s own true-branch narrowing - mirrors `upgrade_
/// array_to_non_empty` exactly, just adding `ArrayList` instead of
/// `NonEmptyArray`. Doesn't force the key type itself to `Type::Int` -
/// every other list-producing call site in this project (`preg_split`,
/// `array_values`, ...) leaves the stored key as `unknown_array_key()`
/// too, relying on `describe_accessory_intersection`'s own display-layer
/// guard (a list's key is always `int` by definition, so showing it
/// would be redundant) rather than encoding it twice. A docblock-parsed
/// `list<T>` still uses a literal `Type::Int` key instead (needed
/// elsewhere - see `parse_type_string`'s own `"list"` arm), so `types::
/// is_subtype`'s dedicated list-vs-list `Intersection` arm and `types::
/// types_equal_for_union` both compare the two representations ignoring
/// key entirely, rather than this project settling on one encoding
/// everywhere.
fn upgrade_array_to_list(ty: &Type) -> Type {
    match ty {
        Type::Array(key, inner) => Type::Intersection(vec![
            Type::Array(key.clone(), inner.clone()),
            Type::Accessory(AccessoryKind::ArrayList),
        ]),
        Type::Intersection(members)
            if members.iter().any(|m| matches!(m, Type::Array(_, _)))
                && !members.contains(&Type::Accessory(AccessoryKind::ArrayList)) =>
        {
            let mut members = members.clone();
            members.push(Type::Accessory(AccessoryKind::ArrayList));
            Type::Intersection(members)
        }
        _ => ty.clone(),
    }
}
