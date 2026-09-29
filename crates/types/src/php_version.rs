//! The target-PHP-version setting (`config.rs`'s `php_version` key /
//! `check-project --php-version`) plus a small lookup table of the
//! PHP-version-gated language features this project's rules need to check
//! against once a version is configured.
//!
//! A `config.rs` field, a CLI flag, `main.rs`'s config-vs-CLI merge (CLI
//! wins), and a `Project` setter that stores onto the shared `Registry`.
//! Unlike `strict_mixed_arrays` (a plain `bool`), a `php_version` is
//! unset by default (`Registry::php_version: Option<PhpVersion>`) rather
//! than defaulting to some specific version - with no configured
//! version, every version-gated rule listed below simply has nothing to
//! check against yet and should stay silent.
//!
//! Real PHPStan's own equivalent is `PHPStan\Php\PhpVersion`, which each
//! version-gated rule takes as a constructor dependency and calls a
//! `supportsX()`/`deprecatesX()` method on. `PhpFeature` below is this
//! project's stand-in: one variant per such check, each carrying the
//! exact version threshold.

/// A target PHP version, major and minor. Patch versions never affect
/// PHPStan's own version gates (every `PhpVersion::supportsX`/
/// `deprecatesX` check in `vendor/phpstan-src/src/Php/PhpVersion.php`
/// compares whole minor releases, e.g. `>= 80100`), so there's no third
/// field to carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhpVersion {
    pub major: u8,
    pub minor: u8,
}

impl PhpVersion {
    pub const fn new(major: u8, minor: u8) -> PhpVersion {
        PhpVersion { major, minor }
    }

    /// `PHP_VERSION_ID` at this version's first release (`80100`).
    pub fn id(self) -> u32 {
        u32::from(self.major) * 10_000 + u32::from(self.minor) * 100
    }

    /// The version `PHP_VERSION_ID` `id` is part of.
    pub fn from_id(id: u32) -> PhpVersion {
        PhpVersion::new(
            u8::try_from(id / 10_000).unwrap_or(u8::MAX),
            u8::try_from(id / 100 % 100).unwrap_or(u8::MAX),
        )
    }
}

impl std::fmt::Display for PhpVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// The PHP versions code may run on, `min..=max` (PHPStan's
/// `PhpVersions`): a `PHP_VERSION_ID` comparison narrows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhpVersions {
    pub min: PhpVersion,
    pub max: PhpVersion,
}

impl PhpVersions {
    pub const fn new(min: PhpVersion, max: PhpVersion) -> PhpVersions {
        PhpVersions { min, max }
    }

    /// Exactly one version.
    pub const fn exactly(version: PhpVersion) -> PhpVersions {
        PhpVersions::new(version, version)
    }

    pub fn contains(self, version: PhpVersion) -> bool {
        self.min <= version && version <= self.max
    }

    /// Whether `feature` is available on every version in the range
    /// (`Yes`), on none (`No`), or on some.
    pub fn supports(self, feature: PhpFeature) -> crate::TrinaryLogic {
        use crate::TrinaryLogic;
        match (
            feature.is_available(self.min),
            feature.is_available(self.max),
        ) {
            (true, true) => TrinaryLogic::Yes,
            (false, false) => TrinaryLogic::No,
            _ => TrinaryLogic::Maybe,
        }
    }

    /// The range where `PHP_VERSION_ID >= id` holds (`holds`) or fails,
    /// `None` when no version in it is left.
    pub fn narrow_at_least(self, id: u32, holds: bool) -> Option<PhpVersions> {
        let at = PhpVersion::from_id(id);
        let narrowed = if holds {
            PhpVersions::new(self.min.max(at), self.max)
        } else {
            let below = if at.minor > 0 {
                PhpVersion::new(at.major, at.minor - 1)
            } else {
                PhpVersion::new(at.major.saturating_sub(1), u8::MAX)
            };
            PhpVersions::new(self.min, self.max.min(below))
        };
        (narrowed.min <= narrowed.max).then_some(narrowed)
    }
}

/// Every PHP release a declared version range is drawn from, oldest first.
pub const KNOWN_PHP_VERSIONS: &[PhpVersion] = &[
    PhpVersion::new(5, 3),
    PhpVersion::new(5, 4),
    PhpVersion::new(5, 5),
    PhpVersion::new(5, 6),
    PhpVersion::new(7, 0),
    PhpVersion::new(7, 1),
    PhpVersion::new(7, 2),
    PhpVersion::new(7, 3),
    PhpVersion::new(7, 4),
    PhpVersion::new(8, 0),
    PhpVersion::new(8, 1),
    PhpVersion::new(8, 2),
    PhpVersion::new(8, 3),
    PhpVersion::new(8, 4),
    PhpVersion::new(8, 5),
];

/// The range from the oldest to the newest known release `admits` accepts,
/// or `None` when it accepts none.
pub fn known_php_versions_where(admits: impl Fn(PhpVersion) -> bool) -> Option<PhpVersions> {
    let mut admitted = KNOWN_PHP_VERSIONS.iter().copied().filter(|&v| admits(v));
    let min = admitted.next()?;
    let max = admitted.last().unwrap_or(min);
    Some(PhpVersions::new(min, max))
}

/// Parses the `"X.Y"` string format used by both the `--php-version` CLI
/// flag and `config.rs`'s `php_version` TOML key (e.g. `"8.1"`) into a
/// `PhpVersion`. Rejects anything else (a bare `"8"`, a patch component
/// like `"8.1.0"`, non-numeric parts) with a message naming the offending
/// input, rather than silently guessing - the CLI/config merge point
/// (`main.rs::main`) reports this the same way it already reports a
/// malformed `--config` file: `eprintln!` + `exit(1)`.
pub fn parse_php_version(s: &str) -> Result<PhpVersion, String> {
    let invalid = || format!("invalid php_version {s:?}: expected \"major.minor\" (e.g. \"8.1\")");
    let mut parts = s.split('.');
    let major = parts.next().ok_or_else(invalid)?;
    let minor = parts.next().ok_or_else(invalid)?;
    if parts.next().is_some() {
        return Err(invalid());
    }
    let major: u8 = major.parse().map_err(|_| invalid())?;
    let minor: u8 = minor.parse().map_err(|_| invalid())?;
    Ok(PhpVersion::new(major, minor))
}

/// One PHP language feature whose availability depends on the configured
/// `php_version` - each variant corresponds to a real
/// `PHPStan\Php\PhpVersion` method (named in its doc comment) that a
/// version-gated portability rule calls. Most variants follow the
/// "available from this version on" direction (`PhpVersion::supportsX`/
/// `deprecatesX`'s own `>=` checks); a "Removed" variant (`UnsetCastRemoved`,
/// `CaseInsensitiveConstantsRemoved`) runs the other way - the feature
/// worked *before* its own threshold version and was removed there
/// (`PhpVersion::supportsUnsetCast`/`supportsCaseInsensitiveConstantNames`'s
/// own `< 80000` checks) - so callers should use `is_available` rather
/// than hand-rolling the comparison themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhpFeature {
    /// Non-capturing `catch` (no bound `$variable`).
    /// `PhpVersion::supportsNoncapturingCatches` (`NoncapturingCatchRule`).
    NoncapturingCatch,
    /// `throw` used as an expression rather than only a statement.
    /// `PhpVersion::supportsThrowExpression` (`ThrowExpressionRule`).
    ThrowExpression,
    /// Constructor property promotion.
    /// `PhpVersion::supportsPromotedProperties`
    /// (`InvalidPromotedPropertiesRule`).
    PromotedProperties,
    /// The `(unset)` cast - removed as of this version, so
    /// `is_available` checks the opposite direction from every other
    /// variant here. `PhpVersion::supportsUnsetCast` (`UnsetCastRule`).
    UnsetCastRemoved,
    /// `final` class constants.
    /// `PhpVersion::supportsFinalConstants` (`FinalConstantRule`).
    FinalClassConstants,
    /// Array unpacking (`[...$a]`) with string keys.
    /// `PhpVersion::supportsArrayUnpackingWithStringKeys`
    /// (`ArrayUnpackingRule`).
    ArrayUnpackingWithStringKeys,
    /// `readonly` classes.
    /// `PhpVersion::supportsReadOnlyClasses` (`ReadOnlyClassRule`).
    ReadOnlyClasses,
    /// Class constants declared inside a `trait`.
    /// `PhpVersion::supportsConstantsInTraits` (`ConstantsInTraitsRule`).
    ConstantsInTraits,
    /// Class constants with a native type declaration.
    /// `PhpVersion::supportsNativeTypesInClassConstants`
    /// (`NativeTypedClassConstantRule`).
    NativeTypedClassConstants,
    /// `readonly` anonymous classes.
    /// `PhpVersion::supportsReadOnlyAnonymousClasses`
    /// (`ReadOnlyClassRule`).
    ReadOnlyAnonymousClasses,
    /// `final` promoted constructor properties.
    /// `PhpVersion::supportsFinalPromotedProperties`
    /// (`InvalidPromotedPropertiesRule`).
    FinalPromotedProperties,
    /// The `(void)` cast.
    /// `PhpVersion::supportsVoidCast` (`VoidCastRule`).
    VoidCast,
    /// Non-standard casts (`(unset)`, `(real)`, `(binary)`) being
    /// deprecated. `PhpVersion::deprecatesNonStandardCasts`
    /// (`DeprecatedCastRule`).
    DeprecatedNonStandardCasts,
    /// The backtick shell-exec operator being deprecated.
    /// `PhpVersion::deprecatesBacktickOperator` (`BacktickRule`).
    DeprecatedBacktickOperator,
    /// Attributes on global `const` declarations.
    /// `PhpVersion::supportsAttributesOnGlobalConstants`
    /// (`ConstantAttributesRule`).
    AttributesOnGlobalConstants,
    /// Case-insensitive constants (`define()`'s own third
    /// `$case_insensitive` argument) - removed as of this version, the
    /// same "no longer available from this version on" direction as
    /// `UnsetCastRemoved`. `PhpVersion::supportsCaseInsensitiveConstantNames`
    /// (`DefineParametersRule`).
    CaseInsensitiveConstantsRemoved,
    /// A `readonly` property may be reassigned once from within the
    /// declaring class's own `__clone()` method (a real, deliberate
    /// "deep clone" idiom, not a bug) - `PhpVersion::
    /// supportsReadonlyPropertyReinitializationOnClone`
    /// (`ReadOnlyPropertyAssignRule`'s own `$inClone` gate).
    /// An element appended after a negative integer key takes the next
    /// integer (`[-5 => 'a', 'b']` puts `'b'` at -4), not 0.
    NegativeArrayKeysAdvanceTheNextIndex,
    ReadonlyPropertyReinitializationOnClone,
    /// The `FILTER_THROW_ON_FAILURE` constant/flag for `filter_var()`/
    /// `filter_input()`. `PhpVersion::hasFilterThrowOnFailureConstant`
    /// (`FilterVarRule`'s own `filterVar.nullOnFailureAndThrowOnFailure`
    /// conflict check) - PHP 8.5.
    FilterThrowOnFailureConstant,
    /// A `float` array key implicitly truncating to `int` - deprecated as
    /// of this version. `PhpVersion::deprecatesImplicitlyFloatConversionToInt`
    /// (`InvalidKeyInArrayItemRule`'s own `AllowedArrayKeysTypes`).
    DeprecatesFloatArrayKey,
    /// A `null` array key implicitly coercing to the empty string.
    /// Deprecated as of this version.
    /// `PhpVersion::deprecatesNullArrayOffset` (`InvalidKeyInArrayItemRule`'s
    /// own `AllowedArrayKeysTypes`).
    DeprecatesNullArrayOffset,
    /// The `%h`/`%H` printf conversion specifiers (locale-independent
    /// decimal-point float formatting). `PhpVersion::
    /// supportsHhPrintfSpecifier` (`PrintfHelper::parsePlaceholders`).
    PrintfHhSpecifier,
    /// Named arguments. `PhpVersion::supportsNamedArguments`
    /// (`FunctionCallParametersCheck`).
    NamedArguments,
    /// A named argument after an unpacked one (`f(...$a, b: 1)`).
    /// `PhpVersion::supportsNamedArgumentAfterUnpackedArgument`
    /// (`FunctionCallParametersCheck`).
    NamedArgumentAfterUnpackedArgument,
    /// PHP's native `#[\Deprecated]` attribute actually being respected
    /// on a *trait* declaration - below this version it's a real, if
    /// silent, no-op there (unlike a function/method/class constant/enum
    /// case, or even a plain class/interface/enum, where PHPStan flags it
    /// as unconditionally wrong regardless of version - real PHP simply
    /// never implemented engine-level support for those at all).
    /// `PhpVersion::supportsDeprecatedTraits`
    /// (`TraitAttributesRule::processNode`).
    DeprecatedAttributeOnTraits,
    /// PHP's native `#[\Override]` attribute being usable on a *property*
    /// declaration, not just a method. `PhpVersion::
    /// supportsOverrideAttributeOnProperty`
    /// (`PropertyAttributesRule::processNode`).
    OverrideAttributeOnProperty,
    /// Native union type-hints (`int|string`) on a
    /// parameter/return type. `PhpVersion::supportsNativeUnionTypes`
    /// (`FunctionDefinitionCheck`'s own `parameter.unionTypeNotSupported`/
    /// `return.unionTypeNotSupported`).
    NativeUnionTypes,
    /// A typed parameter's implicit-nullable-via-`= null`-default form
    /// (`Foo $x = null`, without a real `?Foo`/union-with-`null` type)
    /// being deprecated. `PhpVersion::deprecatesImplicitlyNullableParameterTypes`
    /// (`FunctionDefinitionCheck`'s own `parameter.implicitlyNullable`).
    ImplicitlyNullableParameterDeprecated,
    /// A required parameter following an optional one being deprecated.
    /// `PhpVersion::deprecatesRequiredParameterAfterOptional`
    /// (`FunctionDefinitionCheck`'s own `parameter.requiredAfterOptional`,
    /// noting that this project's own port is deliberately narrower than
    /// real PHPStan's full version-layered logic; see
    /// `check_required_parameter_after_optional`'s own doc comment).
    RequiredParameterAfterOptionalDeprecated,
    /// Native intersection type-hints (`Foo&Bar`), gating whether it's
    /// meaningful to check one for satisfiability at all.
    /// `PhpVersion::supportsPureIntersectionTypes` (`FunctionDefinitionCheck`'s
    /// own `parameter.unresolvableNativeType`/`return.unresolvableNativeType`,
    /// backed by `UnresolvableTypeHelper`).
    PureIntersectionTypes,
    /// A trait's own abstract methods being enforced against the using
    /// class's implementation. `PhpVersion::supportsAbstractTraitMethods`
    /// (`NewStaticRule`'s trait-constructor exemption).
    AbstractTraitMethods,
    /// A `Serializable` implementation also needing `__serialize()`/
    /// `__unserialize()`. `PhpVersion::serializableRequiresMagicMethods`
    /// (`MissingMagicSerializationMethodsRule`).
    SerializableRequiresMagicMethods,
    /// First-class-callable syntax (`foo(...)`, `$obj->method(...)`,
    /// `Foo::method(...)`). `PhpVersion::supportsFirstClassCallables`
    /// (`FunctionCallableRule`/`MethodCallableRule`/
    /// `StaticMethodCallableRule`).
    FirstClassCallables,
    /// Built-in methods' `#[TentativeType]` return types being enforced
    /// on overrides. `PhpVersion::hasTentativeReturnTypes`
    /// (`OverridingMethodRule`).
    TentativeReturnTypes,
    /// Native `readonly` properties. `PhpVersion::supportsReadOnlyProperties`
    /// (`ReadOnlyPropertyRule`).
    ReadOnlyProperties,
    /// Property hooks, and properties declared on an interface.
    /// `PhpVersion::supportsPropertyHooks` (`PropertyInClassRule`/
    /// `PropertiesInInterfaceRule`).
    PropertyHooks,
    /// `final` properties. `PhpVersion::supportsFinalProperties`
    /// (`PropertyInClassRule`).
    FinalProperties,
    /// Asymmetric visibility (`public private(set)`), which also makes a
    /// `readonly` property `protected(set)`.
    /// `PhpVersion::supportsAsymmetricVisibility` (`ReadOnlyPropertyAssignRule`).
    AsymmetricVisibility,
    /// Asymmetric visibility on a `static` property.
    /// `PhpVersion::supportsAsymmetricVisibilityForStaticProperties`
    /// (`PropertyInClassRule`).
    AsymmetricVisibilityForStaticProperties,
    /// `Foo::{$expr}` dynamic class-constant fetches.
    /// `PhpVersion::supportsDynamicClassConstantFetch`
    /// (`DynamicClassConstantFetchRule`).
    DynamicClassConstantFetch,
    /// `null`/`false` as standalone types and the `true` type.
    /// `PhpVersion::supportsNullFalseTrueStandaloneTypes`-equivalent
    /// (engine compile error before 8.2).
    StandaloneNullFalseTrueTypes,
    /// Disjunctive normal form types (`(A&B)|null`).
    DisjunctiveNormalFormTypes,
    /// A non-numeric string in arithmetic throws a `TypeError` ("saner
    /// numeric strings"); before, it is `0` with a warning.
    NonNumericStringArithmeticThrows,
}

impl PhpFeature {
    /// The version threshold this feature is gated on. See `is_available`
    /// for how to interpret it - most variants mean "available from this
    /// version on"; `UnsetCastRemoved` means "no longer available from
    /// this version on".
    pub fn threshold(self) -> PhpVersion {
        use PhpFeature::*;
        match self {
            NoncapturingCatch
            | ThrowExpression
            | PromotedProperties
            | UnsetCastRemoved
            | CaseInsensitiveConstantsRemoved
            | PrintfHhSpecifier
            | NamedArguments
            | NativeUnionTypes
            | RequiredParameterAfterOptionalDeprecated
            | AbstractTraitMethods
            | NonNumericStringArithmeticThrows => PhpVersion::new(8, 0),
            FinalClassConstants
            | ArrayUnpackingWithStringKeys
            | DeprecatesFloatArrayKey
            | PureIntersectionTypes
            | SerializableRequiresMagicMethods
            | FirstClassCallables
            | TentativeReturnTypes
            | NamedArgumentAfterUnpackedArgument
            | ReadOnlyProperties => PhpVersion::new(8, 1),
            ReadOnlyClasses
            | ConstantsInTraits
            | StandaloneNullFalseTrueTypes
            | DisjunctiveNormalFormTypes => PhpVersion::new(8, 2),
            NativeTypedClassConstants
            | DynamicClassConstantFetch
            | ReadOnlyAnonymousClasses
            | ReadonlyPropertyReinitializationOnClone
            | NegativeArrayKeysAdvanceTheNextIndex => PhpVersion::new(8, 3),
            ImplicitlyNullableParameterDeprecated
            | PropertyHooks
            | FinalProperties
            | AsymmetricVisibility => PhpVersion::new(8, 4),
            FinalPromotedProperties
            | VoidCast
            | DeprecatedNonStandardCasts
            | DeprecatedBacktickOperator
            | AttributesOnGlobalConstants
            | FilterThrowOnFailureConstant
            | DeprecatesNullArrayOffset
            | DeprecatedAttributeOnTraits
            | OverrideAttributeOnProperty
            | AsymmetricVisibilityForStaticProperties => PhpVersion::new(8, 5),
        }
    }

    /// Whether this feature is available (or, for a "removed" variant,
    /// still available) under `php_version`. A version-gated rule should
    /// call this rather than comparing against `threshold()` itself, since
    /// a "removed" variant runs in the opposite direction from every
    /// other one.
    pub fn is_available(self, php_version: PhpVersion) -> bool {
        if matches!(
            self,
            PhpFeature::UnsetCastRemoved | PhpFeature::CaseInsensitiveConstantsRemoved
        ) {
            php_version < self.threshold()
        } else {
            php_version >= self.threshold()
        }
    }
}

#[cfg(test)]
mod tests;
