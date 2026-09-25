//! Real PHPStan's `GenericAncestorsCheck`, run the way
//! `ClassAncestorsRule`/`InterfaceAncestorsRule`/`EnumAncestorsRule`/
//! `UsedTraitsRule` run it: every `@extends`/`@implements`/`@use` tag of a
//! declaration must name one of its real ancestors with a well-formed
//! generic type, and an ancestor that is generic needs its tag.

use std::sync::Arc;

use crate::checker::{Checker, class_kind_identifier};
use crate::phpdoc_grammar::{self, PhpDocTagValue};
use crate::registry;
use crate::types::{self, Type};
use syntax::ast::{ClassDecl, ClassKind, EnumDecl, QualifiedName};
use syntax::token::Span;

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

/// A position's variance while walking an ancestor type -
/// `TemplateTypeVariance` with its `static` value.
#[derive(Clone, Copy, PartialEq)]
enum Position {
    Invariant,
    Covariant,
    Contravariant,
    Bivariant,
    Static,
}

impl Position {
    fn compose(self, other: Position) -> Position {
        match self {
            Position::Contravariant => match other {
                Position::Contravariant => Position::Covariant,
                Position::Covariant => Position::Contravariant,
                Position::Bivariant => Position::Bivariant,
                _ => Position::Invariant,
            },
            Position::Covariant => match other {
                Position::Contravariant => Position::Contravariant,
                Position::Covariant => Position::Covariant,
                Position::Bivariant => Position::Bivariant,
                _ => Position::Invariant,
            },
            Position::Invariant => Position::Invariant,
            Position::Bivariant => Position::Bivariant,
            Position::Static => other,
        }
    }

    fn from_variance(v: types::Variance) -> Position {
        match v {
            types::Variance::Invariant => Position::Invariant,
            types::Variance::Covariant => Position::Covariant,
            types::Variance::Contravariant => Position::Contravariant,
        }
    }

    fn from_call_site(v: types::CallSiteVariance) -> Position {
        match v {
            types::CallSiteVariance::Covariant => Position::Covariant,
            types::CallSiteVariance::Contravariant => Position::Contravariant,
            types::CallSiteVariance::Bivariant => Position::Bivariant,
        }
    }

    fn valid_for(self, declared: types::Variance) -> bool {
        declared == types::Variance::Invariant
            || matches!(self, Position::Bivariant | Position::Static)
            || Position::from_variance(declared) == self
    }

    fn describe(self) -> &'static str {
        match self {
            Position::Invariant => "invariant",
            Position::Covariant => "covariant",
            Position::Contravariant => "contravariant",
            Position::Bivariant => "bivariant",
            Position::Static => "static",
        }
    }
}

fn variance_word(v: types::Variance) -> &'static str {
    match v {
        types::Variance::Invariant => "invariant",
        types::Variance::Covariant => "covariant",
        types::Variance::Contravariant => "contravariant",
    }
}

/// `TypeProjectionHelper::describe` for a call-site variance
/// annotation; `None` for an ordinary (invariant) argument.
fn call_site_projection(arg: &Type) -> Option<String> {
    match arg {
        Type::VarianceArg(..) => Some(arg.to_string()),
        _ => None,
    }
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
    pub(crate) fn check_enum_ancestors(&mut self, fqn: &str, e: &EnumDecl) {
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
                registry::build_template_params(&crate::docblock::parse(doc), fqn, &self.name_ctx)
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
            let Type::Object(class, args) = ty else {
                self.report(
                    label.clone(),
                    format!("{} {ty}.", messages.incompatible),
                    span,
                    2,
                    "generics.notCompatible",
                );
                continue;
            };
            if args.is_empty() || class.starts_with("$$") {
                self.report(
                    label.clone(),
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
                        self.report(label.clone(), message, span, 2, "generics.wrongParent");
                    }
                    _ => self.report(
                        label.clone(),
                        messages.no_names.clone(),
                        span,
                        2,
                        "generics.noParent",
                    ),
                }
                continue;
            };
            unused.retain(|n| !n.eq_ignore_ascii_case(&names[position]));
            // `GenericObjectTypeCheck`, without its call-site variance
            // half (never allowed here, see below).
            self.check_ancestor_generic_object(ty, &location, &label, span);
            if self.type_node_unresolvable(node, Some(fqn)) {
                self.report(
                    label.clone(),
                    messages.unresolvable.clone(),
                    span,
                    2,
                    "generics.unresolvable",
                );
            }
            let mut referenced = Vec::new();
            collect_class_names(ty, &mut referenced);
            let mut seen = rustc_hash::FxHashSet::default();
            for name in referenced {
                if !seen.insert(name.clone()) {
                    continue;
                }
                match self.registry.lookup_class(&name) {
                    None => self.report(
                        label.clone(),
                        format!("{location} has invalid type {name}."),
                        span,
                        0,
                        "class.notFound",
                    ),
                    Some((_, info)) if info.kind == ClassKind::Trait && name != *class => self
                        .report(
                            label.clone(),
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
                let mut violations = Vec::new();
                self.collect_ancestor_variance(ty, Position::Static, fqn, &mut violations);
                for (name, declared, position) in violations {
                    self.report(
                        label.clone(),
                        format!(
                            "Template type {name} is declared as {}, but occurs in {} position {context}.",
                            variance_word(declared),
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
                    self.report(
                        label.clone(),
                        format!(
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
                let total = info.templates.len();
                let required = info
                    .templates
                    .iter()
                    .filter(|t| !info.template_defaults.contains_key(*t))
                    .count();
                if required == 0 {
                    continue;
                }
                let mut list = info.templates.join(", ");
                if required != total {
                    list.push_str(&format!(" ({required}-{total} required)"));
                }
                self.report(
                    label.clone(),
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
    ) -> Vec<(phpdoc_grammar::TypeNode, Type)> {
        let parsed = phpdoc_grammar::parse(doc);
        let aliases = self
            .registry
            .classes
            .get(fqn)
            .map(|info| info.type_aliases.clone())
            .unwrap_or_default();
        let mut out: Vec<(String, phpdoc_grammar::TypeNode, Type)> = Vec::new();
        for spelling in tag.spellings() {
            for t in &parsed.tags {
                if t.name != *spelling {
                    continue;
                }
                let PhpDocTagValue::ExtendsLike { ty: node } = &t.value else {
                    continue;
                };
                let ty = types::parse_type_string(&node.to_source_string());
                let ty = registry::mark_template_params(ty, class_templates);
                let ty = registry::resolve_names_in_type(ty, &self.name_ctx);
                let ty = registry::substitute_type_aliases(ty, &aliases);
                let key = match &ty {
                    Type::Object(class, _) => class.to_ascii_lowercase(),
                    other => other.to_string(),
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

    /// `VarianceCheck::check` from a `static` position: every template of
    /// `fqn` referenced in an ancestor type must sit where its declared
    /// variance allows.
    fn collect_ancestor_variance(
        &self,
        ty: &Type,
        position: Position,
        fqn: &str,
        out: &mut Vec<(String, types::Variance, Position)>,
    ) {
        match ty {
            Type::TemplateParam(tp) if tp.scope.as_deref() == Some(fqn) => {
                if !position.valid_for(tp.variance) {
                    out.push((tp.name.clone(), tp.variance, position));
                }
            }
            Type::VarianceArg(variance, inner) => {
                let inner_position = position.compose(Position::from_call_site(*variance));
                self.collect_ancestor_variance(inner, inner_position, fqn, out);
            }
            Type::Object(name, args) if name.starts_with("$$") => {
                for a in args {
                    self.collect_ancestor_variance(
                        a,
                        position.compose(Position::Bivariant),
                        fqn,
                        out,
                    );
                }
            }
            Type::ValueOf(_) | Type::ClassConstant { .. } => ty.for_each_child(&mut |t| {
                self.collect_ancestor_variance(t, position.compose(Position::Bivariant), fqn, out)
            }),
            Type::Object(class, args) => {
                for (i, arg) in args.iter().enumerate() {
                    let slot = self
                        .registry
                        .slot_variance(class, i)
                        .unwrap_or(types::Variance::Invariant);
                    let effective = match arg {
                        Type::VarianceArg(variance, _) => Position::from_call_site(*variance),
                        _ => Position::from_variance(slot),
                    };
                    let inner = match arg {
                        Type::VarianceArg(_, inner)
                        | Type::ValueOf(inner)
                        | Type::ClassConstant { class: inner, .. } => inner,
                        Type::Object(n, inner_args) if n.starts_with("$$") => {
                            inner_args.first().unwrap_or(arg)
                        }
                        _ => arg,
                    };
                    self.collect_ancestor_variance(inner, position.compose(effective), fqn, out);
                }
            }
            Type::Array(key, value) => {
                let inner = position.compose(Position::Covariant);
                self.collect_ancestor_variance(key, inner, fqn, out);
                self.collect_ancestor_variance(value, inner, fqn, out);
            }
            Type::ArrayShape(fields) => {
                let inner = position.compose(Position::Covariant);
                for t in fields.types() {
                    self.collect_ancestor_variance(t, inner, fqn, out);
                }
            }
            Type::Union(members) | Type::Intersection(members) => {
                for m in members {
                    self.collect_ancestor_variance(m, position, fqn, out);
                }
            }
            Type::Callable(Some(sig)) => {
                let params = position.compose(Position::Contravariant);
                for p in &sig.params {
                    self.collect_ancestor_variance(&p.ty, params, fqn, out);
                }
                self.collect_ancestor_variance(
                    &sig.return_type,
                    position.compose(Position::Covariant),
                    fqn,
                    out,
                );
            }
            _ => {}
        }
    }
}

fn upper_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Every class name `ty` references (the ancestor itself included).
fn collect_class_names(ty: &Type, out: &mut Vec<String>) {
    match ty {
        Type::Object(name, args) => {
            if !name.starts_with("$$") {
                out.push(name.clone());
            }
            for a in args {
                collect_class_names(a, out);
            }
        }
        Type::TemplateParam(_) => {}
        other => other.for_each_child(&mut |c| collect_class_names(c, out)),
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
