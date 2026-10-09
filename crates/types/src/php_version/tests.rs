use super::*;

#[test]
fn parses_a_well_formed_version() {
    assert_eq!(parse_php_version("8.1"), Ok(PhpVersion::new(8, 1)));
    assert_eq!(parse_php_version("7.4"), Ok(PhpVersion::new(7, 4)));
    assert_eq!(parse_php_version("8.0"), Ok(PhpVersion::new(8, 0)));
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
fn patch_aware_release_ranges_keep_exact_and_minor_wide_targets_distinct() {
    assert_eq!(
        parse_php_release_range("8.5.4"),
        Ok(PhpReleaseRange::new(
            PhpRelease::new(8, 5, 4),
            PhpRelease::new(8, 5, 4)
        ))
    );
    assert_eq!(
        parse_php_release_range("8.5"),
        Ok(PhpReleaseRange::new(
            PhpRelease::new(8, 5, 0),
            PhpRelease::new(8, 5, u16::MAX)
        ))
    );
    assert!(parse_php_release_range("8").is_err());
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
    assert!(!PhpFeature::FinalClassConstants.is_available(PhpVersion::new(8, 0)));
    assert!(PhpFeature::FinalClassConstants.is_available(PhpVersion::new(8, 1)));
    assert!(PhpFeature::FinalClassConstants.is_available(PhpVersion::new(8, 3)));
}

#[test]
fn unset_cast_runs_the_opposite_direction() {
    // The (unset) cast worked before PHP 8.0 and was removed there - so
    // "available" here means "still available", true only *before* the
    // threshold, unlike every other feature.
    assert!(PhpFeature::UnsetCastRemoved.is_available(PhpVersion::new(7, 4)));
    assert!(!PhpFeature::UnsetCastRemoved.is_available(PhpVersion::new(8, 0)));
    assert!(!PhpFeature::UnsetCastRemoved.is_available(PhpVersion::new(8, 3)));
}

#[test]
fn case_insensitive_constants_run_the_opposite_direction_too() {
    // `define("FOO", 1, true)` on PHP 8.5 emits "Argument #3
    // ($case_insensitive) is ignored since declaration of
    // case-insensitive constants is no longer supported" verbatim - the
    // same message PHPStan's own rule uses.
    assert!(PhpFeature::CaseInsensitiveConstantsRemoved.is_available(PhpVersion::new(7, 4)));
    assert!(!PhpFeature::CaseInsensitiveConstantsRemoved.is_available(PhpVersion::new(8, 0)));
    assert!(!PhpFeature::CaseInsensitiveConstantsRemoved.is_available(PhpVersion::new(8, 5)));
}

#[test]
fn thresholds_match_real_phpstans_own_phpversion_class() {
    // Cross-checked directly against
    // vendor/phpstan-src/src/Php/PhpVersion.php's own supportsX/
    // deprecatesX methods at the time this table was written.
    assert_eq!(
        PhpFeature::NoncapturingCatch.threshold(),
        PhpVersion::new(8, 0)
    );
    assert_eq!(
        PhpFeature::ThrowExpression.threshold(),
        PhpVersion::new(8, 0)
    );
    assert_eq!(
        PhpFeature::PromotedProperties.threshold(),
        PhpVersion::new(8, 0)
    );
    assert_eq!(
        PhpFeature::UnsetCastRemoved.threshold(),
        PhpVersion::new(8, 0)
    );
    assert_eq!(
        PhpFeature::FinalClassConstants.threshold(),
        PhpVersion::new(8, 1)
    );
    assert_eq!(
        PhpFeature::ArrayUnpackingWithStringKeys.threshold(),
        PhpVersion::new(8, 1)
    );
    assert_eq!(
        PhpFeature::PureIntersectionTypes.threshold(),
        PhpVersion::new(8, 1)
    );
    assert_eq!(
        PhpFeature::ReadOnlyClasses.threshold(),
        PhpVersion::new(8, 2)
    );
    assert_eq!(
        PhpFeature::ConstantsInTraits.threshold(),
        PhpVersion::new(8, 2)
    );
    assert_eq!(
        PhpFeature::NativeTypedClassConstants.threshold(),
        PhpVersion::new(8, 3)
    );
    assert_eq!(
        PhpFeature::ReadOnlyAnonymousClasses.threshold(),
        PhpVersion::new(8, 3)
    );
    assert_eq!(
        PhpFeature::FinalPromotedProperties.threshold(),
        PhpVersion::new(8, 5)
    );
    assert_eq!(PhpFeature::VoidCast.threshold(), PhpVersion::new(8, 5));
    assert_eq!(
        PhpFeature::DeprecatedNonStandardCasts.threshold(),
        PhpVersion::new(8, 5)
    );
    assert_eq!(
        PhpFeature::DeprecatedBacktickOperator.threshold(),
        PhpVersion::new(8, 5)
    );
    assert_eq!(
        PhpFeature::AttributesOnGlobalConstants.threshold(),
        PhpVersion::new(8, 5)
    );
    assert_eq!(
        PhpFeature::CaseInsensitiveConstantsRemoved.threshold(),
        PhpVersion::new(8, 0)
    );
}

#[test]
fn a_version_id_names_its_minor_release() {
    assert_eq!(PhpVersion::new(8, 1).id(), 80100);
    assert_eq!(PhpVersion::from_id(80123), PhpVersion::new(8, 1));
    assert_eq!(PhpVersion::new(8, 1).to_string(), "8.1");
}

#[test]
fn a_php_version_id_comparison_narrows_the_range() {
    let range = PhpVersions::new(PhpVersion::new(7, 4), PhpVersion::new(8, 4));
    assert_eq!(
        range.narrow_at_least(80000, true),
        Some(PhpVersions::new(
            PhpVersion::new(8, 0),
            PhpVersion::new(8, 4)
        ))
    );
    assert_eq!(
        range.narrow_at_least(80000, false),
        Some(PhpVersions::new(
            PhpVersion::new(7, 4),
            PhpVersion::new(7, 255)
        ))
    );
    assert_eq!(range.narrow_at_least(90000, true), None);
    assert!(range.contains(PhpVersion::new(8, 1)));
    assert!(!range.contains(PhpVersion::new(7, 3)));
}

#[test]
fn a_range_supports_a_feature_on_all_some_or_none_of_its_versions() {
    use crate::TrinaryLogic;
    let range = PhpVersions::new(PhpVersion::new(7, 4), PhpVersion::new(8, 4));
    assert_eq!(
        range.supports(PhpFeature::NamedArguments),
        TrinaryLogic::Maybe
    );
    assert_eq!(
        PhpVersions::exactly(PhpVersion::new(8, 1)).supports(PhpFeature::NamedArguments),
        TrinaryLogic::Yes
    );
    assert_eq!(
        PhpVersions::exactly(PhpVersion::new(8, 1)).supports(PhpFeature::UnsetCastRemoved),
        TrinaryLogic::No
    );
}

#[test]
fn known_versions_where_spans_the_oldest_to_the_newest_admitted_release() {
    let range = known_php_versions_where(|v| v >= PhpVersion::new(8, 1));
    assert_eq!(
        range,
        Some(PhpVersions::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5)
        ))
    );
    assert_eq!(known_php_versions_where(|_| false), None);
}
