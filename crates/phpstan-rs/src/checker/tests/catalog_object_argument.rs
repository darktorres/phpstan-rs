//! A bare `object` argument to `strlen` may be Stringable, so the call must
//! invalidate what its `__toString` may write, as a `mixed` argument does.
//! The semantic catalog only covers `strlen` for an exact PHP 8.5.4 target,
//! so the project is analysed under that target. Regression for RV-9 in
//! PHP_SEMANTICS_CATALOG_CONTRACT_REVIEW.md.

use crate::project::{ProjectSettings, test_support::Files};

fn diagnostics_at_php_8_5_4(src: &str) -> Vec<diagnostics::Diagnostic> {
    let release = types::PhpRelease::new(8, 5, 4);
    let settings = ProjectSettings {
        php_versions: Some(types::PhpVersions::exactly(types::PhpVersion::new(8, 5))),
        php_releases: Some(types::PhpReleaseRange::new(release, release)),
        ..ProjectSettings::default()
    };
    Files::new()
        .settings(settings)
        .analysed("test.php", src)
        .load()
        .check_file(std::path::Path::new("test.php"))
}

#[test]
#[ignore = "RV-9: the Stringable predicate answers No for a bare object; remove when fixed"]
fn bare_object_argument_invalidates_property_narrowing_like_mixed() {
    let src = "<?php\n\
        class C {\n\
            public int $p = 0;\n\
            public function __toString(): string {\n\
                $this->p = 2;\n\
                return 'x';\n\
            }\n\
        }\n\
        function viaObject(object $o, C $c): void {\n\
            $c->p = 1;\n\
            strlen($o);\n\
            assertType('1', $c->p);\n\
        }\n\
        function viaMixed(mixed $o, C $c): void {\n\
            $c->p = 1;\n\
            strlen($o);\n\
            assertType('1', $c->p);\n\
        }\n";
    let mismatches = diagnostics_at_php_8_5_4(src)
        .iter()
        .filter(|d| d.identifier.ends_with("assertType"))
        .count();
    assert_eq!(mismatches, 2, "both calls must invalidate $c->p");
}
