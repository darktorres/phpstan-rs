//! Opt-in `check_native_type_hints`: a parameter or return type given only
//! by PHPDoc although the configured PHP version can declare it natively
//! (`missingNativeType.parameter`/`missingNativeType.return`). The
//! suggestion is the tightest native type accepting the PHPDoc one.
//!
//! Suggestions that would break the declaration are left out: a
//! parameter of a method overriding one whose parameter isn't typed as
//! narrowly, a return type some overriding method doesn't declare (or
//! declares incompatibly), a default value the type wouldn't accept, and
//! magic methods, whose signatures PHP fixes. Without a configured PHP
//! version only types PHP 7.1 already has are suggested.
//!
//! From PHP 8.3 an untyped class constant gets the same suggestion from
//! its `@var` or value type (`missingNativeType.classConstant`), unless a
//! subclass redeclares it without a type inside the suggested one.
//!
//! A closure or arrow function passed where the callee's signature fixes
//! its parameter types gets the same suggestion for each untyped
//! parameter (unless that type is only `mixed`).

use crate::checker::Checker;
use crate::registry::FunctionReflection;
use syntax::ast::*;
use types::PhpVersion;
use types::TrinaryLogic;
use types::Type;

impl<'a> Checker<'a> {
    pub(crate) fn check_native_type_hints(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        label: &str,
        sig: &FunctionReflection,
    ) {
        if f.name.starts_with("__") && class_name.is_some() {
            return;
        }
        let version = self.registry.php_version_min();
        let kind_word = if class_name.is_some() {
            "Method"
        } else {
            "Function"
        };
        let ancestor = class_name.and_then(|class| {
            self.registry
                .lookup_ancestor_method_with_owner(class, &f.name)
                .map(|(_, sig)| sig.clone())
        });
        for (i, param) in f.params.iter().enumerate() {
            let Some(param_sig) = sig.params().get(i) else {
                break;
            };
            if param.type_hint.is_some() || param_sig.doc_ty().is_none() {
                continue;
            }
            let Some(suggested) = param_sig.ty().native_declaration(false, version) else {
                continue;
            };
            let suggested_ty = self.suggested_type(&suggested, class_name);
            if let Some(default) = param_sig.default_ty()
                && !default.is_mixed()
                && !self.accepts(&suggested_ty, default).yes()
            {
                continue;
            }
            if let Some(ancestor) = &ancestor {
                let Some(overridden) = ancestor.params().get(i) else {
                    continue;
                };
                if self
                    .registry
                    .is_super_type_of(&suggested_ty, overridden.ty_native())
                    != TrinaryLogic::Yes
                    || overridden.ty_native().is_implicit_mixed() && suggested != "mixed"
                {
                    continue;
                }
            }
            self.report(
                format!(
                    "{kind_word} {label}() parameter ${} can be declared natively as {suggested}.",
                    param.name
                ),
                param.name_span,
                0,
                "missingNativeType.parameter",
            );
        }
        if f.return_type.is_some() || sig.return_doc_ty().is_none() {
            return;
        }
        let Some(suggested) = sig.return_type().native_declaration(true, version) else {
            return;
        };
        let suggested_ty = self.suggested_type(&suggested, class_name);
        if let Some(ancestor) = &ancestor
            && !ancestor.return_type_native().is_implicit_mixed()
            && self
                .registry
                .is_super_type_of(ancestor.return_type_native(), &suggested_ty)
                != TrinaryLogic::Yes
        {
            return;
        }
        if let Some(class) = class_name
            && self.overriding_returns_block(class, &f.name, &suggested_ty)
        {
            return;
        }
        self.report(
            format!("{kind_word} {label}() return type can be declared natively as {suggested}."),
            f.name_span,
            0,
            "missingNativeType.return",
        );
    }

    pub(crate) fn suggest_closure_native_params(&mut self, params: &[Param], contextual: &[Type]) {
        for (param, ty) in params.iter().zip(contextual) {
            if param.type_hint.is_some() || ty.has_unknown_part() {
                continue;
            }
            let Some(suggested) = ty.native_declaration(false, self.registry.php_version_min())
            else {
                continue;
            };
            self.report(
                format!(
                    "Parameter ${} of the closure can be declared natively as {suggested}.",
                    param.name
                ),
                param.name_span,
                0,
                "missingNativeType.parameter",
            );
        }
    }

    pub(crate) fn check_native_class_constant_types(&mut self, fqn: &str, consts: &[ClassConst]) {
        if self
            .registry
            .php_version_min()
            .is_none_or(|v| v < PhpVersion::new(8, 3))
        {
            return;
        }
        for constant in consts {
            if constant.type_hint.is_some() {
                continue;
            }
            let Some(sig) = self
                .registry
                .classes
                .get(fqn)
                .and_then(|info| info.consts().get(constant.name.as_ref()))
            else {
                continue;
            };
            if sig.native_ty().is_some() || sig.ty().has_unknown_part() {
                continue;
            }
            let Some(suggested) = sig
                .ty()
                .native_declaration(false, self.registry.php_version_min())
            else {
                continue;
            };
            let suggested_ty = self.suggested_type(&suggested, Some(fqn));
            let redeclared_incompatibly = self.registry.classes.iter().any(|(name, info)| {
                name.as_ref() != fqn
                    && self.registry.is_subclass_of(name, fqn)
                    && info
                        .consts()
                        .get(constant.name.as_ref())
                        .is_some_and(|child| {
                            child.native_ty().as_ref().is_none_or(|child_ty| {
                                self.registry.is_super_type_of(&suggested_ty, child_ty)
                                    != TrinaryLogic::Yes
                            })
                        })
            });
            if redeclared_incompatibly {
                continue;
            }
            self.report(
                format!(
                    "Constant {fqn}::{} can be declared natively as {suggested}.",
                    constant.name
                ),
                constant.name_span,
                0,
                "missingNativeType.classConstant",
            );
        }
    }

    /// `suggested` as a type, with `self`/`static` bound to `class`.
    fn suggested_type(&self, suggested: &str, class_name: Option<&str>) -> Type {
        let ty = types::parse_type_string(suggested);
        match class_name {
            Some(class) => self.resolve_pseudo(&ty, class),
            None => ty,
        }
    }

    /// Whether some class overriding `class::method` declares no return
    /// type, or one outside `suggested`.
    fn overriding_returns_block(&self, class: &str, method: &str, suggested: &Type) -> bool {
        self.registry.classes.keys().any(|name| {
            name.as_ref() != class
                && self.registry.is_subclass_of(name, class)
                && self
                    .registry
                    .lookup_method_with_owner(name, method)
                    .is_some_and(|(owner, overriding)| {
                        owner.as_ref() != class
                            && (overriding.return_type_native().is_implicit_mixed()
                                || self
                                    .registry
                                    .is_super_type_of(suggested, overriding.return_type_native())
                                    != TrinaryLogic::Yes)
                    })
        })
    }
}
