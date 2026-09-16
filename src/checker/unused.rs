//! The unused-symbol detection subsystem - one variable/member-usage
//! walker, applied at four call sites (constructor parameters, closure
//! `use(...)` captures, and private class consts/methods/properties) to
//! back five diagnostics: constructor-parameter, closure-use, private
//! constant, private method, and private property (this last one with
//! five distinct read/write message shapes).
//!
//! Scoped to only `__construct` parameters, never an arbitrary
//! function/method's - a general "this parameter is never read" rule is
//! far noisier in real code (an interface implementation's parameter, an
//! overridden method's parameter, an `array_map`/`usort` callback's
//! parameter, ... all routinely go unused without being a bug).

use crate::ast::*;
use crate::checker::Checker;
use crate::token::Span;

mod member_usage;
mod rebind;
mod var_usage;

use member_usage::{
    MemberUsage, hook_read_write_shape, seed_self_typed_param_aliases, walk_expr_for_members,
    walk_property_hooks_for_members, walk_stmt_body_for_members,
};
pub(crate) use rebind::scan_program_for_rebind_targets;
use var_usage::collect_var_usage;

impl<'a> Checker<'a> {
    /// Flags a `__construct` parameter never referenced anywhere in the
    /// constructor's own body - scoped to constructors only, not a
    /// general "unused parameter" rule. Constructor property promotion
    /// (`private int $id`) is exempt: the promotion itself is an implicit
    /// `$this->id = $id` assignment, a real use, even when the body never
    /// reads `$id` again.
    pub(crate) fn check_unused_constructor_parameters(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        label: &str,
    ) {
        let Some(fqn) = class_name else {
            return;
        };
        if f.name != "__construct" || f.body.is_empty() {
            return;
        }
        let candidates: Vec<&Param> = f
            .params
            .iter()
            .filter(|p| p.promoted_visibility.is_none())
            .collect();
        if candidates.is_empty() {
            return;
        }
        let usage = collect_var_usage(&f.body);
        for p in candidates {
            if usage.contains(&p.name) {
                continue;
            }
            self.report(
                label.to_string(),
                format!("constructor of {fqn} has an unused parameter ${}", p.name),
                p.span,
                1,
                "parameter.unused",
            );
        }
    }

    /// Flags a closure `use ($x)`/`use (&$x)` capture never referenced
    /// anywhere in the closure's own body. Applies to every closure
    /// regardless of where it appears: unlike constructor parameters, an
    /// explicit `use(...)` clause is always the closure author's own
    /// deliberate choice, never dictated by an interface/override
    /// signature, so this carries none of the false-positive risk a
    /// general unused-parameter check would.
    pub(crate) fn check_unused_closure_uses(
        &mut self,
        uses: &[ClosureUse],
        body: &[Stmt],
        label: &str,
        span: Span,
    ) {
        if uses.is_empty() {
            return;
        }
        let usage = collect_var_usage(body);
        for u in uses {
            if usage.contains(&u.name) {
                continue;
            }
            self.report(
                label.to_string(),
                format!("closure has an unused use (${})", u.name),
                span,
                1,
                "closure.unusedUse",
            );
        }
    }

    /// Flags a private const/method/property never referenced anywhere in
    /// this class's own declared bodies. Scanning only
    /// `methods`/`properties`/`consts` exactly as declared on this one
    /// `ClassDecl`/`EnumDecl` (never the registry's own trait-flattened
    /// `ClassInfo`) means a trait-provided member is never a candidate
    /// here at all.
    pub(crate) fn check_unused_private_members(
        &mut self,
        fqn: &str,
        kind: ClassKind,
        methods: &[MethodDecl],
        properties: &[PropertyDecl],
        consts: &[ClassConst],
    ) {
        if kind == ClassKind::Trait || kind == ClassKind::Interface {
            return;
        }
        // See `rebind_target_classes`'s own doc comment - a class reached
        // by a `Closure::bind`/`->bindTo` call from outside its own body
        // can have its private members referenced from a closure this
        // per-class scan below never sees.
        if self.rebind_escape_all || self.rebind_target_classes.contains(fqn) {
            return;
        }

        let mut usage = MemberUsage::default();
        let ctor = methods.iter().find(|m| m.function.name == "__construct");
        for m in methods {
            usage.aliases.clear();
            seed_self_typed_param_aliases(&m.function.params, fqn, &self.name_ctx, &mut usage);
            // Two passes over the same body, same accumulator: pass 1
            // fully populates `usage.aliases` (detecting a same-class
            // `new self()` assignment or `instanceof self` check doesn't
            // depend on any alias already being known, so one top-to-
            // bottom pass finds all of them regardless of order); pass 2
            // (now with complete alias knowledge from the start) correctly
            // attributes every aliased-variable read/write/call even when
            // it textually precedes the statement that established the
            // alias. HashSet inserts are idempotent, so the redundant work
            // in pass 1's own read/write/method accounting is harmless.
            walk_stmt_body_for_members(&m.function.body, fqn, &self.name_ctx, &mut usage);
            walk_stmt_body_for_members(&m.function.body, fqn, &self.name_ctx, &mut usage);
        }
        if let Some(ctor) = ctor {
            for p in &ctor.function.params {
                if let Some(d) = &p.default {
                    // A property/constructor-param default must be a PHP
                    // constant expression - no assignment or `instanceof`
                    // can appear in one, so `usage.aliases` can never be
                    // populated here; cleared anyway.
                    usage.aliases.clear();
                    walk_expr_for_members(d, fqn, &self.name_ctx, &mut usage);
                }
                if let Some(hooks) = &p.hooks {
                    usage.aliases.clear();
                    walk_property_hooks_for_members(hooks, fqn, &self.name_ctx, &mut usage);
                    walk_property_hooks_for_members(hooks, fqn, &self.name_ctx, &mut usage);
                }
            }
        }
        for p in properties {
            if let Some(d) = &p.default {
                usage.aliases.clear();
                walk_expr_for_members(d, fqn, &self.name_ctx, &mut usage);
            }
            if let Some(hooks) = &p.hooks {
                usage.aliases.clear();
                walk_property_hooks_for_members(hooks, fqn, &self.name_ctx, &mut usage);
                walk_property_hooks_for_members(hooks, fqn, &self.name_ctx, &mut usage);
            }
        }
        for c in consts {
            usage.aliases.clear();
            walk_expr_for_members(&c.value, fqn, &self.name_ctx, &mut usage);
        }

        // See `MemberUsage::escaped`'s own doc comment - a fully dynamic
        // member access anywhere in the class means none of these three
        // checks can trust their own "never referenced" conclusion.
        if usage.escaped {
            return;
        }

        for c in consts {
            if c.visibility != Visibility::Private || usage.consts.contains(&c.name) {
                continue;
            }
            self.report(
                fqn.to_string(),
                format!("private constant {fqn}::{} is unused", c.name),
                c.span,
                4,
                "classConstant.unused",
            );
        }

        for m in methods {
            if m.visibility != Visibility::Private
                || m.function.name == "__construct"
                || m.function.name.eq_ignore_ascii_case("__clone")
                || usage.methods.contains(&m.function.name)
            {
                continue;
            }
            let kind_word = if m.is_static {
                "private static method"
            } else {
                "private method"
            };
            self.report(
                fqn.to_string(),
                format!("{kind_word} {fqn}::{}() is unused", m.function.name),
                m.function.span,
                4,
                "method.unused",
            );
        }

        // PHP 8.4 asymmetric visibility (`public private(set)`): a
        // property's own read side can be reached from anywhere, so
        // "never read"/"unused" can never be safely concluded from a
        // same-class-only scan the way the fully-private loop below can -
        // but the write side is exactly as restricted as the fully-
        // private case, so "never written from inside the declaring
        // class" is just as sound. Hooked properties are skipped: a
        // `set` hook's own body is a different write mechanism entirely,
        // not covered by `usage.writes` here. See PHASE_BACKLOG Phase
        // 98's AB2.
        for p in properties {
            if p.write_visibility != Some(Visibility::Private)
                || p.visibility == Visibility::Private
                || p.hooks.is_some()
            {
                continue;
            }
            let written = p.default.is_some() || usage.writes.contains(&p.name);
            if written {
                continue;
            }
            let kind_word = if p.is_static {
                "static property"
            } else {
                "property"
            };
            self.report(
                fqn.to_string(),
                format!(
                    "{kind_word} {fqn}::${} has private(set) visibility but is never written from within the declaring class",
                    p.name
                ),
                p.span,
                4,
                "property.neverWritten",
            );
        }

        for p in properties {
            if p.visibility != Visibility::Private {
                continue;
            }
            let (only_readable, only_writable) = hook_read_write_shape(p.hooks.as_ref());
            self.report_unused_property(
                fqn,
                &p.name,
                p.is_static,
                p.default.is_some(),
                only_readable,
                only_writable,
                p.doc_comment.as_deref(),
                p.span,
                &usage,
            );
        }
        if let Some(ctor) = ctor {
            for param in &ctor.function.params {
                if param.promoted_visibility != Some(Visibility::Private) {
                    continue;
                }
                let (only_readable, only_writable) = hook_read_write_shape(param.hooks.as_deref());
                // Promotion is itself an implicit `$this->x = $x` write -
                // see `check_unused_constructor_parameters`'s own doc
                // comment for the same reasoning applied to the parameter
                // side of this same construct.
                self.report_unused_property(
                    fqn,
                    &param.name,
                    false,
                    true,
                    only_readable,
                    only_writable,
                    None,
                    param.span,
                    &usage,
                );
            }
        }
    }

    /// The shared read/write-state-to-message logic behind the private-
    /// property check's five distinct shapes. `@phpstan-always-read`/
    /// `@phpstan-always-written` (checked directly against the raw
    /// doc-comment text) let a property genuinely read/written only
    /// through reflection, serialization, or a magic method suppress a
    /// false report.
    #[allow(clippy::too_many_arguments)]
    fn report_unused_property(
        &mut self,
        fqn: &str,
        name: &str,
        is_static: bool,
        seed_written: bool,
        only_readable: bool,
        only_writable: bool,
        doc_comment: Option<&str>,
        span: Span,
        usage: &MemberUsage,
    ) {
        let tag_always_read = doc_comment.is_some_and(|d| d.contains("@phpstan-always-read"));
        let tag_always_written = doc_comment.is_some_and(|d| d.contains("@phpstan-always-written"));

        let read = tag_always_read || usage.reads.contains(name);
        let written = tag_always_written || seed_written || usage.writes.contains(name);

        let kind_word = if is_static {
            "static property"
        } else {
            "property"
        };
        let (message, identifier) = if !read {
            if !written {
                (
                    format!("private {kind_word} {fqn}::${name} is unused"),
                    "property.unused",
                )
            } else if only_readable {
                (
                    format!("private read-only {kind_word} {fqn}::${name} is never read"),
                    "property.neverRead",
                )
            } else {
                (
                    format!("private {kind_word} {fqn}::${name} is never read, only written"),
                    "property.onlyWritten",
                )
            }
        } else if !written {
            if only_writable {
                (
                    format!("private write-only {kind_word} {fqn}::${name} is never written"),
                    "property.neverWritten",
                )
            } else {
                (
                    format!("private {kind_word} {fqn}::${name} is never written, only read"),
                    "property.onlyRead",
                )
            }
        } else {
            return;
        };
        self.report(fqn.to_string(), message, span, 4, identifier);
    }
}
