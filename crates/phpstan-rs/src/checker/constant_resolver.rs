//! PHPStan's `ConstantResolver`: the type a constant read sees. A
//! predefined constant whose value depends on the PHP build reads as its
//! general type (`predefined_constants`); a `dynamicConstantNames` entry is
//! only what one environment defines, so a read of it sees either the
//! configured type or the value's general type, never the literal.

use rustc_hash::FxHashMap;
use types::{self, Type};

use crate::checker::predefined_constants::predefined_constant_type;
use crate::registry::Registry;

fn class_constant_key(class: &str, name: &str) -> String {
    format!(
        "{}::{name}",
        class.trim_start_matches('\\').to_ascii_lowercase()
    )
}

#[derive(Debug, Default, Clone)]
pub struct ConstantResolver {
    // The configured `dynamicConstantNames`, keyed by a global constant's
    // name or `lowercased class::NAME` for a class constant; `None` reads
    // the declared value's general type.
    dynamic: FxHashMap<String, Option<Type>>,
}

impl ConstantResolver {
    /// `dynamicConstantNames` as `(name, type)` pairs, `type` unset for
    /// "the value's general type".
    pub fn new(entries: &[(String, Option<String>)]) -> ConstantResolver {
        let dynamic = entries
            .iter()
            .map(|(name, ty)| {
                let name = name.trim_start_matches('\\');
                let key = match name.split_once("::") {
                    Some((class, constant)) => class_constant_key(class, constant),
                    None => name.to_string(),
                };
                let ty = ty.as_deref().map(types::parse_type_string);
                (key, ty)
            })
            .collect();
        ConstantResolver { dynamic }
    }

    /// Whether any constant is configured as dynamic.
    pub fn has_dynamic_constants(&self) -> bool {
        !self.dynamic.is_empty()
    }

    /// The type a read of global constant `primary` sees, else of
    /// `fallback` (an unqualified name's global-namespace candidate):
    /// a predefined constant's general type, a dynamic constant's
    /// configured or general type, or the declared type.
    pub(crate) fn global_constant_type(
        &self,
        registry: &Registry,
        primary: &str,
        fallback: Option<&str>,
    ) -> Option<Type> {
        let predefined =
            |n: &str| registry.php_version_consensus(|v| predefined_constant_type(n, v));
        if let Some(ty) = predefined(primary).or_else(|| fallback.and_then(predefined)) {
            return Some(ty);
        }
        let found = registry
            .global_constants
            .get(primary)
            .map(|ty| (primary, ty))
            .or_else(|| fallback.and_then(|f| registry.global_constants.get(f).map(|ty| (f, ty))));
        // A dynamic constant is known even when nothing declares it.
        let dynamic = match found {
            Some((name, ty)) => self.dynamic_global_constant_type(name, Some(ty)),
            None => self
                .dynamic_global_constant_type(primary, None)
                .or_else(|| fallback.and_then(|f| self.dynamic_global_constant_type(f, None))),
        };
        dynamic.or_else(|| found.map(|(_, ty)| ty.clone()))
    }

    /// The type a read of global constant `name` sees when it's
    /// configured as dynamic, given its declared type (if any).
    fn dynamic_global_constant_type(&self, name: &str, declared: Option<&Type>) -> Option<Type> {
        let configured = self.dynamic.get(name.trim_start_matches('\\'))?;
        configured
            .clone()
            .or_else(|| declared.map(Type::generalize))
    }

    /// The type a read of class constant `owner::name`, declared as
    /// `declared`, sees.
    pub(crate) fn class_constant_type(&self, owner: &str, name: &str, declared: &Type) -> Type {
        match self.dynamic.get(&class_constant_key(owner, name)) {
            Some(configured) => configured.clone().unwrap_or_else(|| declared.generalize()),
            None => declared.clone(),
        }
    }

    /// The configured type of a dynamic global/class constant, when one
    /// was given explicitly.
    pub(crate) fn configured_type(&self, class: Option<&str>, name: &str) -> Option<&Type> {
        let key = match class {
            Some(class) => class_constant_key(class, name),
            None => name.trim_start_matches('\\').to_string(),
        };
        self.dynamic.get(&key)?.as_ref()
    }
}
