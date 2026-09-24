//! A documented, public reflection-level extension surface.
//! `phpstan-rs` is already a library crate: `Project::registry()`/
//! `check_file` are `pub`, `Registry::classes`/`.functions` are `pub`
//! `FxHashMap`s of fully-`pub`-fielded `ClassInfo`/`FunctionSig`,
//! `Diagnostic` is a `pub struct` with every field `pub`, and
//! `ast::{Stmt, Expr, ...}` is fully public - this trait gives an
//! external crate a named entry point for exactly this kind of check,
//! rather than writing it by hand against `project.registry().classes`.
//!
//! **What this tier can see**: whatever a fully-resolved `ClassInfo`/
//! `FunctionSig` records - declared types (native and PHPDoc-merged),
//! visibility, `@template`/`@throws`/`@deprecated`/... metadata,
//! signatures. **What this tier cannot see**: any per-expression
//! *inferred* type, flow-sensitive narrowing, or per-statement `Locals`
//! state - none of that exists once a file's `Checker` pass has
//! finished, and this trait never runs one. Don't mistake this for
//! parity with real PHPStan's own `Rule<TNodeType>` (which runs
//! mid-analysis, with a live `Scope`) - it's the reflection-only half,
//! matching real PHPStan's own `ClassReflectionExtension`/
//! `ReflectionProvider`-consuming tooling more than an ordinary `Rule`.
//! Flow-sensitive, mid-analysis extension points (see
//! `checker::internal_rules`'s own doc comment) would need a real
//! `Scope` facade to expose safely, which doesn't exist yet.

use crate::diagnostic::Diagnostic;
use crate::registry::{ClassInfo, FunctionSig};

/// One reflection-level check. Default no-op bodies (unlike this
/// project's own first-party checks, which enumerate every `ClassKind`
/// exhaustively) - an external implementor shouldn't be forced to
/// override both methods just to check one kind of declaration.
pub trait ReflectionRule: Send + Sync {
    /// `fqn` is `class`'s own fully-qualified name, the same key it's
    /// stored under in `Registry::classes`.
    fn check_class(&self, class: &ClassInfo, fqn: &str) -> Vec<Diagnostic> {
        let _ = (class, fqn);
        Vec::new()
    }

    /// `fqn` is `sig`'s own fully-qualified name, the same key it's
    /// stored under in `Registry::functions` (a plain top-level
    /// function only - a method is reached through `class.methods`
    /// inside `check_class` instead, since `Registry::functions` never
    /// holds one).
    fn check_function(&self, sig: &FunctionSig, fqn: &str) -> Vec<Diagnostic> {
        let _ = (sig, fqn);
        Vec::new()
    }
}
