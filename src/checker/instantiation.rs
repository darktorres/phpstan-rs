//! `new` target legality beyond existence, mirroring real PHPStan's
//! `InstantiationRule`: `self`/`static`/`parent` outside a class, `new
//! parent` without a parent, the kind of the instantiated symbol, a
//! constructor-less class given arguments, an inaccessible constructor,
//! and a dynamic class-name expression that can't be a class name.

use crate::ast::{ClassKind, Visibility};
use crate::checker::{Checker, DiagnosticBuilder};
use crate::token::Span;
use crate::types::Type;

/// Whether `ty` can never be a `string` or an object, so `new` on it
/// always fails. A `Union` answers for its non-`null` members, the
/// same leniency `type_can_never_be_object` applies.
fn type_can_never_name_a_class(ty: &Type) -> bool {
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
    pub(crate) fn check_new_on_non_class_name(&mut self, ty: &Type, span: Span, label: &str) {
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
            label.to_string(),
            format!("cannot instantiate class using {ty}"),
            span,
            if every_member { 0 } else { 7 },
            "new.nonObject",
        );
    }

    /// `new self`/`new static`/`new parent` legality (`outOfClass.*`,
    /// `class.noParent`), for a pseudo-name that resolved to no class.
    pub(crate) fn check_new_pseudo_target(&mut self, name: &str, span: Span, label: &str) {
        let keyword = name.to_ascii_lowercase();
        if !matches!(keyword.as_str(), "self" | "static" | "parent") {
            return;
        }
        let Some(class) = self.current_class.clone() else {
            self.report(
                label.to_string(),
                format!("using {name} outside of class scope"),
                span,
                0,
                &format!("outOfClass.{keyword}"),
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
            .is_none_or(|info| !info.extends.is_empty());
        if has_parent {
            return;
        }
        let method = self.current_method.clone().unwrap_or_default();
        self.report(
            label.to_string(),
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
        label: &str,
    ) -> bool {
        let Some((_, info)) = self.registry.lookup_class(class) else {
            return true;
        };
        if !is_name {
            return true;
        }
        let kind = info.kind;
        let is_abstract = info.is_abstract;
        match kind {
            ClassKind::Trait => {
                self.report(
                    label.to_string(),
                    format!("cannot instantiate trait {class}"),
                    span,
                    0,
                    "new.trait",
                );
                return false;
            }
            ClassKind::Enum => {
                self.report(
                    label.to_string(),
                    format!("cannot instantiate enum {class}"),
                    span,
                    0,
                    "new.enum",
                );
                return false;
            }
            ClassKind::Interface if !is_static => {
                self.report(
                    label.to_string(),
                    format!("cannot instantiate interface {class}"),
                    span,
                    0,
                    "new.interface",
                );
                return false;
            }
            ClassKind::Class if is_abstract && !is_static => {
                self.report_builder(
                    DiagnosticBuilder::new(
                        label.to_string(),
                        format!("cannot instantiate abstract class {class}"),
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
            .map(|(owner, sig)| (owner, sig.visibility.clone()))
        else {
            if has_args && !self.registry.has_unresolvable_ancestor(class) {
                self.report(
                    label.to_string(),
                    format!(
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
                label.to_string(),
                format!(
                    "cannot instantiate class {class} via {word} constructor {owner}::__construct()"
                ),
                span,
                0,
                &format!("new.{word}Constructor"),
            );
        }
        true
    }
}
