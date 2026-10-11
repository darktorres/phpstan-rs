use rustc_hash::FxHashMap;
#[cfg(debug_assertions)]
use rustc_hash::FxHashSet;
use syntax::name::NameMap;

use syntax::name::Name;

use crate::registry::{ClassId, Registry};
use types::{self, Type, Variance};

impl Registry {
    /// This class's own declared variance for the `index`-th `@template`
    /// of `class` (`Covariant`/`Contravariant`/`Invariant` - the same
    /// three-way classification `TemplateParam::variance` itself carries,
    /// just looked up by position rather than read off an already-marked
    /// node) - `None` when `class` isn't a registered class at all, or
    /// declares fewer templates than `index` needs, either of which means
    /// this project has no real basis to say whether subtyping flows
    /// through that slot at all, let alone which direction. Callers treat
    /// `None` as "don't know, don't guess" - matching this project's
    /// general leniency elsewhere - rather than defaulting to `Invariant`
    /// and risking a false positive on a real but unregistered
    /// (uninstalled dependency, PHAR-bundled `phpstan/phpstan` itself, ...)
    /// generic class.
    pub(crate) fn slot_variance(&self, class: &str, index: usize) -> Option<Variance> {
        let class_id = *self.class_ids.get(class)?;
        let info = self.class_by_id(class_id)?;
        let name = info.templates.get(index)?;
        Some(if info.covariant_templates.contains(name) {
            Variance::Covariant
        } else if info.contravariant_templates.contains(name) {
            Variance::Contravariant
        } else {
            Variance::Invariant
        })
    }

    /// Binds a generic-typed receiver's own carried type arguments
    /// (`Collection<User>` -> `{"T": Object("User", [])}`) against
    /// `target`'s own declared `@template` parameter names, walking up
    /// `class`'s `extends`/`implements` chain to reach it if `target`
    /// isn't `class` itself - so a concrete subclass that
    /// fixes its parent's template param (`class IntCollection extends
    /// Collection<int> {}`) resolves `Collection`'s own `T` to `int` even
    /// though `IntCollection` isn't generic itself, and a generic
    /// subclass that passes its own template through (`class Box<T>
    /// extends Collection<T> {}`) resolves it to whatever `Box`'s own `T`
    /// is bound to at each use, not a fixed value.
    ///
    /// `target` matters because template names are scoped to the class
    /// that declares them, not globally unique - two unrelated classes in
    /// the same chain commonly both use `T` (e.g. `NullableType<T of
    /// Type> extends UnionType<T|BuiltinType<...>>`, where `UnionType`
    /// has its own, unrelated `T`). Returning bindings scoped to exactly
    /// `target`'s own level - not a single flat map merged across the
    /// whole chain - is what keeps those two `T`s from colliding into
    /// whichever one happened to be computed last.
    ///
    /// A non-generic class (empty `templates`), a bare unspecified-generic
    /// reference (empty `type_args`), or a chain with no matching
    /// `@extends`/`@implements` tag along the way to `target` all just
    /// leave that level's names unbound - `Type::substitute_templates` already
    /// treats an unbound `TemplateParam` as `Mixed`, the right
    /// conservative fallback for an "erased" generic.
    pub(crate) fn class_template_bindings(
        &self,
        class: &str,
        type_args: &[Type],
        target: &str,
    ) -> FxHashMap<String, Type> {
        let (Some(&class_id), Some(&target_id)) =
            (self.class_ids.get(class), self.class_ids.get(target))
        else {
            return FxHashMap::default();
        };
        let memo = &self.reach_memo.template_paths;
        let mut stats = true;
        let path = memo.get_or_compute((class_id, target_id), || {
            stats = false;
            self.template_path(class_id, target_id).map(Into::into)
        });
        syntax::stats::count(if stats {
            syntax::stats::Counter::TemplatePathHit
        } else {
            syntax::stats::Counter::TemplatePathMiss
        });
        let bindings = path
            .as_deref()
            .map(|path| self.bindings_along(path, type_args))
            .unwrap_or_default();
        #[cfg(debug_assertions)]
        assert_eq!(
            bindings,
            self.class_template_bindings_rec(class, type_args, target, &mut FxHashSet::default())
                .unwrap_or_default()
        );
        bindings
    }

    /// The levels from `from` up to `to` that `class_template_bindings`
    /// walks, `to` included, found in the order that search takes: the
    /// first parent, then each interface. The levels depend on the
    /// declarations alone, not on the type arguments.
    fn template_path(&self, from: ClassId, to: ClassId) -> Option<Vec<TemplateStep>> {
        fn search(
            registry: &Registry,
            class: ClassId,
            to: ClassId,
            stack: &mut Vec<ClassId>,
            path: &mut Vec<TemplateStep>,
        ) -> bool {
            syntax::stack::guard(|| {
                if stack.contains(&class) {
                    return false;
                }
                let Some(references) = registry.class_references(class) else {
                    return false;
                };
                if class == to {
                    path.push(TemplateStep {
                        class,
                        edge: TemplateEdge::Target,
                    });
                    return true;
                }
                stack.push(class);
                let mut found = false;
                if let Some(parent) = references.extends.first()
                    && let Some(&parent_id) = registry.class_ids.get(parent)
                {
                    path.push(TemplateStep {
                        class,
                        edge: TemplateEdge::Extends,
                    });
                    found = search(registry, parent_id, to, stack, path);
                    if !found {
                        path.pop();
                    }
                }
                if !found {
                    for iface in references.implements {
                        let Some(&iface_id) = registry.class_ids.get(iface) else {
                            continue;
                        };
                        path.push(TemplateStep {
                            class,
                            edge: TemplateEdge::Implements(iface.clone()),
                        });
                        if search(registry, iface_id, to, stack, path) {
                            found = true;
                            break;
                        }
                        path.pop();
                    }
                }
                stack.pop();
                found
            })
        }
        let mut path = Vec::new();
        search(self, from, to, &mut Vec::new(), &mut path).then_some(path)
    }

    /// The bindings of the last level of `path`, with `type_args` as the
    /// first level's arguments.
    fn bindings_along(&self, path: &[TemplateStep], type_args: &[Type]) -> FxHashMap<String, Type> {
        let mut args: Vec<Type> = type_args.to_vec();
        for step in path {
            let Some(info) = self.class_by_id(step.class) else {
                return FxHashMap::default();
            };
            let own_bindings: FxHashMap<String, Type> = info
                .templates
                .iter()
                .map(|s| s.to_string())
                .zip(args.iter().cloned())
                .collect();
            args = match &step.edge {
                TemplateEdge::Target => return own_bindings,
                TemplateEdge::Extends => self
                    .effective_extends_args(step.class)
                    .iter()
                    .map(|t| t.substitute_templates(&own_bindings, &self.relation()))
                    .collect(),
                TemplateEdge::Implements(iface) => self
                    .effective_implements_args_for(step.class, iface)
                    .map(|args| {
                        args.iter()
                            .map(|t| t.substitute_templates(&own_bindings, &self.relation()))
                            .collect()
                    })
                    .unwrap_or_default(),
            };
        }
        FxHashMap::default()
    }

    /// `None` means `target` isn't reachable along this path at all (so
    /// the caller should treat every one of its templates as unbound,
    /// same as `Some` with an empty map) - the distinction only matters
    /// internally, to know when to keep searching sibling branches below.
    /// `seen` guards against a cyclic `extends`/`implements` graph
    /// (invalid PHP, but a malformed/partially-loaded real corpus could
    /// still produce one) - cloned per branch, not threaded through by
    /// mutable reference, so two independent ancestors (e.g. two
    /// interfaces that both eventually reach a common shared parent, a
    /// real "diamond" shape) can each still be searched.
    #[cfg(debug_assertions)]
    fn class_template_bindings_rec(
        &self,
        class: &str,
        type_args: &[Type],
        target: &str,
        seen: &mut FxHashSet<String>,
    ) -> Option<FxHashMap<String, Type>> {
        syntax::stack::guard(|| {
            if !seen.insert(class.to_string()) {
                return None;
            }
            let class_id = *self.class_ids.get(class)?;
            let info = self.class_by_id(class_id)?;
            let references = self.class_references(class_id)?;
            let own_bindings: FxHashMap<String, Type> = info
                .templates
                .iter()
                .map(|s| s.to_string())
                .zip(type_args.iter().cloned())
                .collect();

            if class == target {
                return Some(own_bindings);
            }

            if let Some(parent) = references.extends.first() {
                let parent_args: Vec<Type> = self
                    .effective_extends_args(class_id)
                    .iter()
                    .map(|t| t.substitute_templates(&own_bindings, &self.relation()))
                    .collect();
                if let Some(found) = self.class_template_bindings_rec(
                    parent,
                    &parent_args,
                    target,
                    &mut seen.clone(),
                ) {
                    return Some(found);
                }
            }
            let implements_args = self.effective_implements_args(class_id);
            for iface in references.implements {
                let iface_args: Vec<Type> = implements_args
                    .get(iface)
                    .map(|args| {
                        args.iter()
                            .map(|t| t.substitute_templates(&own_bindings, &self.relation()))
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(found) =
                    self.class_template_bindings_rec(iface, &iface_args, target, &mut seen.clone())
                {
                    return Some(found);
                }
            }
            None
        })
    }

    /// Substitutes only the `TemplateParam`s in `ty` whose own `scope` is
    /// a key in `ancestor_bindings` (`{ancestor name => {template name =>
    /// concrete Type}}`, as built by `registry::resolve::flatten_phpdoc_
    /// inheritance_one_pass` for the class a backfilled signature is
    /// landing on) - everything else (a method-scoped template, or a
    /// class-scoped one from an ancestor not present in the map at all)
    /// passes through completely untouched. Real PHPStan's own identical
    /// mechanism, confirmed by direct read: `ResolvedPhpDocBlock::
    /// resolveTemplateTypeInTag` substitutes an *ancestor* tag's own
    /// class-level template through `$parentClass->getActiveTemplateType
    /// Map()` immediately, at docblock-merge time - not deferred to each
    /// later call site the way this project's own method-scoped templates
    /// correctly are (`Relation::infer_template_bindings`/`Type::substitute_templates`'s
    /// own call-site binding, purely name-keyed and blind to which
    /// class's `FunctionReflection` a signature came from). Doing this
    /// immediately, rather than leaving the raw class-scoped `TemplateParam`
    /// in the backfilled signature for some later call-time pass to
    /// resolve, matters for a real, distinct reason from "is a concrete
    /// binding available at all" (`template_param_scopes_all_safe_to_
    /// backfill`'s own job): a signature is also read directly when
    /// *checking its own declaring class's own method body* (seeding
    /// `locals` for its own parameters), a path that has no "external call
    /// site" to resolve anything through at all. Without resolving
    /// eagerly, a method implementing a generic interface (`@param
    /// TNodeType $node` on the interface) would leave `$node`'s own local
    /// type literally named `"TNodeType"` - not a concrete class, not
    /// `Mixed` - the instant its own body reads it. Resolving eagerly
    /// here, using
    /// this exact class's own concrete `@implements Rule<StaticCall>`
    /// binding, makes the stored signature already-concrete before
    /// anything ever reads it, closing that gap regardless of which of
    /// the (at least two) paths reads the signature next.
    pub(crate) fn substitute_class_scoped_ancestor_templates<'b>(
        ty: &Type,
        ancestor_bindings: &dyn Fn() -> &'b NameMap<FxHashMap<String, Type>>,
    ) -> Type {
        // The bindings are asked for at the first template that has a
        // scope, not before: most types hold no template.
        ty.replace_templates(&mut |tp| {
            tp.scope
                .as_deref()
                .and_then(|s| ancestor_bindings().get(s))
                .and_then(|b| b.get(&tp.name))
                .cloned()
        })
    }
}

/// One level of the walk from a class to an ancestor.
#[derive(Debug, Clone)]
pub(crate) struct TemplateStep {
    class: ClassId,
    edge: TemplateEdge,
}

/// How the walk goes on from a level.
#[derive(Debug, Clone)]
enum TemplateEdge {
    Extends,
    Implements(Name),
    /// The level is the ancestor sought.
    Target,
}
