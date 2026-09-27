//! `new` target legality beyond existence, mirroring real PHPStan's
//! `InstantiationRule`: `self`/`static`/`parent` outside a class, `new
//! parent` without a parent, the kind of the instantiated symbol, a
//! constructor-less class given arguments, an inaccessible constructor,
//! and a dynamic class-name expression that can't be a class name.

use crate::checker::Checker;
use diagnostics::DiagnosticBuilder;
use syntax::ast::{ClassKind, Visibility};
use syntax::token::Span;
use types::Type;

/// Whether `ty` can never be a `string` or an object, so `new` on it
/// always fails. A `Union` answers for its non-`null` members, the
/// same leniency `type_can_never_be_object` applies.
pub(crate) fn type_can_never_name_a_class(ty: &Type) -> bool {
    match ty {
        Type::Null
        | Type::Void
        | Type::Bool
        | Type::True
        | Type::False
        | Type::Int
        | Type::IntLiteral(_)
        | Type::IntRange(..)
        | Type::Float
        | Type::FloatLiteral(_)
        | Type::Array(..)
        | Type::ArrayShape(_)
        | Type::HasOffsetValue(..) => true,
        Type::Union(members) => members
            .iter()
            .filter(|m| !matches!(m, Type::Null))
            .any(type_can_never_name_a_class),
        Type::Intersection(members) => members.iter().all(type_can_never_name_a_class),
        Type::TemplateParam(param) => type_can_never_name_a_class(&param.bound),
        Type::Conditional {
            if_true, if_false, ..
        } => type_can_never_name_a_class(if_true) || type_can_never_name_a_class(if_false),
        _ => false,
    }
}

impl<'a> Checker<'a> {
    /// `new $expr()` where `$expr`'s type can never be a class name or
    /// an object (`new.nonObject`). Level 0 when every member is
    /// unusable, level 7 when only some members of a union are.
    pub(crate) fn check_new_on_non_class_name(&mut self, ty: &Type, span: Span) {
        if !type_can_never_name_a_class(ty) {
            return;
        }
        let every_member = match ty {
            Type::Union(members) => members
                .iter()
                .filter(|m| !matches!(m, Type::Null))
                .all(type_can_never_name_a_class),
            _ => true,
        };
        self.report(
            format!("cannot instantiate class using {ty}"),
            span,
            if every_member { 0 } else { 7 },
            "new.nonObject",
        );
    }

    /// `new self`/`new static`/`new parent` legality (`outOfClass.*`,
    /// `class.noParent`), for a pseudo-name that resolved to no class.
    pub(crate) fn check_new_pseudo_target(&mut self, name: &str, span: Span) {
        let keyword = name.to_ascii_lowercase();
        if !matches!(keyword.as_str(), "self" | "static" | "parent") {
            return;
        }
        let Some(class) = self.current_class.clone() else {
            self.report(
                format!("using {name} outside of class scope"),
                span,
                0,
                format!("outOfClass.{keyword}"),
            );
            return;
        };
        if keyword != "parent" {
            return;
        }
        let has_parent = self
            .registry
            .classes
            .get(class.as_str())
            .is_none_or(|info| !info.extends().is_empty());
        if has_parent {
            return;
        }
        let method = self.current_method.clone().unwrap_or_default();
        self.report(
            format!("{class}::{method}() calls new parent but {class} does not extend any class"),
            span,
            0,
            "class.noParent",
        );
    }

    /// The instantiated class's own legality: its kind (`new.trait`,
    /// `new.enum`, `new.interface`, `new.abstract` - the last two never
    /// for `new static`, which names a concrete subclass at runtime), a
    /// constructor-less class given arguments (`new.noConstructor`), and
    /// an inaccessible constructor (`new.privateConstructor`/
    /// `new.protectedConstructor`). `is_name` is false for a dynamic
    /// `new $expr()`, which only ever gets the constructor-argument
    /// checks. Returns whether the constructor's arguments should still
    /// be checked.
    pub(crate) fn check_instantiated_class(
        &mut self,
        class: &str,
        is_name: bool,
        is_static: bool,
        has_args: bool,
        span: Span,
    ) -> bool {
        let Some((_, info)) = self.registry.lookup_class(class) else {
            return true;
        };
        if !is_name {
            return true;
        }
        if is_static {
            self.check_new_static(span);
        }
        let kind = info.kind();
        let is_abstract = info.is_abstract();
        match kind {
            ClassKind::Trait => {
                self.report(
                    format!("cannot instantiate trait {class}"),
                    span,
                    0,
                    "new.trait",
                );
                return false;
            }
            ClassKind::Enum => {
                self.report(
                    format!("cannot instantiate enum {class}"),
                    span,
                    0,
                    "new.enum",
                );
                return false;
            }
            ClassKind::Interface if !is_static => {
                self.report(
                    format!("cannot instantiate interface {class}"),
                    span,
                    0,
                    "new.interface",
                );
                return false;
            }
            ClassKind::Class if is_abstract && !is_static => {
                self.report_builder(
                    DiagnosticBuilder::new(format!("cannot instantiate abstract class {class}"),
                        span,
                        0,
                        "new.abstract",
                    )
                    .tip(
                        "either implement every inherited abstract method, or instantiate a concrete subclass instead",
                    ),
                );
                return false;
            }
            _ => {}
        }
        let Some((owner, visibility)) = self
            .registry
            .lookup_method_with_owner(class, "__construct")
            .map(|(owner, sig)| (owner, sig.visibility()))
        else {
            if has_args && !self.registry.has_unresolvable_ancestor(class) {
                self.report(format!(
                        "class {class} does not have a constructor and must be instantiated without any parameters"
                    ),
                    span,
                    0,
                    "new.noConstructor",
                );
            }
            return false;
        };
        let word = match visibility {
            Visibility::Public => return true,
            Visibility::Private => "private",
            Visibility::Protected => "protected",
        };
        if !self.is_member_access_visible(&owner, &visibility, "__construct", "method") {
            self.report(
                format!(
                    "cannot instantiate class {class} via {word} constructor {owner}::__construct()"
                ),
                span,
                0,
                format!("new.{word}Constructor"),
            );
        }
        true
    }

    /// `new static()` in a class whose subclasses could redeclare the
    /// constructor with an incompatible signature (`new.static`): not
    /// when the class is final, carries `@phpstan-consistent-constructor`
    /// itself or on an ancestor, has its constructor pinned by an
    /// interface, declares it `final`, or overrides an abstract
    /// prototype (including a trait's own abstract constructor). Plus
    /// `new static()` in a static method of an abstract class
    /// (`new.staticInAbstractClassStaticMethod`), which a direct call
    /// to that method on the abstract class itself would crash.
    fn check_new_static(&mut self, span: Span) {
        let Some(class) = self.current_class.clone() else {
            return;
        };
        let Some(info) = self.registry.classes.get(class.as_str()) else {
            return;
        };
        let is_abstract = info.is_abstract();
        if !info.is_final() && !info.is_final_by_phpdoc() && self.new_static_is_unsafe(&class) {
            self.report_builder(
                DiagnosticBuilder::new("unsafe usage of new static()".to_string(),
                    span,
                    0,
                    "new.static",
                )
                .tip("See: https://phpstan.org/blog/solving-phpstan-error-unsafe-usage-of-new-static"),
            );
        }
        let Some(method) = self.current_method.clone() else {
            return;
        };
        let in_static_method = self
            .registry
            .lookup_method_with_owner(&class, &method)
            .and_then(|(owner, _)| self.registry.classes.get(&owner))
            .is_some_and(|owner| {
                owner
                    .static_methods()
                    .iter()
                    .any(|m| m.eq_ignore_ascii_case(&method))
            });
        if is_abstract && in_static_method {
            self.report_builder(
                DiagnosticBuilder::new(format!(
                        "unsafe usage of new static() in abstract class {class} in static method {method}()"
                    ),
                    span,
                    0,
                    "new.staticInAbstractClassStaticMethod",
                )
                .tip(format!(
                    "Direct call to {class}::{method}() would crash because an abstract class cannot be instantiated."
                )),
            );
        }
    }

    fn new_static_is_unsafe(&self, class: &str) -> bool {
        let registry = &self.registry;
        let mut current = Some(class.to_string());
        while let Some(c) = current {
            let Some(info) = registry.classes.get(c.as_str()) else {
                break;
            };
            if info.is_consistent_constructor() {
                return false;
            }
            current = info.extends().first().map(|p| p.to_string());
        }
        let Some((owner, _)) = registry.lookup_method_with_owner(class, "__construct") else {
            return true;
        };
        if !registry
            .all_interfaces_declaring_method(class, "__construct")
            .is_empty()
        {
            return false;
        }
        if registry.classes.get(&owner).is_some_and(|info| {
            info.final_methods().contains("__construct")
                || info.final_methods_by_phpdoc().contains("__construct")
        }) {
            return false;
        }
        let mut prototype_owner = owner.clone();
        let mut current = registry
            .classes
            .get(&owner)
            .and_then(|info| info.extends().first().cloned());
        while let Some(c) = current {
            let Some(info) = registry.classes.get(&c) else {
                break;
            };
            if info.methods().contains_key("__construct") {
                prototype_owner = c.clone();
            }
            current = info.extends().first().cloned();
        }
        if registry
            .classes
            .get(&prototype_owner)
            .is_some_and(|info| info.abstract_methods().contains("__construct"))
        {
            return false;
        }
        let abstract_trait_methods_supported = registry
            .php_supports_everywhere(types::php_version::PhpFeature::AbstractTraitMethods)
            != Some(false);
        if abstract_trait_methods_supported
            && let Some(t) = &self.current_trait
            && registry
                .classes
                .get(t.as_str())
                .is_some_and(|info| info.abstract_methods().contains("__construct"))
        {
            return false;
        }
        true
    }
}
