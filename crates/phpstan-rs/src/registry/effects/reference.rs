//! The hierarchy queries effect resolution runs, written the direct way:
//! each walks the class tables by name and builds its answer from scratch.
//! The indexed versions are compared with these on sorted results.

use super::*;

/// Adds `key` with `transfer` to `out` once, by scanning it.
fn reference_push(out: &mut Vec<(DeclKey, Transfer)>, key: DeclKey, transfer: Transfer) {
    if !out.iter().any(|(k, t)| *k == key && *t == transfer) {
        out.push((key, transfer));
    }
}

impl Registry {
    pub(super) fn reference_implementations(&self, class: &str, method: &str) -> Vec<DeclKey> {
        if let Some((owner, sig)) = self.lookup_method_with_owner(class, method)
            && is_non_overridable(self, class, &owner, method, sig)
        {
            return vec![self.reached(&owner, method, sig)];
        }
        let mut out: Vec<DeclKey> = Vec::new();
        let mut seen: FxHashSet<DeclKey> = FxHashSet::default();
        for candidate in std::iter::once(Name::from(class)).chain(self.subclasses(class)) {
            let key = match self.lookup_method_with_owner(&candidate, method) {
                Some((owner, sig)) => self.reached(&owner, method, sig),
                None => match self.lookup_method_with_owner(&candidate, "__call") {
                    Some((owner, _)) => method_key(&owner, "__call"),
                    None => continue,
                },
            };
            if seen.insert(key.clone()) {
                out.push(key);
            }
        }
        out
    }

    pub(super) fn reference_overrides_of_builtin(
        &self,
        receiver: &str,
        builtin: &str,
    ) -> Vec<DeclKey> {
        if self.class(builtin).is_none() {
            return Vec::new();
        }
        let below = self.subclasses(receiver);
        let below_lower: FxHashSet<&str> = below.iter().map(Name::lower).collect();
        let mut out = Vec::new();
        for method in self.method_names(builtin) {
            if let Some((owner, sig)) = self.lookup_method_with_owner(receiver, method)
                && !sig.is_stub
            {
                out.push(method_key(&owner, method));
            }
            let declaring: Vec<&Name> = self.classes_declaring(method).collect();
            out.extend(
                declaring
                    .iter()
                    .filter(|class| below_lower.contains(class.lower()))
                    .map(|class| method_key(class, method)),
            );
        }
        out
    }

    pub(super) fn reference_methods_named(&self, method: &str) -> Vec<DeclKey> {
        let mut out: Vec<DeclKey> = Vec::new();
        let mut seen: FxHashSet<DeclKey> = FxHashSet::default();
        let declaring = self
            .classes_declaring(method)
            .chain(self.builtin_classes_running_code(method))
            .map(|class| (class, method));
        let magic = ["__call", "__callStatic"].into_iter().flat_map(|magic| {
            self.classes_declaring(magic)
                .map(move |class| (class, magic))
        });
        for (class, method) in declaring.chain(magic) {
            if let Some((owner, sig)) = self.lookup_method_with_owner(class, method) {
                let key = self.reached(&owner, method, sig);
                if seen.insert(key.clone()) {
                    out.push(key);
                }
            }
        }
        out
    }
}

impl Reach<'_> {
    pub(super) fn reference_every_method(&self, class: &str) -> Vec<DeclKey> {
        let registry = self.registry;
        let mut out = Vec::new();
        let mut seen: FxHashSet<DeclKey> = FxHashSet::default();
        for candidate in std::iter::once(Name::from(class)).chain(registry.subclasses(class)) {
            let Some(owners) = registry.method_owners(candidate.as_str()) else {
                continue;
            };
            for (method, owner) in owners.iter() {
                let key = method_key(owner, method);
                if seen.insert(key.clone()) {
                    out.push(key);
                }
            }
        }
        out
    }

    pub(super) fn reference_implicit_targets(
        &self,
        holder: &Holder,
        same: bool,
        operation: &Operation,
        out: &mut Vec<(DeclKey, Transfer)>,
        iterators: &mut Vec<Holder>,
    ) -> Option<()> {
        let registry = self.registry;
        let transfer = |class: Option<&Name>| implicit_transfer(holder, same, operation, class);
        let (classes, exact) = match holder {
            Holder::Classes(classes) => (classes, false),
            Holder::Made(Some(classes)) => (classes, true),
            Holder::Unknown | Holder::Made(None) => {
                for method in implicit_method_names(operation) {
                    for class in registry
                        .classes_declaring(method)
                        .chain(registry.builtin_classes_running_code(method))
                    {
                        if !self.implicit_methods(class, operation).contains(&method) {
                            continue;
                        }
                        if matches!(operation, Operation::Destroy)
                            && registry.is_builtin_class(class)
                        {
                            continue;
                        }
                        if let Some(key) = self.implicit_method(class, method) {
                            reference_push(out, key, transfer(None));
                        }
                    }
                }
                return Some(());
            }
        };
        for class in classes.iter() {
            let declared = matches!(operation, Operation::Property(..))
                && self.implicit_methods(class, operation).is_empty();
            let below = if exact || declared {
                Vec::new()
            } else {
                registry.subclasses(class)
            };
            for candidate in std::iter::once(class).chain(below.iter()) {
                for &method in self.implicit_methods(candidate, operation).iter() {
                    if let Some(key) = self.implicit_method(candidate, method) {
                        reference_push(out, key, transfer(Some(class)));
                    }
                }
                if !matches!(operation, Operation::Iterate { .. })
                    || registry.is_subclass_of(candidate, "Iterator")
                    || !registry.is_subclass_of(candidate, "IteratorAggregate")
                {
                    continue;
                }
                let given = match registry.lookup_method_with_owner(candidate, "getIterator") {
                    Some((owner, sig)) => {
                        declared_holder(registry, sig.return_type(), candidate, &owner)
                    }
                    None => Holder::Unknown,
                };
                if iterators.contains(&given) {
                    continue;
                }
                iterators.push(given.clone());
                let given = if iterators.len() > 4 {
                    Holder::Unknown
                } else {
                    given
                };
                self.reference_implicit_targets(&given, false, operation, out, iterators)?;
            }
        }
        Some(())
    }
}
