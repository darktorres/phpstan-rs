//! Property declaration legality, for plain and promoted properties
//! alike, mirroring real PHPStan's `PropertiesInInterfaceRule`,
//! `PropertyInClassRule`, `ReadOnlyPropertyRule`,
//! `ReadOnlyByPhpDocPropertyRule`, `InvalidCallablePropertyTypeRule`, and
//! `ExistingClassesInPropertiesRule`'s unresolvable-native-type half.

use crate::checker::Checker;
use diagnostics::DiagnosticBuilder;
use phpdoc::docblock;
use std::borrow::Cow;
use syntax::ast::*;
use syntax::span::Span;
use types::php_version::PhpFeature;

/// One property's declaration-relevant facts, shared by a plain
/// `PropertyDecl` and a promoted constructor parameter.
struct PropertyView<'a, 's> {
    name: &'a str,
    span: Span,
    visibility: Visibility,
    write_visibility: Option<Visibility>,
    is_static: bool,
    is_readonly: bool,
    is_abstract: bool,
    is_final: bool,
    type_hint: Option<&'a TypeHint>,
    has_default: bool,
    hooks: Option<&'a PropertyHooks>,
    doc_comment: Option<Cow<'s, str>>,
}

impl PropertyView<'_, '_> {
    fn hook_bodies(&self) -> Vec<(&PropertyHookBody, bool)> {
        let Some(hooks) = self.hooks else {
            return Vec::new();
        };
        let mut bodies = Vec::new();
        for hook in hooks.iter() {
            bodies.push((&hook.body, hook.is_final));
        }
        bodies
    }

    fn has_hooks(&self) -> bool {
        !self.hook_bodies().is_empty()
    }
}

fn is_bodiless(body: &PropertyHookBody) -> bool {
    matches!(body, PropertyHookBody::Abstract)
}

/// Whether a native type hint names `callable` anywhere in a union,
/// intersection, or nullable form.
fn native_hint_mentions_callable(hint: &str) -> bool {
    hint.split(|c: char| matches!(c, '|' | '&' | '(' | ')' | '?') || c.is_whitespace())
        .any(|part| {
            part.trim_start_matches('\\')
                .eq_ignore_ascii_case("callable")
        })
}

impl<'a> Checker<'a> {
    /// `registry_name` keys the class's registry entry (for a hooked
    /// property's virtual-ness), `display` names it in messages.
    pub(crate) fn check_property_declarations(
        &mut self,
        registry_name: &str,
        display: &str,
        kind: ClassKind,
        is_class_abstract: bool,
        properties: &[PropertyDecl],
        methods: &[MethodDecl],
    ) {
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        let source = self.syntax_source_owner();
        let mut views: Vec<PropertyView<'_, '_>> = properties
            .iter()
            .map(|p| PropertyView {
                name: &p.name,
                span: p.span,
                visibility: p.visibility,
                write_visibility: p.write_visibility,
                is_static: p.is_static,
                is_readonly: p.is_readonly,
                is_abstract: p.is_abstract,
                is_final: p.is_final,
                type_hint: p.type_hint.as_ref(),
                has_default: p.default.is_some(),
                hooks: p.hooks.as_ref(),
                doc_comment: p
                    .doc_comment
                    .as_ref()
                    .and_then(|ranges| source.as_ref().and_then(|source| ranges.text(source))),
            })
            .collect();
        if let Some(ctor) = methods.iter().find(|m| m.function.is_constructor()) {
            views.extend(
                ctor.function
                    .params
                    .iter()
                    .filter(|p| p.promotion.is_some())
                    .map(|p| PropertyView {
                        name: &p.name,
                        span: p.span,
                        visibility: p
                            .promotion
                            .map(|promotion| promotion.visibility)
                            .unwrap_or(Visibility::Public),
                        write_visibility: None,
                        is_static: false,
                        is_readonly: p.promotion.is_some_and(|promotion| promotion.readonly),
                        is_abstract: false,
                        is_final: p.promotion.is_some_and(|promotion| promotion.is_final),
                        type_hint: p.type_hint.as_ref(),
                        has_default: false,
                        hooks: p.hooks.as_deref(),
                        doc_comment: arenas
                            .doc_site(p.span.start)
                            .map(|site| Cow::Borrowed(site.text.as_ref())),
                    }),
            );
        }
        for p in properties {
            let property = format!("{display}::${}", p.name);
            self.check_type_declaration(
                p.type_hint.as_ref(),
                crate::checker::type_declarations::TypePosition::Property(&property),
                p.type_hint.as_ref().map(|h| h.span).unwrap_or(p.span),
            );
        }
        for view in &views {
            if kind == ClassKind::Interface {
                self.check_property_in_interface(view);
            } else if kind == ClassKind::Class {
                self.check_property_in_class(view, registry_name, is_class_abstract);
            }
            self.check_readonly_property_declaration(view);
            self.check_unresolvable_native_type(
                view.type_hint,
                format!(
                    "Property {display}::${} has unresolvable native type.",
                    view.name
                ),
                "property.unresolvableNativeType",
                view.span,
            );
            if view
                .type_hint
                .is_some_and(|h| native_hint_mentions_callable(&h.to_string_with(arenas)))
            {
                self.report_property_declaration(
                    format!(
                        "Property {display}::${} cannot have callable in its type declaration.",
                        view.name
                    ),
                    view.span,
                    "property.callableType",
                );
            }
        }
    }

    fn check_property_in_interface(&mut self, view: &PropertyView) {
        let bodies = view.hook_bodies();
        let error = if self
            .registry
            .php_supports_everywhere(PhpFeature::PropertyHooks)
            == Some(false)
        {
            Some((
                "Interfaces can include properties only on PHP 8.4 and later.",
                "property.inInterface",
            ))
        } else if !view.has_hooks() {
            Some((
                "Interfaces can only include hooked properties.",
                "property.nonHookedInInterface",
            ))
        } else if view.visibility != Visibility::Public {
            Some((
                "Interfaces cannot include non-public properties.",
                "property.nonPublicInInterface",
            ))
        } else if view.is_readonly {
            Some((
                "Interfaces cannot include readonly hooked properties.",
                "property.readOnlyInInterface",
            ))
        } else if view.is_static {
            Some((
                "Hooked properties cannot be static.",
                "property.hookedStatic",
            ))
        } else if view.is_abstract {
            Some((
                "Property in interface cannot be explicitly abstract.",
                "property.abstractInInterface",
            ))
        } else if view.is_final {
            Some((
                "Interfaces cannot include final properties.",
                "property.finalInInterface",
            ))
        } else if bodies.iter().any(|(_, is_final)| *is_final) {
            Some((
                "Property hook cannot be both abstract and final.",
                "property.abstractFinalHook",
            ))
        } else if bodies.iter().any(|(body, _)| !is_bodiless(body)) {
            Some((
                "Interfaces cannot include property hooks with bodies.",
                "property.hookBodyInInterface",
            ))
        } else {
            None
        };
        if let Some((message, identifier)) = error {
            self.report_property_declaration(message.to_string(), view.span, identifier);
        }
    }

    fn check_property_in_class(
        &mut self,
        view: &PropertyView,
        registry_name: &str,
        is_class_abstract: bool,
    ) {
        let unavailable =
            |feature: PhpFeature| self.registry.php_supports_everywhere(feature) == Some(false);
        let bodies = view.hook_bodies();
        let has_hooks = view.has_hooks();
        let is_virtual = self
            .registry
            .lookup_property(registry_name, view.name)
            .is_some_and(|sig| sig.is_virtual());
        let has_set_visibility = view.write_visibility.is_some();
        let error = if view.is_final && unavailable(PhpFeature::FinalProperties) {
            Some((
                "Final properties are supported only on PHP 8.4 and later.",
                "property.final",
            ))
        } else if view.is_abstract && !has_hooks {
            Some((
                "Only hooked properties can be declared abstract.",
                "property.abstractNonHooked",
            ))
        } else if view.is_abstract && !bodies.iter().any(|(body, _)| is_bodiless(body)) {
            Some((
                "Abstract properties must specify at least one abstract hook.",
                "property.abstractWithoutAbstractHook",
            ))
        } else if view.is_abstract && !is_class_abstract {
            Some((
                "Non-abstract classes cannot include abstract properties.",
                "property.abstract",
            ))
        } else if !view.is_abstract && bodies.iter().any(|(body, _)| is_bodiless(body)) {
            Some((
                "Non-abstract properties cannot include hooks without bodies.",
                "property.hookWithoutBody",
            ))
        } else if view.visibility == Visibility::Private && view.is_abstract {
            Some((
                "Property cannot be both abstract and private.",
                "property.abstractPrivate",
            ))
        } else if view.visibility == Visibility::Private && view.is_final {
            Some((
                "Property cannot be both final and private.",
                "property.finalPrivate",
            ))
        } else if view.visibility == Visibility::Private
            && bodies.iter().any(|(_, is_final)| *is_final)
        {
            Some((
                "Private property cannot have a final hook.",
                "property.finalPrivateHook",
            ))
        } else if view.is_abstract
            && (view.is_final
                || bodies
                    .iter()
                    .any(|(body, is_final)| is_bodiless(body) && *is_final))
        {
            Some((
                "Property cannot be both abstract and final.",
                "property.abstractFinal",
            ))
        } else if view.is_readonly && has_hooks {
            Some((
                "Hooked properties cannot be readonly.",
                "property.hookReadOnly",
            ))
        } else if view.is_static && has_hooks {
            Some((
                "Hooked properties cannot be static.",
                "property.hookedStatic",
            ))
        } else if has_set_visibility && unavailable(PhpFeature::AsymmetricVisibility) {
            Some((
                "Asymmetric visibility is supported only on PHP 8.4 and later.",
                "property.asymmetricVisibilityNotSupported",
            ))
        } else if view.is_static
            && has_set_visibility
            && unavailable(PhpFeature::AsymmetricVisibilityForStaticProperties)
        {
            Some((
                "Asymmetric visibility for static properties is supported only on PHP 8.5 and later.",
                "property.staticAsymmetricVisibility",
            ))
        } else if is_virtual && view.has_default {
            Some((
                "Virtual hooked properties cannot have a default value.",
                "property.virtualDefault",
            ))
        } else if has_hooks && unavailable(PhpFeature::PropertyHooks) {
            Some((
                "Property hooks are supported only on PHP 8.4 and later.",
                "property.hooksNotSupported",
            ))
        } else {
            None
        };
        if let Some((message, identifier)) = error {
            self.report_property_declaration(message.to_string(), view.span, identifier);
        }
    }

    fn check_readonly_property_declaration(&mut self, view: &PropertyView) {
        if view.is_readonly {
            let mut errors = Vec::new();
            if self
                .registry
                .php_supports_everywhere(PhpFeature::ReadOnlyProperties)
                == Some(false)
            {
                errors.push((
                    "Readonly properties are supported only on PHP 8.1 and later.",
                    "property.readOnlyNotSupported",
                ));
            }
            if view.type_hint.is_none() {
                errors.push((
                    "Readonly property must have a native type.",
                    "property.readOnlyNoNativeType",
                ));
            }
            if view.has_default {
                errors.push((
                    "Readonly property cannot have a default value.",
                    "property.readOnlyDefaultValue",
                ));
            }
            if view.is_static {
                errors.push((
                    "Readonly property cannot be static.",
                    "property.readOnlyStatic",
                ));
            }
            for (message, identifier) in errors {
                self.report_property_declaration(message.to_string(), view.span, identifier);
            }
            return;
        }
        let Some(doc) = view.doc_comment.map(docblock::parse_shared) else {
            return;
        };
        if doc.readonly && !doc.allow_private_mutation && view.has_default {
            self.report_builder(DiagnosticBuilder::new(
                "@readonly property cannot have a default value.".to_string(),
                view.span,
                0,
                "property.readOnlyByPhpDocDefaultValue",
            ));
        }
    }

    fn report_property_declaration(
        &mut self,
        message: String,
        span: Span,
        identifier: &'static str,
    ) {
        self.report_builder(DiagnosticBuilder::new(message, span, 0, identifier).non_ignorable());
    }
}
