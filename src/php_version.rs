//! Phase 38: the target-PHP-version setting (`config.rs`'s `php_version`
//! key / `check-project --php-version`) plus a small lookup table of the
//! PHP-version-gated language features this project's rules need to check
//! against once a version is configured.
//!
//! This mirrors Phase 11 Stage 5's `strict_mixed_arrays` precedent end to
//! end: a `config.rs` field, a CLI flag, `main.rs`'s config-vs-CLI merge
//! (CLI wins), and a `Project` setter that stores onto the shared
//! `Registry`. Unlike `strict_mixed_arrays` (a plain `bool`), a
//! `php_version` is unset by default (`Registry::php_version: Option<
//! PhpVersion>`) rather than defaulting to some specific version - with no
//! configured version, every version-gated rule listed below simply has
//! nothing to check against yet and should stay silent (this phase adds no
//! new diagnostics itself; it only builds the plumbing the following ones
//! need).
//!
//! Real PHPStan's own equivalent is `PHPStan\Php\PhpVersion` (see
//! `vendor/phpstan-src/src/Php/PhpVersion.php`), which every rule this
//! phase's own roadmap entry names (`FinalConstantRule`,
//! `NativeTypedClassConstantRule`, `ConstantAttributesRule`,
//! `DeprecatedCastRule`, `VoidCastRule`, `BacktickRule`, `UnsetCastRule`,
//! `ArrayUnpackingRule`, `ReadOnlyClassRule`, `ConstantsInTraitsRule`,
//! `InvalidPromotedPropertiesRule`, `NoncapturingCatchRule`,
//! `ThrowExpressionRule`) takes as a constructor dependency and calls a
//! `supportsX()`/`deprecatesX()` method on. `PhpFeature` below is this
//! project's stand-in: one variant per such check, each carrying the exact
//! version threshold read directly off that file, so a future rule
//! implementation doesn't have to go spelunking for the right PHP version
//! number itself.

/// A target PHP version as a (major, minor) pair, e.g. `(8, 1)` for PHP
/// 8.1. Patch versions never affect PHPStan's own version gates (every
/// `PhpVersion::supportsX`/`deprecatesX` check in
/// `vendor/phpstan-src/src/Php/PhpVersion.php` compares whole minor
/// releases, e.g. `>= 80100`), so there's no third field to carry.
pub type PhpVersion = (u8, u8);

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
    Ok((major, minor))
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
    ReadonlyPropertyReinitializationOnClone,
    /// The `FILTER_THROW_ON_FAILURE` constant/flag for `filter_var()`/
    /// `filter_input()`. `PhpVersion::hasFilterThrowOnFailureConstant`
    /// (`FilterVarRule`'s own `filterVar.nullOnFailureAndThrowOnFailure`
    /// conflict check) - PHP 8.5, not 8.4 as an earlier ROADMAP draft
    /// stated (confirmed against `vendor/phpstan-src/src/Php/PhpVersion.php`
    /// and the real `filter-var-php85.php` test fixture name).
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
    /// Phase 48.F: native union type-hints (`int|string`) on a
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
            | NativeUnionTypes
            | RequiredParameterAfterOptionalDeprecated => (8, 0),
            FinalClassConstants | ArrayUnpackingWithStringKeys | DeprecatesFloatArrayKey => (8, 1),
            ReadOnlyClasses | ConstantsInTraits => (8, 2),
            NativeTypedClassConstants
            | ReadOnlyAnonymousClasses
            | ReadonlyPropertyReinitializationOnClone => (8, 3),
            ImplicitlyNullableParameterDeprecated => (8, 4),
            FinalPromotedProperties
            | VoidCast
            | DeprecatedNonStandardCasts
            | DeprecatedBacktickOperator
            | AttributesOnGlobalConstants
            | FilterThrowOnFailureConstant
            | DeprecatesNullArrayOffset
            | DeprecatedAttributeOnTraits
            | OverrideAttributeOnProperty => (8, 5),
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
