use crate::relation::{AcceptsResult, Relation};
use crate::{CallableSig, ClassKind, MixedKind, Repr, Shape, ShapeKey, Type, Variance};

// The levels at which PHPStan's `RuleLevelHelper` checks turn on
// (`rule_level_accepts`, `accepts_at_level`).
const UNION_TYPES_CHECK_LEVEL: u8 = 7;
const NULLABLES_CHECK_LEVEL: u8 = 8;
const EXPLICIT_MIXED_CHECK_LEVEL: u8 = 9;
const IMPLICIT_MIXED_CHECK_LEVEL: u8 = 10;

impl Relation<'_> {
    /// PHPStan's `Type::accepts()`: whether a value of `accepted` may
    /// be passed where `accepting` is declared.
    pub fn accepts(&self, accepting: &Type, accepted: &Type) -> AcceptsResult {
        AcceptsResult::from_bool(self.accepts_type(accepted, accepting))
    }

    /// `accepts`, as a `RuleLevelHelper` applies it at the configured
    /// rule level (`checkNullables`, `checkUnionTypes`,
    /// `checkExplicitMixed`/`checkImplicitMixed`,
    /// `checkBenevolentUnionTypes`).
    pub fn rule_level_accepts(&self, accepting: &Type, accepted: &Type) -> AcceptsResult {
        AcceptsResult::from_bool(self.rule_level_accepts_type(accepted, accepting))
    }

    /// `accepts` as a plain relation of `sub` to `sup`, consulting the
    /// class hierarchy for `Object` types; every other case defers to
    /// `Relation::is_super_type_of`, which has no notion of class context.
    pub(crate) fn accepts_type(&self, sub: &Type, sup: &Type) -> bool {
        stack::guard(|| {
            match (sub.repr(), sup.repr()) {
                (Repr::Mixed(MixedKind::Error, None), _)
                | (_, Repr::Mixed(MixedKind::Error, None)) => true,
                // No value to reject.
                (Repr::Never, _) => true,
                // A `key-of<T>`/`T[K]` still over a template is what `T`'s
                // bound makes it.
                (Repr::KeyOf(_) | Repr::OffsetAccess(..), _) => {
                    self.accepts_type(&sub.resolve_late_to_bounds(), sup)
                }
                (_, Repr::KeyOf(_) | Repr::OffsetAccess(..)) => {
                    self.accepts_type(sub, &sup.resolve_late_to_bounds())
                }
                // Two payloads ask the extension; anything else reads a
                // payload as its base type.
                (Repr::Extension(a), Repr::Extension(b)) => {
                    match b.is_super_type_of(&**a, self.hierarchy) {
                        Some(result) => result.yes(),
                        None => self.accepts_type(&a.base(), &b.base()),
                    }
                }
                (Repr::Extension(a), _) => self.accepts_type(&a.base(), sup),
                (_, Repr::Extension(b)) => self.accepts_type(sub, &b.base()),
                // The native `object` type hint is satisfied by *any* class
                // instance - an extremely common pattern, e.g. Doctrine's
                // `EntityRepository<T of object>` - and satisfies no
                // particular known class.
                (Repr::Object(_, _), Repr::ObjectWithoutClass) => true,
                (Repr::ObjectWithoutClass, Repr::Object(b, _)) => {
                    self.hierarchy.find_class(b).is_none()
                }
                // A value is callable as PHP's `is_callable()` says: an
                // object with `__invoke` (whose signature must fit a shaped
                // callable), a string naming a function or a static
                // `Class::method`, or `[object|class-string, method]`. What
                // the type doesn't settle is accepted.
                (Repr::Object(class, _), Repr::Callable(sig)) => {
                    self.object_is_callable(class, sig.as_deref())
                }
                (
                    Repr::String
                    | Repr::StringLiteral(_)
                    | Repr::ClassString(_)
                    | Repr::ClassKindString(_),
                    Repr::Callable(_),
                ) => self.string_is_callable(sub),
                (Repr::Array(..) | Repr::ArrayShape(_), Repr::Callable(_)) => {
                    self.array_is_callable(sub)
                }
                // Type arguments are compared with real variance
                // (`Repr::TemplateParam::variance`, `@template-covariant`/
                // `-contravariant`, and call-site `covariant`/`contravariant`/
                // `*` annotations, `crate::parse_generic_arg`), not just
                // skipped (`Collection<Cat>` and `Collection<Dog>` are not
                // mutually assignable). If `sup`'s own class isn't registered
                // at all (an uninstalled dependency, or a PHAR-bundled one
                // like `phpstan/phpstan` itself with no loose source to
                // parse), falls back to a plain name/hierarchy-only
                // comparison - the one deliberate leniency left in this
                // design.
                (Repr::Object(a, args_a), Repr::Object(b, args_b)) => {
                    if !a.eq_ignore_case(b) && !self.hierarchy.is_subclass_of(a, b) {
                        return false;
                    }
                    let Some(info_b) = self.hierarchy.find_class(b) else {
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
                    // (`accepts(bound, concrete)` fails whenever
                    // `concrete` is a strict subtype of `bound`) - a real,
                    // dangerous new false-positive class this project doesn't
                    // have data to avoid anyway. Plain `Mixed` is what
                    // already skips this slot's comparison entirely via the
                    // "either side Mixed" rule below - correctly lenient for
                    // the common case without guessing wrong.
                    // `args_a` is in `a`'s own frame - map it into `b`'s own
                    // template names via the same ancestor-substitution walk
                    // `extends_args`/`implements_args` already use elsewhere,
                    // unless `a == b` and it's already in the right frame.
                    let effective_args_a: Vec<Type> = if a.eq_ignore_case(b) {
                        args_a.clone()
                    } else {
                        let bindings = self.hierarchy.ancestor_template_bindings(a, args_a, b);
                        info_b
                            .templates
                            .iter()
                            .map(|template| {
                                bindings
                                    .get(template.name.as_str())
                                    .cloned()
                                    .unwrap_or(Type::mixed())
                            })
                            .collect()
                    };
                    for (i, _name) in info_b.templates.iter().enumerate() {
                        let a_i = effective_args_a.get(i).cloned().unwrap_or(Type::mixed());
                        let b_i = args_b.get(i).cloned().unwrap_or(Type::mixed());
                        // A call-site variance marker on `sup`'s own argument
                        // (`Collection<covariant Foo>`) overrides `b`'s own
                        // declared slot variance for this one comparison,
                        // exactly matching `GenericObjectType::isSuperTypeOf`/
                        // `getReferencedTemplateTypes` read directly from
                        // `vendor/phpstan-src` - `None` here means a `*`
                        // annotation, real PHPStan's own "don't check this
                        // argument at all."
                        let Some(variance) =
                            crate::effective_slot_variance(self.hierarchy, b, i, &b_i)
                        else {
                            continue;
                        };
                        let a_i = a_i
                            .without_call_site_variance()
                            .cloned()
                            .unwrap_or(Type::mixed());
                        let b_i = b_i
                            .without_call_site_variance()
                            .cloned()
                            .unwrap_or(Type::mixed());
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
                        if a_i == Type::mixed()
                            || b_i == Type::mixed()
                            || matches!(a_i.repr(), Repr::TemplateParam(_))
                            || matches!(b_i.repr(), Repr::TemplateParam(_))
                        {
                            continue;
                        }
                        let ok = match variance {
                            Variance::Covariant => self.accepts_type(&a_i, &b_i),
                            Variance::Contravariant => self.accepts_type(&b_i, &a_i),
                            // Mutual assignability approximates real
                            // PHPStan's exact `equals()` here - deliberately
                            // chosen over raw structural `==`, since this
                            // codebase's own `Union`/`Intersection` values
                            // have no canonical member order
                            // (`checker::expr::union_types` dedupes but never
                            // sorts), making `==` order-sensitive and *more*
                            // likely to falsely reject two independently
                            // built but semantically identical unions than
                            // mutual `accepts` is to miss a genuine
                            // mismatch.
                            Variance::Invariant => {
                                self.accepts_type(&a_i, &b_i) && self.accepts_type(&b_i, &a_i)
                            }
                        };
                        if !ok {
                            return false;
                        }
                    }
                    true
                }
                // A case is an instance of its enum and of everything the
                // enum implements: `UnitEnum`, `BackedEnum` and its declared
                // interfaces.
                (Repr::EnumCase(fqn, _), Repr::Object(b, _)) => {
                    fqn == b || self.hierarchy.is_subclass_of(fqn, b)
                }
                // A case is only ever itself; no hierarchy to widen through.
                (Repr::EnumCase(class_a, case_a), Repr::EnumCase(class_b, case_b)) => {
                    class_a == class_b && case_a == case_b
                }
                // `class-string<A>` is accepted where `class-string<B>` is
                // when an `A` is where a `B` is (`Derived extends Base`).
                // `interface-string`/`trait-string`: real PHPStan draws no
                // distinction from bare `class-string` here, but this project
                // is allowed to check more strictly - `ClassReflection::kind`
                // already tracks exactly this.
                (Repr::ClassString(a), Repr::ClassKindString(kind)) => {
                    self.class_string_may_be_kind(a, *kind)
                }
                (Repr::ClassString(a), Repr::ClassString(b)) => self.accepts_type(a, b),
                // A template is accepted as its bound is, before the union
                // arms below take the bound apart (`TNodeType of
                // PreInc|PreDec` where a `Node` is expected, each member
                // extending `Node` through `Expr`).
                (Repr::TemplateParam(tp), _) => self.accepts_type(&tp.bound, sup),
                // An unresolved conditional, on either side, is its two
                // branches' union. Before the arms that take `sub` apart: a
                // `list<T>`'s members, checked one by one, aren't lists.
                (
                    Repr::Conditional {
                        if_true, if_false, ..
                    },
                    _,
                ) => self.accepts_type(
                    &Type::new(Repr::Union(vec![(*if_true).clone(), (*if_false).clone()])),
                    sup,
                ),
                (
                    _,
                    Repr::Conditional {
                        if_true, if_false, ..
                    },
                ) => self.accepts_type(
                    sub,
                    &Type::new(Repr::Union(vec![(*if_true).clone(), (*if_false).clone()])),
                ),
                (Repr::Union(members), _) => members.iter().all(|m| self.accepts_type(m, sup)),
                (_, Repr::Union(members) | Repr::BenevolentUnion(members)) => {
                    members.iter().any(|m| self.accepts_type(sub, m))
                }
                // Mirrors `Relation::is_super_type_of`'s own dedicated
                // Intersection-vs-Intersection rule, and for the same reason
                // - without it, this function's own generic `(Intersection,
                // _) => any(...)` rule below intercepts a same-shaped pair
                // (e.g. `list<string>` checked against a `list<string>`-
                // typed parameter) first and gets it wrong, since it only
                // asks whether one bare member of `sub` alone satisfies the
                // whole `sup` intersection, never whether `sub`'s own
                // combination does.
                (Repr::Intersection(_), Repr::Intersection(sup_members)) => {
                    sup_members.iter().all(|m| self.accepts_type(sub, m))
                }
                // A benevolent union given (`RuleLevelHelper::accepts()`) is
                // accepted if *any* member matches `sup`, not only when *all*
                // do - the behavioral difference a benevolent union exists
                // for.
                (Repr::BenevolentUnion(members), _) => {
                    members.iter().any(|m| self.accepts_type(m, sup))
                }
                // Each parameter/return comparison consults the class
                // hierarchy: `Closure(Animal): Dog` fits `callable(Dog):
                // Animal`, and `Event` fits a `T of object` template's bound.
                (Repr::Callable(Some(a)), Repr::Callable(Some(b))) => {
                    self.callable_sig_fits(a, b, &|x, y| self.accepts_type(x, y), true)
                }
                // A closure literal (`Type::closure_literal`) checked against a
                // shaped `callable(...)`/`Closure(...)`: only its own
                // signature decides. The generic "any member satisfies" rule
                // just below would otherwise accept it through the `Closure`
                // object member alone (`Closure` declares `__invoke`, which
                // satisfies every callable shape), skipping the structural
                // comparison entirely.
                (Repr::Intersection(members), Repr::Callable(Some(_)))
                    if members
                        .iter()
                        .any(|m| matches!(m.repr(), Repr::Callable(Some(_)))) =>
                {
                    members
                        .iter()
                        .filter(|m| matches!(m.repr(), Repr::Callable(Some(_))))
                        .any(|m| self.accepts_type(m, sup))
                }
                (Repr::Intersection(members), _) => {
                    members.iter().any(|m| self.accepts_type(m, sup))
                }
                (_, Repr::Intersection(members)) => {
                    members.iter().all(|m| self.accepts_type(sub, m))
                }
                // Deliberately more lenient here than Relation::is_super_type_of's pure
                // structural relation (which correctly treats Array(Mixed) as
                // NOT a subtype of Array(Int)): an array literal's element type
                // is never inferred precisely by this checker, so it always
                // shows up as Array(Mixed) meaning "unknown", not "really can
                // hold anything" - without this, e.g. a PHPDoc `int[]` param
                // would false-positive against every `[1, 2, 3]` literal.
                // `strict_mixed_arrays` opts out of this leniency, matching
                // real PHPStan's own level-9-style "be strict about values
                // with mixed type" philosophy.
                (Repr::Array(_, a), Repr::Array(_, b)) => {
                    (!self.strict_mixed_arrays && matches!(a.repr(), Repr::Mixed(..)))
                        || self.accepts_type(a, b)
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
                // *genuinely narrower, known* key type (a real `Repr::Int`/
                // `Repr::String`) is held to the real subtyping check.
                (Repr::Array(key, a), Repr::ArrayShape(shape)) => {
                    !self.strict_mixed_arrays
                        && matches!(a.repr(), Repr::Mixed(..))
                        && (*key == Type::unknown_array_key()
                            || self.accepts_type(key, &shape.key_type()))
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
                (Repr::ArrayShape(shape), Repr::Array(key, b)) => {
                    self.accepts_type(&shape.key_type(), key)
                        && shape.fields().iter().all(|f| {
                            (!self.strict_mixed_arrays && matches!(f.ty.repr(), Repr::Mixed(..)))
                                || self.accepts_type(&f.ty, b)
                        })
                }
                // Same completion for shape-to-shape: a literal's own shape
                // satisfies a declared shape when every field the declared
                // shape actually requires is present (an optional field may
                // be missing) with an assignable type - extra fields the
                // literal has beyond what's required are ignored, the same
                // "structural, not exact" spirit `Array(Mixed)`'s own
                // leniency already uses elsewhere in this match. Either
                // side's unsealed tail takes no part.
                (Repr::ArrayShape(sub), Repr::ArrayShape(sup)) => {
                    self.shape_accepts_fields(sub, sup)
                        // A key `sup` doesn't list must fit its unsealed
                        // tail; a sealed `sup` has none.
                        && sub
                            .fields()
                            .iter()
                            .filter(|f| sup.field(&f.key).is_none())
                            .all(|f| {
                                sup.unsealed.as_deref().is_some_and(|(key, value)| {
                                    self.accepts_type(&f.key.to_type(), key)
                                        && self.accepts_type(&f.ty, value)
                                })
                            })
                        && match (sub.unsealed.as_deref(), sup.unsealed.as_deref()) {
                            (None, _) => true,
                            (Some(_), None) => false,
                            (Some((sub_key, sub_value)), Some((key, value))) => {
                                self.accepts_type(sub_key, key) && self.accepts_type(sub_value, value)
                            }
                        }
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
                (Repr::ObjectWithoutClass, Repr::ObjectShape(_)) => true,
                // A class the registry never saw can't be checked either way.
                (Repr::Object(class, _), Repr::HasMethod(method)) => {
                    self.hierarchy.find_class(class).is_none()
                        || self.hierarchy.has_method(class, method)
                }
                (Repr::Object(class, _), Repr::HasProperty(property)) => {
                    self.hierarchy.find_class(class).is_none()
                        || self.hierarchy.property_type(class, property).is_some()
                }
                (Repr::ObjectShape(shape), Repr::HasProperty(property)) => shape
                    .field(&ShapeKey::from_name(property))
                    .is_some_and(|f| !f.optional),
                (Repr::Object(class, _), Repr::ObjectShape(shape)) => {
                    class == "stdClass"
                        || shape.fields().iter().all(|f| {
                            match self.hierarchy.property_type(class, &f.key.to_string()) {
                                Some(property_ty) => self.accepts_type(&property_ty, &f.ty),
                                None => f.optional,
                            }
                        })
                }
                // An object may have more properties than a shape lists.
                (Repr::ObjectShape(sub), Repr::ObjectShape(sup)) => {
                    self.shape_accepts_fields(sub, sup)
                }
                // A class the registry doesn't know may be any class: a value
                // that may be an object may be one; a scalar or an array
                // never is.
                (_, Repr::Object(name, _)) if self.hierarchy.find_class(name).is_none() => {
                    !sub.is_object().no()
                }
                // An unresolved `value-of<T>`/`Foo::BAR` is just as unknown.
                (_, Repr::ValueOf(_) | Repr::ClassConstant { .. }) => true,
                _ => self.is_super_type_of(sup, sub).yes() || accepted_though_no_subtype(sub, sup),
            }
        })
    }

    /// The level-aware counterpart to `accepts` above - real
    /// PHPStan's `RuleLevelHelper::accepts()` two genuinely level-gated
    /// behaviors (`checkNullables`/`checkUnionTypes`) applied to the
    /// identical argument/return/property-type check `accepts`
    /// already performs, for the handful of real call sites a `Rule`
    /// class actually consumes `RuleLevelHelper` through (not every
    /// `accepts` call site project-wide - narrowing, LSP-override
    /// compatibility, and generics bound-checking are real PHPStan's own
    /// separate, never-level-gated `isSuperTypeOf()` domain, and must
    /// keep calling `accepts` directly).
    ///
    /// `self.configured_level` is the project's own `level` setting
    /// *before* `check-project`'s own `MAX_LEVEL`-driven `.unwrap_or(6)`
    /// collapse - `None` (never configured) keeps today's always-strict
    /// answer exactly, the safe default for every existing caller/test
    /// that never opts into this.
    ///
    /// Below `checkUnionTypes`' level a union given is accepted when some
    /// member is (PHPStan accepts its `Maybe`); from it on, every member
    /// must be.
    fn rule_level_accepts_type(&self, sub: &Type, sup: &Type) -> bool {
        if matches!(sub.repr(), Repr::Mixed(MixedKind::Error, None))
            || matches!(sup.repr(), Repr::Mixed(MixedKind::Error, None))
        {
            return true;
        }
        // Never configured means "assume the strictest threshold for
        // `checkNullables`/`checkUnionTypes`" (`NULLABLES_CHECK_LEVEL`,
        // matching this method's own pre-existing behavior exactly - the
        // safe default for every caller/test that never opts in) - but
        // NOT the strictest threshold for the newer `checkExplicitMixed`/
        // `checkImplicitMixed` (9/10, strictly above 8): those are real
        // PHPStan's own always-off-unless-opted-in checks, so an
        // unconfigured level must keep accepting `Mixed` unconditionally,
        // exactly like every other caller already did before either
        // existed. `NULLABLES_CHECK_LEVEL` sits below both, so passing it
        // through `rule_level_accepts` gets both properties
        // at once rather than needing its own separate branch.
        let level = self.configured_level.unwrap_or(NULLABLES_CHECK_LEVEL);
        self.rule_level_accepts_rec(sub, sup, level)
    }

    fn rule_level_accepts_rec(&self, sub: &Type, sup: &Type, level: u8) -> bool {
        stack::guard(|| {
            // Real PHPStan's own `checkBenevolentUnionTypes` (`RuleLevelHelper::
            // transformAcceptedType`) runs first, unconditionally, independent
            // of level - mirrored here ahead of both fast paths below, which
            // would otherwise fall straight through to `accepts_at_level`
            // (and from there, `accepts`'s own always-lenient Benevolent
            // arm) without ever seeing this override. Strips the marker and
            // demands *every* member of the wrapped `Union` satisfy `sup`,
            // recursing back into this same method (not plain `accepts`)
            // so the rest of this method's own level-aware behavior (`checkNullables`/
            // `checkUnionTypes`/`checkExplicitMixed`/`checkImplicitMixed`) still
            // applies per-member exactly as it would for an ordinary Union.
            if self.check_benevolent_union_types
                && let Repr::BenevolentUnion(wrapped) = sub.repr()
            {
                return wrapped
                    .iter()
                    .all(|m| self.rule_level_accepts_rec(m, sup, level));
            }

            // Fast path: both real gates already at their strict threshold -
            // byte-identical to `accepts_at_level` (which still applies
            // `checkExplicitMixed`/`checkImplicitMixed`, real PHPStan's own
            // level 9/10 - unlike `checkNullables`/`checkUnionTypes`, neither
            // is subsumed by this fast path, since 9/10 sit *above* 8), the
            // safe default for a project explicitly configured at (or above)
            // real PHPStan's own strictest level for *this* behavior.
            // `NULLABLES_CHECK_LEVEL` (8) is strictly above
            // `UNION_TYPES_CHECK_LEVEL` (7), so `level >= NULLABLES_CHECK_LEVEL`
            // alone already implies the other - both consts are still named
            // here (rather than one bare `8`) so this reads as "both real
            // gates," matching real PHPStan's own two separately-named flags.
            if level >= NULLABLES_CHECK_LEVEL {
                debug_assert!(level >= UNION_TYPES_CHECK_LEVEL);
                return self.accepts_at_level(sub, sup, level);
            }
            // Fast path: neither side is a `Union` at all - nothing for
            // either level-gated behavior to touch, so this collapses to
            // `accepts_at_level` too (the `Union` arms below are the
            // only ones this method ever needs to diverge from plain
            // assignability on).
            if !matches!(sub.repr(), Repr::Union(_)) && !matches!(sup.repr(), Repr::Union(_)) {
                return self.accepts_at_level(sub, sup, level);
            }

            // `checkNullables` (real PHPStan's own `transformAcceptedType`,
            // `RuleLevelHelper.php:130-137`): below level 8, strip `Null` out
            // of a multi-member `sub` ("acceptedType" in real PHPStan's own
            // naming - the *actual/given* type, e.g. a `?string`-typed
            // argument) Union before deciding acceptance - real PHPStan's own
            // guard, applied exactly as it is there: never when `sup`
            // ("acceptingType" - the *declared/expected* type) is bare
            // `Repr::Null` (a nullable value must never satisfy a definite-
            // null-only target), and never when `sub` is bare `Repr::Null`
            // itself (a definite null literal stays definitely rejected at
            // every level, `sub` must be a multi-member Union that *includes*
            // Null, not Null alone).
            let stripped_sub = if level < NULLABLES_CHECK_LEVEL
                && *sup != Type::null()
                && *sub != Type::null()
                && let Repr::Union(members) = sub.repr()
                && members.contains(&Type::null())
            {
                let non_null: Vec<Type> = members
                    .iter()
                    .filter(|m| **m != Type::null())
                    .cloned()
                    .collect();
                Some(match non_null.len() {
                    // Unreachable in practice (`sub` was excluded above
                    // whenever it's bare `Repr::Null`, so a real Union always
                    // has at least one non-Null member) - kept as a safe,
                    // total fallback rather than an `unwrap`/panic.
                    0 => Type::never(),
                    1 => non_null.into_iter().next().unwrap(),
                    _ => Type::new(Repr::Union(non_null)),
                })
            } else {
                None
            };
            let sub = stripped_sub.as_ref().unwrap_or(sub);

            // Both sides recurse here rather than through plain `accepts`, so
            // the level-gated leniencies reach every member: a nullable
            // declared type (`?string`) is itself a union.
            match (sub.repr(), sup.repr()) {
                (Repr::Union(members), _) if level < UNION_TYPES_CHECK_LEVEL => members
                    .iter()
                    .any(|m| self.rule_level_accepts_rec(m, sup, level)),
                (Repr::Union(members), _) => members
                    .iter()
                    .all(|m| self.rule_level_accepts_rec(m, sup, level)),
                (_, Repr::Union(members)) => members
                    .iter()
                    .any(|m| self.rule_level_accepts_rec(sub, m, level)),
                _ => self.accepts_at_level(sub, sup, level),
            }
        })
    }

    /// The level at which real PHPStan's `RuleLevelHelper::
    /// findTypeToCheck` turns `ty` into a `StrictMixedType` receiver -
    /// one that can't have a method called, a property read, or an
    /// offset taken on it (`Cannot call method foo() on mixed.`) - or
    /// `None` when `ty` isn't `Mixed` or the configured level is below
    /// that threshold. An unconfigured level never triggers it, the same
    /// "`checkExplicitMixed`/`checkImplicitMixed` stay off unless opted
    /// into" default `rule_level_accepts` documents.
    pub fn strict_mixed_receiver_level(&self, ty: &Type) -> Option<u8> {
        let Repr::Mixed(kind, _) = ty.repr() else {
            return None;
        };
        let threshold = match kind {
            MixedKind::Explicit => EXPLICIT_MIXED_CHECK_LEVEL,
            MixedKind::Implicit => IMPLICIT_MIXED_CHECK_LEVEL,
            MixedKind::Error => return None,
        };
        (self.configured_level? >= threshold).then_some(threshold)
    }

    /// `accepts` at rule level `level`, with PHPStan's level 9/10
    /// `checkExplicitMixed`/`checkImplicitMixed`: below its level a
    /// `mixed` value (or a template bounded by nothing) is accepted
    /// anywhere; at or past it, only where `mixed` itself is
    /// (`StrictMixedType::isAcceptedBy`).
    fn accepts_at_level(&self, sub: &Type, sup: &Type, level: u8) -> bool {
        // An error is accepted and accepts everything at every level.
        if sub.is_error() || sup.is_error() {
            return true;
        }
        // A template bounded by nothing is as unknown as an explicit
        // `mixed` (PHPStan's `TemplateMixedType`).
        if let Repr::TemplateParam(tp) = sub.repr()
            && matches!((*tp.bound).repr(), Repr::Mixed(..))
            && level < EXPLICIT_MIXED_CHECK_LEVEL
        {
            return true;
        }
        if let Repr::Mixed(kind, _) = sub.repr() {
            let strict_level = if *kind == MixedKind::Explicit {
                EXPLICIT_MIXED_CHECK_LEVEL
            } else {
                IMPLICIT_MIXED_CHECK_LEVEL
            };
            if level < strict_level {
                return true;
            }
            return matches!(sup.repr(), Repr::Mixed(..));
        }
        self.accepts_type(sub, sup)
    }

    /// Every field `sup` lists is in `sub` - present for sure where
    /// `sup` requires it - with a type `sup` accepts.
    fn shape_accepts_fields(&self, sub: &Shape, sup: &Shape) -> bool {
        stack::guard(|| {
            sup.fields().iter().all(|f| match sub.field(&f.key) {
                Some(sub_field) => {
                    (f.optional || !sub_field.optional) && self.accepts_type(&sub_field.ty, &f.ty)
                }
                None => f.optional,
            })
        })
    }

    /// An instance of `class` is callable when it has `__invoke`, whose
    /// signature must fit `sig`; an unknown class may be.
    fn object_is_callable(&self, class: &str, sig: Option<&CallableSig>) -> bool {
        stack::guard(|| {
            if self.hierarchy.find_class(class).is_none() {
                return true;
            }
            match (self.hierarchy.method_signature(class, "__invoke"), sig) {
                (None, _) => false,
                (Some(_), None) => true,
                (Some(invoke), Some(sig)) => {
                    self.callable_sig_fits(&invoke, sig, &|x, y| self.accepts_type(x, y), true)
                }
            }
        })
    }

    /// A string is callable when it names a function or a static
    /// `Class::method`; one not known literally may be.
    fn string_is_callable(&self, sub: &Type) -> bool {
        let Repr::StringLiteral(text) = sub.repr() else {
            return true;
        };
        let text = text.to_str_lossy();
        let text = text.trim_start_matches('\\');
        match text.split_once("::") {
            Some((class, method)) => self.static_method_may_be_callable(class, method),
            None => !self.hierarchy.function_exists(text).no(),
        }
    }

    /// `[object|class-string, method]` is callable when the method is
    /// there (static for a class name); an array not known to be such a
    /// pair may be one if its elements may be objects or strings.
    fn array_is_callable(&self, sub: &Type) -> bool {
        match sub.repr() {
            Repr::ArrayShape(shape) => {
                let (Some(target), Some(method)) = (
                    shape.field(&ShapeKey::Int(0)),
                    shape.field(&ShapeKey::Int(1)),
                ) else {
                    return false;
                };
                if shape.is_sealed() && shape.fields().len() != 2 {
                    return false;
                }
                if method.ty.is_string().no()
                    || (target.ty.is_object().no() && target.ty.is_string().no())
                {
                    return false;
                }
                let Repr::StringLiteral(name) = method.ty.repr() else {
                    return true;
                };
                let name = name.to_str_lossy();
                match target.ty.repr() {
                    Repr::Object(class, _) => {
                        self.hierarchy.find_class(class).is_none()
                            || self.hierarchy.has_method(class, &name)
                            || self.hierarchy.has_method(class, "__call")
                    }
                    Repr::ClassString(inner) => match inner.repr() {
                        Repr::Object(class, _) => self.static_method_may_be_callable(class, &name),
                        _ => true,
                    },
                    Repr::StringLiteral(class) => {
                        self.static_method_may_be_callable(&class.to_str_lossy(), &name)
                    }
                    _ => true,
                }
            }
            Repr::Array(key, value) => {
                !key.is_int().no() && !(value.is_object().no() && value.is_string().no())
            }
            _ => true,
        }
    }

    /// Whether `class::method` may be called statically: an unknown or
    /// relative class may; a known one needs the method static, or
    /// `__callStatic`.
    fn static_method_may_be_callable(&self, class: &str, method: &str) -> bool {
        let class = class.trim_start_matches('\\');
        if ["self", "parent", "static"]
            .iter()
            .any(|r| class.eq_ignore_ascii_case(r))
            || self.hierarchy.find_class(class).is_none()
        {
            return true;
        }
        (self.hierarchy.has_method(class, method) && self.hierarchy.is_static_method(class, method))
            || self.hierarchy.has_method(class, "__callStatic")
    }

    /// Whether a `class-string<inner>` may name a class-like of `kind`:
    /// bare `class-string` takes any; an interface or trait needs `inner`
    /// to be one (a class's name, or a subclass's, never is), each
    /// member of a union alike. What `inner` doesn't pin down may.
    fn class_string_may_be_kind(&self, inner: &Type, kind: ClassKind) -> bool {
        stack::guard(|| {
            if kind == ClassKind::Class {
                return true;
            }
            match inner.repr() {
                Repr::Object(name, _) => self
                    .hierarchy
                    .find_class(name)
                    .is_none_or(|class| class.kind == kind),
                Repr::Union(members) | Repr::BenevolentUnion(members) => members
                    .iter()
                    .all(|m| self.class_string_may_be_kind(m, kind)),
                Repr::Intersection(members) => members
                    .iter()
                    .any(|m| self.class_string_may_be_kind(m, kind)),
                Repr::TemplateParam(tp) => self.class_string_may_be_kind(&tp.bound, kind),
                _ => true,
            }
        })
    }

    /// Real PHPStan's own `InitializerExprTypeResolver::
    /// resolveIdenticalType` - what `left === right` provably evaluates
    /// to from both operands' own types alone, `None` when it could go
    /// either way. Two independent halves, mirroring that real algorithm
    /// exactly: (1) both operands already reduce to the exact same kind
    /// of single finite value (a literal, an enum case, `null`, `true`/
    /// `false`, or an empty array) - direct value equality decides both
    /// directions at once (`Type`'s own `PartialEq`, deliberately
    /// including `FloatLiteral`: comparing two floats via `==` already
    /// gives the correct, real-PHP-matching `NAN !== NAN` answer for
    /// free, since IEEE-754 float `==` is exactly what PHP's own runtime
    /// `===` uses). (2) otherwise, `types_provably_disjoint` decides only
    /// the "always false" direction - a type-family/unrelated-closed-
    /// class mismatch proves the two operands can never be equal, but
    /// proves nothing about "always true" (two different, non-finite
    /// `int`-typed expressions could coincidentally hold the same
    /// runtime value). Shared by `checker::check_strict_comparison_of_
    /// different_types` (`===`/`!==` as a real comparison expression) and
    /// `checker::expr`'s own `match.unhandled`-adjacent `match(true)`/
    /// per-arm exhaustiveness reasoning (`MatchExpressionRule.php`'s own
    /// `$armConditionExpr = new Identical($matchCondition, $rawCondition)`
    /// reaches this exact same question, for exactly the same reason).
    pub fn identical_result(&self, left: &Type, right: &Type) -> Option<bool> {
        if is_finite_scalar_type(left) && is_finite_scalar_type(right) {
            return Some(left == right);
        }
        if self.disjoint(left, right) {
            return Some(false);
        }
        None
    }
}

/// A value `accepts` takes although it isn't provably a `sup`: an `int`
/// where a `float` is declared (PHP widens it), a plain string where a
/// class-string is (it may name one), a bare callable where a shaped one
/// is, a `self`/`static` not yet bound where a class is, and anything
/// where a template not yet bound is.
fn accepted_though_no_subtype(sub: &Type, sup: &Type) -> bool {
    matches!(
        (sub.repr(), sup.repr()),
        (
            Repr::Int | Repr::IntLiteral(_) | Repr::IntRange(..),
            Repr::Float
        ) | (
            Repr::String | Repr::StringLiteral(_),
            Repr::ClassString(_) | Repr::ClassKindString(_)
        ) | (Repr::Callable(None), Repr::Callable(Some(_)))
            | (
                Repr::SelfRef(_) | Repr::ParentRef | Repr::StaticRef(_) | Repr::This,
                Repr::Object(..) | Repr::ObjectWithoutClass
            )
            | (_, Repr::TemplateParam(_))
    )
}

/// `identical_result`'s own "already the one exact value it'll ever be"
/// leaf test - deliberately broader than `narrowing::is_known_single_
/// value` (`checker/narrowing/type_ops.rs`), which excludes
/// `FloatLiteral` for a *different* reason specific to its own
/// narrowing-exclusion purpose (excluding a float literal from a
/// `Union` on a false `!==` branch would wrongly also exclude a real
/// `NAN`, which never satisfies `!==` against anything, including
/// itself) - that concern doesn't apply here at all, since this
/// function only ever feeds a direct `Type::eq` comparison between two
/// *independently* inferred operands.
fn is_finite_scalar_type(ty: &Type) -> bool {
    matches!(
        ty.repr(),
        Repr::Null
            | Repr::True
            | Repr::False
            | Repr::IntLiteral(_)
            | Repr::StringLiteral(_)
            | Repr::FloatLiteral(_)
            | Repr::EnumCase(_, _)
    ) || matches!(ty.repr(), Repr::ArrayShape(shape) if shape.is_empty_array())
}
