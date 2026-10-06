//! Static-call legality beyond method existence, mirroring real PHPStan's
//! `StaticMethodCallCheck`: `self::`/`static::`/`parent::` outside a
//! class, `parent::` without a parent, a static call on a value that can
//! never name a class (`staticMethod.nonObject`), an instance method
//! called statically from a context with no compatible `$this`
//! (`method.staticCall`), and a call to an abstract method
//! (`method.callToAbstract`/`staticMethod.callToAbstract`).

use crate::checker::{Checker, Locals};
use syntax::ast::StaticTarget;
use syntax::span::Span;

impl<'a> Checker<'a> {
    /// For a static call whose target resolved to no class: the
    /// pseudo-name checks, or `staticMethod.nonObject` for a dynamic
    /// target.
    pub(crate) fn check_unresolved_static_call_target(
        &mut self,
        target: &StaticTarget,
        method: &str,
        locals: &Locals,
        span: Span,
    ) {
        match target {
            StaticTarget::Name(name) => {
                let keyword = name.to_ascii_lowercase();
                if !matches!(keyword.as_str(), "self" | "static" | "parent") {
                    return;
                }
                let Some(class) = self.current_class.clone() else {
                    self.report(
                        format!("calling {name}::{method}() outside of class scope"),
                        span,
                        0,
                        format!("outOfClass.{keyword}"),
                    );
                    return;
                };
                let has_parent = self
                    .registry
                    .classes()
                    .get(class.as_str())
                    .is_none_or(|info| !info.extends().is_empty());
                if keyword == "parent" && !has_parent {
                    let current = self.current_method.clone().unwrap_or_default();
                    self.report(format!(
                            "{class}::{current}() calls parent::{method}() but {class} does not extend any class"
                        ),
                        span,
                        0,
                        "class.noParent",
                    );
                }
            }
            StaticTarget::Expr(receiver) => {
                let ty = self.infer_expr(receiver, locals);
                let Some(level) =
                    self.failing_level(&ty, 0, |m| self.relation().can_name_a_class(m).no())
                else {
                    return;
                };
                self.report(
                    format!("cannot call static method {method}() on {ty}"),
                    span,
                    level,
                    "staticMethod.nonObject",
                );
            }
        }
    }

    /// For a static call resolved to `owner::method` through `class`:
    /// an instance method called with no compatible `$this` in scope
    /// (`method.staticCall`), and an abstract method called through a
    /// literal class name (never `static::`, which dispatches to a
    /// concrete subclass at runtime).
    pub(crate) fn check_static_call_context(
        &mut self,
        target: &StaticTarget,
        class: &str,
        owner: &str,
        method: &str,
        span: Span,
    ) {
        let Some(owner_info) = self.registry.classes().get(owner) else {
            return;
        };
        let canonical = self
            .registry
            .lookup_method_canonical_name(owner, method)
            .unwrap_or(method)
            .to_string();
        let is_static = self.registry.method_is_static(owner, canonical.as_str());
        let is_abstract = self.registry.method_is_abstract(owner, canonical.as_str());
        let owner_is_interface = owner_info.kind() == syntax::ast::ClassKind::Interface;
        if !is_static && !self.static_call_has_compatible_this(class) {
            let dom_load_html = canonical.eq_ignore_ascii_case("loadHTML")
                && owner.eq_ignore_ascii_case("DOMDocument");
            if !dom_load_html {
                self.report(
                    format!("static call to instance method {owner}::{canonical}()"),
                    span,
                    0,
                    "method.staticCall",
                );
                return;
            }
        }
        let through_static = target
            .as_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("static"));
        if (is_abstract || owner_is_interface) && target.as_name().is_some() && !through_static {
            let (word, identifier) = if is_static {
                (" static", "staticMethod.callToAbstract")
            } else {
                ("", "method.callToAbstract")
            };
            self.report(
                format!("cannot call abstract{word} method {owner}::{canonical}()"),
                span,
                0,
                identifier,
            );
        }
    }

    /// Whether the current scope is a non-static method of `class` or a
    /// subclass of it, the one context where `Foo::instanceMethod()`
    /// forwards `$this`.
    fn static_call_has_compatible_this(&self, class: &str) -> bool {
        let (Some(current_class), Some(current_method)) = (
            self.current_class.as_deref(),
            self.current_method.as_deref(),
        ) else {
            return false;
        };
        let in_static_method = self
            .registry
            .lookup_method_with_owner(current_class, current_method)
            .is_some_and(|(owner, _)| self.registry.method_is_static(&owner, current_method));
        let class = self
            .registry
            .lookup_class(class)
            .map_or(class, |(canonical, _)| canonical);
        !in_static_method && self.registry.is_subclass_of(current_class, class)
    }
}
