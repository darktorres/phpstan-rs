//! The `dave-liddament/php-language-extensions` access-control
//! attributes on a class or method, read from its declaration: `Friend`,
//! `NamespaceVisibility` (and the older `Package`), `TestTag`,
//! `InjectableVersion`/`CheckInjectableVersion`, `RestrictTraitTo`, and
//! `Sealed` (checked alongside `ClassReflection::sealed`). `MustUseResult` feeds
//! `FunctionReflection::must_use_result`.

use std::sync::Arc;

use syntax::ast::{Attribute, Expr, ExprKind, Literal, StaticTarget};
use syntax::intern::intern;

/// `NamespaceVisibility`: callers must be in `namespace` (or, unless
/// `exclude_sub_namespaces`, below it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceVisibility {
    pub namespace: String,
    pub exclude_sub_namespaces: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessAttributes {
    /// `Friend`: the only other classes that may call it.
    pub friends: Vec<Arc<str>>,
    pub namespace_visibility: Option<NamespaceVisibility>,
    /// `TestTag`: only test code may use it.
    pub test_only: bool,
    /// `InjectableVersion`: type-hint this, never a subtype, in
    /// constructors and `CheckInjectableVersion` methods.
    pub injectable_version: bool,
    pub check_injectable_version: bool,
    /// `RestrictTraitTo`: only these classes (and subclasses) may use
    /// the trait.
    pub restrict_trait_to: Vec<Arc<str>>,
    /// `Sealed`: the only classes that may extend/implement it.
    pub sealed: Vec<Arc<str>>,
}

impl AccessAttributes {
    pub fn is_empty(&self) -> bool {
        *self == AccessAttributes::default()
    }
}

/// The attribute's short name when it's one of the library's, which
/// lives under a `PhpLanguageExtensions` namespace.
fn library_attribute(resolved: &str) -> Option<&str> {
    resolved
        .contains("PhpLanguageExtensions")
        .then(|| resolved.rsplit('\\').next().unwrap_or(resolved))
}

fn class_names(args: &[Expr], out: &mut Vec<Arc<str>>) {
    syntax::stack::guard(|| {
        for arg in args {
            match &arg.kind {
                ExprKind::ClassNameFetch(StaticTarget::Name(class, _)) => {
                    out.push(crate::registry::name_context::class_ref(class));
                }
                ExprKind::Literal(Literal::String(s)) => {
                    out.push(intern(s.to_str_lossy().trim_start_matches('\\')));
                }
                ExprKind::ArrayLiteral(items) => {
                    let values: Vec<Expr> = items.iter().map(|i| i.value.clone()).collect();
                    class_names(&values, out);
                }
                _ => {}
            }
        }
    })
}

/// The access attributes among `attrs`, on a declaration in `namespace`.
pub(crate) fn access_attributes(attrs: &[Attribute], namespace: &str) -> AccessAttributes {
    let mut out = AccessAttributes::default();
    for attribute in attrs {
        let Some(short) = attribute.name.resolved_class().and_then(library_attribute) else {
            continue;
        };
        let values: Vec<Expr> = attribute.args.iter().map(|a| a.value.clone()).collect();
        match short {
            "Friend" => class_names(&values, &mut out.friends),
            "RestrictTraitTo" => class_names(&values, &mut out.restrict_trait_to),
            "Sealed" => class_names(&values, &mut out.sealed),
            "TestTag" => out.test_only = true,
            "InjectableVersion" => out.injectable_version = true,
            "CheckInjectableVersion" => out.check_injectable_version = true,
            "Package" => {
                out.namespace_visibility = Some(NamespaceVisibility {
                    namespace: namespace.to_string(),
                    exclude_sub_namespaces: true,
                })
            }
            "NamespaceVisibility" => {
                let mut visibility = NamespaceVisibility {
                    namespace: namespace.to_string(),
                    exclude_sub_namespaces: false,
                };
                for arg in &attribute.args {
                    match (&arg.name, &arg.value.kind) {
                        (Some(name), ExprKind::Literal(Literal::Bool(b)))
                            if name == "excludeSubNamespaces" =>
                        {
                            visibility.exclude_sub_namespaces = *b
                        }
                        (Some(name), ExprKind::Literal(Literal::String(s)))
                            if name == "namespace" =>
                        {
                            visibility.namespace = s.to_str_lossy().trim_matches('\\').to_string()
                        }
                        (None, ExprKind::Literal(Literal::Bool(b))) => {
                            visibility.exclude_sub_namespaces = *b
                        }
                        (None, ExprKind::Literal(Literal::String(s))) => {
                            visibility.namespace = s.to_str_lossy().trim_matches('\\').to_string()
                        }
                        _ => {}
                    }
                }
                out.namespace_visibility = Some(visibility);
            }
            _ => {}
        }
    }
    out
}

/// Whether `attrs` includes the library's `MustUseResult`.
pub(crate) fn has_must_use_result(attrs: &[Attribute]) -> bool {
    attrs
        .iter()
        .any(|a| a.name.resolved_class().and_then(library_attribute) == Some("MustUseResult"))
}
