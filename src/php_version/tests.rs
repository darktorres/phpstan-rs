use super::*;

#[test]
fn parses_a_well_formed_version() {
    assert_eq!(parse_php_version("8.1"), Ok((8, 1)));
    assert_eq!(parse_php_version("7.4"), Ok((7, 4)));
    assert_eq!(parse_php_version("8.0"), Ok((8, 0)));
}

#[test]
fn rejects_a_bare_major_version() {
    assert!(parse_php_version("8").is_err());
}

#[test]
fn rejects_a_patch_component() {
    assert!(parse_php_version("8.1.0").is_err());
}

#[test]
fn rejects_non_numeric_parts() {
    assert!(parse_php_version("a.b").is_err());
    assert!(parse_php_version("8.x").is_err());
}

#[test]
fn error_message_names_the_offending_input() {
    let err = parse_php_version("nope").unwrap_err();
    assert!(err.contains("nope"), "got: {err}");
}

#[test]
fn feature_available_from_its_threshold_onward() {
    assert!(!PhpFeature::FinalClassConstants.is_available((8, 0)));
    assert!(PhpFeature::FinalClassConstants.is_available((8, 1)));
    assert!(PhpFeature::FinalClassConstants.is_available((8, 3)));
}

#[test]
fn unset_cast_runs_the_opposite_direction() {
    // The (unset) cast worked before PHP 8.0 and was removed there - so
    // "available" here means "still available", true only *before* the
    // threshold, unlike every other feature.
    assert!(PhpFeature::UnsetCastRemoved.is_available((7, 4)));
    assert!(!PhpFeature::UnsetCastRemoved.is_available((8, 0)));
    assert!(!PhpFeature::UnsetCastRemoved.is_available((8, 3)));
}

#[test]
fn case_insensitive_constants_run_the_opposite_direction_too() {
    // Confirmed directly via `php`: `define("FOO", 1, true)` on PHP 8.5
    // emits "Argument #3 ($case_insensitive) is ignored since
    // declaration of case-insensitive constants is no longer supported"
    // verbatim - the same message real PHPStan's own rule uses.
    assert!(PhpFeature::CaseInsensitiveConstantsRemoved.is_available((7, 4)));
    assert!(!PhpFeature::CaseInsensitiveConstantsRemoved.is_available((8, 0)));
    assert!(!PhpFeature::CaseInsensitiveConstantsRemoved.is_available((8, 5)));
}

#[test]
fn thresholds_match_real_phpstans_own_phpversion_class() {
    // Cross-checked directly against
    // vendor/phpstan-src/src/Php/PhpVersion.php's own supportsX/
    // deprecatesX methods at the time this table was written.
    assert_eq!(PhpFeature::NoncapturingCatch.threshold(), (8, 0));
    assert_eq!(PhpFeature::ThrowExpression.threshold(), (8, 0));
    assert_eq!(PhpFeature::PromotedProperties.threshold(), (8, 0));
    assert_eq!(PhpFeature::UnsetCastRemoved.threshold(), (8, 0));
    assert_eq!(PhpFeature::FinalClassConstants.threshold(), (8, 1));
    assert_eq!(PhpFeature::ArrayUnpackingWithStringKeys.threshold(), (8, 1));
    assert_eq!(PhpFeature::ReadOnlyClasses.threshold(), (8, 2));
    assert_eq!(PhpFeature::ConstantsInTraits.threshold(), (8, 2));
    assert_eq!(PhpFeature::NativeTypedClassConstants.threshold(), (8, 3));
    assert_eq!(PhpFeature::ReadOnlyAnonymousClasses.threshold(), (8, 3));
    assert_eq!(PhpFeature::FinalPromotedProperties.threshold(), (8, 5));
    assert_eq!(PhpFeature::VoidCast.threshold(), (8, 5));
    assert_eq!(PhpFeature::DeprecatedNonStandardCasts.threshold(), (8, 5));
    assert_eq!(PhpFeature::DeprecatedBacktickOperator.threshold(), (8, 5));
    assert_eq!(PhpFeature::AttributesOnGlobalConstants.threshold(), (8, 5));
    assert_eq!(
        PhpFeature::CaseInsensitiveConstantsRemoved.threshold(),
        (8, 0)
    );
}
