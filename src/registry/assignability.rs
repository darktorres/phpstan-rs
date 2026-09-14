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
            // is satisfied by *any* class instance - an extremely common
            // pattern, e.g. Doctrine's `EntityRepository<T of object>`.
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
            // Type arguments are compared with real variance
            // (`Type::TemplateParam::variance`, `@template-covariant`/
            // `-contravariant`, and call-site `covariant`/`contravariant`/
            // `*` annotations, `types::parse_generic_arg`), not just
            // skipped (`Collection<Cat>` and `Collection<Dog>` are not
            // mutually assignable). If `sup`'s own class isn't registered
            // at all (an uninstalled dependency, or a PHAR-bundled one
            // like `phpstan/phpstan` itself with no loose source to
            // parse), falls back to a plain name/hierarchy-only
            // comparison - the one deliberate leniency left in this
            // design.
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
                // no-op regardless of `effective_args_a`'s value.
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
                    // `Mixed`: `checker::calls`'s own call-argument
                    // template binding only ever binds a template used
                    // as a bare param type (`T $value`), never one
                    // nested inside another class's own generic argument
                    // list, so `E` reaches this comparison exactly as
                    // unbound as it was declared - comparing it
                    // structurally against the real, concrete argument
                    // is exactly as wrong as comparing `Mixed` would be.
                    // Skipping this slot entirely for an unbound template
                    // - rather than falling back to the bound - is what's
                    // actually correct: an arbitrary object is not
                    // guaranteed to be a specific concrete class, so
                    // comparing against a weak bound like `object` would
                    // wrongly reject a legitimately sound assignment; the
                    // real answer is "not yet known, ask again once `E`
                    // is bound," which this project's own call-argument
                    // binding is simply never invoked for at this nested
                    // position.
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
            // Hierarchy-aware version of `types::is_subtype`'s own bare
            // `EnumCase -> Object` name-equality rule - a case
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
            // plain, hierarchy-*unaware* `types::is_subtype`: delegating
            // straight to `types::is_subtype(sub, sup)` would silently
            // drop the class-hierarchy-walking half of this whole method
            // for any bound check involving a `TemplateParam`, wrongly
            // flagging code where a template's own bound is a union of
            // concrete classes needing a real ancestor walk to match a
            // single-class target (e.g. `@phpstan-template TNodeType of
            // PreInc|PreDec|PostInc|PostDec` satisfying `Rule<TNodeType>`'s
            // own `TNodeType of Node` bound - each of those four classes
            // extends `Node` only via `Expr`, a multi-level ancestor
            // chain plain name-equality can't see).
            (Type::TemplateParam(tp), _) => {
                matches!(*tp.bound, Type::Mixed) || self.is_assignable(&tp.bound, sup)
            }
            (Type::Union(members), _) => members.iter().all(|m| self.is_assignable(m, sup)),
            (_, Type::Union(members)) => members.iter().any(|m| self.is_assignable(sub, m)),
            // Mirrors `types::is_subtype`'s own dedicated
            // Intersection-vs-Intersection rule, and for the same reason
            // - without it, this function's own generic `(Intersection,
            // _) => any(...)` rule below intercepts a same-shaped pair
            // (e.g. `list<string>` checked against a `list<string>`-
            // typed parameter) first and gets it wrong, since it only
            // asks whether one bare member of `sub` alone satisfies the
            // whole `sup` intersection, never whether `sub`'s own
            // combination does.
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
            // `strict_mixed_arrays` opts out of this leniency, matching
            // real PHPStan's own level-9-style "be strict about values
            // with mixed type" philosophy.
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
            // The `Array(Mixed)` leniency also requires the array's own
            // key type to actually accept the shape's real aggregate key
            // type - without this, `array<
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
            // Now that an array literal carries a real per-position/
            // per-key shape instead of always reading as `Array(Mixed)`,
            // it needs a real rule saying such a shape satisfies an
            // ordinary `array`/`array<T>` hint - without this, `[1, 2,
            // 3]` would stop being assignable to `int[]`/`array<int>`
            // the moment its own type became precise. An individual
            // shape field with no inferred type of its own (an
            // unresolved global constant, most commonly) reads as
            // `Mixed`, not "really can hold anything", so confidently
            // failing it against `b` (e.g. `array<int|string>`) is
            // exactly the false positive the `Array(Mixed)` leniency
            // already exists to prevent - a shape literal deserves the
            // identical treatment per field. Also requires the shape's
            // own real aggregate key type to be assignable to the
            // array's own declared key type - without this,
            // `array{name: string}` (an all-string-keyed shape) would be
            // silently accepted as satisfying `array<int, string>`,
            // checking only whether every field's value type matched.
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
            // The `object{...}`-flavored sibling of the two `ArrayShape`
            // rules just above - a real class instance
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
            //
            // Also excludes `resource` - this project's own fixed
            // pseudo-type sentinel for PHP's native resource type
            // (`types::parse_type_string`'s own `"resource" =>
            // Type::Object("resource", ...)` arm, reused verbatim by
            // `assertIsResource`'s own narrowing target and `is_resource`'s
            // own predicate type, among others - never a real, resolvable
            // class name this leniency's own "can't verify, don't guess
            // wrong" reasoning was written for). Without this exclusion,
            // `is_assignable(Type::False, Object("resource"))` would
            // wrongly return `true` (since `"resource"` is never a key
            // in `self.classes` either), so `narrow_type_to`'s own
            // `Union` arm would keep a genuinely-impossible `False`
            // member instead of stripping it - e.g. `assertIsResource
            // ($proc)` (a common PHPUnit-assertion narrowing idiom)
            // silently failing to narrow away `resource|false`'s own
            // `false` member, unlike every sibling `assertIs*` narrowing
            // target that isn't itself an `Object`.
            (_, Type::Object(name, _))
                if name != "object" && name != "resource" && !self.classes.contains_key(name) =>
            {
                true
            }
            _ => types::is_subtype(sub, sup),
        }
    }

    /// The level-aware counterpart to `is_assignable` above - real
    /// PHPStan's `RuleLevelHelper::accepts()` two genuinely level-gated
    /// behaviors (`checkNullables`/`checkUnionTypes`) applied to the
    /// identical argument/return/property-type check `is_assignable`
    /// already performs, for the handful of real call sites a `Rule`
    /// class actually consumes `RuleLevelHelper` through (not every
    /// `is_assignable` call site project-wide - narrowing, LSP-override
    /// compatibility, and generics bound-checking are real PHPStan's own
    /// separate, never-level-gated `isSuperTypeOf()` domain, and must
    /// keep calling `is_assignable` directly).
    ///
    /// `self.configured_level` is the project's own `level` setting
    /// *before* `check-project`'s own `MAX_LEVEL`-driven `.unwrap_or(6)`
    /// collapse - `None` (never configured) keeps today's always-strict
    /// answer exactly, the safe default for every existing caller/test
    /// that never opts into this.
    ///
    /// Deliberately not a full tri-state (`Yes`/`No`/`Maybe`) rewrite of
    /// `is_assignable`'s own boolean API: real PHPStan's `checkUnionTypes`
    /// distinguishes "definitely accepts" from "maybe accepts" via
    /// `TrinaryLogic`, a distinction this project's plain boolean
    /// `.all()`/`.any()` combinators (already used, unconditionally, by
    /// `is_assignable` itself) have no way to represent without that
    /// larger rewrite - `UNION_TYPES_CHECK_LEVEL` is still named and
    /// folded into the fast-path guard below (both real gates must be at
    /// their strict threshold before this collapses to `is_assignable`
    /// unchanged), but doesn't get its own separate `.all()`-vs-`.any()`
    /// switch the way `NULLABLES_CHECK_LEVEL` gets its own null-stripping
    /// switch.
    pub fn accepts_at_configured_level(&self, sub: &Type, sup: &Type) -> bool {
        let Some(level) = self.configured_level else {
            return self.is_assignable(sub, sup);
        };
        self.accepts_at_configured_level_rec(sub, sup, level)
    }

    fn accepts_at_configured_level_rec(&self, sub: &Type, sup: &Type, level: u8) -> bool {
        const UNION_TYPES_CHECK_LEVEL: u8 = 7;
        const NULLABLES_CHECK_LEVEL: u8 = 8;

        // Fast path: both real gates already at their strict threshold -
        // byte-identical to plain `is_assignable`, the safe default for
        // a project explicitly configured at (or above) real PHPStan's
        // own strictest level for this behavior. `NULLABLES_CHECK_LEVEL`
        // (8) is strictly above `UNION_TYPES_CHECK_LEVEL` (7), so
        // `level >= NULLABLES_CHECK_LEVEL` alone already implies the
        // other - both consts are still named here (rather than one bare
        // `8`) so this reads as "both real gates," matching real
        // PHPStan's own two separately-named flags.
        if level >= NULLABLES_CHECK_LEVEL {
            debug_assert!(level >= UNION_TYPES_CHECK_LEVEL);
            return self.is_assignable(sub, sup);
        }
        // Fast path: neither side is a `Union` at all - nothing for
        // either level-gated behavior to touch, so this collapses to
        // plain `is_assignable` too (the `Union` arms below are the only
        // ones this method ever needs to diverge from `is_assignable`
        // on).
        if !matches!(sub, Type::Union(_)) && !matches!(sup, Type::Union(_)) {
            return self.is_assignable(sub, sup);
        }

        // `checkNullables` (real PHPStan's own `transformAcceptedType`,
        // `RuleLevelHelper.php:130-137`): below level 8, strip `Null` out
        // of a multi-member `sub` ("acceptedType" in real PHPStan's own
        // naming - the *actual/given* type, e.g. a `?string`-typed
        // argument) Union before deciding acceptance - real PHPStan's own
        // guard, applied exactly as it is there: never when `sup`
        // ("acceptingType" - the *declared/expected* type) is bare
        // `Type::Null` (a nullable value must never satisfy a definite-
        // null-only target), and never when `sub` is bare `Type::Null`
        // itself (a definite null literal stays definitely rejected at
        // every level, `sub` must be a multi-member Union that *includes*
        // Null, not Null alone).
        let stripped_sub = if level < NULLABLES_CHECK_LEVEL
            && *sup != Type::Null
            && *sub != Type::Null
            && let Type::Union(members) = sub
            && members.contains(&Type::Null)
        {
            let non_null: Vec<Type> = members
                .iter()
                .filter(|m| **m != Type::Null)
                .cloned()
                .collect();
            Some(match non_null.len() {
                // Unreachable in practice (`sub` was excluded above
                // whenever it's bare `Type::Null`, so a real Union always
                // has at least one non-Null member) - kept as a safe,
                // total fallback rather than an `unwrap`/panic.
                0 => Type::Never,
                1 => non_null.into_iter().next().unwrap(),
                _ => Type::Union(non_null),
            })
        } else {
            None
        };
        let sub = stripped_sub.as_ref().unwrap_or(sub);

        // Real bug this method exists to fix (found during this phase's
        // own adversarial review): a naive version of this method bailed
        // straight to plain `is_assignable` whenever `sup` was *also* a
        // Union, on the theory that a nested Union-vs-Union pair is rare
        // - but this project represents *every* nullable declared type
        // (`?string`, `?int`, `?Foo`, the single most common PHP
        // nullability idiom) as `Type::Union([X, Type::Null])`
        // (`types/parse.rs`), so `sup` is a Union at every one of this
        // method's own real call sites checking a nullable-declared
        // parameter/property/return type - exactly the scenario
        // `checkNullables` exists for. Recursing into `self.accepts_at_
        // configured_level_rec` (not plain `is_assignable`) on both sides
        // whenever either is a Union, symmetric to `is_assignable`'s own
        // existing `282`/`283` arms, is what keeps that leniency reaching
        // its single most common trigger.
        match (sub, sup) {
            (Type::Union(members), _) => members
                .iter()
                .all(|m| self.accepts_at_configured_level_rec(m, sup, level)),
            (_, Type::Union(members)) => members
                .iter()
                .any(|m| self.accepts_at_configured_level_rec(sub, m, level)),
            _ => self.is_assignable(sub, sup),
        }
    }
}
