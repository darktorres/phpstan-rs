//! Real PHPStan's `UnresolvableTypeHelper`: a PHPDoc type that its
//! `TypeNodeResolver` turns into an `ErrorType` or an implicit `never`
//! "contains unresolvable type". Decided on the tag's parsed
//! `phpdoc_grammar` node, against the name scope and registry, so the
//! lenient type the registry keeps for such a tag doesn't mask it.
//!
//! Stricter than PHPStan in one place: an unimported `Closure(...)` in a
//! namespace resolves to the namespace's (nonexistent) `Closure` class
//! there and is unresolvable; phpstan-rs reports it the same way but
//! still types the value as the global `Closure` the author meant.

use super::*;
use crate::phpdoc_grammar::{self, PhpDocTagValue, TypeNode};

/// The winning type node of each tag real PHPStan resolves: plain,
/// `@phan-`, `@psalm-`, then `@phpstan-`, a later spelling replacing an
/// earlier one, and a foreign spelling that didn't parse skipped.
#[derive(Default)]
pub(crate) struct TagTypeNodes {
    pub params: FxHashMap<String, TypeNode>,
    pub param_out: FxHashMap<String, TypeNode>,
    pub return_type: Option<TypeNode>,
    pub var: Option<TypeNode>,
}

pub(crate) fn tag_type_nodes(doc: &str) -> TagTypeNodes {
    let node = phpdoc_grammar::parse(doc);
    let mut out = TagTypeNodes::default();
    for prefix in ["", "phan-", "psalm-", "phpstan-"] {
        for tag in &node.tags {
            let Some(name) = tag.name.strip_prefix(prefix) else {
                continue;
            };
            if !prefix.is_empty() || !has_known_prefix(&tag.name) {
                let (ty, variable) = match &tag.value {
                    PhpDocTagValue::TypedVar { ty, variable, .. } => {
                        (ty.as_ref(), variable.clone())
                    }
                    PhpDocTagValue::Type { ty, .. } => (ty.as_ref(), None),
                    _ => continue,
                };
                let Some(ty) = ty else { continue };
                if !prefix.is_empty() && prefix != "phpstan-" && contains_error(ty) {
                    continue;
                }
                match (name, variable) {
                    ("param", Some(v)) => {
                        out.params.insert(v, ty.clone());
                    }
                    ("param-out", Some(v)) => {
                        out.param_out.insert(v, ty.clone());
                    }
                    ("return", _) => out.return_type = Some(ty.clone()),
                    ("var", _) => out.var = Some(ty.clone()),
                    _ => {}
                }
            }
        }
    }
    out
}

/// Every `@var` tag's winning type node, keyed by its variable name
/// (`None` for a bare `@var Type`), in first-appearance order - the same
/// prefix precedence as `tag_type_nodes`.
pub(crate) fn var_tag_nodes(doc: &str) -> Vec<(Option<String>, TypeNode)> {
    let node = phpdoc_grammar::parse(doc);
    let mut named: Vec<(Option<String>, TypeNode)> = Vec::new();
    // When any tag is unnamed, only the unnamed tags of the last tag kind
    // (`@var` < `@phan-var` < `@psalm-var` < `@phpstan-var`) that has one
    // count; the named tags are dropped.
    let mut unnamed: Vec<(Option<String>, TypeNode)> = Vec::new();
    for prefix in ["", "phan-", "psalm-", "phpstan-"] {
        let mut group = Vec::new();
        for tag in &node.tags {
            let Some(name) = tag.name.strip_prefix(prefix) else {
                continue;
            };
            if name != "var" || (prefix.is_empty() && has_known_prefix(&tag.name)) {
                continue;
            }
            let PhpDocTagValue::TypedVar {
                ty: Some(ty),
                variable,
                ..
            } = &tag.value
            else {
                continue;
            };
            if !prefix.is_empty() && prefix != "phpstan-" && contains_error(ty) {
                continue;
            }
            match variable {
                None => group.push((None, ty.clone())),
                Some(_) => match named.iter_mut().find(|(v, _)| v == variable) {
                    Some(slot) => slot.1 = ty.clone(),
                    None => named.push((variable.clone(), ty.clone())),
                },
            }
        }
        if !group.is_empty() {
            unnamed = group;
        }
    }
    if unnamed.is_empty() { named } else { unnamed }
}

fn has_known_prefix(name: &str) -> bool {
    ["phan-", "psalm-", "phpstan-"]
        .iter()
        .any(|p| name.starts_with(p))
}

fn contains_error(node: &TypeNode) -> bool {
    let mut found = false;
    walk(node, &mut |n| found |= matches!(n, TypeNode::Error));
    found
}

/// Calls `f` on `node` and every node nested in it.
fn walk(node: &TypeNode, f: &mut dyn FnMut(&TypeNode)) {
    f(node);
    match node {
        TypeNode::Nullable(inner)
        | TypeNode::ArrayOf(inner)
        | TypeNode::Paren(inner)
        | TypeNode::VarianceArg(_, inner) => walk(inner, f),
        TypeNode::Union(ms) | TypeNode::Intersection(ms) => ms.iter().for_each(|m| walk(m, f)),
        TypeNode::Generic(_, args) => args.iter().for_each(|a| walk(a, f)),
        TypeNode::OffsetAccess(base, offset) => {
            walk(base, f);
            walk(offset, f);
        }
        TypeNode::ArrayShape {
            fields,
            unsealed_type,
            ..
        } => {
            fields.iter().for_each(|field| walk(&field.ty, f));
            if let Some((key, value)) = unsealed_type {
                if let Some(key) = key {
                    walk(key, f);
                }
                walk(value, f);
            }
        }
        TypeNode::Callable {
            templates,
            params,
            return_type,
            ..
        } => {
            for t in templates {
                for ty in [&t.bound, &t.lower, &t.default].into_iter().flatten() {
                    walk(ty, f);
                }
            }
            params.iter().for_each(|p| walk(&p.ty, f));
            if let Some(ret) = return_type {
                walk(ret, f);
            }
        }
        TypeNode::Conditional {
            target,
            if_true,
            if_false,
            ..
        } => {
            walk(target, f);
            walk(if_true, f);
            walk(if_false, f);
        }
        _ => {}
    }
}

impl<'a> Checker<'a> {
    /// Whether real PHPStan resolves `node` - a PHPDoc type written in
    /// `class` (`None` outside any class) - to an unresolvable type.
    pub(crate) fn type_node_unresolvable(&self, node: &TypeNode, class: Option<&str>) -> bool {
        let mut unresolvable = false;
        walk(node, &mut |n| {
            if !unresolvable {
                unresolvable = self.node_itself_unresolvable(n, class);
            }
        });
        unresolvable
    }

    fn node_itself_unresolvable(&self, node: &TypeNode, class: Option<&str>) -> bool {
        match node {
            TypeNode::Identifier(name) => self.identifier_unresolvable(name, class),
            TypeNode::Generic(name, args) => generic_arity_invalid(name, args.len()),
            TypeNode::IntRange(Some(min), Some(max)) => min > max,
            TypeNode::ConstFetch {
                class: owner,
                const_name,
            } => self.const_fetch_unresolvable(owner, const_name, class),
            TypeNode::OffsetAccess(base, offset) => {
                self.offset_access_unresolvable(base, offset, class)
            }
            TypeNode::Intersection(members) => {
                let types: Vec<Type> = members.iter().map(|m| self.node_type(m, class)).collect();
                // A class the registry never saw proves nothing either way.
                let unknown_class = |t: &Type| matches!(t, Type::Object(n, _) if self.registry.lookup_class(n).is_none());
                if types.iter().any(unknown_class) {
                    return false;
                }
                types.iter().enumerate().any(|(i, a)| {
                    types[i + 1..]
                        .iter()
                        .any(|b| types::types_definitely_disjoint(a, b))
                })
            }
            TypeNode::Callable {
                keyword,
                return_type,
                ..
            } => {
                let lower = keyword.to_ascii_lowercase();
                // `Closure(...)` names whatever `Closure` resolves to here:
                // only PHP's own class is a closure type.
                let foreign_closure = lower == "closure"
                    && self.name_ctx.resolve(keyword).trim_start_matches('\\') != "Closure";
                let pure_void = matches!(lower.as_str(), "pure-callable" | "pure-closure")
                    && matches!(
                        return_type.as_deref(),
                        Some(TypeNode::Identifier(r)) if r.eq_ignore_ascii_case("void")
                    );
                foreign_closure || pure_void
            }
            _ => false,
        }
    }

    fn identifier_unresolvable(&self, name: &str, class: Option<&str>) -> bool {
        match name.to_ascii_lowercase().as_str() {
            "$this" | "self" | "static" => class.is_none(),
            "parent" => class.is_none_or(|c| {
                self.registry
                    .lookup_class(c)
                    .is_none_or(|(_, info)| info.extends.is_empty())
            }),
            _ => {
                // A hyphenated name that isn't one of the keywords (`non-
                // empty-foo`) - PHPStan's `str_contains($name, '-')` rule.
                name.contains('-')
                    && !name.trim_start_matches('\\').starts_with("OCI-")
                    && matches!(types::parse_type_string(name),
                        Type::Object(n, _) if n.contains('-'))
            }
        }
    }

    fn const_fetch_unresolvable(&self, owner: &str, const_name: &str, class: Option<&str>) -> bool {
        let owner = match owner.to_ascii_lowercase().as_str() {
            "self" | "static" => match class {
                Some(c) => c.to_string(),
                None => return true,
            },
            "parent" => match class
                .and_then(|c| self.registry.lookup_class(c))
                .and_then(|(_, info)| info.extends.first().cloned())
            {
                Some(p) => p.to_string(),
                None => return true,
            },
            _ => self
                .name_ctx
                .resolve(owner)
                .trim_start_matches('\\')
                .to_string(),
        };
        let Some((canonical, _)) = self.registry.lookup_class(&owner) else {
            return true;
        };
        if const_name.eq_ignore_ascii_case("class") {
            return false;
        }
        match const_name.strip_suffix('*') {
            Some(prefix) => !self
                .registry
                .const_owner_by_class
                .get(canonical)
                .is_some_and(|consts| consts.keys().any(|k| k.starts_with(prefix))),
            None => self.registry.lookup_const(canonical, const_name).is_none(),
        }
    }

    fn offset_access_unresolvable(
        &self,
        base: &TypeNode,
        offset: &TypeNode,
        class: Option<&str>,
    ) -> bool {
        let base = self.node_type(base, class);
        let key = match offset.unwrap_paren() {
            TypeNode::IntLiteral(n) => Some(n.to_string()),
            TypeNode::StringLiteral(s) => Some(s.clone()),
            _ => None,
        };
        let int_key = matches!(offset.unwrap_paren(), TypeNode::IntLiteral(_));
        match &base {
            Type::ArrayShape(shape) => match &key {
                Some(k) => {
                    shape.is_sealed() && shape.field(&types::ShapeKey::from_name(k)).is_none()
                }
                None => false,
            },
            Type::Array(..) | Type::Intersection(_) => {
                key.is_none() && is_class_name_offset(offset)
            }
            Type::String | Type::StringLiteral(_) => !int_key,
            Type::Int
            | Type::IntLiteral(_)
            | Type::IntRange(..)
            | Type::Float
            | Type::FloatLiteral(_)
            | Type::Bool
            | Type::True
            | Type::False
            | Type::Null
            | Type::Void => true,
            _ => false,
        }
    }

    /// `node`'s type, resolved in the current name scope - for the
    /// structural checks above, not stored anywhere.
    fn node_type(&self, node: &TypeNode, class: Option<&str>) -> Type {
        let ty = self.resolve_type_hint(Some(&node.to_source_string()));
        let Some(c) = class else {
            return ty;
        };
        // `@phpstan-type`/`@phpstan-import-type` aliases of the class.
        let ty = match self.registry.classes.get(c) {
            Some(info) if !info.type_aliases.is_empty() => {
                crate::registry::substitute_type_aliases(ty, &info.type_aliases)
            }
            _ => ty,
        };
        self.resolve_pseudo(&ty, c)
    }
}

/// A bare name used as an array offset (`string[optional]`): a class
/// reference there, never a key.
fn is_class_name_offset(offset: &TypeNode) -> bool {
    matches!(offset.unwrap_paren(), TypeNode::Identifier(name)
        if !matches!(types::parse_type_string(name), Type::Int | Type::String | Type::Mixed(_)))
}

/// Too many type arguments for a built-in generic PHPStan resolves
/// itself (`array<a, b, c>`, `list<int, string>`).
fn generic_arity_invalid(name: &str, count: usize) -> bool {
    let max = match name.trim_start_matches('\\').to_ascii_lowercase().as_str() {
        "array" | "non-empty-array" | "iterable" => 2,
        "list" | "non-empty-list" | "class-string" | "interface-string" | "trait-string"
        | "enum-string" | "key-of" | "value-of" => 1,
        _ => return false,
    };
    count > max
}
