use crate::ast::ClassKind;
use crate::registry::Registry;
use crate::types::{self, Type, Variance};

impl Registry {
    /// Type-compatibility check that consults the class hierarchy for
    /// `Object` types; every other case defers to `types::is_subtype`,
    /// which has no notion of class context.
    pub fn is_assignable(&self, sub: &Type, sup: &Type) -> bool {
        match (sub, sup) {
            // The native `object` type hint (`Type::Object("object", _)` -
            // `types::parse_type_string`'s own sentinel for it, since
            // `object` is a reserved word no real class can ever be named)
            // is satisfied by *any* class instance - a real, pre-existing
            // gap this had never hit before Phase 10.5's bound-checking
            // started comparing against it for the first time (a real,
            // extremely common pattern: Doctrine's `EntityRepository
            // <T of object>`). Confirmed live: a plain native `function
            // f(): object { return new Foo(); }` was already
            // incorrectly flagged before this fix, unrelated to generics
            // at all.
            (Type::Object(_, _), Type::Object(b, _)) if b == "object" => true,
            // Real PHP recognizes (at least) four distinct shapes as a
            // valid `callable`: an actual `Closure` (already handled by
            // the ordinary class-hierarchy arm just below, since
            // `Closure` is now its own real `Object` type rather than
            // being conflated with `Type::Callable` itself), an object
            // whose class declares `__invoke()`, a plain string naming a
            // real function, and a two-element array naming an instance-
            // or static-method callable. The latter two can't be
            // statically verified further without evaluating the string/
            // array's own literal content - accepting them outright
            // matches this project's existing "don't guess, don't
            // false-positive" leniency elsewhere (e.g. the `Array(Mixed)`
            // leniency just below).
            (Type::Object(class, _), Type::Callable(_)) => {
                self.lookup_method(class, "__invoke").is_some()
            }
            (Type::String | Type::StringLiteral(_), Type::Callable(_)) => true,
            (Type::Array(_, _) | Type::ArrayShape(_), Type::Callable(_)) => true,
            // Phase 54: type arguments used to be ignored here entirely
            // (`Collection<Cat>` and `Collection<Dog>` were mutually
            // assignable, always) - this project now genuinely models
            // variance (`Type::TemplateParam::variance`, `@template-
            // covariant`/`-contravariant`, and Phase 54's own call-site
            // `covariant`/`contravariant`/`*` annotations,
            // `types::parse_generic_arg`), so this comparison now
            // actually applies it instead of skipping it. If `sup`'s own
            // class isn't registered at all (an uninstalled dependency,
            // or a PHAR-bundled one like `phpstan/phpstan` itself with no
            // loose source to parse - the same "can't verify, don't
            // guess" case `check_generic_bounds`'s own doc comment
            // names), fall back to the pre-Phase-54 name/hierarchy-only
            // comparison unchanged - the one deliberate leniency left in
            // this design.
            // Phase 54: type arguments used to be ignored here entirely
            // (`Collection<Cat>` and `Collection<Dog>` were mutually
            // assignable, always) - this project now genuinely models
            // variance (`Type::TemplateParam::variance`, `@template-
            // covariant`/`-contravariant`, and Phase 54's own call-site
            // `covariant`/`contravariant`/`*` annotations,
            // `types::parse_generic_arg`), so this comparison now
            // actually applies it instead of skipping it. If `sup`'s own
            // class isn't registered at all (an uninstalled dependency,
            // or a PHAR-bundled one like `phpstan/phpstan` itself with no
            // loose source to parse - the same "can't verify, don't
            // guess" case `check_generic_bounds`'s own doc comment
            // names), fall back to the pre-Phase-54 name/hierarchy-only
            // comparison unchanged - the one deliberate leniency left in
            // this design.
            (Type::Object(a, args_a), Type::Object(b, args_b)) => {
                if a != b && !self.is_subclass_of(a, b) {
                    return false;
                }
                let Some(info_b) = self.classes.get(b) else {
                    return true;
                };
                // `b` isn't generic at all, so the whole template-binding
                // walk below (`class_template_bindings`'s ancestor-chain
                // recursion, allocating a `HashSet`/`HashMap` per call) is
                // guaranteed to produce nothing the loop below would ever
                // read - `info_b.templates` being empty makes that loop a
                // no-op regardless of `effective_args_a`'s value. Found and
                // fixed via a perf-optimization investigation workflow;
                // measured as the single largest real, cleanly-attributable
                // cost in this function across two real corpora.
                if info_b.templates.is_empty() {
                    return true;
                }
                // A missing trailing slot (an "erased"/under-supplied
                // generic reference - a bare `Collection` with no `<T>`
                // at all is the overwhelmingly common real shape) resolves
                // to plain `Mixed`, not that template's own declared
                // bound. Deliberately verified, not assumed: padding with
                // the *bound* instead would make a bare reference to any
                // class with a real (non-`Mixed`) bound - a very common
                // shape, `@template T of SomeInterface` - fail the
                // Invariant comparison below against *any* concrete
                // argument, since `bound != concrete-subtype` invariantly
                // (`is_assignable(bound, concrete)` fails whenever
                // `concrete` is a strict subtype of `bound`) - a real,
                // dangerous new false-positive class this project doesn't
                // have data to avoid anyway: `ClassInfo` only persists
                // `template_bounds`, never a class-level `@template T =
                // Default` map the way `TemplateParam.default` carries
                // one for a function/method's own templates (`types::
                // substitute_template`'s own real, shipped behavior pads
                // with `.default`, falling back to `Mixed` - never the
                // bound). Plain `Mixed` is both what's actually available
                // and what already skips this slot's comparison entirely
                // via the "either side Mixed" rule below - correctly
                // lenient for the common case without guessing wrong.
                // `args_a` is in `a`'s own frame - map it into `b`'s own
                // template names via the same ancestor-substitution walk
                // `extends_args`/`implements_args` already use elsewhere,
                // unless `a == b` and it's already in the right frame.
                let effective_args_a: Vec<Type> = if a == b {
                    args_a.clone()
                } else {
                    let bindings = self.class_template_bindings(a, args_a, b);
                    info_b
                        .templates
                        .iter()
                        .map(|name| bindings.get(name).cloned().unwrap_or(Type::Mixed))
                        .collect()
                };
                for (i, _name) in info_b.templates.iter().enumerate() {
                    let a_i = effective_args_a.get(i).cloned().unwrap_or(Type::Mixed);
                    let b_i = args_b.get(i).cloned().unwrap_or(Type::Mixed);
                    // A call-site variance marker on `sup`'s own argument
                    // (`Collection<covariant Foo>`) overrides `b`'s own
                    // declared slot variance for this one comparison,
                    // exactly matching `GenericObjectType::isSuperTypeOf`/
                    // `getReferencedTemplateTypes` read directly from
                    // `vendor/phpstan-src` - `None` here means a
                    // `"$$bivariant"` (`*`) marker, real PHPStan's own
                    // "don't check this argument at all."
                    let Some(variance) = self.effective_slot_variance(b, i, &b_i) else {
                        continue;
                    };
                    let a_i = types::strip_call_site_variance(&a_i)
                        .cloned()
                        .unwrap_or(Type::Mixed);
                    let b_i = types::strip_call_site_variance(&b_i)
                        .cloned()
                        .unwrap_or(Type::Mixed);
                    // Either side unresolved/erased - don't know, don't
                    // guess, matches this project's leniency everywhere
                    // else a generic argument can't be verified. A raw,
                    // still-unbound `TemplateParam` (a method/function's
                    // own `@template`, e.g. `E` in `EntityRepository<E>
                    // $repository`) is included here too, not just
                    // `Mixed` - a real, corpus-found false positive this
                    // fixed: `checker::calls`'s own call-argument
                    // template binding only ever binds a template used
                    // as a *bare* param type (`T $value`), never one
                    // nested inside another class's own generic argument
                    // list, so `E` reaches this comparison exactly as
                    // unbound as it was declared - comparing it
                    // structurally against the real, concrete argument
                    // is exactly as wrong as comparing `Mixed` would be.
                    // Real corpus shape: Piwigo's own `TypedRepository::
                    // narrow(EntityRepository<E> $repository)` with
                    // `@template E of object` - before this skip existed,
                    // the pre-existing `(TemplateParam(tp), _) =>
                    // matches!(*tp.bound, Type::Mixed) || is_assignable(
                    // &tp.bound, sup)` arm ran the `object`-bound `E`'s
                    // own upper bound against the real argument's
                    // concrete class, `is_assignable(object, Concrete
                    // Entity)`, and correctly said `false` (an arbitrary
                    // object is genuinely not guaranteed to be that one
                    // specific class - not a bug in that arm; a weak
                    // bound like `object` legitimately can't prove the
                    // narrower assignment sound). Skipping this slot
                    // entirely for an unbound template - rather than
                    // trying to make that comparison artificially permit
                    // it - is what's actually correct: the real answer
                    // isn't "yes" here, it's "not yet known, ask again
                    // once `E` is bound," which this project's own call-
                    // argument binding is simply never invoked for at
                    // this nested position.
                    if a_i == Type::Mixed
                        || b_i == Type::Mixed
                        || matches!(a_i, Type::TemplateParam(_))
                        || matches!(b_i, Type::TemplateParam(_))
                    {
                        continue;
                    }
                    let ok = match variance {
                        Variance::Covariant => self.is_assignable(&a_i, &b_i),
                        Variance::Contravariant => self.is_assignable(&b_i, &a_i),
                        // Mutual assignability approximates real
                        // PHPStan's exact `equals()` here - deliberately
                        // chosen over raw structural `==`, since this
                        // codebase's own `Union`/`Intersection` values
                        // have no canonical member order
                        // (`checker::expr::union_types` dedupes but never
                        // sorts), making `==` order-sensitive and *more*
                        // likely to falsely reject two independently
                        // built but semantically identical unions than
                        // mutual `is_assignable` is to miss a genuine
                        // mismatch.
                        Variance::Invariant => {
                            self.is_assignable(&a_i, &b_i) && self.is_assignable(&b_i, &a_i)
                        }
                    };
                    if !ok {
                        return false;
                    }
                }
                true
            }
            // Phase 79: hierarchy-aware version of `types::is_subtype`'s
            // own bare `EnumCase -> Object` name-equality rule - a case
            // is assignable not just to its own enum's bare type but to
            // `UnitEnum`/`BackedEnum` (both synthesized `implements`
            // entries every enum gets, see `ClassInfo::from_enum_decl`)
            // and any interface the enum itself declares.
            (Type::EnumCase(fqn, _), Type::Object(b, _)) => fqn == b || self.is_subclass_of(fqn, b),
            // Two different cases of the same enum are never
            // interchangeable (`types::is_subtype`'s own `a == b` fast
            // path already covers the identical-case case) - no
            // hierarchy to widen through beyond what's handled above.
            (Type::EnumCase(_, _), Type::EnumCase(_, _)) => false,
            // Same hierarchy-aware override as the plain Object-vs-Object
            // arm just above, one level up: `types::is_subtype`'s own
            // `ClassString` arm inherits its non-hierarchy-aware Object
            // comparison (name equality only), so without this a
            // `class-string<Derived>` value would incorrectly fail to
            // satisfy a `class-string<Base>`-typed parameter even when
            // `Derived extends Base`.
            // `interface-string`/`trait-string`'s own internal
            // `"$$interface"`/`"$$trait"` sentinel bound (`types::
            // parse_type_keyword_or_class`) - the *only* two `ClassString`
            // inner types this project ever builds that aren't a real
            // class reference at all. real PHPStan itself draws no
            // distinction here (bare `interface-string`/`trait-string`
            // resolve to the identical untyped `ClassStringType()` as
            // bare `class-string`), but this project is allowed to check
            // more strictly - `ClassInfo::kind` already tracks exactly
            // this, so check it directly instead of the ordinary
            // class-hierarchy walk below (which would need `sub`'s own
            // name to be a real ancestor of a sentinel name that was
            // never indexed at all, always failing).
            (Type::ClassString(a), Type::ClassString(b)) if matches!(&**b, Type::Object(name, _) if name == "$$interface" || name == "$$trait") =>
            {
                let Type::Object(name, _) = &**a else {
                    return false;
                };
                let Type::Object(sentinel, _) = &**b else {
                    unreachable!("guarded by the match arm's own condition above")
                };
                let wanted_kind = if sentinel == "$$interface" {
                    ClassKind::Interface
                } else {
                    ClassKind::Trait
                };
                self.lookup_class(name)
                    .is_some_and(|(_, info)| info.kind == wanted_kind)
            }
            (Type::ClassString(a), Type::ClassString(b)) => self.is_assignable(a, b),
            // Mirrors `types::is_subtype`'s own bound-aware `TemplateParam`
            // arm (see its doc comment for the full reasoning and why
            // this needs to run *before* the `Union` arms just below),
            // but recurses through `self.is_assignable` rather than the
            // plain, hierarchy-*unaware* `types::is_subtype` - a real bug
            // in an earlier version of this exact fix: delegating
            // straight to `types::is_subtype(sub, sup)` silently dropped
            // the class-hierarchy-walking half of this whole method for
            // any bound check involving a `TemplateParam`, wrongly
            // flagging real corpus code where a template's own bound is a
            // union of *concrete classes* needing a real ancestor walk to
            // match a single-class target (`phpstan/phpstan-strict-rules`'s
            // own `@phpstan-template TNodeType of PreInc|PreDec|PostInc|
            // PostDec` satisfying `Rule<TNodeType>`'s own `TNodeType of
            // Node` bound - each of those four classes extends `Node`
            // only via `Expr`, a real, multi-level ancestor chain plain
            // name-equality can't see).
            (Type::TemplateParam(tp), _) => {
                matches!(*tp.bound, Type::Mixed) || self.is_assignable(&tp.bound, sup)
            }
            (Type::Union(members), _) => members.iter().all(|m| self.is_assignable(m, sup)),
            (_, Type::Union(members)) => members.iter().any(|m| self.is_assignable(sub, m)),
            // Phase 14: mirrors `types::is_subtype`'s own dedicated
            // Intersection-vs-Intersection rule, and for the same reason
            // - without it, this function's own generic `(Intersection,
            // _) => any(...)` rule below intercepts a same-shaped pair
            // (e.g. `list<string>` checked against a `list<string>`-
            // typed parameter) first and gets it wrong, since it only
            // asks whether one *bare* member of `sub` alone satisfies
            // the whole `sup` intersection, never whether `sub`'s own
            // combination does - a severe, real regression confirmed via
            // a full corpus re-check (211 -> 1861 diagnostics) before
            // this fix landed.
            (Type::Intersection(_), Type::Intersection(sup_members)) => {
                sup_members.iter().all(|m| self.is_assignable(sub, m))
            }
            (Type::Intersection(members), _) => members.iter().any(|m| self.is_assignable(m, sup)),
            (_, Type::Intersection(members)) => members.iter().all(|m| self.is_assignable(sub, m)),
            // Deliberately more lenient here than types::is_subtype's pure
            // structural relation (which correctly treats Array(Mixed) as
            // NOT a subtype of Array(Int)): an array literal's element type
            // is never inferred precisely by this checker, so it always
            // shows up as Array(Mixed) meaning "unknown", not "really can
            // hold anything" - without this, e.g. a PHPDoc `int[]` param
            // would false-positive against every `[1, 2, 3]` literal.
            // `strict_mixed_arrays` (Phase 11 Stage 5) opts out of this
            // leniency, matching real PHPStan's own level-9-style "be
            // strict about values with mixed type" philosophy.
            (Type::Array(_, a), Type::Array(_, b)) => {
                (!self.strict_mixed_arrays && matches!(**a, Type::Mixed))
                    || self.is_assignable(a, b)
            }
            // Same leniency, same reason, for an array of unknown structure
            // (an array literal, most commonly) checked against a specific
            // expected shape: e.g. `return ["count" => ..., "first" => ...]`
            // against `@return array{count: int, first: string}` shouldn't
            // false-positive just because literal keys/values aren't
            // inferred. A genuinely known, homogeneous Array(V) still
            // correctly fails to satisfy an arbitrary shape.
            // Phase 50: the pre-existing `Array(Mixed)` leniency now also
            // requires the array's own key type to actually accept the
            // shape's real aggregate key type - without this, `array<
            // string, mixed>` leniently satisfied `array{0: X}` (an
            // int-keyed shape) purely because its *value* type was
            // Mixed, ignoring a genuine key-type mismatch the leniency
            // was never meant to paper over. `unknown_array_key()`
            // itself (the "no more precise key type available" default
            // essentially every `Array(_, Mixed)` construction site
            // uses, `types.rs`'s own doc comment) stays unconditionally
            // lenient here too, the exact same "don't know" spirit as
            // the value-side `Mixed` check right next to it - only a
            // *genuinely narrower, known* key type (a real `Type::Int`/
            // `Type::String`) is held to the real subtyping check.
            (Type::Array(key, a), Type::ArrayShape(fields)) => {
                !self.strict_mixed_arrays
                    && matches!(**a, Type::Mixed)
                    && (**key == Type::unknown_array_key()
                        || self.is_assignable(key, &types::shape_key_type(fields)))
            }
            // Finding 48's own completion: now that an array literal
            // carries a real per-position/per-key shape instead of
            // always reading as `Array(Mixed)`, it needs a real rule
            // saying such a shape satisfies an ordinary `array`/
            // `array<T>` hint - without this, `[1, 2, 3]` would stop
            // being assignable to `int[]`/`array<int>` the moment its
            // own type became precise, a real regression `types::
            // is_subtype`'s own (deliberately conservative, pre-Finding-48)
            // rule doesn't allow for.
            // Phase 53: the same `Array(Mixed)` leniency two arms above -
            // an individual shape field with no inferred type of its own
            // (an unresolved global constant, most commonly - real shape:
            // nikic/php-parser's own `$token->is([\T_COMMENT,
            // \T_DOC_COMMENT])`, where neither constant is a name this
            // project's registry models) reads as `Mixed`, not "really
            // can hold anything", so confidently failing it against `b`
            // (e.g. `array<int|string>`) is exactly the false positive
            // this leniency already exists to prevent for a homogeneous
            // `Array(Mixed)` - a shape literal deserves the identical
            // treatment per field, not a stricter one just because its
            // own type happens to be more structurally precise.
            // Phase 50: also requires the shape's own real aggregate key
            // type to be assignable to the array's own declared key type
            // - without this, `array{name: string}` (an all-string-keyed
            // shape) was silently accepted as satisfying `array<int,
            // string>`, checking only whether every field's *value* type
            // matched.
            (Type::ArrayShape(fields), Type::Array(key, b)) => {
                self.is_assignable(&types::shape_key_type(fields), key)
                    && fields
                        .iter()
                        .filter(|(name, _, _)| name != "...")
                        .all(|(_, ty, _)| {
                            (!self.strict_mixed_arrays && matches!(ty, Type::Mixed))
                                || self.is_assignable(ty, b)
                        })
            }
            // Same completion for shape-to-shape: a literal's own shape
            // satisfies a declared shape when every field the declared
            // shape actually requires is present (an optional field may
            // be missing) with an assignable type - extra fields the
            // literal has beyond what's required are ignored, the same
            // "structural, not exact" spirit `Array(Mixed)`'s own
            // leniency already uses elsewhere in this match. The
            // unsealed-shape sentinel (`is_unsealed_shape`'s own doc
            // comment) is filtered out of both sides first - it's a
            // marker, not a real required field, and would otherwise
            // wrongly fail this check the moment either shape happened
            // to be unsealed (`None => *optional` sees a "required"
            // field named `"..."` that plainly never exists on the
            // other side).
            (Type::ArrayShape(sub_fields), Type::ArrayShape(sup_fields)) => {
                sup_fields.iter().filter(|(name, _, _)| name != "...").all(
                    |(name, sup_ty, optional)| match sub_fields
                        .iter()
                        .filter(|(n, _, _)| n != "...")
                        .find(|(n, _, _)| n == name)
                    {
                        Some((_, sub_ty, _)) => self.is_assignable(sub_ty, sup_ty),
                        None => *optional,
                    },
                )
            }
            // Phase 80: the `object{...}`-flavored sibling of the two
            // `ArrayShape` rules just above - a real class instance
            // satisfies a declared object shape when it has every
            // property the shape actually requires (an optional field
            // may be missing) with an assignable type, checked via
            // `lookup_property` rather than an array field lookup;
            // extra properties the class has beyond what's required are
            // ignored, the same "structural, not exact" spirit. `object`/
            // `stdClass` (dynamic-property bags this project doesn't
            // track the real fields of) are accepted unconditionally,
            // matching `Array(Mixed)`'s own established leniency for the
            // identical "can't verify, don't guess wrong" situation.
            (Type::Object(class, _), Type::ObjectShape(fields)) => {
                class == "object"
                    || class == "stdClass"
                    || fields.iter().all(|(name, sup_ty, optional)| {
                        match self.lookup_property(class, name) {
                            Some(p) => self.is_assignable(&p.ty, sup_ty),
                            None => *optional,
                        }
                    })
            }
            (Type::ObjectShape(sub_fields), Type::ObjectShape(sup_fields)) => {
                sup_fields.iter().all(|(name, sup_ty, optional)| {
                    match sub_fields.iter().find(|(n, _, _)| n == name) {
                        Some((_, sub_ty, _)) => self.is_assignable(sub_ty, sup_ty),
                        None => *optional,
                    }
                })
            }
            // Same unresolved-`@phpstan-type`-alias leniency as the
            // Object-vs-Object arm above, generalized to every other
            // `sub` shape: Doctrine DBAL's own `DriverManager::
            // getConnection(Params $params)` is called with a real array
            // *literal* (an `ArrayShape`) at nearly every real call site,
            // never an actual `Params`-named object - only the
            // Object-vs-Object arm's leniency would leave those failing
            // just the same way. Excludes the real `object` pseudo-type
            // (PHP's own native `object` hint - already handled, and
            // correctly strict, in the dedicated arm above): unlike a
            // genuinely-unresolvable alias name, `object` means exactly
            // "some real object instance" and a plain array/string/etc.
            // is correctly never one.
            (_, Type::Object(name, _)) if name != "object" && !self.classes.contains_key(name) => {
                true
            }
            _ => types::is_subtype(sub, sup),
        }
    }
}
