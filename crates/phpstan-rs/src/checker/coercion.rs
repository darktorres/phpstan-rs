//! The cast, `echo` and `print` validity rules, over `Type`'s own
//! coercions (`types::coercion`).

use crate::checker::Checker;
use diagnostics::DiagnosticBuilder;
use syntax::ast::{CastKind, CastSpelling};
use syntax::span::Span;
use types::Type;

impl<'a> Checker<'a> {
    /// Real PHPStan's `InvalidCastRule` (`cast.int`/`cast.double`/
    /// `cast.string`) - `(bool)`/`(array)`/`(object)`/`(unset)` are never
    /// checked, matching real PHPStan's own callback (only the four
    /// scalar-coercion casts it names).
    pub(crate) fn check_cast_validity(&mut self, cast: CastKind, inner_ty: &Type, span: Span) {
        let (result, identifier, target_name) = match cast {
            CastKind::Int => (inner_ty.to_integer(self.registry), "cast.int", "int"),
            CastKind::Float => (inner_ty.to_float(self.registry), "cast.double", "float"),
            CastKind::String => (
                inner_ty.to_string_type(self.registry),
                "cast.string",
                "string",
            ),
            _ => return,
        };
        if result.is_some() {
            return;
        }
        report!(
            self,
            format!("Cannot cast {inner_ty} to {target_name}."),
            span,
            2,
            identifier,
        );
    }

    /// Real PHPStan's `UnsetCastRule` (`cast.unset`) - the `(unset)`
    /// cast was removed as of PHP 8.0.
    pub(crate) fn check_unset_cast_supported(&mut self, span: Span) {
        if self
            .registry
            .php_supports_everywhere(types::php_version::PhpFeature::UnsetCastRemoved)
            != Some(false)
        {
            return;
        }
        report_builder!(
            self,
            DiagnosticBuilder::new(
                "The (unset) cast is no longer supported in PHP 8.0 and later.".to_string(),
                span,
                0,
                "cast.unset",
            )
            .non_ignorable(),
        );
    }

    /// The `(real)` cast, removed in PHP 8.0 alongside `(unset)`.
    pub(crate) fn check_real_cast_supported(&mut self, span: Span) {
        if self
            .registry
            .php_supports_everywhere(types::php_version::PhpFeature::UnsetCastRemoved)
            != Some(false)
        {
            return;
        }
        report_builder!(
            self,
            DiagnosticBuilder::new(
                "The (real) cast has been removed, use (float) instead.".to_string(),
                span,
                0,
                "cast.real",
            )
            .non_ignorable(),
        );
    }

    /// Real PHPStan's `VoidCastRule` (`cast.voidNotSupported`/
    /// `cast.void`) - the `(void)` cast needs PHP 8.5+, and
    /// (independently of version) is only ever legal as an entire,
    /// top-level statement on its own (`(void) foo();`), never nested
    /// inside a larger expression - both checks can fire together on the
    /// same occurrence.
    pub(crate) fn check_void_cast(&mut self, span: Span, is_bare_statement: bool) {
        if self
            .registry
            .php_supports_everywhere(types::php_version::PhpFeature::VoidCast)
            == Some(false)
        {
            report_builder!(
                self,
                DiagnosticBuilder::new(
                    "The (void) cast is supported only on PHP 8.5 and later.".to_string(),
                    span,
                    0,
                    "cast.voidNotSupported",
                )
                .non_ignorable(),
            );
        }
        if is_bare_statement {
            return;
        }
        report_builder!(
            self,
            DiagnosticBuilder::new(
                "The (void) cast cannot be used within an expression.".to_string(),
                span,
                0,
                "cast.void",
            )
            .non_ignorable(),
        );
    }

    /// Real PHPStan's `DeprecatedCastRule` (`cast.deprecated`) - the
    /// four non-canonical cast spellings (`(integer)`, `(boolean)`,
    /// `(double)`, `(binary)`) are deprecated as of PHP 8.5; `(real)`
    /// isn't flagged, matching real PHPStan (its rule only knows
    /// `KIND_DOUBLE`).
    pub(crate) fn check_deprecated_cast_spelling(&mut self, cast: CastSpelling, span: Span) {
        if !matches!(
            cast,
            CastSpelling::Integer
                | CastSpelling::Double
                | CastSpelling::Binary
                | CastSpelling::Boolean
        ) {
            return;
        }
        if self
            .registry
            .php_supports_somewhere(types::php_version::PhpFeature::DeprecatedNonStandardCasts)
            != Some(true)
        {
            return;
        }
        let (spelling, canonical) = (cast.keyword(), cast.kind().keyword());
        report!(
            self,
            format!(
                "Non-standard ({spelling}) cast is deprecated in PHP 8.5. Use ({canonical}) instead."
            ),
            span,
            0,
            "cast.deprecated",
        );
    }

    /// Real PHPStan's `BacktickRule` (`backtick.deprecated`, not
    /// non-ignorable) - the backtick shell-exec operator is deprecated
    /// as of PHP 8.5 in favor of a plain `shell_exec()` call.
    pub(crate) fn check_backtick_deprecation(&mut self, span: Span) {
        if self
            .registry
            .php_supports_somewhere(types::php_version::PhpFeature::DeprecatedBacktickOperator)
            != Some(true)
        {
            return;
        }
        report!(
            self,
            "Backtick operator is deprecated in PHP 8.5. Use shell_exec() function call instead."
                .to_string(),
            span,
            0,
            "backtick.deprecated",
        );
    }

    /// Real PHPStan's `EchoRule` (`echo.nonString`) - `position` is
    /// 0-based, real PHPStan's own message counts from 1.
    pub(crate) fn check_echo_validity(&mut self, expr_ty: &Type, position: usize, span: Span) {
        if expr_ty.to_string_type(self.registry).is_some() {
            return;
        }
        report!(
            self,
            format!(
                "Parameter #{} ({expr_ty}) of echo cannot be converted to string.",
                position + 1
            ),
            span,
            2,
            "echo.nonString",
        );
    }

    /// Real PHPStan's `PrintRule` (`print.nonString`) - unlike
    /// `check_echo_validity`, `print` only ever has one operand, so real
    /// PHPStan's own message has no `#%d` position number at all
    /// (`PrintRule::processNode` interpolates the type description
    /// straight into `"Parameter %s of print..."` with no separate
    /// position argument - not a typo to "fix" into echo's shape).
    pub(crate) fn check_print_validity(&mut self, expr_ty: &Type, span: Span) {
        if expr_ty.to_string_type(self.registry).is_some() {
            return;
        }
        report!(
            self,
            format!("Parameter {expr_ty} of print cannot be converted to string."),
            span,
            2,
            "print.nonString",
        );
    }
}
