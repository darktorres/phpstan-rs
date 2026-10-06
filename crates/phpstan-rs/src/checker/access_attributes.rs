//! Enforces the `php-language-extensions` access attributes read into
//! `ClassReflection::access`/`method_access`: `Friend` and `NamespaceVisibility`
//! (who may call a method or instantiate a class), `TestTag` (only test
//! code may), `RestrictTraitTo` (who may use a trait), and
//! `InjectableVersion` (constructors and `CheckInjectableVersion` methods
//! type-hint the injectable version, never a subtype of it). A class may
//! always use its own members. Enforced while the language-extensions
//! extension is active.

use crate::checker::Checker;
use crate::registry::access_attributes::AccessAttributes;
use syntax::ast::*;
use syntax::span::Span;
use types::Name;

const TEST_BASE_CLASSES: [&str; 2] = ["PHPUnit\\Framework\\TestCase", "Codeception\\Test\\Unit"];

impl<'a> Checker<'a> {
    /// Whether the code being checked is test code: a PHPUnit/Codeception
    /// test class, a `Test`/`Tests` namespace segment, or a file under a
    /// `test`/`tests` directory or named `*Test.php`.
    fn in_test_code(&self) -> bool {
        if let Some(class) = self.current_class.as_deref()
            && TEST_BASE_CLASSES
                .iter()
                .any(|base| self.registry.is_subclass_of(class, base))
        {
            return true;
        }
        if self
            .name_ctx
            .namespace()
            .split('\\')
            .any(|segment| segment == "Test" || segment == "Tests")
        {
            return true;
        }
        self.current_file.as_ref().is_some_and(|path| {
            path.components().any(|c| {
                let part = c.as_os_str().to_string_lossy().to_ascii_lowercase();
                part == "test" || part == "tests"
            }) || path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with("Test.php"))
        })
    }

    fn calling_class_is(&self, class: &str) -> bool {
        self.current_class
            .as_deref()
            .is_some_and(|c| c.eq_ignore_ascii_case(class))
    }

    /// `owner::method()` (or instantiating `owner`, `method` being
    /// `__construct`) against its access attributes.
    pub(crate) fn check_access_attributes(
        &mut self,
        owner: &str,
        method: &str,
        is_public: bool,
        span: Span,
    ) {
        if !self.settings.language_extensions {
            return;
        }
        let Some(info) = self.registry.classes().get(owner) else {
            return;
        };
        let own = info
            .method_access()
            .get(method.to_ascii_lowercase().as_str());
        let class_level: Option<&AccessAttributes> = is_public.then_some(info.access());
        if own.is_none() && class_level.is_none_or(|a| a.is_empty()) {
            return;
        }
        if self.calling_class_is(owner) {
            return;
        }
        let target = if syntax::ast::is_constructor_name(method) {
            format!("{owner}::__construct()")
        } else {
            format!("{owner}::{method}()")
        };
        let friends: Vec<&str> = own
            .iter()
            .chain(class_level.iter())
            .flat_map(|a| a.friends.iter().map(|f| f.as_ref()))
            .collect();
        if !friends.is_empty()
            && !friends.iter().any(|f| {
                self.current_class
                    .as_deref()
                    .is_some_and(|c| c.eq_ignore_ascii_case(f))
            })
        {
            let caller = self
                .current_class
                .clone()
                .unwrap_or_else(|| "code outside a class".to_string());
            self.report(
                format!(
                    "{target} can only be called by its friends ({}), not by {caller}.",
                    friends.join(", ")
                ),
                span,
                2,
                "attribute.friend",
            );
        }
        let visibility = own
            .and_then(|a| a.namespace_visibility.as_ref())
            .or_else(|| class_level.and_then(|a| a.namespace_visibility.as_ref()));
        if let Some(visibility) = visibility {
            let caller_ns = self.name_ctx.namespace().to_string();
            let allowed = caller_ns.eq_ignore_ascii_case(&visibility.namespace)
                || (!visibility.exclude_sub_namespaces
                    && caller_ns.len() > visibility.namespace.len()
                    && caller_ns[..visibility.namespace.len()]
                        .eq_ignore_ascii_case(&visibility.namespace)
                    && caller_ns.as_bytes()[visibility.namespace.len()] == b'\\');
            if !allowed {
                let scope = if visibility.exclude_sub_namespaces {
                    format!("namespace {}", visibility.namespace)
                } else {
                    format!("namespace {} and below", visibility.namespace)
                };
                self.report(
                    format!("{target} is only visible from {scope}."),
                    span,
                    2,
                    "attribute.namespaceVisibility",
                );
            }
        }
        let test_only =
            own.is_some_and(|a| a.test_only) || class_level.is_some_and(|a| a.test_only);
        if test_only && !self.in_test_code() {
            self.report(
                format!("{target} is marked #[TestTag] and can only be used from test code."),
                span,
                2,
                "attribute.testTag",
            );
        }
    }

    /// `RestrictTraitTo` on the traits `fqn` uses, and `InjectableVersion`
    /// in its constructor's (and `CheckInjectableVersion` methods')
    /// parameter types.
    pub(crate) fn check_class_access_attributes(&mut self, fqn: &str, c: &ClassDecl) {
        if !self.settings.language_extensions {
            return;
        }
        let Some(references) = self.registry.class_references_by_name(fqn) else {
            return;
        };
        let uses: Vec<(String, Vec<Name>)> = references
            .uses
            .iter()
            .filter_map(|t| {
                let restriction = &self
                    .registry
                    .classes()
                    .get(t.as_ref())?
                    .access()
                    .restrict_trait_to;
                (!restriction.is_empty()).then(|| (t.to_string(), restriction.clone()))
            })
            .collect();
        for (trait_name, allowed) in uses {
            if !allowed
                .iter()
                .any(|a| self.registry.is_subclass_of(fqn, a) || fqn.eq_ignore_ascii_case(a))
            {
                let span = c
                    .def
                    .used_traits()
                    .find(|u| {
                        u.resolved_class()
                            .is_some_and(|resolved| resolved.eq_ignore_ascii_case(&trait_name))
                    })
                    .map_or(c.name.span, |u| u.span);
                self.report(format!(
                        "Trait {trait_name} can only be used by {} and their subclasses, not {fqn}.",
                        allowed.iter().map(|a| a.as_ref()).collect::<Vec<_>>().join(", ")
                    ),
                    span,
                    2,
                    "attribute.restrictTraitTo",
                );
            }
        }
        for m in &c.def.methods {
            let checked = m.function.is_constructor()
                || info
                    .method_access()
                    .get(m.function.name.to_ascii_lowercase().as_str())
                    .is_some_and(|a| a.check_injectable_version);
            if !checked {
                continue;
            }
            let Some(sig) = self
                .registry
                .lookup_own_method(fqn, m.function.name.as_ref())
            else {
                continue;
            };
            let findings: Vec<(Span, String, String, String)> = sig
                .params()
                .iter()
                .zip(&m.function.params)
                .flat_map(|(param, decl)| {
                    param
                        .ty()
                        .referenced_class_names()
                        .into_iter()
                        .map(|name| name.to_string())
                        .filter_map(|name| {
                            let injectable = self.injectable_version_of(&name)?;
                            Some((decl.span, decl.name.to_string(), name, injectable))
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            for (span, param, used, injectable) in findings {
                self.report(format!(
                        "Parameter ${param} of {fqn}::{}() is type-hinted with {used}; inject its #[InjectableVersion] {injectable} instead.",
                        m.function.name
                    ),
                    span,
                    2,
                    "attribute.injectableVersion",
                );
            }
        }
    }

    /// The `InjectableVersion` class or interface `class` is a strict
    /// subtype of, if any.
    fn injectable_version_of(&self, class: &str) -> Option<String> {
        let (canonical, info) = self.registry.lookup_class(class)?;
        if info.access().injectable_version {
            return None;
        }
        self.registry
            .all_ancestor_names(canonical)
            .into_iter()
            .find(|a| {
                !a.eq_ignore_ascii_case(canonical)
                    && self
                        .registry
                        .classes()
                        .get(a.as_ref())
                        .is_some_and(|i| i.access().injectable_version)
            })
            .map(|a| a.to_string())
    }
}
