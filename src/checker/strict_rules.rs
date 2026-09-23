//! Opt-in ports of `phpstan-strict-rules`, one registry flag per family:
//!
//! - `check_illegal_constructor_calls` - `$obj->__construct()`, and a
//!   static `X::__construct()` anywhere but a constructor calling one of
//!   its own parent classes' (`constructor.call`).

use crate::ast::*;
use crate::checker::{Checker, Locals};

impl<'a> Checker<'a> {
    /// The expression-level ports, before `e` itself is checked.
    pub(crate) fn check_strict_rules_expression(
        &mut self,
        e: &Expr,
        _locals: &Locals,
        label: &str,
    ) {
        if self.registry.check_illegal_constructor_calls {
            match &e.kind {
                ExprKind::MethodCall(_, method, _, _)
                    if method.eq_ignore_ascii_case("__construct") =>
                {
                    self.report(
                        label.to_string(),
                        "Call to __construct() on an existing object is not allowed.".to_string(),
                        e.span,
                        0,
                        "constructor.call",
                    );
                }
                ExprKind::StaticCall(target, method, _)
                    if method.eq_ignore_ascii_case("__construct")
                        && !self.is_parent_constructor_call(target) =>
                {
                    self.report(
                        label.to_string(),
                        "Static call to __construct() is only allowed on a parent class in the constructor.".to_string(),
                        e.span,
                        0,
                        "constructor.call",
                    );
                }
                _ => {}
            }
        }
    }

    /// Whether `target::__construct()` runs a parent class's constructor
    /// from inside a constructor, or from a trait method some class
    /// imports as its constructor.
    fn is_parent_constructor_call(&self, target: &StaticTarget) -> bool {
        let Some(method) = self.current_method.as_deref() else {
            return false;
        };
        let StaticTarget::Name(name, _) = target else {
            return false;
        };
        if let Some(trait_name) = self.current_trait.as_deref() {
            let users: Vec<&str> = self
                .registry
                .classes
                .iter()
                .filter(|(_, info)| {
                    info.uses.iter().any(|u| u.eq_ignore_ascii_case(trait_name))
                        && (method.eq_ignore_ascii_case("__construct")
                            || info.trait_aliases.iter().any(|alias| {
                                alias.alias.eq_ignore_ascii_case("__construct")
                                    && alias.method.eq_ignore_ascii_case(method)
                                    && alias
                                        .trait_name
                                        .as_deref()
                                        .is_none_or(|t| t.eq_ignore_ascii_case(trait_name))
                            }))
                })
                .map(|(class, _)| class.as_ref())
                .collect();
            if name.eq_ignore_ascii_case("parent") {
                return !users.is_empty();
            }
            let Some(called) = self.resolve_static_class_name(name) else {
                return false;
            };
            return users.iter().any(|user| {
                self.parent_classes(user)
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(&called))
            });
        }
        if !method.eq_ignore_ascii_case("__construct") {
            return false;
        }
        let Some(class) = self.current_class.as_deref() else {
            return false;
        };
        let Some(called) = self.resolve_static_class_name(name) else {
            return false;
        };
        self.parent_classes(class)
            .iter()
            .any(|p| p.eq_ignore_ascii_case(&called))
    }

    /// `class`'s parent classes, nearest first.
    fn parent_classes(&self, class: &str) -> Vec<String> {
        let mut parents: Vec<String> = Vec::new();
        let mut current = class.to_string();
        while let Some(parent) = self
            .registry
            .classes
            .get(current.as_str())
            .and_then(|info| info.extends.first())
        {
            let parent = parent.to_string();
            if parents.iter().any(|p| p.eq_ignore_ascii_case(&parent))
                || parent.eq_ignore_ascii_case(class)
            {
                break;
            }
            parents.push(parent.clone());
            current = parent;
        }
        parents
    }
}
