//! A real, flow-sensitive, per-node-kind extension surface - Axis 3 of
//! Phase 18's own 3-axis extension/plugin API (Axis 1: declarative
//! `restricted_symbols`; Axis 2: `reflection_rule::ReflectionRule`, a
//! reflection-only tier with no live `Scope` - see that module's own
//! doc comment contrasting the two). Mirrors real PHPStan's own
//! `Rule<TNodeType>`/`Scope`: an implementor declares which `ExprKind`/
//! `StmtKind` variants it cares about (`wants_expr`/`wants_stmt`,
//! real PHPStan's `getNodeType()`), and only fires for those, with a
//! live [`Scope`] into the exact flow-sensitive state at that node -
//! not reflection data computed after the fact.
//!
//! **Compile-time-registered, not dynamically loaded.** There is no
//! stable Rust ABI for loading a separately-compiled `dlopen`/`cdylib`
//! plugin without a large separate FFI project - still genuinely
//! blocked, unchanged by this axis landing. The realistic deliverable,
//! and what this module actually is: an embedder depending on
//! `phpstan-rs` as a library crate writes an ordinary Rust type
//! implementing [`Rule`], builds a `Vec<Box<dyn Rule>>` of every rule
//! they want enabled, and passes it to [`crate::checker::Checker::
//! set_external_rules`] - the same "pass a `Vec` at the call site,
//! no project-wide registration file" shape `reflection_rule::
//! ReflectionRule`'s own `Project::run_reflection_rules` already
//! established for Axis 2, deliberately not a TOML-driven analogue of
//! real PHPStan's NEON service-registration syntax (no primitive in a
//! Rust binary corresponds to "name a PHP class for a DI container to
//! instantiate", and this project's own config format is TOML by
//! design regardless).
//!
//! **What [`Scope`] exposes, and what it deliberately doesn't.**
//! `Checker`'s own flow-sensitive state spans ten `Locals`-scoped
//! fields (`checker.rs`'s own `Locals` struct) plus `current_class`/
//! `current_method`. This facade exposes the ones a general-purpose
//! third-party rule plausibly needs, each a real, disclosed decision:
//! - `label`, `current_class`, `current_method` - a rule's own
//!   "where am I" identity, exactly what every first-party check
//!   already threads through as `label` for its own diagnostics.
//! - `local_type` - the single most useful piece: a variable's own
//!   currently-inferred type on this exact control-flow path.
//! - `is_function_guarded`/`is_method_guarded`/`is_property_guarded` -
//!   `function_exists()`/`method_exists()`/`property_exists()`-guarded
//!   facts, exposed as boolean queries (not the raw `GuardedFunctionSet`/
//!   `GuardedMemberSet` types) so this facade's own shape stays stable
//!   even if their internal representation changes.
//! - **Omitted, disclosed**: `provenance` (blocked on Phase 84's own
//!   not-yet-built explicit/implicit-`Mixed` provenance semantics,
//!   which would likely reshape this field before it's worth a public
//!   contract), `condition_aliases`/`branch_implied_facts`/
//!   `query_builder_aliases`/`container_field_facts`/
//!   `discriminant_array_facts` (internal narrowing-mechanism
//!   bookkeeping specific to how *this* checker's own narrowing engine
//!   works, not a general-purpose fact a third-party rule needs - a
//!   rule that wants "what type is this expression" already gets that
//!   from `local_type`/the `ty` parameter `check_expr` already passes
//!   it).

use crate::checker::{Checker, Locals};
use crate::diagnostic::Diagnostic;
use crate::types::Type;
use syntax::ast::{Expr, ExprKind, Stmt, StmtKind};

/// One flow-sensitive, per-node-kind rule - see this module's own doc
/// comment. Default no-op bodies, matching `reflection_rule::
/// ReflectionRule`'s own convention: an implementor overrides only the
/// `wants_*`/`check_*` pair for the node kind it actually cares about.
pub trait Rule: Send + Sync {
    /// Whether this rule should fire for an expression of this kind -
    /// real PHPStan's own `Rule::getNodeType()`, translated to a
    /// predicate over this project's own `ExprKind` rather than a
    /// single class-string, since one Rust type can plausibly want more
    /// than one variant (real PHPStan's own `Rule` is generic over
    /// exactly one `TNodeType`). Checked *before* `check_expr` is ever
    /// called, so a rule that returns `false` here pays nothing beyond
    /// this one match.
    fn wants_expr(&self, kind: &ExprKind) -> bool {
        let _ = kind;
        false
    }

    /// The real check, for an expression `wants_expr` returned `true`
    /// for. `ty` is this exact expression's own already-inferred
    /// `Type` - the same value `Checker::check_expr` itself just
    /// computed, handed over so a rule never needs its own duplicate
    /// inference pass.
    fn check_expr(&self, scope: &Scope, expr: &Expr, ty: &Type) -> Vec<Diagnostic> {
        let _ = (scope, expr, ty);
        Vec::new()
    }

    /// See `wants_expr`'s own doc comment - the statement-level sibling.
    fn wants_stmt(&self, kind: &StmtKind) -> bool {
        let _ = kind;
        false
    }

    /// See `check_expr`'s own doc comment - the statement-level sibling.
    /// No `ty` parameter: a statement has no type of its own.
    fn check_stmt(&self, scope: &Scope, stmt: &Stmt) -> Vec<Diagnostic> {
        let _ = (scope, stmt);
        Vec::new()
    }
}

/// A live, read-only view into the exact flow-sensitive state at one
/// node during a real `Checker::check_program` pass - see this module's
/// own doc comment for exactly what's exposed and why. Never outlives
/// the single `check_expr`/`check_stmt` dispatch call that constructs
/// it; a `Rule` implementation has no way to store one.
pub struct Scope<'s, 'r> {
    checker: &'s Checker<'r>,
    locals: &'s Locals,
    label: &'s str,
}

impl<'s, 'r> Scope<'s, 'r> {
    pub(crate) fn new(checker: &'s Checker<'r>, locals: &'s Locals, label: &'s str) -> Self {
        Scope {
            checker,
            locals,
            label,
        }
    }

    /// This node's own enclosing declaration, human-readable
    /// (`"Foo::bar"`, or a bare function/file-level name) - exactly the
    /// string every first-party check already uses as a diagnostic's
    /// own `location`.
    pub fn label(&self) -> &str {
        self.label
    }

    /// The class whose method body this node is inside, if any.
    pub fn current_class(&self) -> Option<&str> {
        self.checker.current_class.as_deref()
    }

    /// The method whose body this node is inside, if any (`None` for a
    /// top-level function or file-level code - see `label` for that
    /// case's own identity instead).
    pub fn current_method(&self) -> Option<&str> {
        self.checker.current_method.as_deref()
    }

    /// `$name`'s own currently-inferred type on this exact control-flow
    /// path, if this checker has ever bound it - `None` for a variable
    /// this checker hasn't seen assigned/narrowed at all (real PHPStan's
    /// own `Scope::hasVariableType()->no()` case), not the same as
    /// `Some(Type::Mixed(false))`.
    pub fn local_type(&self, name: &str) -> Option<&Type> {
        self.locals.get(name)
    }

    /// Whether `function_exists($name)` (case-insensitively) has proven
    /// true on this exact control-flow path.
    pub fn is_function_guarded(&self, name: &str) -> bool {
        self.locals
            .guarded_functions
            .contains(&name.to_ascii_lowercase())
    }

    /// Whether `method_exists($receiver_var, $method)` has proven true
    /// on this exact control-flow path - `receiver_var` is the bare
    /// variable name (`"x"` for `$x`), matching how this checker's own
    /// narrowing engine keys this fact.
    pub fn is_method_guarded(&self, receiver_var: &str, method: &str) -> bool {
        self.locals
            .guarded_methods
            .contains(&(receiver_var.to_string(), method.to_string()))
    }

    /// See `is_method_guarded`'s own doc comment - the
    /// `property_exists()` sibling.
    pub fn is_property_guarded(&self, receiver_var: &str, property: &str) -> bool {
        self.locals
            .guarded_properties
            .contains(&(receiver_var.to_string(), property.to_string()))
    }
}

#[cfg(test)]
mod tests;
