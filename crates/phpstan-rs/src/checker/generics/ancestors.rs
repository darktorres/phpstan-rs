//! Real PHPStan's `GenericAncestorsCheck`, run the way
//! `ClassAncestorsRule`/`InterfaceAncestorsRule`/`EnumAncestorsRule`/
//! `UsedTraitsRule` run it: every `@extends`/`@implements`/`@use` tag of a
//! declaration must name one of its real ancestors with a well-formed
//! generic type, and an ancestor that is generic needs its tag.

use std::sync::Arc;

use crate::checker::{Checker, class_kind_identifier};
use crate::registry;
use phpdoc::grammar::{self, PhpDocTagValue};
use syntax::ast::{ClassDecl, ClassKind, QualifiedName};
use syntax::token::Span;
use types::{self, PositionVariance, Type};

type Templates = rustc_hash::FxHashMap<String, types::TemplateParam>;

#[derive(Clone, Copy, PartialEq)]
enum AncestorTag {
    Extends,
    Implements,
    Use,
}

impl AncestorTag {
    fn word(self) -> &'static str {
        match self {
            AncestorTag::Extends => "extends",
            AncestorTag::Implements => "implements",
            AncestorTag::Use => "use",
        }
    }

    /// Every spelling of the tag, in `PhpDocNodeResolver`'s order (a
    /// later one replaces an earlier one for the same class).
    fn spellings(self) -> &'static [&'static str] {
        match self {
            AncestorTag::Extends => &[
                "extends",
                "phan-extends",
                "phan-inherits",
                "template-extends",
                "phpstan-extends",
            ],
            AncestorTag::Implements => &["implements", "template-implements", "phpstan-implements"],
            AncestorTag::Use => &["use", "template-use", "phpstan-use"],
        }
    }
}

/// The per-rule wording of one `GenericAncestorsCheck` run. `None` for a
/// message the rule never reaches (its declaration can't have that kind
/// of ancestor at all).
struct AncestorMessages {
    incompatible: String,
    unresolvable: String,
    no_names: String,
    /// "The @extends tag of class C describes" and the rest after the
    /// named class: "but the class extends" and whether the real
    /// ancestors follow a colon (no final period) or not.
    wrong_parent: Option<(String, String, bool)>,
    /// "... generic class X but does not specify its types: ".
    missing_generics: Option<String>,
    variance_context: Option<String>,
}

/// `TypeProjectionHelper::describe` for a call-site variance
/// annotation; `None` for an ordinary (invariant) argument.
fn call_site_projection(arg: &Type) -> Option<String> {
    arg.call_site_variance().map(|_| arg.to_string())
}

impl<'a> Checker<'a> {
    /// The ancestor checks of a class, interface or trait declaration.
    pub(crate) fn check_class_ancestors(&mut self, fqn: &str, c: &ClassDecl) {
        let doc = c.doc_comment.as_deref();
        let templates = self.declared_templates(fqn, doc);
        match c.kind {
            ClassKind::Class => {
                let extends: Vec<String> = c
                    .extends
                    .iter()
                    .map(|n| self.resolve_class_ref(n))
                    .collect();
                let implements: Vec<String> = c
                    .implements
                    .iter()
                    .map(|n| self.resolve_class_ref(n))
                    .collect();
                let messages = class_messages(fqn, AncestorTag::Extends);
                self.check_ancestor_tags(
                    fqn,
                    &templates,
                    doc,
                    AncestorTag::Extends,
                    &extends,
                    &messages,
                    c.span,
                );
                let messages = class_messages(fqn, AncestorTag::Implements);
                self.check_ancestor_tags(
                    fqn,
                    &templates,
                    doc,
                    AncestorTag::Implements,
                    &implements,
                    &messages,
                    c.span,
                );
            }
            ClassKind::Interface => {
                let extends: Vec<String> = c
                    .extends
                    .iter()
                    .map(|n| self.resolve_class_ref(n))
                    .collect();
                let messages = interface_messages(fqn, AncestorTag::Extends);
                self.check_ancestor_tags(
                    fqn,
                    &templates,
                    doc,
                    AncestorTag::Extends,
                    &extends,
                    &messages,
                    c.span,
                );
                let messages = interface_messages(fqn, AncestorTag::Implements);
                self.check_ancestor_tags(
                    fqn,
                    &templates,
                    doc,
                    AncestorTag::Implements,
                    &[],
                    &messages,
                    c.span,
                );
            }
            ClassKind::Trait | ClassKind::Enum => {}
        }
        self.check_used_traits(
            fqn,
            &templates,
            c.kind,
            &c.uses,
            &c.uses_doc_comments,
            c.span,
        );
    }

    /// The ancestor checks of an enum declaration.
    pub(crate) fn check_enum_ancestors(&mut self, fqn: &str, e: &ClassDecl) {
        let doc = e.doc_comment.as_deref();
        let templates = self.declared_templates(fqn, doc);
        let implements: Vec<String> = e
            .implements
            .iter()
            .map(|n| self.resolve_class_ref(n))
            .collect();
        let messages = enum_messages(fqn, AncestorTag::Extends);
        self.check_ancestor_tags(
            fqn,
            &templates,
            doc,
            AncestorTag::Extends,
            &[],
            &messages,
            e.span,
        );
        let messages = enum_messages(fqn, AncestorTag::Implements);
        self.check_ancestor_tags(
            fqn,
            &templates,
            doc,
            AncestorTag::Implements,
            &implements,
            &messages,
            e.span,
        );
        self.check_used_traits(
            fqn,
            &templates,
            ClassKind::Enum,
            &e.uses,
            &e.uses_doc_comments,
            e.span,
        );
    }

    /// This declaration's own `@template`s, from its own doc comment (not
    /// the registry's, which may hold another declaration of the name).
    fn declared_templates(&self, fqn: &str, doc: Option<&str>) -> Templates {
        match doc {
            Some(doc) if doc.contains("template") => {
                registry::build_template_params(&phpdoc::docblock::parse(doc), fqn, &self.name_ctx)
            }
            _ => Templates::default(),
        }
    }

    fn resolve_class_ref(&self, name: &str) -> String {
        self.name_ctx
            .resolve(name)
            .trim_start_matches('\\')
            .to_string()
    }

    /// `UsedTraitsRule`, once per `use` statement: its traits, and the
    /// `@use` tags on its own doc comment.
    fn check_used_traits(
        &mut self,
        fqn: &str,
        templates: &Templates,
        kind: ClassKind,
        uses: &[QualifiedName],
        docs: &[Option<Arc<str>>],
        span: Span,
    ) {
        let (type_description, description) = match kind {
            ClassKind::Trait => ("trait", format!("trait {fqn}")),
            other => {
                let word = class_kind_identifier(&other);
                (word, format!("{word} {fqn}"))
            }
        };
        let upper = upper_first(&description);
        let messages = AncestorMessages {
            incompatible: format!("{upper} @use tag contains incompatible type"),
            unresolvable: format!("{upper} @use tag contains unresolvable type."),
            no_names: format!("{upper} has @use tag, but does not use any trait."),
            wrong_parent: Some((
                format!("The @use tag of {description} describes"),
                format!("but the {type_description} uses"),
                false,
            )),
            missing_generics: Some(format!("{upper} uses generic trait")),
            variance_context: Some(format!("of {description}")),
        };
        // Consecutive entries of one `use A, B;` statement share one doc
        // comment (the same `Arc`).
        let mut i = 0;
        while i < uses.len() {
            let doc = docs.get(i).cloned().flatten();
            let mut j = i + 1;
            while j < uses.len()
                && let (Some(a), Some(Some(b))) = (&doc, docs.get(j))
                && Arc::ptr_eq(a, b)
            {
                j += 1;
            }
            let names: Vec<String> = uses[i..j]
                .iter()
                .map(|n| self.resolve_class_ref(n))
                .collect();
            self.check_ancestor_tags(
                fqn,
                templates,
                doc.as_deref(),
                AncestorTag::Use,
                &names,
                &messages,
                span,
            );
            i = j;
        }
    }

    /// `GenericAncestorsCheck::check` for one kind of tag.
    #[allow(clippy::too_many_arguments)]
    fn check_ancestor_tags(
        &mut self,
        fqn: &str,
        templates: &Templates,
        doc: Option<&str>,
        tag: AncestorTag,
        names: &[String],
        messages: &AncestorMessages,
        span: Span,
    ) {
        let label = fqn.to_string();
        let tag_types = match doc {
            Some(doc) if doc.contains(tag.word()) => {
                self.ancestor_tag_types(fqn, templates, doc, tag)
            }
            _ => Vec::new(),
        };
        let location = format!("PHPDoc tag @{}", tag.word());
        let mut unused: Vec<&String> = names.iter().collect();
        for (node, ty) in &tag_types {
            let Some((class, args)) = ty.as_object() else {
                self.report(
                    format!("{} {ty}.", messages.incompatible),
                    span,
                    2,
                    "generics.notCompatible",
                );
                continue;
            };
            if args.is_empty() {
                self.report(
                    format!("{} {ty}.", messages.incompatible),
                    span,
                    2,
                    "generics.notCompatible",
                );
                continue;
            }
            let Some(position) = names.iter().position(|n| n.eq_ignore_ascii_case(class)) else {
                match (&messages.wrong_parent, names.is_empty()) {
                    (Some((prefix, rest, colon)), false) => {
                        let listed = names.join(", ");
                        let message = if *colon {
                            format!("{prefix} {class} {rest}: {listed}")
                        } else {
                            format!("{prefix} {class} {rest} {listed}.")
                        };
                        self.report(message, span, 2, "generics.wrongParent");
                    }
                    _ => self.report(messages.no_names.clone(), span, 2, "generics.noParent"),
                }
                continue;
            };
            unused.retain(|n| !n.eq_ignore_ascii_case(&names[position]));
            // `GenericObjectTypeCheck`, without its call-site variance
            // half (never allowed here, see below).
            self.check_ancestor_generic_object(ty, &location, &label, span);
            if self.type_node_unresolvable(node, Some(fqn)) {
                self.report(
                    messages.unresolvable.clone(),
                    span,
                    2,
                    "generics.unresolvable",
                );
            }
            let referenced: Vec<String> = ty
                .nested_class_names()
                .iter()
                .map(|n| n.to_string())
                .collect();
            let mut seen = rustc_hash::FxHashSet::default();
            for name in referenced {
                if !seen.insert(name.clone()) {
                    continue;
                }
                match self.registry.lookup_class(&name) {
                    None => self.report(
                        format!("{location} has invalid type {name}."),
                        span,
                        0,
                        "class.notFound",
                    ),
                    Some((_, info)) if info.kind() == ClassKind::Trait && name != *class => self
                        .report(
                            format!("{location} has invalid type {name}."),
                            span,
                            2,
                            "generics.trait",
                        ),
                    _ => {}
                }
            }
            if let Some(context) = &messages.variance_context {
                let verb = match tag {
                    AncestorTag::Extends => "extended",
                    AncestorTag::Implements => "implemented",
                    AncestorTag::Use => "used",
                };
                let context = format!("in {verb} type {ty} {context}");
                let violations: Vec<_> = ty
                    .referenced_templates(PositionVariance::Static, self.registry)
                    .into_iter()
                    .filter(|(tp, position)| {
                        tp.scope.as_deref() == Some(fqn) && !position.is_valid_for(tp.variance)
                    })
                    .map(|(tp, position)| (tp.name.clone(), tp.variance, position))
                    .collect();
                for (name, declared, position) in violations {
                    self.report(format!(
                            "Template type {name} is declared as {}, but occurs in {} position {context}.",
                            PositionVariance::from(declared).describe(),
                            position.describe()
                        ),
                        span,
                        2,
                        "generics.variance",
                    );
                }
            }
            for arg in args {
                if let Some(projection) = call_site_projection(arg) {
                    self.report(format!(
                            "Call-site variance annotation of {projection} in generic type {ty} in {location} is not allowed."
                        ),
                        span,
                        2,
                        "generics.callSiteVarianceNotAllowed",
                    );
                }
            }
        }
        if let Some(missing) = &messages.missing_generics {
            for name in unused {
                let Some((canonical, info)) = self.registry.lookup_class(name) else {
                    continue;
                };
                let total = info.templates().len();
                let required = info
                    .templates()
                    .iter()
                    .filter(|t| !info.template_defaults().contains_key(*t))
                    .count();
                if required == 0 {
                    continue;
                }
                let mut list = info.templates().join(", ");
                if required != total {
                    list.push_str(&format!(" ({required}-{total} required)"));
                }
                self.report(
                    format!("{missing} {canonical} but does not specify its types: {list}"),
                    span,
                    6,
                    "missingType.generics",
                );
            }
        }
    }

    /// Each tag's parsed node and resolved type, keyed (in first-seen
    /// order) by the class it names, a later spelling replacing an
    /// earlier one.
    fn ancestor_tag_types(
        &self,
        fqn: &str,
        class_templates: &Templates,
        doc: &str,
        tag: AncestorTag,
    ) -> Vec<(grammar::TypeNode, Type)> {
        let parsed = grammar::parse(doc);
        let aliases = self
            .registry
            .classes
            .get(fqn)
            .map(|info| info.type_aliases().clone())
            .unwrap_or_default();
        let mut out: Vec<(String, grammar::TypeNode, Type)> = Vec::new();
        for spelling in tag.spellings() {
            for t in &parsed.tags {
                if t.name != *spelling {
                    continue;
                }
                let PhpDocTagValue::ExtendsLike { ty: node } = &t.value else {
                    continue;
                };
                let ty = registry::parse_type_node(node);
                let ty = registry::mark_template_params(ty, class_templates);
                let ty = registry::resolve_names_in_type(ty, &self.name_ctx);
                let ty = registry::substitute_type_aliases(ty, &aliases);
                let ty = self.registry.resolve_deferred_value_of_now(ty);
                let key = match ty.as_object() {
                    Some((class, _)) => class.to_ascii_lowercase(),
                    None => ty.to_string(),
                };
                match out.iter_mut().find(|(k, _, _)| *k == key) {
                    Some(slot) => {
                        slot.1 = node.clone();
                        slot.2 = ty;
                    }
                    None => out.push((key, node.clone(), ty)),
                }
            }
        }
        out.into_iter().map(|(_, node, ty)| (node, ty)).collect()
    }
}

fn upper_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn class_messages(fqn: &str, tag: AncestorTag) -> AncestorMessages {
    match tag {
        AncestorTag::Extends => AncestorMessages {
            incompatible: format!("Class {fqn} @extends tag contains incompatible type"),
            unresolvable: format!("Class {fqn} @extends tag contains unresolvable type."),
            no_names: format!("Class {fqn} has @extends tag, but does not extend any class."),
            wrong_parent: Some((
                format!("The @extends tag of class {fqn} describes"),
                "but the class extends".to_string(),
                false,
            )),
            missing_generics: Some(format!("Class {fqn} extends generic class")),
            variance_context: Some(format!("of class {fqn}")),
        },
        _ => AncestorMessages {
            incompatible: format!("Class {fqn} @implements tag contains incompatible type"),
            unresolvable: format!("Class {fqn} @implements tag contains unresolvable type."),
            no_names: format!(
                "Class {fqn} has @implements tag, but does not implement any interface."
            ),
            wrong_parent: Some((
                format!("The @implements tag of class {fqn} describes"),
                "but the class implements".to_string(),
                true,
            )),
            missing_generics: Some(format!("Class {fqn} implements generic interface")),
            variance_context: Some(format!("of class {fqn}")),
        },
    }
}

fn interface_messages(fqn: &str, tag: AncestorTag) -> AncestorMessages {
    match tag {
        AncestorTag::Extends => AncestorMessages {
            incompatible: format!("Interface {fqn} @extends tag contains incompatible type"),
            unresolvable: format!("Interface {fqn} @extends tag contains unresolvable type."),
            no_names: format!(
                "Interface {fqn} has @extends tag, but does not extend any interface."
            ),
            wrong_parent: Some((
                format!("The @extends tag of interface {fqn} describes"),
                "but the interface extends".to_string(),
                true,
            )),
            missing_generics: Some(format!("Interface {fqn} extends generic interface")),
            variance_context: Some(format!("of interface {fqn}")),
        },
        _ => AncestorMessages {
            incompatible: format!("Interface {fqn} @implements tag contains incompatible type"),
            unresolvable: format!("Interface {fqn} @implements tag contains unresolvable type."),
            no_names: format!(
                "Interface {fqn} has @implements tag, but can not implement any interface, must extend from it."
            ),
            wrong_parent: None,
            missing_generics: None,
            variance_context: None,
        },
    }
}

fn enum_messages(fqn: &str, tag: AncestorTag) -> AncestorMessages {
    match tag {
        AncestorTag::Extends => AncestorMessages {
            incompatible: format!("Enum {fqn} @extends tag contains incompatible type"),
            unresolvable: format!("Enum {fqn} @extends tag contains unresolvable type."),
            no_names: format!("Enum {fqn} has @extends tag, but cannot extend anything."),
            wrong_parent: None,
            missing_generics: None,
            variance_context: None,
        },
        _ => AncestorMessages {
            incompatible: format!("Enum {fqn} @implements tag contains incompatible type"),
            unresolvable: format!("Enum {fqn} @implements tag contains unresolvable type."),
            no_names: format!(
                "Enum {fqn} has @implements tag, but does not implement any interface."
            ),
            wrong_parent: Some((
                format!("The @implements tag of enum {fqn} describes"),
                "but the enum implements".to_string(),
                true,
            )),
            missing_generics: Some(format!("Enum {fqn} implements generic interface")),
            variance_context: Some(format!("of enum {fqn}")),
        },
    }
}
