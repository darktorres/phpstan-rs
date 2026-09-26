//! What a check reports: a [`Diagnostic`], assembled through
//! [`DiagnosticBuilder`], with an optional textual [`DiagnosticFix`].

use syntax::token::Span;

/// Loosely mirrors PHPStan's own rule-gating levels (0 lowest/laxest,
/// per `conf/config.levelN.neon`): 0 for the
/// "definitely broken" undefined-symbol/arity family (matches real
/// PHPStan's own level-0 bucket - "unknown classes, unknown functions,
/// unknown methods..., wrong number of arguments..., always undefined
/// variables" - though this project doesn't implement PHPStan's own
/// $this-only-vs-any-expression distinction for method/property checks,
/// a documented simplification, not an oversight), 1 for unused-
/// constructor-parameter/unused-closure-use checks (matches real
/// PHPStan's `UnusedConstructorParametersRule`/`UnusedClosureUsesRule`
/// level exactly), 2 for undefined-property/array-shape-key (PHPDoc-
/// derived structural checks, real PHPStan's `checkThisOnly: false`/
/// `checkPhpDocMissingReturn` territory), 3 for return-type/property-type
/// mismatches (matches real PHPStan's `checkPhpDocMethodSignatures` level),
/// 4 for unused-private-const/method/property checks (again matching real
/// PHPStan's own level), 5 for argument-type mismatches (matches real
/// PHPStan's `checkFunctionArgumentTypes` level), and 6 for this project's
/// own generic bound/variance checks (no direct real-PHPStan equivalent -
/// generics are baked into its core type engine, not level-gated - placed
/// here as this project's own "advanced, opt-in" tier, alongside real
/// PHPStan's missing-typehints level).
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub message: String,
    pub span: Span,
    pub level: u8,
    /// A dot-separated, PHPStan-style machine identifier for this
    /// diagnostic's *kind* (e.g. `"argument.type"`, `"class.notFound"`,
    /// `"return.type"`) - stable across occurrences of the same underlying
    /// problem, unlike `message`, which embeds the specific types/names
    /// involved. Not guaranteed to match phpstan/phpstan's own strings
    /// one-for-one, though it reuses real PHPStan's spelling where
    /// confident.
    pub identifier: String,
    /// An optional one-line, human-facing hint appended below the main
    /// message - real PHPStan's own "💡 Tip: ..." line.
    pub tip: Option<String>,
    /// Mirrors real PHPStan's `RuleErrorBuilder::nonIgnorable()`: when
    /// `true`, this diagnostic must never be suppressible via
    /// `@phpstan-ignore <identifier>`.
    pub non_ignorable: bool,
    /// A suggested textual auto-fix. This project has no code-generation-
    /// from-AST facility, so a fix is a literal source-text replacement
    /// over a `Span` rather than a replacement AST node.
    pub fix: Option<DiagnosticFix>,
}

/// The total order every consumer presents diagnostics in within a file:
/// position, then identifier, message and tip, so diagnostics sharing a
/// position don't come out in whatever order the checks happened to run.
pub fn diagnostic_order(a: &Diagnostic, b: &Diagnostic) -> std::cmp::Ordering {
    (
        a.span.start.line,
        a.span.start.col,
        &a.identifier,
        &a.message,
        &a.tip,
    )
        .cmp(&(
            b.span.start.line,
            b.span.start.col,
            &b.identifier,
            &b.message,
            &b.tip,
        ))
}

/// Sorts one file's diagnostics into `diagnostic_order`.
pub fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(diagnostic_order);
}

/// A single suggested textual replacement: replace whatever source text
/// `span` covers with `replacement` - or, with `within`, only the first
/// whole-word occurrence of that text inside `span` (for a name whose own
/// span isn't known, resolved against the source by `fix::resolve_span`).
/// `description` is a short human label for the fix itself (real
/// PHPStan's own fixer CLI output shows one per applied fix, e.g. "Add
/// missing return statement").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticFix {
    pub span: Span,
    pub replacement: String,
    pub description: String,
    pub within: Option<String>,
}

/// Fluent assembly for a [`Diagnostic`], mirroring real PHPStan's own
/// `RuleErrorBuilder`. A plain diagnostic with no tip/fix still
/// goes through this via `Checker::report` (a thin wrapper), so there's one
/// real construction path for every `Diagnostic`.
///
/// Consumed by value (`build(self)`, each setter takes/returns `Self`): a
/// `DiagnosticBuilder` is always a short-lived expression built and
/// immediately consumed at one `report_builder` call site, never stored or
/// branched on mid-construction.
pub struct DiagnosticBuilder {
    message: String,
    span: Span,
    level: u8,
    identifier: String,
    tip: Option<String>,
    non_ignorable: bool,
    fix: Option<DiagnosticFix>,
}

impl DiagnosticBuilder {
    /// `identifier` is required up front, unlike real PHPStan's builder
    /// (which allows a later `.identifier(...)` call) - every call site
    /// here already knows its identifier, so there's no "forgot to set it"
    /// state worth representing.
    pub fn new(
        message: impl Into<String>,
        span: Span,
        level: u8,
        identifier: impl Into<String>,
    ) -> Self {
        DiagnosticBuilder {
            message: message.into(),
            span,
            level,
            identifier: identifier.into(),
            tip: None,
            non_ignorable: false,
            fix: None,
        }
    }

    pub fn tip(mut self, tip: impl Into<String>) -> Self {
        self.tip = Some(tip.into());
        self
    }

    pub fn non_ignorable(mut self) -> Self {
        self.non_ignorable = true;
        self
    }

    pub fn fix(
        mut self,
        span: Span,
        replacement: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.fix = Some(DiagnosticFix {
            span,
            replacement: replacement.into(),
            description: description.into(),
            within: None,
        });
        self
    }

    /// `fix` replacing only the first whole-word occurrence of `text`
    /// inside `span`.
    pub fn fix_within(
        mut self,
        span: Span,
        text: impl Into<String>,
        replacement: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.fix = Some(DiagnosticFix {
            span,
            replacement: replacement.into(),
            description: description.into(),
            within: Some(text.into()),
        });
        self
    }

    pub fn build(self) -> Diagnostic {
        Diagnostic {
            message: self.message,
            span: self.span,
            level: self.level,
            identifier: self.identifier,
            tip: self.tip,
            non_ignorable: self.non_ignorable,
            fix: self.fix,
        }
    }
}
