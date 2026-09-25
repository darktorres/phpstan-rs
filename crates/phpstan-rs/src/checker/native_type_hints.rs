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
use crate::php_version::PhpVersion;
use crate::registry::{FunctionSig, Trinary};
use crate::types::Type;
use syntax::ast::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Parameter,
    Return,
}

/// A native type as a set of atoms: keywords and class names, with
/// intersections as `A&B`.
#[derive(Default)]
struct Native {
    atoms: Vec<String>,
}

impl Native {
    fn add(&mut self, atom: &str) {
        if !self.atoms.iter().any(|a| a.eq_ignore_ascii_case(atom)) {
            self.atoms.push(atom.to_string());
        }
    }
}

/// Collects `ty`'s native atoms; `false` when some part has no native
/// spelling.
fn collect(ty: &Type, position: Position, out: &mut Native) -> bool {
    match ty {
        Type::Int | Type::IntLiteral(_) | Type::IntRange(..) => out.add("int"),
        Type::Float | Type::FloatLiteral(_) => out.add("float"),
        Type::String | Type::StringLiteral(_) | Type::ClassString(_) => out.add("string"),
        Type::Bool => out.add("bool"),
        Type::True => out.add("true"),
        Type::False => out.add("false"),
        Type::Null => out.add("null"),
        Type::Array(..) | Type::ArrayShape(_) => out.add("array"),
        Type::Callable(_) => out.add("callable"),
        Type::ObjectShape(_) | Type::ObjectWithoutClass => out.add("object"),
        Type::Mixed(_) => out.add("mixed"),
        Type::Void if position == Position::Return => out.add("void"),
        Type::Never if position == Position::Return => out.add("never"),
        Type::StaticRef if position == Position::Return => out.add("static"),
        Type::SelfRef => out.add("self"),
        Type::ParentRef => out.add("parent"),
        Type::EnumCase(class, _) => out.add(class),
        Type::Object(class, _) => out.add(class),
        Type::TemplateParam(param) => return collect(&param.bound, position, out),
        Type::Union(members) | Type::BenevolentUnion(members) => {
            return members.iter().all(|m| collect(m, position, out));
        }
        Type::Intersection(members) => {
            let bases: Vec<&Type> = members
                .iter()
                .filter(|m| !matches!(m, Type::Accessory(_) | Type::HasOffsetValue(..)))
                .collect();
            if bases.len() == 1 {
                return bases.iter().all(|b| collect(b, position, out));
            }
            let mut classes = Vec::new();
            for base in bases {
                let Type::Object(class, _) = base else {
                    return false;
                };
                classes.push(class.clone());
            }
            out.add(&classes.join("&"));
        }
        _ => return false,
    }
    true
}

/// Renders `native` for `version`, or `None` when that version can't
/// declare it.
fn render(mut native: Native, version: Option<PhpVersion>) -> Option<String> {
    let at_least = |v: PhpVersion| version.is_some_and(|configured| configured >= v);
    let atoms = &mut native.atoms;
    if atoms.iter().any(|a| a == "mixed") {
        return at_least((8, 0)).then(|| "mixed".to_string());
    }
    if atoms.len() > 1 && atoms.iter().any(|a| a == "void" || a == "never") {
        return None;
    }
    if atoms.iter().any(|a| a == "true") && atoms.iter().any(|a| a == "false") {
        atoms.retain(|a| a != "true" && a != "false");
        atoms.push("bool".to_string());
    }
    if atoms.iter().any(|a| a == "bool") {
        atoms.retain(|a| a != "true" && a != "false");
    }
    if atoms.iter().any(|a| a == "array") && atoms.iter().any(|a| a == "Traversable") {
        atoms.retain(|a| a != "array" && a != "Traversable");
        atoms.push("iterable".to_string());
    }
    if atoms.iter().any(|a| a == "object") {
        atoms.retain(|a| {
            matches!(
                a.as_str(),
                "object"
                    | "int"
                    | "float"
                    | "string"
                    | "bool"
                    | "true"
                    | "false"
                    | "null"
                    | "array"
                    | "iterable"
                    | "callable"
            )
        });
    }
    let nullable = atoms.iter().any(|a| a == "null");
    let others: Vec<&String> = atoms.iter().filter(|a| *a != "null").collect();
    let has_intersection = others.iter().any(|a| a.contains('&'));
    let needs = |feature: PhpVersion| at_least(feature);
    match others.as_slice() {
        [] => needs((8, 2)).then(|| "null".to_string()),
        [only] => {
            let standalone_literal = matches!(only.as_str(), "true" | "false");
            if has_intersection {
                if nullable {
                    return needs((8, 2)).then(|| format!("({only})|null"));
                }
                return needs((8, 1)).then(|| only.to_string());
            }
            if (standalone_literal && !needs((8, 2)))
                || (only.as_str() == "static" && !needs((8, 0)))
                || (only.as_str() == "never" && !needs((8, 1)))
                || (only.as_str() == "object" && !needs((7, 2)))
            {
                if standalone_literal && nullable && needs((8, 0)) {
                    return Some(format!("{only}|null"));
                }
                return None;
            }
            Some(if nullable {
                format!("?{only}")
            } else {
                only.to_string()
            })
        }
        many => {
            if !needs((8, 0)) || (has_intersection && !needs((8, 2))) {
                return None;
            }
            if many.iter().any(|a| a.as_str() == "true") && !needs((8, 2)) {
                return None;
            }
            let mut parts: Vec<String> = many
                .iter()
                .map(|a| {
                    if a.contains('&') {
                        format!("({a})")
                    } else {
                        a.to_string()
                    }
                })
                .collect();
            if nullable {
                parts.push("null".to_string());
            }
            Some(parts.join("|"))
        }
    }
}

impl<'a> Checker<'a> {
    pub(crate) fn check_native_type_hints(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        label: &str,
        sig: &FunctionSig,
    ) {
        if f.name.starts_with("__") && class_name.is_some() {
            return;
        }
        let version = self.registry.php_version;
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
            let Some(param_sig) = sig.params.get(i) else {
                break;
            };
            if param.type_hint.is_some() || param_sig.doc_ty.is_none() {
                continue;
            }
            let mut native = Native::default();
            if !collect(&param_sig.ty, Position::Parameter, &mut native) {
                continue;
            }
            let Some(suggested) = render(native, version) else {
                continue;
            };
            let suggested_ty = self.suggested_type(&suggested, class_name);
            if let Some(default) = &param_sig.default_ty
                && !matches!(default, Type::Mixed(_))
                && !self.registry.is_assignable(default, &suggested_ty)
            {
                continue;
            }
            if let Some(ancestor) = &ancestor {
                let Some(overridden) = ancestor.params.get(i) else {
                    continue;
                };
                if self
                    .registry
                    .is_super_type_of(&suggested_ty, &overridden.ty_native)
                    != Trinary::Yes
                    || matches!(overridden.ty_native, Type::Mixed(false)) && suggested != "mixed"
                {
                    continue;
                }
            }
            self.report(
                label.to_string(),
                format!(
                    "{kind_word} {label}() parameter ${} can be declared natively as {suggested}.",
                    param.name
                ),
                param.name_span,
                0,
                "missingNativeType.parameter",
            );
        }
        if f.return_type.is_some() || sig.return_doc_ty.is_none() {
            return;
        }
        let mut native = Native::default();
        if !collect(&sig.return_type, Position::Return, &mut native) {
            return;
        }
        let Some(suggested) = render(native, version) else {
            return;
        };
        let suggested_ty = self.suggested_type(&suggested, class_name);
        if let Some(ancestor) = &ancestor
            && !matches!(ancestor.return_type_native, Type::Mixed(false))
            && self
                .registry
                .is_super_type_of(&ancestor.return_type_native, &suggested_ty)
                != Trinary::Yes
        {
            return;
        }
        if let Some(class) = class_name
            && self.overriding_returns_block(class, &f.name, &suggested_ty)
        {
            return;
        }
        self.report(
            label.to_string(),
            format!("{kind_word} {label}() return type can be declared natively as {suggested}."),
            f.name_span,
            0,
            "missingNativeType.return",
        );
    }

    pub(crate) fn suggest_closure_native_params(
        &mut self,
        params: &[Param],
        contextual: &[Type],
        label: &str,
    ) {
        for (param, ty) in params.iter().zip(contextual) {
            if param.type_hint.is_some() || contains_mixed_or_template(ty) {
                continue;
            }
            let mut native = Native::default();
            if !collect(ty, Position::Parameter, &mut native) {
                continue;
            }
            let Some(suggested) = render(native, self.registry.php_version) else {
                continue;
            };
            self.report(
                label.to_string(),
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
        if self.registry.php_version.is_none_or(|v| v < (8, 3)) {
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
                .and_then(|info| info.consts.get(constant.name.as_ref()))
            else {
                continue;
            };
            if sig.native_ty.is_some() || contains_mixed_or_template(&sig.ty) {
                continue;
            }
            let mut native = Native::default();
            if !collect(&sig.ty, Position::Parameter, &mut native) {
                continue;
            }
            let Some(suggested) = render(native, self.registry.php_version) else {
                continue;
            };
            let suggested_ty = self.suggested_type(&suggested, Some(fqn));
            let redeclared_incompatibly = self.registry.classes.iter().any(|(name, info)| {
                name.as_ref() != fqn
                    && self.registry.is_subclass_of(name, fqn)
                    && info
                        .consts
                        .get(constant.name.as_ref())
                        .is_some_and(|child| {
                            child.native_ty.as_ref().is_none_or(|child_ty| {
                                self.registry.is_super_type_of(&suggested_ty, child_ty)
                                    != Trinary::Yes
                            })
                        })
            });
            if redeclared_incompatibly {
                continue;
            }
            self.report(
                fqn.to_string(),
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
        let ty = crate::types::parse_type_string(suggested);
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
                            && (matches!(overriding.return_type_native, Type::Mixed(false))
                                || self
                                    .registry
                                    .is_super_type_of(suggested, &overriding.return_type_native)
                                    != Trinary::Yes)
                    })
        })
    }
}

fn contains_mixed_or_template(ty: &Type) -> bool {
    match ty {
        Type::Mixed(_) | Type::TemplateParam(_) => true,
        Type::Union(members) | Type::Intersection(members) => {
            members.iter().any(contains_mixed_or_template)
        }
        _ => false,
    }
}
