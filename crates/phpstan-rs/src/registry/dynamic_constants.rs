//! `dynamicConstantNames`: a configured constant's declared value is
//! only what one environment defines, so a read of it sees either the
//! configured type or the value's general type, never the literal.

use crate::registry::{Registry, resolve_names_in_type};
use crate::types::{self, Type};
use syntax::name_resolution::NameContext;

/// A value's general type: every literal widened to its base type.
fn generalize(ty: &Type) -> Type {
    match ty {
        Type::IntLiteral(_) | Type::IntRange(..) => Type::Int,
        Type::StringLiteral(_) => Type::String,
        Type::FloatLiteral(_) => Type::Float,
        Type::True | Type::False => Type::Bool,
        Type::ArrayShape(shape) => {
            let value = shape
                .fields
                .iter()
                .map(|f| generalize(&f.ty))
                .reduce(|a, b| if a == b { a } else { Type::Union(vec![a, b]) })
                .unwrap_or(Type::Mixed(false));
            Type::Array(Box::new(Type::unknown_array_key()), Box::new(value))
        }
        Type::Union(members) => {
            let mut out: Vec<Type> = Vec::new();
            for m in members.iter().map(generalize) {
                if !out.contains(&m) {
                    out.push(m);
                }
            }
            if out.len() == 1 {
                out.remove(0)
            } else {
                Type::Union(out)
            }
        }
        other => other.clone(),
    }
}

fn class_constant_key(class: &str, name: &str) -> String {
    format!(
        "{}::{name}",
        class.trim_start_matches('\\').to_ascii_lowercase()
    )
}

impl Registry {
    /// Replaces the configured `dynamicConstantNames` - `(name, type)`
    /// pairs, `type` empty for "the value's general type".
    pub fn set_dynamic_constant_names(&mut self, entries: &[(String, Option<String>)]) {
        let ctx = NameContext::default();
        self.dynamic_constant_names = entries
            .iter()
            .map(|(name, ty)| {
                let name = name.trim_start_matches('\\');
                let key = match name.split_once("::") {
                    Some((class, constant)) => class_constant_key(class, constant),
                    None => name.to_string(),
                };
                let ty = ty
                    .as_deref()
                    .map(|t| resolve_names_in_type(types::parse_type_string(t), &ctx));
                (key, ty)
            })
            .collect();
    }

    /// The type a read of global constant `name` sees when it's
    /// configured as dynamic, given its declared type (if any).
    pub(crate) fn dynamic_global_constant_type(
        &self,
        name: &str,
        declared: Option<&Type>,
    ) -> Option<Type> {
        let configured = self
            .dynamic_constant_names
            .get(name.trim_start_matches('\\'))?;
        configured.clone().or_else(|| declared.map(generalize))
    }

    /// The class-constant counterpart of `dynamic_global_constant_type`.
    pub(crate) fn dynamic_class_constant_type(
        &self,
        class: &str,
        name: &str,
        declared: &Type,
    ) -> Option<Type> {
        let configured = self
            .dynamic_constant_names
            .get(&class_constant_key(class, name))?;
        Some(configured.clone().unwrap_or_else(|| generalize(declared)))
    }

    /// The configured type of a dynamic global/class constant, when one
    /// was given explicitly.
    pub(crate) fn configured_constant_type(
        &self,
        class: Option<&str>,
        name: &str,
    ) -> Option<&Type> {
        let key = match class {
            Some(class) => class_constant_key(class, name),
            None => name.trim_start_matches('\\').to_string(),
        };
        self.dynamic_constant_names.get(&key)?.as_ref()
    }
}
