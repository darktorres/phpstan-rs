//! The `dave-liddament/php-language-extensions` access-control
//! attributes on a class or method, read from its declaration: `Friend`,
//! `NamespaceVisibility` (and the older `Package`), `TestTag`,
//! `InjectableVersion`/`CheckInjectableVersion`, `RestrictTraitTo`, and
//! `Sealed` (checked alongside `ClassReflection::sealed`). `MustUseResult` feeds
//! `FunctionReflection::must_use_result`.

use syntax::name::Name;

use syntax::ast::{Arg, Attribute, ExprKind, Literal, NodeArenas, StaticTarget};

/// `NamespaceVisibility`: callers must be in `namespace` (or, unless
/// `exclude_sub_namespaces`, below it).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NamespaceVisibility {
    pub namespace: String,
    pub exclude_sub_namespaces: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AccessAttributes {
    /// `Friend`: the only other classes that may call it.
    pub friends: Vec<Name>,
    pub namespace_visibility: Option<NamespaceVisibility>,
    /// `TestTag`: only test code may use it.
    pub test_only: bool,
    /// `InjectableVersion`: type-hint this, never a subtype, in
    /// constructors and `CheckInjectableVersion` methods.
    pub injectable_version: bool,
    pub check_injectable_version: bool,
    /// `RestrictTraitTo`: only these classes (and subclasses) may use
    /// the trait.
    pub restrict_trait_to: Vec<Name>,
    /// `Sealed`: the only classes that may extend/implement it.
    pub sealed: Vec<Name>,
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

fn class_name_expr(arenas: &NodeArenas, id: syntax::ast::ExprId, out: &mut Vec<Name>) {
    let Some(value) = arenas.expr(id) else {
        return;
    };
    match &value.kind {
        ExprKind::ClassNameFetch(StaticTarget::Name(class)) => {
            out.push(crate::registry::name_context::class_ref(class));
        }
        ExprKind::Literal(Literal::String(s)) => {
            if let Some(bytes) = arenas.string_bytes(s) {
                out.push(Name::from(
                    String::from_utf8_lossy(bytes)
                        .trim_start_matches('\\')
                        .to_string(),
                ));
            }
        }
        ExprKind::ArrayLiteral(items) => {
            for item in arenas.array_items(items.clone()) {
                class_name_expr(arenas, item.value, out);
            }
        }
        _ => {}
    }
}

fn class_names(arenas: &NodeArenas, args: &[Arg], out: &mut Vec<Name>) {
    syntax::stack::guard(|| {
        for arg in args {
            class_name_expr(arenas, arg.value, out);
        }
    })
}

/// The access attributes among `attrs`, on a declaration in `namespace`.
pub(crate) fn access_attributes(
    arenas: &NodeArenas,
    attrs: &[Attribute],
    namespace: &str,
) -> AccessAttributes {
    let mut out = AccessAttributes::default();
    for attribute in attrs {
        let Some(short) = attribute.name.resolved_class().and_then(library_attribute) else {
            continue;
        };
        let values = arenas.args(attribute.args.clone());
        match short {
            "Friend" => class_names(arenas, values, &mut out.friends),
            "RestrictTraitTo" => class_names(arenas, values, &mut out.restrict_trait_to),
            "Sealed" => class_names(arenas, values, &mut out.sealed),
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
                for arg in values {
                    let Some(value) = arenas.expr(arg.value) else {
                        continue;
                    };
                    match (&arg.name, &value.kind) {
                        (Some(name), ExprKind::Literal(Literal::Bool(b)))
                            if name == "excludeSubNamespaces" =>
                        {
                            visibility.exclude_sub_namespaces = *b
                        }
                        (Some(name), ExprKind::Literal(Literal::String(s)))
                            if name == "namespace" =>
                        {
                            if let Some(bytes) = arenas.string_bytes(s) {
                                visibility.namespace = String::from_utf8_lossy(bytes)
                                    .trim_matches('\\')
                                    .to_string();
                            }
                        }
                        (None, ExprKind::Literal(Literal::Bool(b))) => {
                            visibility.exclude_sub_namespaces = *b
                        }
                        (None, ExprKind::Literal(Literal::String(s))) => {
                            if let Some(bytes) = arenas.string_bytes(s) {
                                visibility.namespace = String::from_utf8_lossy(bytes)
                                    .trim_matches('\\')
                                    .to_string();
                            }
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
