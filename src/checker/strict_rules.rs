//! Opt-in ports of `phpstan-strict-rules`, one registry flag per family:
//!
//! - `check_illegal_constructor_calls` - `$obj->__construct()`, and a
//!   static `X::__construct()` anywhere but a constructor calling one of
//!   its own parent classes' (`constructor.call`).
//! - `check_dynamic_static_calls` - a static method called, or turned
//!   into a first-class callable, through an instance
//!   (`staticMethod.dynamicCall`).

use crate::ast::*;
use crate::checker::{Checker, Locals};
use crate::types::Type;

impl<'a> Checker<'a> {
    /// The expression-level ports, before `e` itself is checked.
    pub(crate) fn check_strict_rules_expression(&mut self, e: &Expr, locals: &Locals, label: &str) {
        if self.registry.check_dynamic_static_calls {
            let call = match &e.kind {
                ExprKind::FirstClassCallable(inner) => inner,
                _ => e,
            };
            if let ExprKind::MethodCall(receiver, method, _, _) = &call.kind
                && let Some((owner, name)) =
                    self.static_method_through_instance(receiver, method, locals)
            {
                self.report(
                    label.to_string(),
                    format!("Dynamic call to static method {owner}::{name}()."),
                    e.span,
                    0,
                    "staticMethod.dynamicCall",
                );
            }
        }
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

    /// The declaring class and name of `method` when every object type
    /// `receiver` can be declares it static.
    fn static_method_through_instance(
        &self,
        receiver: &Expr,
        method: &str,
        locals: &Locals,
    ) -> Option<(String, String)> {
        let ty = self.infer_expr(receiver, locals);
        let members: Vec<&Type> = match &ty {
            Type::Union(members) => members
                .iter()
                .filter(|m| !matches!(m, Type::Null))
                .collect(),
            other => vec![other],
        };
        let mut found: Option<(String, String)> = None;
        for member in members {
            let Type::Object(class, _) = member else {
                return None;
            };
            let (owner, _) = self.registry.lookup_method_with_owner(class, method)?;
            let name = self
                .registry
                .lookup_method_canonical_name(class, method)
                .unwrap_or(method)
                .to_string();
            let owner_info = self.registry.classes.get(owner.as_ref())?;
            if !owner_info.static_methods.contains(name.as_str()) {
                return None;
            }
            // PHPStan's own test-case base classes expose their static
            // assertion helpers for `$this->` use.
            let prototype_in_phpstan_test_case = [
                "PHPStan\\Testing\\TypeInferenceTestCase",
                "PHPStan\\Testing\\PHPStanTestCase",
            ]
            .iter()
            .any(|base| {
                self.registry.is_subclass_of(&owner, base)
                    && self.registry.lookup_method(base, &name).is_some()
            });
            if prototype_in_phpstan_test_case {
                return None;
            }
            found.get_or_insert((owner.to_string(), name));
        }
        found
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
