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

use crate::checker::Checker;
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use syntax::span::Span;

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
    /// general "unused parameter" rule. A promoted parameter is exempt:
    /// the promotion itself is an implicit `$this->id = $id` assignment,
    /// a real use, even when the body never reads `$id` again. An attribute class, or
    /// a class implementing an interface that declares `__construct`, is
    /// exempt entirely, since its constructor signature is dictated from
    /// outside the class.
    pub(crate) fn check_unused_constructor_parameters(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
    ) {
        let Some(fqn) = class_name else {
            return;
        };
        if !f.is_constructor() || f.body.is_none() {
            return;
        }
        let arenas = self
            .syntax_arenas()
            .expect("constructor checks must retain the parsed file owner");
        let candidates: Vec<_> = arenas
            .params(f.params.clone())
            .iter()
            .filter(|p| p.promotion.is_none())
            .map(|p| (p.name.clone(), p.span))
            .collect();
        if candidates.is_empty() {
            return;
        }
        let is_anonymous = self
            .registry
            .lookup_class(fqn)
            .is_some_and(|(_, class)| class.is_anonymous());
        if self.constructor_signature_is_dictated(fqn) {
            return;
        }
        let usage = collect_var_usage(arenas, f.body_stmts(arenas));
        for (name, span) in candidates {
            if usage.contains(&name) {
                continue;
            }
            let message = if is_anonymous {
                format!(
                    "Constructor of an anonymous class has an unused parameter ${}.",
                    name
                )
            } else {
                format!(
                    "Constructor of class {fqn} has an unused parameter ${}.",
                    name
                )
            };
            report!(self, message, span, 1, "constructor.unusedParameter");
        }
    }

    /// Whether `class` is an attribute class, or implements (directly or
    /// through any ancestor) an interface that itself declares
    /// `__construct`.
    fn constructor_signature_is_dictated(&self, class: &str) -> bool {
        let Some(info) = self.registry.class_by_name(class) else {
            return false;
        };
        if info.attribute_targets().is_some() {
            return true;
        }
        self.registry.all_ancestor_names(class).iter().any(|name| {
            self.registry.class_by_name(name).is_some_and(|ancestor| {
                ancestor.kind() == ClassKind::Interface
                    && self
                        .registry
                        .method_names(name)
                        .any(|m| syntax::ast::is_constructor_name(m))
            })
        })
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
        body: &[StmtId],
        span: Span,
    ) {
        if uses.is_empty() {
            return;
        }
        let arenas = self
            .syntax_arenas()
            .expect("closure checks must retain the parsed file owner");
        let usage = collect_var_usage(arenas, body);
        for u in uses {
            if usage.contains(&u.name) {
                continue;
            }
            report!(
                self,
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
    /// `ClassDecl` (never the registry's own trait-flattened
    /// `ClassReflection`) means a trait-provided member is never a candidate
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
        let Some(owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = owner.arenas();

        let mut usage = MemberUsage::default();
        let ctor = methods.iter().find(|m| m.function.is_constructor());
        for m in methods {
            usage.aliases.clear();
            seed_self_typed_param_aliases(
                arenas,
                arenas.params(m.function.params.clone()),
                fqn,
                &mut usage,
            );
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
            usage.scan_twice(|usage| {
                walk_stmt_body_for_members(arenas, m.function.body_stmts(arenas), fqn, usage);
            });
        }
        if let Some(ctor) = ctor {
            for p in arenas.params(ctor.function.params.clone()) {
                if let Some(d) = &p.default {
                    // A property/constructor-param default must be a PHP
                    // constant expression - no assignment or `instanceof`
                    // can appear in one, so `usage.aliases` can never be
                    // populated here; cleared anyway.
                    usage.aliases.clear();
                    walk_expr_for_members(arenas, *d, fqn, &mut usage);
                }
                if let Some(hooks) = &p.hooks {
                    usage.aliases.clear();
                    usage.scan_twice(|usage| {
                        walk_property_hooks_for_members(arenas, hooks, fqn, usage);
                    });
                }
            }
        }
        for p in properties {
            if let Some(d) = &p.default {
                usage.aliases.clear();
                walk_expr_for_members(arenas, *d, fqn, &mut usage);
            }
            if let Some(hooks) = &p.hooks {
                usage.aliases.clear();
                usage.scan_twice(|usage| {
                    walk_property_hooks_for_members(arenas, hooks, fqn, usage);
                });
            }
        }
        for c in consts {
            usage.aliases.clear();
            walk_expr_for_members(arenas, c.value, fqn, &mut usage);
        }

        // See `MemberUsage::escaped`'s own doc comment - a fully dynamic
        // member access anywhere in the class means none of these three
        // checks can trust their own "never referenced" conclusion.
        if usage.escaped {
            return;
        }

        for c in consts {
            if c.visibility != Visibility::Private || usage.consts.contains(c.name.as_ref()) {
                continue;
            }
            if self.shows_level(4) {
                let mut builder = DiagnosticBuilder::new(
                    format!("private constant {fqn}::{} is unused", c.name),
                    c.span,
                    4,
                    "classConstant.unused",
                );
                if let Some(removal) = c.removal_span {
                    builder = builder.fix(removal, "", format!("Remove constant {}", c.name));
                }
                self.report_builder(builder);
            } else {
                self.note_below_level();
            }
        }

        for m in methods {
            if m.visibility != Visibility::Private
                || m.function.is_constructor()
                || m.function.name.eq_ignore_ascii_case("__clone")
                || usage
                    .methods
                    .contains(&member_usage::method_key(&m.function.name))
            {
                continue;
            }
            let kind_word = if m.is_static {
                "private static method"
            } else {
                "private method"
            };
            if self.shows_level(4) {
                let mut builder = DiagnosticBuilder::new(
                    format!("{kind_word} {fqn}::{}() is unused", m.function.name),
                    m.function.span,
                    4,
                    "method.unused",
                );
                if let Some(removal) = m.removal_span {
                    builder =
                        builder.fix(removal, "", format!("Remove method {}()", m.function.name));
                }
                self.report_builder(builder);
            } else {
                self.note_below_level();
            }
        }

        // PHP 8.4 asymmetric visibility (`public private(set)`): a
        // property's own read side can be reached from anywhere, so
        // "never read"/"unused" can never be safely concluded from a
        // same-class-only scan the way the fully-private loop below can -
        // but the write side is exactly as restricted as the fully-
        // private case, so "never written from inside the declaring
        // class" is just as sound. Hooked properties are skipped: a
        // `set` hook's own body is a different write mechanism entirely,
        // not covered by `usage.writes` here.
        for p in properties {
            if p.write_visibility != Some(Visibility::Private)
                || p.visibility == Visibility::Private
                || p.hooks.is_some()
            {
                continue;
            }
            let written = p.default.is_some() || usage.writes.contains(p.name.as_ref());
            if written {
                continue;
            }
            let kind_word = if p.is_static {
                "static property"
            } else {
                "property"
            };
            report!(
                self,
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
                p.doc_comment
                    .as_ref()
                    .and_then(|ranges| arenas.doc_comment_text(ranges))
                    .as_deref(),
                p.span,
                p.removal_span,
                &usage,
            );
        }
        if let Some(ctor) = ctor {
            let private_promotions: Vec<_> = arenas
                .params(ctor.function.params.clone())
                .iter()
                .filter(|param| {
                    param.promotion.map(|promotion| promotion.visibility)
                        == Some(Visibility::Private)
                })
                .map(|param| (param.name.clone(), param.hooks.clone(), param.span))
                .collect();
            for (name, hooks, span) in private_promotions {
                let (only_readable, only_writable) = hook_read_write_shape(hooks.as_deref());
                // Promotion is itself an implicit `$this->x = $x` write -
                // see `check_unused_constructor_parameters`'s own doc
                // comment for the same reasoning applied to the parameter
                // side of this same construct.
                self.report_unused_property(
                    fqn,
                    &name,
                    false,
                    true,
                    only_readable,
                    only_writable,
                    None,
                    span,
                    None,
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
        removal: Option<Span>,
        usage: &MemberUsage,
    ) {
        let doc = doc_comment.map(phpdoc::docblock::parse_shared);
        let tag_always_read = doc.as_ref().is_some_and(|d| d.always_read);
        let tag_always_written = doc.as_ref().is_some_and(|d| d.always_written);

        let read = tag_always_read || usage.reads.contains(name);
        let written = tag_always_written || seed_written || usage.writes.contains(name);

        let kind_word = if is_static {
            "static property"
        } else {
            "property"
        };
        if !self.shows_level(4) {
            self.note_below_level();
            return;
        }
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
        let mut builder = DiagnosticBuilder::new(message, span, 4, identifier);
        if identifier == "property.unused"
            && let Some(removal) = removal
        {
            builder = builder.fix(removal, "", format!("Remove property ${name}"));
        }
        self.report_builder(builder);
    }
}
