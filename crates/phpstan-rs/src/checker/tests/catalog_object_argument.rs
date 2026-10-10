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
        php_target_profile: Some(php_semantics::TargetProfile {
            os: Some(php_semantics::Bytes(b"linux".to_vec())),
            arch: Some(php_semantics::Bytes(b"x86_64".to_vec())),
            sapi: Some(php_semantics::Bytes(b"cli".to_vec())),
            zts: Some(false),
            debug: Some(false),
            extensions: None,
            ini: None,
        }),
        ..ProjectSettings::default()
    };
    Files::new()
        .settings(settings)
        .analysed("test.php", src)
        .load()
        .check_file(std::path::Path::new("test.php"))
}

#[test]
fn exact_dynamic_strlen_preserves_argument_diagnostics_once() {
    let direct = diagnostics_at_php_8_5_4("<?php function f(): void { strlen([]); }");
    let dynamic = diagnostics_at_php_8_5_4("<?php function f(): void { $fn = 'strlen'; $fn([]); }");
    let argument_errors = |diagnostics: &[diagnostics::Diagnostic]| {
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.identifier.as_str() == "argument.type")
            .map(|diagnostic| diagnostic.message.clone())
            .collect::<Vec<_>>()
    };

    let direct_errors = argument_errors(&direct);
    let dynamic_errors = argument_errors(&dynamic);
    assert_eq!(direct_errors.len(), 1, "direct control: {direct:#?}");
    assert_eq!(
        dynamic_errors.len(),
        1,
        "exact-name dynamic call: {dynamic:#?}"
    );
    assert_eq!(dynamic_errors, direct_errors);
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

#[test]
fn mixed_strlen_argument_keeps_stringable_and_non_stringable_paths_distinct() {
    let src = "<?php\n\
        class C { public int $p = 0; }\n\
        final class S implements Stringable {\n\
            public function __construct(private C $c) {}\n\
            public function __toString(): string { $this->c->p = 2; return 'x'; }\n\
        }\n\
        function viaMixed(mixed $o, C $c): void {\n\
            $c->p = 1;\n\
            strlen($o);\n\
            assertType('int', $c->p);\n\
        }\n\
        final class ThrowingS implements Stringable {\n\
            public function __construct(private C $c) {}\n\
            public function __toString(): string { $this->c->p = 2; throw new RuntimeException(); }\n\
        }\n\
        function viaMixedThrow(mixed $o, C $c): void {\n\
            $c->p = 1;\n\
            try { strlen($o); } catch (Throwable $e) { assertType('int', $c->p); }\n\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let assertion_mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        assertion_mismatches.is_empty(),
        "mixed Stringable/non-Stringable path handling: {diagnostics:#?}"
    );
}

#[test]
fn caught_stringable_throw_keeps_only_reachable_straight_line_writes() {
    let src = "<?php\n\
        class C { public int|string $before = 0; public int|string $after = 0; }\n\
        final class S implements Stringable {\n\
            private C $c = new C();\n\
            public function __construct(C $c) { $this->c = $c; }\n\
            public function __toString(): string {\n\
                $this->c->before = 'written';\n\
                throw new RuntimeException();\n\
                $this->c->after = 'unreachable';\n\
            }\n\
        }\n\
        function check(S $s, C $c): void {\n\
            $c->before = 1; $c->after = 1;\n\
            try { strlen($s); } catch (Throwable $e) {\n\
                assertType(\"1|'written'\", $c->before);\n\
                assertType('1', $c->after);\n\
            }\n\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "callback throw must include only reachable pre-throw writes: {diagnostics:#?}"
    );
}

#[test]
fn invalid_stringable_result_throws_after_callback_writes_without_normal_return() {
    let src = "<?php\n\\
        class C { public int|string $p = 0; }\n\\
        final class S {\n\\
            private C $c = new C();\n\\
            public function __construct(C $c) { $this->c = $c; }\n\\
            public function __toString() { $this->c->p = 'written'; return null; }\n\\
        }\n\\
        function check(S $s, C $c): void {\n\\
            $c->p = 1;\n\\
            assertType('never', strlen($s));\n\\
            try { strlen($s); } catch (Throwable $e) { assertType(\"1|'written'\", $c->p); }\n\\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "invalid __toString result must throw after preserving callback writes: {diagnostics:#?}"
    );
}

#[test]
fn weak_scalar_stringable_result_remains_a_valid_normal_return() {
    let src = "<?php\n\\
        class C { public int|string $p = 0; }\n\\
        final class S {\n\\
            private C $c = new C();\n\\
            public function __construct(C $c) { $this->c = $c; }\n\\
            public function __toString() { $this->c->p = 'written'; return 1; }\n\\
        }\n\\
        function check(S $s, C $c): void {\n\\
            $c->p = 1;\n\\
            assertType('int<0, max>', strlen($s));\n\\
            assertType(\"1|'written'\", $c->p);\n\\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "PHP weakly converts scalar __toString results: {diagnostics:#?}"
    );
}

#[test]
fn strict_scalar_stringable_result_throws_after_callback_writes() {
    let src = "<?php\n\\
        declare(strict_types=1);\n\\
        class C { public int|string $p = 0; }\n\\
        final class S {\n\\
            private C $c = new C();\n\\
            public function __construct(C $c) { $this->c = $c; }\n\\
            public function __toString() { $this->c->p = 'written'; return 1; }\n\\
        }\n\\
        function check(S $s, C $c): void {\n\\
            $c->p = 1;\n\\
            assertType('never', strlen($s));\n\\
            try { strlen($s); } catch (Throwable $e) { assertType(\"1|'written'\", $c->p); }\n\\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "strict callback result checks must preserve the throw-state write: {diagnostics:#?}"
    );
}

#[test]
fn conditional_invalid_stringable_result_keeps_normal_and_type_error_states_separate() {
    let src = "<?php\n\\
        class C { public int|string $p = 0; }\n\\
        final class S { public bool $invalid = false;\n\\
            private C $c = new C();\n\\
            public function __construct(C $c) { $this->c = $c; }\n\\
            public function __toString() {\n\\
                if ($this->invalid) { $this->c->p = 'invalid'; return null; }\n\\
                $this->c->p = 'valid'; return 'x';\n\\
            }\n\\
        }\n\\
        function check(S $s, C $c): void {\n\\
            $c->p = 1;\n\\
            try { strlen($s); } catch (Throwable $e) { assertType(\"1|'invalid'\", $c->p); }\n\\
            assertType(\"1|'invalid'|'valid'\", $c->p);\n\\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "valid and TypeError outcomes must retain their own writes: {diagnostics:#?}"
    );
}

#[test]
fn temporary_stringable_release_applies_destructor_writes_after_conversion_and_throw() {
    let src = "<?php\n\\
        class C { public int|string $p = 0; }\n\\
        final class NormalS {\n\\
            private C $c = new C();\n\\
            public function __construct(C $c) { $this->c = $c; }\n\\
            public function __toString(): string { $this->c->p = 'cast'; return 'x'; }\n\\
            public function __destruct() { $this->c->p = 'destroy'; }\n\\
        }\n\\
        final class ThrowingS {\n\\
            private C $c = new C();\n\\
            public function __construct(C $c) { $this->c = $c; }\n\\
            public function __toString(): string { $this->c->p = 'cast'; throw new RuntimeException(); }\n\\
            public function __destruct() { $this->c->p = 'destroy'; }\n\\
        }\n\\
        final class ThrowingDestructorS {\n\\
            private C $c = new C();\n\\
            public function __construct(C $c) { $this->c = $c; }\n\\
            public function __toString(): string { $this->c->p = 'cast'; return 'x'; }\n\\
            public function __destruct() { $this->c->p = 'destroy'; throw new RuntimeException(); }\n\\
        }\n\\
        final class ReentrantS {\n\\
            private C $c = new C();\n\\
            public function __construct(C $c) { $this->c = $c; }\n\\
            public function __toString(): string { $this->c->p = 'cast'; return 'x'; }\n\\
            public function __destruct() { echo 'release'; strlen(new NormalS($this->c)); $this->c->p = 'reentered'; }\n\\
        }\n\\
        function normal(C $c): void {\n\\
            $c->p = 1; strlen(new NormalS($c)); assertType(\"1|'cast'|'destroy'\", $c->p);\n\\
        }\n\\
        function retained(NormalS $s, C $c): void {\n\\
            $c->p = 1; strlen($s); assertType(\"1|'cast'\", $c->p);\n\\
        }\n\\
        function dynamic_normal(C $c): void {\n\\
            $callable = 'strlen'; $c->p = 1; $callable(new NormalS($c));\n\\
            assertType(\"1|'cast'|'destroy'\", $c->p);\n\\
        }\n\\
        function dynamic_retained(NormalS $s, C $c): void {\n\\
            $callable = 'strlen'; $c->p = 1; $callable($s);\n\\
            assertType(\"1|'cast'\", $c->p);\n\\
        }\n\\
        function ambiguous_dynamic(NormalS $s, C $c, bool $choice): void {\n\\
            $callable = $choice ? 'strlen' : 'unknown_function'; $c->p = 1;\n\\
            $callable($s); assertType('int|string', $c->p);\n\\
        }\n\\
        function throwing(C $c): void {\n\\
            $c->p = 1;\n\\
            try { strlen(new ThrowingS($c)); } catch (Throwable $e) { assertType(\"1|'cast'|'destroy'\", $c->p); }\n\\
        }\n\\
        function dynamic_throwing(C $c): void {\n\\
            $callable = 'strlen'; $c->p = 1;\n\\
            try { $callable(new ThrowingS($c)); } catch (Throwable $e) { assertType(\"1|'cast'|'destroy'\", $c->p); }\n\\
        }\n\\
        function destructor_throw(C $c): void {\n\\
            $c->p = 1;\n\\
            try { strlen(new ThrowingDestructorS($c)); } catch (Throwable $e) { assertType(\"1|'cast'|'destroy'\", $c->p); }\n\\
        }\n\\
        function dynamic_destructor_throw(C $c): void {\n\\
            $callable = 'strlen'; $c->p = 1;\n\\
            try { $callable(new ThrowingDestructorS($c)); } catch (Throwable $e) { assertType(\"1|'cast'|'destroy'\", $c->p); }\n\\
        }\n\\
        function complex_destructor(C $c): void {\n\\
            $c->p = 1; strlen(new ReentrantS($c));\n\\
            assertType('int|string', $c->p);\n\\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "temporary destructor writes follow conversion and precede the caught throw: {diagnostics:#?}"
    );
}

#[test]
fn catalog_consumers_apply_branch_effects_once_and_keep_uncovered_legacy_effects() {
    let exact = diagnostics_at_php_8_5_4(
        "<?php\n\\
        class C { public int $p = 0; }\n\\
        final class S implements Stringable {\n\\
            public function __construct(private C $c, private bool $throws) {}\n\\
            public function __toString(): string {\n\\
                if ($this->throws) { $this->c->p = 3; throw new RuntimeException(); }\n\\
                $this->c->p = 2; return 'x';\n\\
            }\n\\
        }\n\\
        function direct(C $c, bool $throws): void {\n\\
            $c->p = 1; strlen(new S($c, $throws));\n\\
            assertType('1|2', $c->p);\n\\
            try { strlen(new S($c, true)); } catch (Throwable $e) {\n\\
                assertType('1|3', $c->p);\n\\
            }\n\\
        }\n\\
        function dynamic(C $c, bool $throws): void {\n\\
            $strlen = 'strlen'; $c->p = 1; $strlen(new S($c, $throws));\n\\
            assertType('1|2', $c->p);\n\\
            try { $strlen(new S($c, true)); } catch (Throwable $e) {\n\\
                assertType('1|3', $c->p);\n\\
            }\n\\
        }\n",
    );
    let assert_type_failures = exact
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        assert_type_failures.is_empty(),
        "catalog return/effect consumers disagree: {exact:#?}"
    );

    // Without the exact catalog target profile, the ordinary builtin-run
    // effect must still reach Stringable::__toString and invalidate state.
    let uncovered = Files::new()
        .analysed(
            "uncovered.php",
            "<?php\n\\
                class C { public int $p = 0; }\n\\
                final class S implements Stringable {\n\\
                    public function __construct(private C $c) {}\n\\
                    public function __toString(): string { $this->c->p = 2; return 'x'; }\n\\
                }\n\\
                function check(C $c): void {\n\\
                    $c->p = 1; strlen(new S($c)); assertType('int', $c->p);\n\\
                }\n",
        )
        .load()
        .check_file(std::path::Path::new("uncovered.php"));
    assert!(
        uncovered
            .iter()
            .all(|diagnostic| !diagnostic.identifier.ends_with("assertType")),
        "uncatalogued call lost the legacy builtin-run transfer: {uncovered:#?}"
    );
}

#[test]
fn caught_stringable_throw_at_method_entry_keeps_following_writes_unreachable() {
    let src = "<?php\n\
        class C { public int|string $after = 0; }\n\
        final class S implements Stringable {\n\
            private C $c = new C();\n\
            public function __construct(C $c) { $this->c = $c; }\n\
            public function __toString(): string {\n\
                throw new RuntimeException();\n\
                $this->c->after = 'unreachable';\n\
            }\n\
        }\n\
        function check(S $s, C $c): void {\n\
            $c->after = 1;\n\
            try { strlen($s); } catch (Throwable $e) {\n\
                assertType('1', $c->after);\n\
            }\n\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "writes after an entry throw must stay unreachable: {diagnostics:#?}"
    );
}

#[test]
fn caught_stringable_throw_at_method_end_preserves_reachable_write_value() {
    let src = "<?php\n\
        class C { public int|string $before = 0; }\n\
        final class S implements Stringable {\n\
            private C $c = new C();\n\
            public function __construct(C $c) { $this->c = $c; }\n\
            public function __toString(): string {\n\
                $this->c->before = 'written';\n\
                throw new RuntimeException();\n\
            }\n\
        }\n\
        function check(S $s, C $c): void {\n\
            $c->before = 1;\n\
            try { strlen($s); } catch (Throwable $e) {\n\
                assertType(\"1|'written'\", $c->before);\n\
            }\n\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "reachable writes before an end throw must reach the catch: {diagnostics:#?}"
    );
}

#[test]
fn caught_stringable_conditional_throw_joins_only_throwing_branch_writes() {
    let src = "<?php\n\
        class C { public int|string $p = 0; public int|string $after = 0; }\n\
        final class S implements Stringable {\n\
            private C $c = new C();\n\
            private bool $throws = false;\n\
            public function __construct(C $c, bool $throws) { $this->c = $c; $this->throws = $throws; }\n\
            public function __toString(): string {\n\
                if ($this->throws) {\n\
                    $this->c->p = 'throw-path';\n\
                    throw new RuntimeException();\n\
                    $this->c->after = 'unreachable';\n\
                } else {\n\
                    $this->c->p = 'normal-path';\n\
                }\n\
                return 'ok';\n\
            }\n\
        }\n\
        function check(S $s, C $c): void {\n\
            $c->p = 1; $c->after = 1;\n\
            try { strlen($s); } catch (Throwable $e) {\n\
                assertType(\"1|'throw-path'\", $c->p);\n\
                assertType('1', $c->after);\n\
            }\n\
            assertType(\"1|'normal-path'|'throw-path'\", $c->p);\n\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "a catch must join only writes on the throwing branch: {diagnostics:#?}"
    );
}

#[test]
fn normal_stringable_completion_uses_only_normal_branch_writes() {
    let src = "<?php\n\
        class C { public int|string $p = 0; }\n\
        final class S implements Stringable {\n\
            private C $c = new C();\n\
            private bool $throws = false;\n\
            public function __construct(C $c, bool $throws) { $this->c = $c; $this->throws = $throws; }\n\
            public function __toString(): string {\n\
                if ($this->throws) {\n\
                    $this->c->p = 'throw-path';\n\
                    throw new RuntimeException();\n\
                } else {\n\
                    $this->c->p = 'normal-path';\n\
                }\n\
                return 'ok';\n\
            }\n\
        }\n\
        function check(S $s, C $c): void {\n\
            $c->p = 1;\n\
            strlen($s);\n\
            assertType(\"1|'normal-path'\", $c->p);\n\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "normal completion must carry only normal-branch writes: {diagnostics:#?}"
    );
}

#[test]
fn cross_file_stringable_callback_keeps_conservative_invalidation() {
    let release = types::PhpRelease::new(8, 5, 4);
    let settings = ProjectSettings {
        php_versions: Some(types::PhpVersions::exactly(types::PhpVersion::new(8, 5))),
        php_releases: Some(types::PhpReleaseRange::new(release, release)),
        php_target_profile: Some(php_semantics::TargetProfile {
            os: Some(php_semantics::Bytes(b"linux".to_vec())),
            arch: Some(php_semantics::Bytes(b"x86_64".to_vec())),
            sapi: Some(php_semantics::Bytes(b"cli".to_vec())),
            zts: Some(false),
            debug: Some(false),
            extensions: None,
            ini: None,
        }),
        ..ProjectSettings::default()
    };
    let diagnostics = Files::new()
        .settings(settings)
        .analysed(
            "callback.php",
            "<?php\n\
                class C { public int $p = 0; }\n\
                final class S implements Stringable {\n\
                    public function __construct(private C $c) {}\n\
                    public function __toString(): string {\n\
                        $this->c->p = 2;\n\
                        throw new RuntimeException();\n\
                    }\n\
                }\n",
        )
        .analysed(
            "main.php",
            "<?php\n\
                function check(S $s, C $c): void {\n\
                    $c->p = 1;\n\
                    strlen($s);\n\
                    assertType('int', $c->p);\n\
                }\n",
        )
        .load()
        .check_file(std::path::Path::new("main.php"));
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "a callback body outside the current source file must stay conservative: {diagnostics:#?}"
    );
}

#[test]
fn aliased_write_and_unsupported_loop_keep_conservative_invalidation() {
    let src = "<?php\n\
        class C { public int $p = 0; }\n\
        final class AliasS implements Stringable {\n\
            public function __construct(private C $c) {}\n\
            public function __toString(): string {\n\
                $target = $this->c;\n\
                $target->p = 2;\n\
                throw new RuntimeException();\n\
            }\n\
        }\n\
        final class LoopS implements Stringable {\n\
            public function __construct(private C $c, private bool $loop) {}\n\
            public function __toString(): string {\n\
                $this->c->p = 2;\n\
                while ($this->loop) { throw new RuntimeException(); }\n\
                return 'ok';\n\
            }\n\
        }\n\
        function checkAlias(AliasS $s, C $c): void {\n\
            $c->p = 1;\n\
            try { strlen($s); } catch (Throwable $e) { assertType('int', $c->p); }\n\
        }\n\
        function checkLoop(LoopS $s, C $c): void {\n\
            $c->p = 1;\n\
            try { strlen($s); } catch (Throwable $e) { assertType('int', $c->p); }\n\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "aliased writes or unsupported loops must keep conservative invalidation: {diagnostics:#?}"
    );
}

#[test]
fn callback_assignment_that_can_raise_type_error_keeps_conservative_throw_state() {
    let src = "<?php\n\
        class C { public int $p = 0; public int $bad = 0; }\n\
        final class S implements Stringable {\n\
            private C $c = new C();\n\
            private bool $throws = false;\n\
            public function __construct(C $c, bool $throws) {\n\
                $this->c = $c; $this->throws = $throws;\n\
            }\n\
            public function __toString(): string {\n\
                if ($this->throws) { throw new RuntimeException(); }\n\
                $this->c->p = 2;\n\
                $this->c->bad = 'invalid';\n\
                return 'ok';\n\
            }\n\
        }\n\
        function check(S $s, C $c): void {\n\
            $c->p = 1;\n\
            try { strlen($s); } catch (Throwable $e) { assertType('int', $c->p); }\n\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "a potentially throwing typed-property write must not be summarized as normal: {diagnostics:#?}"
    );
}

#[test]
fn caught_stringable_unsupported_callback_body_keeps_conservative_invalidation() {
    let src = "<?php\n\
        class C { public int|string $p = 0; }\n\
        function mutate(C $c): void { $c->p = 'from-call'; }\n\
        final class S implements Stringable {\n\
            public function __construct(private C $c, private bool $throws) {}\n\
            public function __toString(): string {\n\
                if ($this->throws) {\n\
                    $this->c->p = 'before-call';\n\
                    mutate($this->c);\n\
                    throw new RuntimeException();\n\
                }\n\
                return 'ok';\n\
            }\n\
        }\n\
        function check(S $s, C $c): void {\n\
            $c->p = 1;\n\
            try { strlen($s); } catch (Throwable $e) {\n\
                assertType('int|string', $c->p);\n\
            }\n\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "unsupported callback effects must remain conservatively invalidated: {diagnostics:#?}"
    );
}

#[test]
fn catalog_stringable_invocation_retains_normal_result_and_invalidates_callback_writes() {
    let src = "<?php\n\
        class C { public int $p = 0; }\n\
        final class S implements Stringable {\n\
            private C $c = new C();\n\
            private bool $throws = false;\n\
            public function __construct(C $c, bool $throws = false) { $this->c = $c; $this->throws = $throws; }\n\
            public function __toString(): string {\n\
                $this->c->p = 2;\n\
                if ($this->throws) { throw new RuntimeException(); }\n\
                return 'x';\n\
            }\n\
        }\n\
        function viaStringable(S $s, C $c): void {\n\
            $c->p = 1;\n\
            $length = strlen($s);\n\
            assertType('int<0, max>', $length);\n\
            assertType('1|2', $c->p);\n\
        }\n\
        function viaStringableCatch(S $s, C $c): void {\n\
            $c->p = 1;\n\
            try { strlen($s); }\n\
            catch (Throwable $e) { assertType('1|2', $c->p); }\n\
        }\n\
        function viaNamedAfterSpread(S $s, C $c): void {\n\
            $c->p = 1;\n\
            strlen(...[], string: $s);\n\
            assertType('1|2', $c->p);\n\
        }\n\
        function viaUnpackedStringable(S $s, C $c): void {\n\
            $c->p = 1;\n\
            strlen(...[$s]);\n\
            assertType('int', $c->p);\n\
        }\n\
        function viaLaterUnpackedStringable(S $s, C $c): void {\n\
            $c->p = 1;\n\
            strlen(...[], ...[$s]);\n\
            assertType('int', $c->p);\n\
        }\n";
    let diagnostics = diagnostics_at_php_8_5_4(src);
    let assertions = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.identifier.ends_with("assertType"))
        .collect::<Vec<_>>();
    assert_eq!(
        assertions.len(),
        0,
        "catalog result/effect transfer: {diagnostics:#?}"
    );
}
