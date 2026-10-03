//! `Project::check_architecture`: the configured `[[architecture]]`
//! rules against every analysed class. A class's dependencies are the
//! classes its declaration and body name (type hints, `new`, static
//! access, `instanceof`, `catch`, attributes) plus every class in the
//! PHPDoc-refined types of its methods and properties; built-in classes
//! and the class itself don't count.

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;

use crate::architecture::{ArchitectureRule, NamePattern, Selector};
use crate::registry::stubs;
use syntax::ast::*;
use syntax::span::Span;
use syntax::visit::{Visitor, walk_expr, walk_stmts};
use types::Type;

use super::Project;
use super::use_sites::{RefTarget, index_class_fn_refs_without_source};
use diagnostics::{Diagnostic, DiagnosticBuilder};

/// A class-like declaration's own facts the registry doesn't keep.
struct Declared<'a> {
    name_span: Span,
    span: Span,
    attributes: Vec<String>,
    public_methods: Vec<&'a str>,
}

/// Every class-like `stmts` declares unconditionally, by its lowercased
/// fully-qualified name.
fn collect_declared<'a>(stmts: &'a [Stmt], out: &mut FxHashMap<String, Declared<'a>>) {
    syntax::declarations::for_each_declaration(stmts, |declaration, _, _| {
        let syntax::declarations::Declared::Class(c) = declaration.declared else {
            return;
        };
        if declaration.conditional {
            return;
        }
        out.insert(
            declaration.name().to_ascii_lowercase(),
            Declared {
                name_span: c.name.span,
                span: declaration.span,
                attributes: c
                    .def
                    .attrs
                    .iter()
                    .map(|a| {
                        a.name
                            .resolved_class()
                            .unwrap_or(&a.name.written)
                            .to_string()
                    })
                    .collect(),
                public_methods: c
                    .def
                    .methods
                    .iter()
                    .filter(|m| m.visibility == Visibility::Public && !m.function.is_constructor())
                    .map(|m| m.function.name.as_ref())
                    .collect(),
            },
        );
    });
}

fn contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// Spans of the class operand of every `new` in `stmts`, wherever it is:
/// parameter defaults, property hooks and constants included.
fn new_class_spans(stmts: &[Stmt], out: &mut FxHashSet<Span>) {
    struct News<'o>(&'o mut FxHashSet<Span>);
    impl<'ast> Visitor<'ast> for News<'_> {
        fn visit_expr(&mut self, e: &'ast Expr) {
            if let ExprKind::New(class, _) = &e.kind {
                self.0.insert(class.span);
            }
            walk_expr(self, e);
        }
    }
    walk_stmts(&mut News(out), stmts);
}

/// `(selectors, the class's related classes, whether one must match,
/// verb, identifier)`.
type RelationCheck<'a> = (&'a [Selector], &'a [String], bool, &'a str, &'static str);

struct Dependency {
    target: String,
    span: Span,
    constructed: bool,
    construction_span: Span,
}

/// A violation of an architecture rule: where, what and its identifier.
type Finding = (Span, String, &'static str);

struct Context<'p> {
    project: &'p Project,
    declared: FxHashMap<String, Declared<'p>>,
    patterns: FxHashMap<String, NamePattern>,
}

impl<'p> Context<'p> {
    fn pattern(&mut self, text: &str) -> &NamePattern {
        self.patterns.entry(text.to_string()).or_insert_with(|| {
            NamePattern::parse(text).unwrap_or(NamePattern::Text(text.to_string()))
        })
    }

    fn canonical(&self, fqn: &str) -> String {
        let name = fqn.trim_start_matches('\\');
        self.project
            .registry
            .lookup_class(name)
            .map(|(canonical, _)| canonical.to_string())
            .unwrap_or_else(|| name.to_string())
    }

    fn extends_chain(&self, fqn: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = self.canonical(fqn);
        let mut seen = FxHashSet::default();
        while let Some((_, info)) = self.project.registry.lookup_class(&current) {
            if info.kind() != ClassKind::Class {
                break;
            }
            let Some(parent) = info.extends().first() else {
                break;
            };
            let parent = self.canonical(parent);
            if !seen.insert(parent.to_ascii_lowercase()) {
                break;
            }
            out.push(parent.clone());
            current = parent;
        }
        out
    }

    fn matches(&mut self, selector: &Selector, fqn: &str) -> bool {
        let fqn = self.canonical(fqn);
        let registry = &self.project.registry;
        let info = registry.lookup_class(&fqn).map(|(_, info)| info.clone());
        if let Some(ns) = &selector.namespace
            && !self.pattern(ns).matches_namespace(&fqn)
        {
            return false;
        }
        if let Some(name) = &selector.classname
            && !self.pattern(name).matches_name(&fqn)
        {
            return false;
        }
        if let Some(parent) = &selector.extends {
            let parent = self.canonical(parent);
            if !self
                .extends_chain(&fqn)
                .iter()
                .any(|p| p.eq_ignore_ascii_case(&parent))
            {
                return false;
            }
        }
        if let Some(iface) = &selector.implements {
            let iface = self.canonical(iface);
            let is_interface = registry
                .lookup_class(&iface)
                .is_some_and(|(_, i)| i.kind() == ClassKind::Interface);
            if fqn.eq_ignore_ascii_case(&iface)
                || !is_interface
                || !registry.is_subclass_of(&fqn, &iface)
            {
                return false;
            }
        }
        if let Some(trait_name) = &selector.uses_trait {
            let trait_name = self.canonical(trait_name).to_ascii_lowercase();
            if !registry
                .all_ancestor_names(&fqn)
                .iter()
                .any(|a| a.to_ascii_lowercase() == trait_name)
            {
                return false;
            }
        }
        let declared_attributes: Vec<String> = self
            .declared
            .get(&fqn.to_ascii_lowercase())
            .map(|d| d.attributes.clone())
            .unwrap_or_default();
        let has_attribute = |name: &str| {
            let name = name.trim_start_matches('\\');
            declared_attributes
                .iter()
                .any(|a| a.trim_start_matches('\\').eq_ignore_ascii_case(name))
        };
        if let Some(attribute) = &selector.applies_attribute
            && !has_attribute(&self.canonical(attribute))
        {
            return false;
        }
        let flags = [
            (
                selector.is_abstract,
                info.as_ref()
                    .map(|i| i.is_abstract() && i.kind() == ClassKind::Class),
            ),
            (selector.is_final, info.as_ref().map(|i| i.is_final())),
            (selector.is_readonly, info.as_ref().map(|i| i.is_readonly())),
            (
                selector.is_interface,
                info.as_ref().map(|i| i.kind() == ClassKind::Interface),
            ),
            (
                selector.is_trait,
                info.as_ref().map(|i| i.kind() == ClassKind::Trait),
            ),
            (
                selector.is_enum,
                info.as_ref().map(|i| i.kind() == ClassKind::Enum),
            ),
            (
                selector.is_exception,
                info.as_ref()
                    .map(|_| registry.is_subclass_of(&fqn, "Throwable")),
            ),
            (selector.is_attribute, Some(has_attribute("Attribute"))),
            (
                selector.is_standard_class,
                info.as_ref()
                    .map(|i| i.kind() == ClassKind::Class && !i.is_abstract()),
            ),
        ];
        for (wanted, actual) in flags {
            if let Some(wanted) = wanted
                && actual != Some(wanted)
            {
                return false;
            }
        }
        !selector.excluding.iter().any(|ex| self.matches(ex, &fqn))
    }

    fn matches_any(&mut self, selectors: &[Selector], fqn: &str) -> bool {
        selectors.iter().any(|s| self.matches(s, fqn))
    }
}

fn is_builtin(fqn: &str) -> bool {
    stubs::builtin_class_extension(fqn).is_some()
}

impl Project {
    /// Every `rules` violation among the analysed classes, keyed by file.
    pub fn check_architecture(
        &self,
        rules: &[ArchitectureRule],
    ) -> FxHashMap<PathBuf, Vec<Diagnostic>> {
        let mut out: FxHashMap<PathBuf, Vec<Diagnostic>> = FxHashMap::default();
        if rules.is_empty() {
            return out;
        }
        let mut declared = FxHashMap::default();
        for file in self.files.values() {
            collect_declared(&file.ast, &mut declared);
        }
        let mut ctx = Context {
            project: self,
            declared,
            patterns: FxHashMap::default(),
        };

        let mut analyzed: Vec<(&PathBuf, &super::FileState)> =
            self.files.iter().filter(|(_, f)| f.analyzed).collect();
        analyzed.sort_by(|a, b| a.0.cmp(b.0));
        for (path, file) in analyzed {
            let mut classes = FxHashMap::default();
            collect_declared(&file.ast, &mut classes);
            if classes.is_empty() {
                continue;
            }
            let refs = index_class_fn_refs_without_source(&file.ast);
            let mut constructed = FxHashSet::default();
            new_class_spans(&file.ast, &mut constructed);
            let mut class_names: Vec<&String> = classes.keys().collect();
            class_names.sort();
            for lower in class_names {
                let decl = &classes[lower];
                let fqn = ctx.canonical(lower);
                let dependencies = self.class_dependencies(&fqn, decl, &refs, &constructed);
                for (index, rule) in rules.iter().enumerate() {
                    if !ctx.matches_any(&rule.classes, &fqn)
                        || ctx.matches_any(&rule.excluding, &fqn)
                    {
                        continue;
                    }
                    let rule_name = rule
                        .name
                        .clone()
                        .unwrap_or_else(|| format!("#{}", index + 1));
                    let found =
                        self.check_rule(&mut ctx, rule, &rule_name, &fqn, decl, &dependencies);
                    if !found.is_empty() {
                        out.entry((*path).clone())
                            .or_default()
                            .extend(found.into_iter().map(|(span, message, id)| {
                                DiagnosticBuilder::new(message, span, 0, id).build(&file.line_index)
                            }));
                    }
                }
            }
        }
        for diagnostics in out.values_mut() {
            diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.col));
        }
        out
    }

    fn class_dependencies(
        &self,
        fqn: &str,
        decl: &Declared,
        refs: &[(Span, RefTarget)],
        constructed: &FxHashSet<Span>,
    ) -> Vec<Dependency> {
        let mut deps: Vec<Dependency> = Vec::new();
        let mut index: FxHashMap<String, usize> = FxHashMap::default();
        let mut push = |target: String, span: Span, is_new: bool, deps: &mut Vec<Dependency>| {
            let Some((canonical, _)) = self.registry.lookup_class(target.trim_start_matches('\\'))
            else {
                return;
            };
            let target = canonical.to_string();
            if target.eq_ignore_ascii_case(fqn) || is_builtin(&target) {
                return;
            }
            match index.get(&target.to_ascii_lowercase()) {
                Some(&i) => {
                    if is_new && !deps[i].constructed {
                        deps[i].constructed = true;
                        deps[i].construction_span = span;
                    }
                }
                None => {
                    index.insert(target.to_ascii_lowercase(), deps.len());
                    deps.push(Dependency {
                        target,
                        span,
                        constructed: is_new,
                        construction_span: span,
                    });
                }
            }
        };
        for (span, target) in refs {
            if let RefTarget::Class(target) = target
                && contains(decl.span, *span)
            {
                push(target.clone(), *span, constructed.contains(span), &mut deps);
            }
        }
        if let Some((_, info)) = self.registry.lookup_class(fqn) {
            let mut names: Vec<String> = Vec::new();
            let mut add = |ty: &Type| {
                names.extend(ty.referenced_class_names().iter().map(|n| n.to_string()));
            };
            for sig in info.methods().values() {
                for p in sig.params() {
                    add(p.ty());
                }
                add(sig.return_type());
            }
            for prop in info.properties().values() {
                add(prop.ty());
            }
            for name in names {
                if self.registry.lookup_class(&name).is_some() {
                    push(name, decl.name_span, false, &mut deps);
                }
            }
        }
        deps
    }

    fn check_rule(
        &self,
        ctx: &mut Context,
        rule: &ArchitectureRule,
        rule_name: &str,
        fqn: &str,
        decl: &Declared,
        deps: &[Dependency],
    ) -> Vec<Finding> {
        let mut out = Vec::new();
        let report = |out: &mut Vec<Finding>, span: Span, message: String, id: &'static str| {
            out.push((span, message, id));
        };
        for dep in deps {
            if ctx.matches_any(&rule.should_not_depend_on, &dep.target) {
                report(
                    &mut out,
                    dep.span,
                    format!(
                        "Class {fqn} depends on {}, which architecture rule \"{rule_name}\" forbids.",
                        dep.target
                    ),
                    "architecture.dependency",
                );
            } else if let Some(allowed) = &rule.can_only_depend_on
                && !ctx.matches_any(allowed, &dep.target)
            {
                report(
                    &mut out,
                    dep.span,
                    format!(
                        "Class {fqn} depends on {}, but architecture rule \"{rule_name}\" doesn't allow it.",
                        dep.target
                    ),
                    "architecture.dependency",
                );
            }
            if dep.constructed && ctx.matches_any(&rule.should_not_construct, &dep.target) {
                report(
                    &mut out,
                    dep.construction_span,
                    format!(
                        "Class {fqn} constructs {}, which architecture rule \"{rule_name}\" forbids.",
                        dep.target
                    ),
                    "architecture.construction",
                );
            }
        }
        if !rule.should_depend_on.is_empty()
            && !deps
                .iter()
                .any(|d| ctx.matches_any(&rule.should_depend_on, &d.target))
        {
            report(
                &mut out,
                decl.name_span,
                format!(
                    "Class {fqn} should depend on a class architecture rule \"{rule_name}\" selects."
                ),
                "architecture.missingDependency",
            );
        }
        let info = self.registry.lookup_class(fqn).map(|(_, i)| i.clone());
        let parents = ctx.extends_chain(fqn);
        let interfaces: Vec<String> = self
            .registry
            .ancestor_names
            .get(fqn)
            .map(|a| a.iter().map(|s| s.to_string()).collect::<Vec<String>>())
            .unwrap_or_default()
            .into_iter()
            .filter(|a: &String| {
                self.registry
                    .lookup_class(a)
                    .is_some_and(|(_, i)| i.kind() == ClassKind::Interface)
            })
            .collect();
        let traits: Vec<String> = info
            .as_ref()
            .map(|i| i.uses().iter().map(|u| u.to_string()).collect())
            .unwrap_or_default();
        let relation_checks: [RelationCheck; 6] = [
            (
                &rule.should_extend,
                &parents,
                true,
                "extend",
                "architecture.extends",
            ),
            (
                &rule.should_not_extend,
                &parents,
                false,
                "extend",
                "architecture.extends",
            ),
            (
                &rule.should_implement,
                &interfaces,
                true,
                "implement",
                "architecture.implements",
            ),
            (
                &rule.should_not_implement,
                &interfaces,
                false,
                "implement",
                "architecture.implements",
            ),
            (
                &rule.should_use_trait,
                &traits,
                true,
                "use",
                "architecture.usesTrait",
            ),
            (
                &rule.should_not_use_trait,
                &traits,
                false,
                "use",
                "architecture.usesTrait",
            ),
        ];
        for (selectors, related, required, verb, id) in relation_checks {
            if selectors.is_empty() {
                continue;
            }
            let matching: Vec<&String> = related
                .iter()
                .filter(|r| ctx.matches_any(selectors, r))
                .collect();
            if required && matching.is_empty() {
                report(
                    &mut out,
                    decl.name_span,
                    format!(
                        "Class {fqn} should {verb} a class architecture rule \"{rule_name}\" selects."
                    ),
                    id,
                );
            } else if !required {
                for m in matching {
                    report(
                        &mut out,
                        decl.name_span,
                        format!(
                            "Class {fqn} should not {verb} {m} (architecture rule \"{rule_name}\")."
                        ),
                        id,
                    );
                }
            }
        }
        let flag_checks = [
            (
                rule.should_be_final,
                info.as_ref().map(|i| i.is_final()),
                "final",
                "architecture.final",
            ),
            (
                rule.should_be_abstract,
                info.as_ref().map(|i| i.is_abstract()),
                "abstract",
                "architecture.abstract",
            ),
            (
                rule.should_be_readonly,
                info.as_ref().map(|i| i.is_readonly()),
                "readonly",
                "architecture.readonly",
            ),
        ];
        for (wanted, actual, word, id) in flag_checks {
            if let Some(wanted) = wanted
                && actual.is_some_and(|a| a != wanted)
            {
                let not = if wanted { "" } else { "not " };
                report(
                    &mut out,
                    decl.name_span,
                    format!(
                        "Class {fqn} should {not}be {word} (architecture rule \"{rule_name}\")."
                    ),
                    id,
                );
            }
        }
        if let Some(name) = &rule.should_be_named {
            let pattern = NamePattern::parse(name).unwrap_or(NamePattern::Text(name.clone()));
            let short = fqn.rsplit('\\').next().unwrap_or(fqn);
            let named = match &pattern {
                NamePattern::Regex(_) => pattern.matches_name(fqn) || pattern.matches_name(short),
                NamePattern::Text(_) => pattern.matches_name(fqn),
            };
            if !named {
                report(
                    &mut out,
                    decl.name_span,
                    format!(
                        "Class {fqn} should be named {name} (architecture rule \"{rule_name}\")."
                    ),
                    "architecture.naming",
                );
            }
        }
        if let Some(attribute) = &rule.should_apply_attribute {
            let wanted = ctx.canonical(attribute);
            if !decl
                .attributes
                .iter()
                .any(|a| a.trim_start_matches('\\').eq_ignore_ascii_case(&wanted))
            {
                report(
                    &mut out,
                    decl.name_span,
                    format!(
                        "Class {fqn} should apply attribute {wanted} (architecture rule \"{rule_name}\")."
                    ),
                    "architecture.attribute",
                );
            }
        }
        if rule.should_have_only_one_public_method == Some(true) && decl.public_methods.len() != 1 {
            report(
                &mut out,
                decl.name_span,
                format!(
                    "Class {fqn} should have exactly one public method, but has {} (architecture rule \"{rule_name}\").",
                    decl.public_methods.len()
                ),
                "architecture.publicMethods",
            );
        }
        out
    }
}
