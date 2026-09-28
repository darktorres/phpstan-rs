use crate::Repr;
use crate::relation::{AcceptsResult, Relation};
use crate::{ShapeKey, Type, Variance};

// Real PHPStan's own level-gated `RuleLevelHelper` behaviors
// (`rule_level_accepts`/`rule_level_accepts`/
// `accepts_at_level`, below) - named here, at module scope, so
// `rule_level_accepts`'s own unconfigured-level fallback can
// reference `NULLABLES_CHECK_LEVEL` directly rather than duplicating the
// literal `8`.
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
        match (sub, sup) {
            (Type(Repr::Error), _) | (_, Type(Repr::Error)) => true,
            // A `key-of<T>`/`T[K]` still over a template is what `T`'s
            // bound makes it.
            (Type(Repr::KeyOf(_)) | Type(Repr::OffsetAccess(..)), _) => {
                self.accepts_type(&crate::resolve_late_to_bounds(sub), sup)
            }
            (_, Type(Repr::KeyOf(_)) | Type(Repr::OffsetAccess(..))) => {
                self.accepts_type(sub, &crate::resolve_late_to_bounds(sup))
            }
            // Two payloads ask the extension; anything else reads a
            // payload as its base type.
            (Type(Repr::Extension(a)), Type(Repr::Extension(b))) => {
                match b.is_super_type_of(&**a, self.hierarchy) {
                    Some(result) => result.yes(),
                    None => self.accepts_type(&a.base(), &b.base()),
                }
            }
            (Type(Repr::Extension(a)), _) => self.accepts_type(&a.base(), sup),
            (_, Type(Repr::Extension(b))) => self.accepts_type(sub, &b.base()),
            // The native `object` type hint is satisfied by *any* class
            // instance - an extremely common pattern, e.g. Doctrine's
            // `EntityRepository<T of object>` - and satisfies no
            // particular class.
            (Type(Repr::Object(_, _)), Type(Repr::ObjectWithoutClass)) => true,
            (Type(Repr::ObjectWithoutClass), Type(Repr::Object(_, _))) => false,
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
            (Type(Repr::Object(class, _)), Type(Repr::Callable(_))) => {
                self.hierarchy.has_method(class, "__invoke")
            }
            (Type(Repr::String) | Type(Repr::StringLiteral(_)), Type(Repr::Callable(_))) => true,
            (Type(Repr::Array(_, _)) | Type(Repr::ArrayShape(_)), Type(Repr::Callable(_))) => true,
            // Type arguments are compared with real variance
            // (`Type::TemplateParam::variance`, `@template-covariant`/
            // `-contravariant`, and call-site `covariant`/`contravariant`/
            // `*` annotations, `crate::parse_generic_arg`), not just
            // skipped (`Collection<Cat>` and `Collection<Dog>` are not
            // mutually assignable). If `sup`'s own class isn't registered
            // at all (an uninstalled dependency, or a PHAR-bundled one
            // like `phpstan/phpstan` itself with no loose source to
            // parse), falls back to a plain name/hierarchy-only
            // comparison - the one deliberate leniency left in this
            // design.
            (Type(Repr::Object(a, args_a)), Type(Repr::Object(b, args_b))) => {
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
                // have data to avoid anyway: `ClassReflection` only persists
                // `template_bounds`, never a class-level `@template T =
                // Default` map the way `TemplateParam.default` carries
                // one for a function/method's own templates (`crate::
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
                    let bindings = self.hierarchy.ancestor_template_bindings(a, args_a, b);
                    info_b
                        .templates
                        .iter()
                        .map(|name| {
                            bindings
                                .get(name.as_ref())
                                .cloned()
                                .unwrap_or(Type(Repr::Mixed(false)))
                        })
                        .collect()
                };
                for (i, _name) in info_b.templates.iter().enumerate() {
                    let a_i = effective_args_a
                        .get(i)
                        .cloned()
                        .unwrap_or(Type(Repr::Mixed(false)));
                    let b_i = args_b.get(i).cloned().unwrap_or(Type(Repr::Mixed(false)));
                    // A call-site variance marker on `sup`'s own argument
                    // (`Collection<covariant Foo>`) overrides `b`'s own
                    // declared slot variance for this one comparison,
                    // exactly matching `GenericObjectType::isSuperTypeOf`/
                    // `getReferencedTemplateTypes` read directly from
                    // `vendor/phpstan-src` - `None` here means a `*`
                    // annotation, real PHPStan's own "don't check this
                    // argument at all."
                    let Some(variance) = crate::effective_slot_variance(self.hierarchy, b, i, &b_i)
                    else {
                        continue;
                    };
                    let a_i = a_i
                        .without_call_site_variance()
                        .cloned()
                        .unwrap_or(Type(Repr::Mixed(false)));
                    let b_i = b_i
                        .without_call_site_variance()
                        .cloned()
                        .unwrap_or(Type(Repr::Mixed(false)));
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
                    if a_i == Type(Repr::Mixed(false))
                        || b_i == Type(Repr::Mixed(false))
                        || matches!(a_i, Type(Repr::TemplateParam(_)))
                        || matches!(b_i, Type(Repr::TemplateParam(_)))
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
            // Hierarchy-aware version of `Relation::is_super_type_of`'s own bare
            // `EnumCase -> Object` name-equality rule - a case
            // is assignable not just to its own enum's bare type but to
            // `UnitEnum`/`BackedEnum` (both synthesized `implements`
            // entries every enum gets, see `ClassReflection::from_enum_decl`)
            // and any interface the enum itself declares.
            (Type(Repr::EnumCase(fqn, _)), Type(Repr::Object(b, _))) => {
                fqn == b || self.hierarchy.is_subclass_of(fqn, b)
            }
            // A case is only ever itself; no hierarchy to widen through.
            (Type(Repr::EnumCase(class_a, case_a)), Type(Repr::EnumCase(class_b, case_b))) => {
                class_a == class_b && case_a == case_b
            }
            // Same hierarchy-aware override as the plain Object-vs-Object
            // arm just above, one level up: `Relation::is_super_type_of`'s own
            // `ClassString` arm inherits its non-hierarchy-aware Object
            // comparison (name equality only), so without this a
            // `class-string<Derived>` value would incorrectly fail to
            // satisfy a `class-string<Base>`-typed parameter even when
            // `Derived extends Base`.
            // `interface-string`/`trait-string`: real PHPStan draws no
            // distinction from bare `class-string` here, but this project
            // is allowed to check more strictly - `ClassReflection::kind`
            // already tracks exactly this.
            (Type(Repr::ClassString(a)), Type(Repr::ClassKindString(kind))) => {
                let Type(Repr::Object(name, _)) = &**a else {
                    return false;
                };
                self.hierarchy
                    .find_class(name)
                    .is_some_and(|class| class.kind == *kind)
            }
            (Type(Repr::ClassString(a)), Type(Repr::ClassString(b))) => self.accepts_type(a, b),
            // Mirrors `Relation::is_super_type_of`'s own bound-aware `TemplateParam`
            // arm (see its doc comment for the full reasoning and why
            // this needs to run *before* the `Union` arms just below),
            // but recurses through `self.accepts` rather than the
            // plain, hierarchy-*unaware* `Relation::is_super_type_of`: delegating
            // straight to `Relation::is_super_type_of(sub, sup)` would silently
            // drop the class-hierarchy-walking half of this whole method
            // for any bound check involving a `TemplateParam`, wrongly
            // flagging code where a template's own bound is a union of
            // concrete classes needing a real ancestor walk to match a
            // single-class target (e.g. `@phpstan-template TNodeType of
            // PreInc|PreDec|PostInc|PostDec` satisfying `Rule<TNodeType>`'s
            // own `TNodeType of Node` bound - each of those four classes
            // extends `Node` only via `Expr`, a multi-level ancestor
            // chain plain name-equality can't see).
            (Type(Repr::TemplateParam(tp)), _) => {
                matches!(*tp.bound, Type(Repr::Mixed(_))) || self.accepts_type(&tp.bound, sup)
            }
            // Mirrors `Relation::is_super_type_of`'s own identically-placed
            // `Type::Conditional` arm exactly (see that arm's own doc
            // comment) - a real, independent gap this project's own
            // `Type::Conditional`-returning-function corpus checks were
            // silently missing: without this, a `sub` that's itself an
            // `Intersection` (any `list<T>`/`non-empty-array<T>`-typed
            // value - an extremely common real return type) hits this
            // function's own generic `(Intersection, _) => any(...)` rule
            // below *first*, decomposing `sub`'s own members (`Array`,
            // a bare `Accessory(ArrayList)`, a bare `Accessory(
            // NonEmptyArray)`) and checking each *in isolation* against
            // the still-unflattened `Conditional` - none of which,
            // stripped from its own `Intersection` wrapper, actually
            // represents "a real list" any more, so a value that
            // genuinely satisfies one of the two conditional arms was
            // wrongly rejected. Placed before every arm that could
            // decompose `sub` first, exactly like `is_super_type_of`'s own
            // arm, so `sup`'s real shape is always resolved before
            // `sub`'s is.
            (
                Type(Repr::Conditional {
                    if_true, if_false, ..
                }),
                _,
            ) => self.accepts_type(
                &Type(Repr::Union(vec![(**if_true).clone(), (**if_false).clone()])),
                sup,
            ),
            (
                _,
                Type(Repr::Conditional {
                    if_true, if_false, ..
                }),
            ) => self.accepts_type(
                sub,
                &Type(Repr::Union(vec![(**if_true).clone(), (**if_false).clone()])),
            ),
            (Type(Repr::Union(members)), _) => members.iter().all(|m| self.accepts_type(m, sup)),
            (_, Type(Repr::Union(members)) | Type(Repr::BenevolentUnion(members))) => {
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
            (Type(Repr::Intersection(_)), Type(Repr::Intersection(sup_members))) => {
                sup_members.iter().all(|m| self.accepts_type(sub, m))
            }
            // A benevolent union given (`RuleLevelHelper::accepts()`) is
            // accepted if *any* member matches `sup`, not only when *all*
            // do - the behavioral difference a benevolent union exists
            // for.
            (Type(Repr::BenevolentUnion(members)), _) => {
                members.iter().any(|m| self.accepts_type(m, sup))
            }
            // Each parameter/return comparison consults the class
            // hierarchy: `Closure(Animal): Dog` fits `callable(Dog):
            // Animal`, and `Event` fits a `T of object` template's bound.
            (Type(Repr::Callable(Some(a))), Type(Repr::Callable(Some(b)))) => {
                crate::callable_sig_is_subtype(a, b, &|x, y| self.accepts_type(x, y))
            }
            // A closure literal (`Type::closure_literal`) checked against a
            // shaped `callable(...)`/`Closure(...)`: only its own
            // signature decides. The generic "any member satisfies" rule
            // just below would otherwise accept it through the `Closure`
            // object member alone (`Closure` declares `__invoke`, which
            // satisfies every callable shape), skipping the structural
            // comparison entirely.
            (Type(Repr::Intersection(members)), Type(Repr::Callable(Some(_))))
                if members
                    .iter()
                    .any(|m| matches!(m, Type(Repr::Callable(Some(_))))) =>
            {
                members
                    .iter()
                    .filter(|m| matches!(m, Type(Repr::Callable(Some(_)))))
                    .any(|m| self.accepts_type(m, sup))
            }
            (Type(Repr::Intersection(members)), _) => {
                members.iter().any(|m| self.accepts_type(m, sup))
            }
            (_, Type(Repr::Intersection(members))) => {
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
            (Type(Repr::Array(_, a)), Type(Repr::Array(_, b))) => {
                (!self.strict_mixed_arrays && matches!(**a, Type(Repr::Mixed(_))))
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
            // *genuinely narrower, known* key type (a real `Type::Int`/
            // `Type::String`) is held to the real subtyping check.
            (Type(Repr::Array(key, a)), Type(Repr::ArrayShape(shape))) => {
                !self.strict_mixed_arrays
                    && matches!(**a, Type(Repr::Mixed(_)))
                    && (**key == Type::unknown_array_key()
                        || self.accepts_type(key, &crate::shape_key_type(shape)))
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
            (Type(Repr::ArrayShape(shape)), Type(Repr::Array(key, b))) => {
                self.accepts_type(&crate::shape_key_type(shape), key)
                    && shape.fields.iter().all(|f| {
                        (!self.strict_mixed_arrays && matches!(f.ty, Type(Repr::Mixed(_))))
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
            (Type(Repr::ArrayShape(sub)), Type(Repr::ArrayShape(sup))) => {
                sup.fields.iter().all(|f| match sub.field(&f.key) {
                    Some(sub_field) => self.accepts_type(&sub_field.ty, &f.ty),
                    None => f.optional,
                })
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
            (Type(Repr::ObjectWithoutClass), Type(Repr::ObjectShape(_))) => true,
            // A class the registry never saw can't be checked either way.
            (Type(Repr::Object(class, _)), Type(Repr::HasMethod(method))) => {
                self.hierarchy.find_class(class).is_none()
                    || self.hierarchy.has_method(class, method)
            }
            (Type(Repr::Object(class, _)), Type(Repr::HasProperty(property))) => {
                self.hierarchy.find_class(class).is_none()
                    || self.hierarchy.property_type(class, property).is_some()
            }
            (Type(Repr::ObjectShape(shape)), Type(Repr::HasProperty(property))) => shape
                .field(&ShapeKey::from_name(property))
                .is_some_and(|f| !f.optional),
            (Type(Repr::Object(class, _)), Type(Repr::ObjectShape(shape))) => {
                class == "stdClass"
                    || shape.fields.iter().all(|f| {
                        match self.hierarchy.property_type(class, &f.key.to_string()) {
                            Some(property_ty) => self.accepts_type(&property_ty, &f.ty),
                            None => f.optional,
                        }
                    })
            }
            (Type(Repr::ObjectShape(sub)), Type(Repr::ObjectShape(sup))) => {
                sup.fields.iter().all(|f| match sub.field(&f.key) {
                    Some(sub_field) => self.accepts_type(&sub_field.ty, &f.ty),
                    None => f.optional,
                })
            }
            // Same unresolved-`@phpstan-type`-alias leniency as the
            // Object-vs-Object arm above, generalized to every other
            // `sub` shape: Doctrine DBAL's own `DriverManager::
            // getConnection(Params $params)` is called with a real array
            // *literal* (an `ArrayShape`) at nearly every real call site,
            // never an actual `Params`-named object - only the
            // Object-vs-Object arm's leniency would leave those failing
            // just the same way.
            (_, Type(Repr::Object(name, _))) if self.hierarchy.find_class(name).is_none() => true,
            // An unresolved `value-of<T>`/`Foo::BAR` is just as unknown.
            (_, Type(Repr::ValueOf(_)) | Type(Repr::ClassConstant { .. })) => true,
            _ => self.is_super_type_of(sup, sub).yes() || accepted_though_no_subtype(sub, sup),
        }
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
    /// Deliberately not a full tri-state (`Yes`/`No`/`Maybe`) rewrite of
    /// `accepts`'s own boolean API: real PHPStan's `checkUnionTypes`
    /// distinguishes "definitely accepts" from "maybe accepts" via
    /// `TrinaryLogic`, a distinction this project's plain boolean
    /// `.all()`/`.any()` combinators (already used, unconditionally, by
    /// `accepts` itself) have no way to represent without that
    /// larger rewrite - `UNION_TYPES_CHECK_LEVEL` is still named and
    /// folded into the fast-path guard below (both real gates must be at
    /// their strict threshold before this collapses to `accepts`
    /// unchanged), but doesn't get its own separate `.all()`-vs-`.any()`
    /// switch the way `NULLABLES_CHECK_LEVEL` gets its own null-stripping
    /// switch.
    fn rule_level_accepts_type(&self, sub: &Type, sup: &Type) -> bool {
        if matches!(sub, Type(Repr::Error)) || matches!(sup, Type(Repr::Error)) {
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
            && let Type(Repr::BenevolentUnion(wrapped)) = sub
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
        if !matches!(sub, Type(Repr::Union(_))) && !matches!(sup, Type(Repr::Union(_))) {
            return self.accepts_at_level(sub, sup, level);
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
            && *sup != Type(Repr::Null)
            && *sub != Type(Repr::Null)
            && let Type(Repr::Union(members)) = sub
            && members.contains(&Type(Repr::Null))
        {
            let non_null: Vec<Type> = members
                .iter()
                .filter(|m| **m != Type(Repr::Null))
                .cloned()
                .collect();
            Some(match non_null.len() {
                // Unreachable in practice (`sub` was excluded above
                // whenever it's bare `Type::Null`, so a real Union always
                // has at least one non-Null member) - kept as a safe,
                // total fallback rather than an `unwrap`/panic.
                0 => Type(Repr::Never),
                1 => non_null.into_iter().next().unwrap(),
                _ => Type(Repr::Union(non_null)),
            })
        } else {
            None
        };
        let sub = stripped_sub.as_ref().unwrap_or(sub);

        // A naive version of this method would bail straight to plain
        // `accepts` whenever `sup` was also a Union, on the theory
        // that a nested Union-vs-Union pair is rare - but this project
        // represents every nullable declared type (`?string`, `?int`,
        // `?Foo`, the single most common PHP nullability idiom) as
        // `Type::Union([X, Type::Null])` (`types/parse.rs`), so `sup` is
        // a Union at every one of this method's real call sites checking
        // a nullable-declared parameter/property/return type - exactly
        // the scenario `checkNullables` exists for. Recursing into
        // `self.rule_level_accepts` (not plain
        // `accepts`) on both sides whenever either is a Union,
        // symmetric to `accepts`'s own existing `282`/`283` arms,
        // is what keeps that leniency reaching its single most common
        // trigger.
        match (sub, sup) {
            (Type(Repr::Union(members)), _) => members
                .iter()
                .all(|m| self.rule_level_accepts_rec(m, sup, level)),
            (_, Type(Repr::Union(members))) => members
                .iter()
                .any(|m| self.rule_level_accepts_rec(sub, m, level)),
            _ => self.accepts_at_level(sub, sup, level),
        }
    }

    /// `accepts`, additionally aware of real PHPStan's own level
    /// 9/10 `checkExplicitMixed`/`checkImplicitMixed`.
    ///
    /// This project's `is_super_type_of`/`accepts` never actually gave
    /// `Mixed` a blanket "accepted anywhere" structural rule the way real
    /// PHPStan's own `MixedType::isAcceptedBy()` unconditionally does
    /// (confirmed directly: there is no `(Type::Mixed(_), _) => true` arm
    /// in `Relation::is_super_type_of`, only the mirror-image `(_, Type::Mixed(_))
    /// => true`, a different direction entirely) - every one of dozens of
    /// existing call sites instead bails out ad hoc, on its own, whenever
    /// its *own* actual type happens to be `Mixed`. Rather than widen
    /// `is_super_type_of` itself with a new unconditional arm (real, foundational
    /// surgery with unknown blast radius across every existing caller,
    /// not something this one opt-in feature should force), this method
    /// supplies that same "accepted anywhere below the matching strict
    /// level" leniency directly and only here - self-contained, so
    /// nothing outside a caller that opts into `accepts_at_configured_
    /// level` ever sees a behavior change. Once at or past the matching
    /// level, `sub` only survives when `sup` itself accepts `Mixed` too -
    /// mirrors `RuleLevelHelper::transformCommonType`/`accepts` wrapping
    /// a matching `MixedType` as `StrictMixedType` before the real
    /// `accepts()` call, whose own `isAcceptedBy()` (`vendor/phpstan-src/
    /// src/Type/StrictMixedType.php`) returns Yes only when the accepting
    /// type is itself `(Strict)MixedType`, Maybe otherwise - collapsed to
    /// a plain `false` here, this method's own boolean-only API (see
    /// `rule_level_accepts`'s own doc comment on why a full
    /// trinary rewrite isn't attempted).
    /// The level at which real PHPStan's `RuleLevelHelper::
    /// findTypeToCheck` turns `ty` into a `StrictMixedType` receiver -
    /// one that can't have a method called, a property read, or an
    /// offset taken on it (`Cannot call method foo() on mixed.`) - or
    /// `None` when `ty` isn't `Mixed` or the configured level is below
    /// that threshold. An unconfigured level never triggers it, the same
    /// "`checkExplicitMixed`/`checkImplicitMixed` stay off unless opted
    /// into" default `rule_level_accepts` documents.
    pub fn strict_mixed_receiver_level(&self, ty: &Type) -> Option<u8> {
        let Type(Repr::Mixed(is_explicit)) = ty else {
            return None;
        };
        let threshold = if *is_explicit {
            EXPLICIT_MIXED_CHECK_LEVEL
        } else {
            IMPLICIT_MIXED_CHECK_LEVEL
        };
        (self.configured_level? >= threshold).then_some(threshold)
    }

    fn accepts_at_level(&self, sub: &Type, sup: &Type, level: u8) -> bool {
        if let Type(Repr::Mixed(is_explicit)) = sub {
            let strict_level = if *is_explicit {
                EXPLICIT_MIXED_CHECK_LEVEL
            } else {
                IMPLICIT_MIXED_CHECK_LEVEL
            };
            if level < strict_level {
                return true;
            }
            return matches!(sup, Type(Repr::Mixed(_)));
        }
        self.accepts_type(sub, sup)
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
        (sub, sup),
        (
            Type(Repr::Int) | Type(Repr::IntLiteral(_)) | Type(Repr::IntRange(..)),
            Type(Repr::Float)
        ) | (
            Type(Repr::String) | Type(Repr::StringLiteral(_)),
            Type(Repr::ClassString(_)) | Type(Repr::ClassKindString(_))
        ) | (Type(Repr::Callable(None)), Type(Repr::Callable(Some(_))))
            | (
                Type(Repr::SelfRef(_))
                    | Type(Repr::ParentRef)
                    | Type(Repr::StaticRef(_))
                    | Type(Repr::This),
                Type(Repr::Object(..)) | Type(Repr::ObjectWithoutClass)
            )
            | (_, Type(Repr::TemplateParam(_)))
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
        ty,
        Type(Repr::Null)
            | Type(Repr::True)
            | Type(Repr::False)
            | Type(Repr::IntLiteral(_))
            | Type(Repr::StringLiteral(_))
            | Type(Repr::FloatLiteral(_))
            | Type(Repr::EnumCase(_, _))
    ) || matches!(ty, Type(Repr::ArrayShape(shape)) if shape.is_empty_array())
}
