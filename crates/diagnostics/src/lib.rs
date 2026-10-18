//! What a check reports: a [`Diagnostic`], assembled through
//! [`DiagnosticBuilder`], with an optional textual [`DiagnosticFix`]
//! (applied by [`fix`]), and how reports and messages spell a file's path
//! and configured path text names one ([`report_path`]).

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]

pub mod fix;
pub mod report_path;

pub use report_path::{path_byte_names, path_text_names, report_path, slash_spelling};

use std::borrow::Cow;

use syntax::span::{LineIndex, Range, Span};

/// A diagnostic kind's machine identifier: dot-separated ASCII
/// alphanumeric segments (`argument.type`, `class.notFound`), the syntax
/// PHPStan identifiers and `@phpstan-ignore` share.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Identifier(Cow<'static, str>);

impl Identifier {
    /// Whether `text` is a well-formed identifier.
    pub fn is_valid(text: &str) -> bool {
        !text.is_empty()
            && text.split('.').all(|segment| {
                !segment.is_empty() && segment.bytes().all(|b| b.is_ascii_alphanumeric())
            })
    }

    fn checked(text: Cow<'static, str>) -> Identifier {
        debug_assert!(Identifier::is_valid(&text), "malformed identifier {text:?}");
        Identifier(text)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&'static str> for Identifier {
    fn from(text: &'static str) -> Identifier {
        Identifier::checked(Cow::Borrowed(text))
    }
}

impl From<String> for Identifier {
    fn from(text: String) -> Identifier {
        Identifier::checked(Cow::Owned(text))
    }
}

impl std::ops::Deref for Identifier {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Identifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for Identifier {
    fn eq(&self, other: &str) -> bool {
        *self.0 == *other
    }
}

impl PartialEq<&str> for Identifier {
    fn eq(&self, other: &&str) -> bool {
        *self.0 == **other
    }
}

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
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub message: String,
    pub span: Range,
    pub level: u8,
    /// A dot-separated, PHPStan-style machine identifier for this
    /// diagnostic's *kind* (e.g. `"argument.type"`, `"class.notFound"`,
    /// `"return.type"`) - stable across occurrences of the same underlying
    /// problem, unlike `message`, which embeds the specific types/names
    /// involved. Not guaranteed to match phpstan/phpstan's own strings
    /// one-for-one, though it reuses real PHPStan's spelling where
    /// confident.
    pub identifier: Identifier,
    /// Human-facing hints shown below the main message - real PHPStan's
    /// "💡 Tip: ..." lines. See `tip_text` for how several are shown.
    pub tips: Vec<String>,
    /// Mirrors real PHPStan's `RuleErrorBuilder::nonIgnorable()`: when
    /// `true`, this diagnostic must never be suppressible via
    /// `@phpstan-ignore <identifier>`.
    pub non_ignorable: bool,
    /// A suggested textual auto-fix. This project has no code-generation-
    /// from-AST facility, so a fix is a literal source-text replacement
    /// over a `Span` rather than a replacement AST node.
    pub fix: Option<DiagnosticFix>,
}

impl Diagnostic {
    /// The tips as one text: a single tip as written, several as a
    /// bulleted list, the way PHPStan shows them. `None` without tips.
    pub fn tip_text(&self) -> Option<String> {
        match self.tips.as_slice() {
            [] => None,
            [tip] => Some(tip.clone()),
            tips => Some(
                tips.iter()
                    .map(|tip| format!("• {tip}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        }
    }
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
        &a.tips,
    )
        .cmp(&(
            b.span.start.line,
            b.span.start.col,
            &b.identifier,
            &b.message,
            &b.tips,
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
    identifier: Identifier,
    tips: Vec<String>,
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
        identifier: impl Into<Identifier>,
    ) -> Self {
        DiagnosticBuilder {
            message: message.into(),
            span,
            level,
            identifier: identifier.into(),
            tips: Vec::new(),
            non_ignorable: false,
            fix: None,
        }
    }

    pub fn tip(mut self, tip: impl Into<String>) -> Self {
        self.tips.push(tip.into());
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

    /// The diagnostic, at the lines and columns the span has in the file
    /// whose lines `index` holds.
    pub fn build(self, index: &LineIndex) -> Diagnostic {
        let range = index.range(self.span);
        self.build_at(range)
    }

    /// The diagnostic at `range`, for one whose lines and columns are
    /// known and not an offset of a file.
    pub fn build_at(self, range: Range) -> Diagnostic {
        Diagnostic {
            message: self.message,
            span: range,
            level: self.level,
            identifier: self.identifier,
            tips: self.tips,
            non_ignorable: self.non_ignorable,
            fix: self.fix,
        }
    }
}

#[cfg(test)]
mod tests;
