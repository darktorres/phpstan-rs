//! Real PHPStan's `GenericAncestorsCheck`, run the way
//! `ClassAncestorsRule`/`InterfaceAncestorsRule`/`EnumAncestorsRule`/
//! `UsedTraitsRule` run it: every `@extends`/`@implements`/`@use` tag of a
//! declaration must name one of its real ancestors with a well-formed
//! generic type, and an ancestor that is generic needs its tag.

use crate::checker::{Checker, class_kind_identifier};
use crate::registry;
use phpdoc::grammar::{self, PhpDocTagValue};
use syntax::ast::{ClassDef, ClassKind, QualifiedName, TraitUse};
use syntax::span::Span;
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

/// Which rule runs a `GenericAncestorsCheck`, for which tag.
#[derive(Clone, Copy)]
enum Rule {
    ClassExtends,
    ClassImplements,
    InterfaceExtends,
    InterfaceImplements,
    EnumExtends,
    EnumImplements,
    /// `UsedTraitsRule` on a declaration of this kind (`class`, `trait`...).
    Use(&'static str),
}

/// The per-rule wording of one `GenericAncestorsCheck` run, written out
/// when a message is reported. `None` for a message the rule never reaches
/// (its declaration can't have that kind of ancestor at all).
struct AncestorMessages<'a> {
    rule: Rule,
    fqn: &'a str,
}

impl<'a> AncestorMessages<'a> {
    fn new(rule: Rule, fqn: &'a str) -> AncestorMessages<'a> {
        AncestorMessages { rule, fqn }
    }

    /// The kind of declaration, as the messages name it.
    fn word(&self) -> &'static str {
        match self.rule {
            Rule::ClassExtends | Rule::ClassImplements => "class",
            Rule::InterfaceExtends | Rule::InterfaceImplements => "interface",
            Rule::EnumExtends | Rule::EnumImplements => "enum",
            Rule::Use(word) => word,
        }
    }

    fn tag(&self) -> &'static str {
        match self.rule {
            Rule::ClassExtends | Rule::InterfaceExtends | Rule::EnumExtends => "extends",
            Rule::ClassImplements | Rule::InterfaceImplements | Rule::EnumImplements => {
                "implements"
            }
            Rule::Use(_) => "use",
        }
    }

    /// "Class C": the declaration, the first word capitalized.
    fn head(&self) -> String {
        upper_first(&format!("{} {}", self.word(), self.fqn))
    }

    fn incompatible(&self) -> String {
        format!(
            "{} @{} tag contains incompatible type",
            self.head(),
            self.tag()
        )
    }

    fn unresolvable(&self) -> String {
        format!(
            "{} @{} tag contains unresolvable type.",
            self.head(),
            self.tag()
        )
    }

    fn no_names(&self) -> String {
        let head = self.head();
        match self.rule {
            Rule::ClassExtends => {
                format!("{head} has @extends tag, but does not extend any class.")
            }
            Rule::ClassImplements => {
                format!("{head} has @implements tag, but does not implement any interface.")
            }
            Rule::InterfaceExtends => {
                format!("{head} has @extends tag, but does not extend any interface.")
            }
            Rule::InterfaceImplements => format!(
                "{head} has @implements tag, but can not implement any interface, must extend from it."
            ),
            Rule::EnumExtends => format!("{head} has @extends tag, but cannot extend anything."),
            Rule::EnumImplements => {
                format!("{head} has @implements tag, but does not implement any interface.")
            }
            Rule::Use(_) => format!("{head} has @use tag, but does not use any trait."),
        }
    }

    /// "The @extends tag of class C describes" and the rest after the
    /// named class: "but the class extends" and whether the real
    /// ancestors follow a colon (no final period) or not.
    fn wrong_parent(&self) -> Option<(String, String, bool)> {
        let (word, fqn) = (self.word(), self.fqn);
        let (verb, colon) = match self.rule {
            Rule::ClassExtends => ("extends", false),
            Rule::ClassImplements => ("implements", true),
            Rule::InterfaceExtends => ("extends", true),
            Rule::EnumImplements => ("implements", true),
            Rule::Use(_) => ("uses", false),
            Rule::InterfaceImplements | Rule::EnumExtends => return None,
        };
        Some((
            format!("The @{} tag of {word} {fqn} describes", self.tag()),
            format!("but the {word} {verb}"),
            colon,
        ))
    }

    /// Whether the rule asks for the tag of a generic ancestor.
    fn can_miss_generics(&self) -> bool {
        !matches!(self.rule, Rule::InterfaceImplements | Rule::EnumExtends)
    }

    /// "... generic class X but does not specify its types: ".
    fn missing_generics(&self) -> Option<String> {
        let head = self.head();
        Some(match self.rule {
            Rule::ClassExtends => format!("{head} extends generic class"),
            Rule::ClassImplements => format!("{head} implements generic interface"),
            Rule::InterfaceExtends => format!("{head} extends generic interface"),
            Rule::EnumImplements => format!("{head} implements generic interface"),
            Rule::Use(_) => format!("{head} uses generic trait"),
            Rule::InterfaceImplements | Rule::EnumExtends => return None,
        })
    }

    fn variance_context(&self) -> Option<String> {
        match self.rule {
            Rule::InterfaceImplements | Rule::EnumExtends => None,
            _ => Some(format!("of {} {}", self.word(), self.fqn)),
        }
    }
}

/// `TypeProjectionHelper::describe` for a call-site variance
/// annotation; `None` for an ordinary (invariant) argument.
fn call_site_projection(arg: &Type) -> Option<String> {
    arg.call_site_variance().map(|_| arg.to_string())
}

impl<'a> Checker<'a> {
    /// The ancestor checks of a class, interface or trait declaration.
    pub(crate) fn check_class_ancestors(&mut self, fqn: &str, c: &ClassDef) {
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
                let messages = AncestorMessages::new(Rule::ClassExtends, fqn);
                self.check_ancestor_tags(
                    fqn,
                    &templates,
                    doc,
                    AncestorTag::Extends,
                    &extends,
                    &messages,
                    c.span,
                );
                let messages = AncestorMessages::new(Rule::ClassImplements, fqn);
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
                let messages = AncestorMessages::new(Rule::InterfaceExtends, fqn);
                self.check_ancestor_tags(
                    fqn,
                    &templates,
                    doc,
                    AncestorTag::Extends,
                    &extends,
                    &messages,
                    c.span,
                );
                let messages = AncestorMessages::new(Rule::InterfaceImplements, fqn);
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
            ClassKind::Enum => {
                let implements: Vec<String> = c
                    .implements
                    .iter()
                    .map(|n| self.resolve_class_ref(n))
                    .collect();
                let messages = AncestorMessages::new(Rule::EnumExtends, fqn);
                self.check_ancestor_tags(
                    fqn,
                    &templates,
                    doc,
                    AncestorTag::Extends,
                    &[],
                    &messages,
                    c.span,
                );
                let messages = AncestorMessages::new(Rule::EnumImplements, fqn);
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
            ClassKind::Trait => {}
        }
        self.check_used_traits(fqn, &templates, c.kind, &c.trait_uses);
    }

    /// This declaration's own `@template`s, from its own doc comment (not
    /// the registry's, which may hold another declaration of the name).
    fn declared_templates(&self, fqn: &str, doc: Option<&str>) -> Templates {
        match doc {
            Some(doc) if doc.contains("template") => registry::build_template_params(
                &phpdoc::docblock::parse_shared(doc),
                fqn,
                &self.name_ctx,
            ),
            _ => Templates::default(),
        }
    }

    fn resolve_class_ref(&self, name: &QualifiedName) -> String {
        name.resolved_class()
            .map_or_else(|| name.written.to_string(), str::to_string)
    }

    /// `UsedTraitsRule`, once per `use` statement: its traits, and the
    /// `@use` tags on its own doc comment.
    fn check_used_traits(
        &mut self,
        fqn: &str,
        templates: &Templates,
        kind: ClassKind,
        trait_uses: &[TraitUse],
    ) {
        if trait_uses.is_empty() {
            return;
        }
        let word = match kind {
            ClassKind::Trait => "trait",
            other => class_kind_identifier(&other),
        };
        let messages = AncestorMessages::new(Rule::Use(word), fqn);
        for trait_use in trait_uses {
            let names: Vec<String> = trait_use
                .traits
                .iter()
                .map(|n| self.resolve_class_ref(n))
                .collect();
            self.check_ancestor_tags(
                fqn,
                templates,
                trait_use.doc_comment.as_deref(),
                AncestorTag::Use,
                &names,
                &messages,
                trait_use.span,
            );
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
        messages: &AncestorMessages<'_>,
        span: Span,
    ) {
        let tag_types = match doc {
            Some(doc) if doc.contains(tag.word()) => {
                self.ancestor_tag_types(fqn, templates, doc, tag)
            }
            _ => Vec::new(),
        };
        // Only a tag's own types are reported against these.
        let (label, location) = if tag_types.is_empty() {
            (String::new(), String::new())
        } else {
            (fqn.to_string(), format!("PHPDoc tag @{}", tag.word()))
        };
        let mut unused: Vec<&String> = names.iter().collect();
        for (node, ty) in &tag_types {
            let Some((class, args)) = ty.as_object() else {
                self.report(
                    format!("{} {ty}.", messages.incompatible()),
                    span,
                    2,
                    "generics.notCompatible",
                );
                continue;
            };
            if args.is_empty() {
                self.report(
                    format!("{} {ty}.", messages.incompatible()),
                    span,
                    2,
                    "generics.notCompatible",
                );
                continue;
            }
            let Some(position) = names.iter().position(|n| n.eq_ignore_ascii_case(class)) else {
                match (messages.wrong_parent(), names.is_empty()) {
                    (Some((prefix, rest, colon)), false) => {
                        let listed = names.join(", ");
                        let message = if colon {
                            format!("{prefix} {class} {rest}: {listed}")
                        } else {
                            format!("{prefix} {class} {rest} {listed}.")
                        };
                        self.report(message, span, 2, "generics.wrongParent");
                    }
                    _ => self.report(messages.no_names(), span, 2, "generics.noParent"),
                }
                continue;
            };
            unused.retain(|n| !n.eq_ignore_ascii_case(&names[position]));
            // `GenericObjectTypeCheck`, without its call-site variance
            // half (never allowed here, see below).
            self.check_ancestor_generic_object(ty, &location, &label, span);
            if self.type_node_unresolvable(node, Some(fqn)) {
                self.report(messages.unresolvable(), span, 2, "generics.unresolvable");
            }
            let referenced: Vec<String> = ty
                .referenced_class_names()
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
            if let Some(context) = messages.variance_context() {
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
        if messages.can_miss_generics() {
            for name in unused {
                let Some((canonical, info)) = self.registry.lookup_class(name) else {
                    continue;
                };
                let total = info.templates().len();
                let required = info
                    .templates()
                    .iter()
                    .filter(|t| t.default.is_none())
                    .count();
                if required == 0 {
                    continue;
                }
                let mut list = info.template_names().collect::<Vec<_>>().join(", ");
                if required != total {
                    list.push_str(&format!(" ({required}-{total} required)"));
                }
                let missing = messages.missing_generics().unwrap_or_default();
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
        let parsed = grammar::parse_shared(doc);
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
