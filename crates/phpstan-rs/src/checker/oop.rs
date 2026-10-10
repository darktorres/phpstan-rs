use rustc_hash::FxHashMap;
use types::Name;

use crate::checker::{Checker, class_kind_identifier};
use crate::registry::{
    FunctionReflection, PropertyReflection, collect_class_name_case_refs, resolve_names_in_type,
};
use diagnostics::DiagnosticBuilder;
use phpdoc::docblock;
use syntax::ast::*;
use syntax::span::{LineIndex, Span};
use syntax::visit::arena::{self, Visitor};
use types::{self, Type};

mod attributes;
mod enums;
mod method_overrides;
mod property_declarations;
mod property_hooks;
mod property_overrides;

pub(crate) use attributes::{
    ATTRIBUTE_TARGET_CLASS, ATTRIBUTE_TARGET_CLASS_CONSTANT, ATTRIBUTE_TARGET_FUNCTION,
    ATTRIBUTE_TARGET_METHOD, ATTRIBUTE_TARGET_PARAMETER, ATTRIBUTE_TARGET_PROPERTY,
};

impl<'a> Checker<'a> {
    /// Every `extends`/`implements`/trait-`use` target of `registry_name`
    /// checked against the kind of symbol it actually resolves to: a class
    /// may only extend a non-final class, implement interfaces and use
    /// traits; an interface may only extend interfaces and use nothing; an
    /// enum may only implement interfaces and use traits; a trait may only
    /// use traits. Extending a `final` class is a real, unconditional fatal
    /// error in PHP at class-declaration time, regardless of whether the
    /// subclass overrides anything; extending an `@final` one is the
    /// docblock-only convention. `readonly`-ness must match between a class
    /// and its parent class. `label` is the diagnostic's location. The method-level
    /// finality half (overriding a `final` method) lives in
    /// `check_method_overrides`. An unresolvable target is
    /// `check_class_references_exist`'s concern, never reported here.
    pub(crate) fn check_inheritance_target_kinds(
        &mut self,
        registry_name: &str,
        is_anonymous: bool,
        span: Span,
    ) {
        let Some(info) = self.class_info(registry_name) else {
            return;
        };
        let Some(references) = self.registry.class_references_by_name(registry_name) else {
            return;
        };
        let kind = info.kind();
        let subject = DeclarationName {
            word: (!is_anonymous).then(|| class_kind_identifier(&kind)),
            name: registry_name,
        };
        let bare_name = DeclarationName {
            word: (!is_anonymous).then_some("class"),
            name: registry_name,
        };
        let is_readonly = info.is_readonly();
        let target_kind = |name: &str| {
            self.registry
                .lookup_class(name)
                .map(|(fqn, i)| (fqn.to_string(), i.kind()))
        };
        let mut reports: Vec<(String, &'static str)> = Vec::new();

        for parent in references.extends {
            let Some((parent_fqn, parent_info)) = self.registry.lookup_class(parent) else {
                continue;
            };
            let parent_kind = class_kind_identifier(&parent_info.kind());
            if kind == ClassKind::Interface {
                if parent_info.kind() != ClassKind::Interface {
                    reports.push((
                        format!("{subject} cannot extend {parent_kind} {parent_fqn}"),
                        match parent_info.kind() {
                            ClassKind::Trait => "interfaceExtends.trait",
                            ClassKind::Enum => "interfaceExtends.enum",
                            _ => "interfaceExtends.class",
                        },
                    ));
                }
                continue;
            }
            match parent_info.kind() {
                ClassKind::Interface | ClassKind::Trait | ClassKind::Enum => reports.push((
                    format!("{subject} cannot extend {parent_kind} {parent_fqn}"),
                    match parent_info.kind() {
                        ClassKind::Interface => "class.extendsInterface",
                        ClassKind::Trait => "class.extendsTrait",
                        _ => "class.extendsEnum",
                    },
                )),
                ClassKind::Class => {
                    if parent_info.is_final() {
                        reports.push((
                            format!("{subject} cannot extend final class {parent_fqn}"),
                            "class.extendsFinal",
                        ));
                    } else if parent_info.is_final_by_phpdoc() {
                        reports.push((
                            format!("{subject} extends @final class {parent_fqn}"),
                            "class.extendsFinalByPhpDoc",
                        ));
                    }
                    if is_readonly && !parent_info.is_readonly() {
                        reports.push((
                            format!(
                                "readonly {bare_name} cannot extend non-readonly class {parent_fqn}"
                            ),
                            "class.readOnly",
                        ));
                    } else if !is_readonly && parent_info.is_readonly() {
                        reports.push((
                            format!(
                                "non-readonly {bare_name} cannot extend readonly class {parent_fqn}"
                            ),
                            "class.nonReadOnly",
                        ));
                    }
                }
            }
        }

        for iface in references.implements {
            let Some((iface_fqn, iface_kind)) = target_kind(iface) else {
                continue;
            };
            if iface_kind == ClassKind::Interface {
                continue;
            }
            reports.push((
                format!(
                    "{subject} cannot implement {} {iface_fqn}",
                    class_kind_identifier(&iface_kind)
                ),
                match (kind == ClassKind::Enum, iface_kind) {
                    (true, ClassKind::Trait) => "enumImplements.trait",
                    (true, ClassKind::Enum) => "enumImplements.enum",
                    (true, _) => "enumImplements.class",
                    (false, ClassKind::Trait) => "classImplements.trait",
                    (false, ClassKind::Enum) => "classImplements.enum",
                    (false, _) => "classImplements.class",
                },
            ));
        }

        for used in references.uses {
            let Some((used_fqn, used_kind)) = target_kind(used) else {
                continue;
            };
            if kind == ClassKind::Interface {
                reports.push((
                    format!("{subject} cannot use trait {used_fqn}"),
                    "interface.traitUse",
                ));
                continue;
            }
            if used_kind == ClassKind::Trait {
                continue;
            }
            reports.push((
                format!(
                    "{subject} cannot use {} {used_fqn} as a trait",
                    class_kind_identifier(&used_kind)
                ),
                match used_kind {
                    ClassKind::Interface => "traitUse.interface",
                    ClassKind::Enum => "traitUse.enum",
                    _ => "traitUse.class",
                },
            ));
        }

        for (message, identifier) in reports {
            report!(self, message, span, 0, identifier);
        }
    }

    /// The class/enum-declaration-level half of the "does this name
    /// actually resolve" existence pipeline - every `extends` target (a
    /// class's single parent, or an interface's possibly-multiple parent
    /// interfaces), every `implements` target, and every `use`d trait.
    /// Checks every entry - an interface's `extends A, B;` is real,
    /// valid PHP with more than one target. Operates purely on the
    /// already-indexed, already name-resolved `ClassReflection`, so this needs
    /// no raw `ClassDecl` at all and covers every class-like uniformly
    /// from one call site. Existence-only - kind compatibility (a
    /// class `implements`-ing another class instead of an interface) is
    /// `check_inheritance_target_kinds`'.
    pub(crate) fn check_class_references_exist(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.class_info(fqn) else {
            return;
        };
        let Some(references) = self.registry.class_references_by_name(fqn) else {
            return;
        };
        for parent in references.extends {
            if !self.class_name_exists(parent) {
                let verb = if info.kind() == ClassKind::Interface {
                    "extends unknown interface"
                } else {
                    "extends unknown class"
                };
                report!(
                    self,
                    format!("{fqn} {verb} {parent}"),
                    span,
                    0,
                    "class.notFound"
                );
            }
        }
        for iface in references.implements {
            if !self.class_name_exists(iface) {
                report!(
                    self,
                    format!("{fqn} implements unknown interface {iface}"),
                    span,
                    0,
                    // Mirrors real PHPStan's
                    // `ExistingClassesInClassImplementsRule` identifier
                    // for this message shape (unlike `extends`/trait-use
                    // above/below, which share the generic
                    // `class.notFound`).
                    "interface.notFound",
                );
            }
        }
        for used_trait in references.uses {
            if !self.class_name_exists(used_trait) {
                report!(
                    self,
                    format!("{fqn} uses unknown trait {used_trait}"),
                    span,
                    0,
                    // Mirrors real PHPStan's `ExistingClassInTraitUseRule`
                    // identifier.
                    "trait.notFound",
                );
            }
        }
    }

    /// Mirrors real PHPStan's `MethodTagCheck` - a `@method` tag's
    /// parameter/return types are already resolved into a real
    /// `FunctionReflection` (`ClassReflection::method_tags`) but never existence-
    /// checked. Pure reuse of `check_type_class_names_exist` - no new
    /// existence machinery. `methodTag.trait` (the type resolves but is
    /// a trait, invalid there) is deliberately not built - no analog
    /// exists anywhere else in this project for any typehint location.
    pub(crate) fn check_method_tag_types(&mut self, fqn: &str, doc: Option<&str>, span: Span) {
        if self.class_info(fqn).is_none() {
            return;
        }
        // `Registry::resolve_method_tags` drains `ClassReflection::method_tags`
        // into the method result table before this checker pass runs, so
        // these are read through the same effective-class API as traits.
        // `FunctionReflection::is_magic` is the one field `method_tag_sig` sets
        // that a real declared method's `FunctionReflection::from_decl` never
        // does, so it's the correct filter here. Also matches enum
        // `cases()`/`from()`/`tryFrom()` (the other `is_magic: true`
        // producer) harmlessly - their synthesized types never reference
        // an unresolvable class.
        for name in self.registry.method_names(fqn) {
            let Some(sig) = self.registry.lookup_own_method(fqn, name) else {
                continue;
            };
            if !sig.is_magic() {
                continue;
            }
            for param in sig.params() {
                self.check_type_class_names_exist(
                    param.ty(),
                    format!("PHPDoc tag @method for method {fqn}::{name}()"),
                    span,
                );
            }
            self.check_type_class_names_exist(
                sig.return_type(),
                format!("PHPDoc tag @method for method {fqn}::{name}()"),
                span,
            );
        }
        if let Some(doc) = doc {
            let block = docblock::parse_shared(doc);
            for tag in &block.method_tags {
                for param in &tag.params {
                    if let Some(ty) = &param.ty {
                        let (_, mut refs) = self.resolve_type_node_with_case_refs(ty);
                        refs.retain(|(raw, _)| {
                            !tag.templates
                                .iter()
                                .any(|template| template.name == raw.as_ref())
                        });
                        self.check_type_class_name_case(&refs, span);
                        self.check_type_internal_usage(&refs, span);
                    }
                }
                if let Some(ty) = &tag.return_type {
                    let (_, mut refs) = self.resolve_type_node_with_case_refs(ty);
                    refs.retain(|(raw, _)| {
                        !tag.templates
                            .iter()
                            .any(|template| template.name == raw.as_ref())
                    });
                    self.check_type_class_name_case(&refs, span);
                    self.check_type_internal_usage(&refs, span);
                }
            }
        }
    }

    /// `MethodTagTemplateTypeCheck`: a `@method` tag's own templates are
    /// validated like `@template` tags, and may not reuse a class
    /// template's name (`methodTag.shadowTemplate`).
    pub(crate) fn check_method_tag_templates(&mut self, fqn: &str, doc: Option<&str>, span: Span) {
        let Some(doc) = doc else {
            return;
        };
        if !doc.contains("method") {
            return;
        }
        let block = docblock::parse_shared(doc);
        if block.method_tags.iter().all(|t| t.templates.is_empty()) {
            return;
        }
        let previous_class = self.current_class.replace(fqn.to_string());
        let class_templates = self.current_class_template_params();
        self.current_class = previous_class;
        let type_aliases = self.registry.effective_type_aliases_by_name(fqn);
        for tag in &block.method_tags {
            if tag.templates.is_empty() {
                continue;
            }
            let mut scope = (*class_templates).clone();
            for t in &tag.templates {
                scope.insert(
                    t.name.clone(),
                    types::TemplateParam::unbounded(t.name.clone()),
                );
            }
            let resolve = |node: &dyn phpdoc::WrittenType| {
                let written = crate::registry::parse_type_node(node);
                let ty = crate::registry::mark_template_params(written.clone(), &scope);
                let ty = crate::registry::resolve_names_in_type(ty, &self.name_ctx);
                self.record_doc_resolution("@method template", &written, &ty);
                ty
            };
            let names: Vec<Name> = tag
                .templates
                .iter()
                .map(|t| t.name.as_str().into())
                .collect();
            let bounds = tag
                .templates
                .iter()
                .filter_map(|t| Some((t.name.as_str().into(), resolve(t.bound.as_ref()?))))
                .collect();
            let defaults = tag
                .templates
                .iter()
                .filter_map(|t| Some((t.name.as_str().into(), resolve(t.default.as_ref()?))))
                .collect();
            self.check_template_declaration_validity(
                &names,
                &bounds,
                &defaults,
                &type_aliases,
                crate::checker::generics::TemplateSubject::MethodTag {
                    class: fqn,
                    method: &tag.name,
                },
                span,
            );
            for name in tag.templates.iter().map(|t| &t.name) {
                if class_templates.contains_key(name) {
                    report!(
                        self,
                        format!(
                            "PHPDoc tag @method template {name} for method {fqn}::{}() shadows @template {name} for class {fqn}.",
                            tag.name
                        ),
                        span,
                        2,
                        "methodTag.shadowTemplate",
                    );
                }
            }
        }
    }

    /// Mirrors real PHPStan's `MixinCheck` - `@mixin Type` is already
    /// resolved to a bare FQN string (`ClassReflection::mixins`) and consumed
    /// for its real semantic effect (the mixin's methods/properties
    /// become available on the host class), but its own existence needs
    /// checking too - an unresolvable `@mixin` target would otherwise
    /// silently contribute nothing instead of surfacing a real typo.
    /// `mixin.nonObject`/`mixin.trait` are deliberately not built, same
    /// scope as `check_method_tag_types`.
    pub(crate) fn check_mixin_types(&mut self, fqn: &str, doc: Option<&str>, span: Span) {
        if self.class_info(fqn).is_none() {
            return;
        }
        let Some(references) = self.registry.class_references_by_name(fqn) else {
            return;
        };
        for mixin in references.mixins {
            if !self.class_name_exists(mixin) {
                report!(
                    self,
                    format!("PHPDoc tag @mixin references unknown class {mixin}."),
                    span,
                    0,
                    "class.notFound",
                );
            }
        }
        if let Some(doc) = doc {
            let block = docblock::parse_shared(doc);
            for mixin in &block.mixins {
                let (_, refs) = self.resolve_type_node_with_case_refs(mixin);
                self.check_type_class_name_case(&refs, span);
                self.check_type_internal_usage(&refs, span);
            }
        }
    }

    /// Mirrors real PHPStan's `RequireExtendsCheck`/
    /// `RequireImplementsDefinitionTraitRule`'s full declaration-validity
    /// family, not just the existence half - `@phpstan-require-extends`/
    /// `-implements`'s RHS is re-parsed directly from `c`'s own doc
    /// comment (rather than read back from `ClassReflection::require_extends`/
    /// `require_implements`, which is already flattened across every tag
    /// into one combined `Vec<String>` of only the successfully-
    /// extracted object class names - exactly the information a
    /// `.nonObject`/`.duplicate` check needs to tell apart from "no tag
    /// at all"), mirroring real PHPStan's `getObjectClassNames()`/
    /// `RequireExtendsTag` resolution pipeline. The RHS's own case-
    /// sensitivity re-check is deliberately not built here - that's the
    /// dedicated `class.nameCase` pipeline-wide expansion, not
    /// re-litigated per tag family.
    pub(crate) fn check_require_tag_types(&mut self, c: &ClassDef) {
        let source = self.syntax_source_owner();
        let doc_text = c
            .doc_comment
            .as_ref()
            .and_then(|ranges| source.as_ref().and_then(|source| ranges.text(source)));
        let Some(doc) = doc_text.as_deref().map(docblock::parse_shared) else {
            return;
        };
        if doc.require_extends.len() > 1 {
            report!(
                self,
                "PHPDoc tag @phpstan-require-extends can only be used once.".to_string(),
                c.span,
                0,
                "requireExtends.duplicate",
            );
        }
        for raw in &doc.require_extends {
            let parsed = crate::registry::parse_type_node(raw);
            let mut case_refs = Vec::new();
            collect_class_name_case_refs(&parsed, &self.name_ctx, &mut case_refs);
            self.check_type_class_name_case(&case_refs, c.span);
            let ty = resolve_names_in_type(parsed.clone(), &self.name_ctx);
            self.record_doc_resolution("@phpstan-require-extends", &parsed, &ty);
            let names: Vec<String> = ty
                .object_class_names()
                .iter()
                .map(|n| n.to_string())
                .collect();
            if names.is_empty() {
                report!(
                    self,
                    format!("PHPDoc tag @phpstan-require-extends contains non-object type {ty}."),
                    c.span,
                    0,
                    "requireExtends.nonObject",
                );
                continue;
            }
            for name in names {
                if !self.class_name_exists(&name) {
                    report!(
                        self,
                        format!(
                            "PHPDoc tag @phpstan-require-extends contains unknown class {name}."
                        ),
                        c.span,
                        0,
                        "class.notFound",
                    );
                    continue;
                }
                let Some((_, info)) = self.registry.lookup_class(&name) else {
                    continue;
                };
                match info.kind() {
                    ClassKind::Interface => {
                        report_builder!(self, 
                            DiagnosticBuilder::new(format!(
                                    "PHPDoc tag @phpstan-require-extends cannot contain an interface {name}, expected a class."
                                ),
                                c.span,
                                0,
                                "requireExtends.interface",
                            )
                            .tip("If you meant an interface, use @phpstan-require-implements instead."),
                        );
                    }
                    ClassKind::Trait | ClassKind::Enum => {
                        report!(
                            self,
                            format!(
                                "PHPDoc tag @phpstan-require-extends cannot contain non-class type {name}."
                            ),
                            c.span,
                            0,
                            format!("requireExtends.{}", class_kind_identifier(&info.kind())),
                        );
                    }
                    ClassKind::Class if info.is_final() => {
                        report!(
                            self,
                            format!(
                                "PHPDoc tag @phpstan-require-extends cannot contain final class {name}."
                            ),
                            c.span,
                            0,
                            "requireExtends.finalClass",
                        );
                    }
                    ClassKind::Class => {}
                }
            }
        }
        for raw in &doc.require_implements {
            let parsed = crate::registry::parse_type_node(raw);
            let mut case_refs = Vec::new();
            collect_class_name_case_refs(&parsed, &self.name_ctx, &mut case_refs);
            self.check_type_class_name_case(&case_refs, c.span);
            let ty = resolve_names_in_type(parsed.clone(), &self.name_ctx);
            self.record_doc_resolution("@phpstan-require-implements", &parsed, &ty);
            let names: Vec<String> = ty
                .object_class_names()
                .iter()
                .map(|n| n.to_string())
                .collect();
            if names.is_empty() {
                report!(
                    self,
                    format!(
                        "PHPDoc tag @phpstan-require-implements contains non-object type {ty}."
                    ),
                    c.span,
                    0,
                    "requireImplements.nonObject",
                );
                continue;
            }
            for name in names {
                if !self.class_name_exists(&name) {
                    report!(
                        self,
                        format!(
                            "PHPDoc tag @phpstan-require-implements contains unknown class {name}."
                        ),
                        c.span,
                        0,
                        "class.notFound",
                    );
                    continue;
                }
                let Some((_, info)) = self.registry.lookup_class(&name) else {
                    continue;
                };
                if info.kind() != ClassKind::Interface {
                    report!(
                        self,
                        format!(
                            "PHPDoc tag @phpstan-require-implements cannot contain non-interface type {name}."
                        ),
                        c.span,
                        0,
                        format!("requireImplements.{}", class_kind_identifier(&info.kind())),
                    );
                }
            }
        }
    }

    /// Mirrors real PHPStan's `SealedDefinitionClassRule`/
    /// `SealedDefinitionTraitRule`'s full declaration-validity family -
    /// see `check_require_tag_types`'s identical doc comment for why the
    /// RHS is re-parsed directly from the declaration's own doc comment
    /// rather than read back from the already-flattened
    /// `ClassReflection::sealed`.
    pub(crate) fn check_sealed_tag_types(&mut self, c: &ClassDef) {
        let source = self.syntax_source_owner();
        let doc_text = c
            .doc_comment
            .as_ref()
            .and_then(|ranges| source.as_ref().and_then(|source| ranges.text(source)));
        let Some(doc) = doc_text.as_deref().map(docblock::parse_shared) else {
            return;
        };
        if doc.sealed.is_empty() {
            return;
        }
        let span = c.span;
        if c.kind == ClassKind::Enum {
            report!(
                self,
                "PHPDoc tag @phpstan-sealed is only valid on class or interface.".to_string(),
                span,
                0,
                "sealed.onEnum",
            );
            return;
        }
        for raw in &doc.sealed {
            let parsed = crate::registry::parse_type_node(raw);
            let mut case_refs = Vec::new();
            collect_class_name_case_refs(&parsed, &self.name_ctx, &mut case_refs);
            self.check_type_class_name_case(&case_refs, span);
            let ty = resolve_names_in_type(parsed.clone(), &self.name_ctx);
            self.record_doc_resolution("@phpstan-sealed", &parsed, &ty);
            let names: Vec<String> = ty
                .object_class_names()
                .iter()
                .map(|n| n.to_string())
                .collect();
            if names.is_empty() {
                report!(
                    self,
                    format!("PHPDoc tag @phpstan-sealed contains non-object type {ty}."),
                    span,
                    0,
                    "sealed.nonObject",
                );
                continue;
            }
            for name in names {
                if !self.class_name_exists(&name) {
                    report!(
                        self,
                        format!("PHPDoc tag @phpstan-sealed contains unknown class {name}."),
                        span,
                        0,
                        "class.notFound",
                    );
                }
            }
        }
    }

    /// Mirrors real PHPStan's `LocalTypeAliasesCheck` existence half - a
    /// `@phpstan-type Name TypeExpr` alias's RHS is already resolved
    /// (`ClassReflection::type_aliases`) but never existence-checked. Only the
    /// alias-RHS half is built here, not `@phpstan-import-type`'s
    /// `from Class` existence: that class is commonly only reachable via
    /// Composer autoloading this registry may legitimately have no data
    /// for in a partial-analysis run.
    pub(crate) fn check_type_alias_types(&mut self, fqn: &str, doc: Option<&str>, span: Span) {
        if self.class_info(fqn).is_none() {
            return;
        }
        let aliases = self.registry.effective_type_aliases_by_name(fqn);
        for (name, ty) in &aliases {
            self.check_type_class_names_exist(
                ty,
                format!("PHPDoc tag @phpstan-type {name} for class {fqn}"),
                span,
            );
        }
        if let Some(doc) = doc {
            let block = docblock::parse_shared(doc);
            for ty in block.type_aliases.values() {
                let (_, refs) = self.resolve_type_node_with_case_refs(ty);
                self.check_type_class_name_case(&refs, span);
                self.check_type_internal_usage(&refs, span);
            }
        }
    }

    /// Two traits used by the same class declaring the same property
    /// (with a genuinely different type) or method (with a genuinely
    /// different signature) is a real, unconditional PHP fatal error at
    /// class-declaration time unless the class explicitly resolves the
    /// conflict (`insteadof`/`as` - `resolved_trait_conflicts`, tracked
    /// regardless of whether this project models exactly which trait
    /// `insteadof` picked). Compares only pairs of different traits - a
    /// name declared by just one used trait is never a conflict.
    pub(crate) fn check_trait_conflicts(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.class_info(fqn) else {
            return;
        };
        let Some(references) = self.registry.class_references_by_name(fqn) else {
            return;
        };
        if references.uses.len() < 2 {
            return;
        }

        let mut seen_props: FxHashMap<&str, (&str, &PropertyReflection)> = FxHashMap::default();
        let mut seen_methods: FxHashMap<&str, (&str, &FunctionReflection)> = FxHashMap::default();
        for trait_name in references.uses {
            for prop_name in self.registry.property_names(trait_name) {
                let Some(sig) = self.registry.lookup_property(trait_name, prop_name) else {
                    continue;
                };
                if info.resolved_trait_conflicts().contains(prop_name) {
                    continue;
                }
                match seen_props.get(prop_name.as_ref()) {
                    Some((first_trait, first_sig)) if *first_sig.ty() != *sig.ty() => {
                        report!(
                            self,
                            format!(
                                "{first_trait} and {trait_name} define the same property (${prop_name}) with incompatible types in the composition of {fqn}"
                            ),
                            span,
                            0,
                            "trait.propertyConflict"
                        );
                    }
                    Some(_) => {}
                    None => {
                        seen_props.insert(prop_name, (trait_name, sig));
                    }
                }
            }
            for method_name in self.registry.method_names(trait_name) {
                let Some(sig) = self.registry.lookup_own_method(trait_name, method_name) else {
                    continue;
                };
                if info.resolved_trait_conflicts().contains(method_name) {
                    continue;
                }
                match seen_methods.get(method_name.as_ref()) {
                    Some((first_trait, first_sig)) if !signatures_compatible(first_sig, sig) => {
                        report!(
                            self,
                            format!(
                                "{first_trait} and {trait_name} define the same method ({method_name}) with an incompatible signature in the composition of {fqn}"
                            ),
                            span,
                            0,
                            "trait.methodConflict"
                        );
                    }
                    Some(_) => {}
                    None => {
                        seen_methods.insert(method_name, (trait_name, sig));
                    }
                }
            }
        }
    }

    /// `class` is reserved for `ClassName::class` name-fetching, so
    /// declaring a class constant by that name is a real, unconditional
    /// PHP fatal error. Shared between `check_method_declaration_sanity`'s
    /// two call sites' const lists (a `ClassDecl`'s `consts` and an
    /// `ClassDecl`'s) rather than living inside that function itself -
    /// unrelated to methods.
    pub(crate) fn check_reserved_class_constant_name(&mut self, consts: &[ClassConst]) {
        for c in consts {
            if c.name.eq_ignore_ascii_case("class") {
                report_builder!(self, 
                    DiagnosticBuilder::new("a class constant must not be called 'class'; it is reserved for class name fetching".to_string(),
                        c.span,
                        0,
                        "classConstant.class",
                    )
                    .non_ignorable(),
                );
            }
        }
    }

    /// Mirrors real PHPStan's `InvalidPHPStanDocTagRule`
    /// (`phpDoc.phpstanTag`) and its sibling `InvalidPhpDocTagValueRule`
    /// (`phpDoc.parseError`) - a `@phpstan-*` tag real PHPStan doesn't
    /// recognize at all (catches a typo, e.g. `@phpstan-parm`, that
    /// would otherwise silently do nothing). Scoped to a class's own
    /// docblock plus every one of its methods'/properties'/consts' -
    /// `check_unknown_phpstan_tags_on` is called separately for
    /// top-level functions (`check_function`'s call site) - a bare
    /// statement's own inline docblock isn't covered, since
    /// `@phpstan-*` tags are overwhelmingly declaration-level in
    /// practice.
    pub(crate) fn check_unknown_phpstan_tags_in_class(&mut self, c: &ClassDecl) {
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        let Some(def) = arenas.class(c.def) else {
            return;
        };
        let class_doc = def
            .doc_comment
            .as_ref()
            .and_then(|ranges| arenas.doc_comment_text(ranges));
        self.check_unknown_phpstan_tags_on(class_doc.as_deref(), c.name.span);
        for m in arenas.methods(def.methods.clone()) {
            let doc = m
                .function
                .doc_comment
                .as_ref()
                .and_then(|ranges| arenas.doc_comment_text(ranges));
            self.check_unknown_phpstan_tags_on(doc.as_deref(), m.function.name.span);
        }
        for p in def.properties(arenas) {
            let doc = p
                .doc_comment
                .as_ref()
                .and_then(|ranges| arenas.doc_comment_text(ranges));
            self.check_unknown_phpstan_tags_on(doc.as_deref(), p.span);
        }
        for cc in def.consts(arenas) {
            let doc = cc
                .doc_comment
                .as_ref()
                .and_then(|ranges| arenas.doc_comment_text(ranges));
            self.check_unknown_phpstan_tags_on(doc.as_deref(), cc.name.span);
        }
    }

    /// See `check_unknown_phpstan_tags_in_class`'s doc comment - the
    /// one-declaration-at-a-time worker both that function and
    /// `check_function`'s top-level-function call site share.
    pub(crate) fn check_unknown_phpstan_tags_on(&mut self, doc_comment: Option<&str>, span: Span) {
        let Some(doc) = doc_comment else {
            return;
        };
        // A tag is a line that starts with `@`: without one nothing below
        // has anything to report, and without `@phpstan-` there is no
        // unknown PHPStan tag.
        if !doc.contains('@') {
            return;
        }
        if doc.contains("@phpstan-") {
            for tag in docblock::unknown_phpstan_tags(doc) {
                report!(
                    self,
                    format!("Unknown PHPDoc tag: @phpstan-{tag}"),
                    span,
                    2,
                    "phpDoc.phpstanTag",
                );
            }
        }
        let block = docblock::parse_shared(doc);
        self.report_invalid_tag_values_of(&block, span);
        self.check_doc_see_tag_targets(&block, span);
    }

    /// `InvalidPhpDocTagValueRule` (`phpDoc.parseError`), level 2: a tag
    /// whose value this project's PHPDoc grammar couldn't parse. A
    /// type-alias tag names its alias instead of repeating the whole value,
    /// matching real PHPStan's own two message shapes.
    pub(crate) fn report_invalid_doc_tag_values(&mut self, doc: &str, span: Span) {
        self.report_invalid_tag_values_of(&docblock::parse_shared(doc), span);
    }

    /// `report_invalid_doc_tag_values` for a docblock already parsed.
    fn report_invalid_tag_values_of(&mut self, block: &docblock::DocBlock, span: Span) {
        for invalid in &block.invalid_tag_values {
            let message = if invalid.tag == "type" || invalid.tag == "phpstan-type" {
                let alias = invalid.value.split_whitespace().next().unwrap_or("");
                format!(
                    "PHPDoc tag @{} {alias} has invalid value: {}",
                    invalid.tag, invalid.message
                )
            } else {
                format!(
                    "PHPDoc tag @{} has invalid value ({}): {}",
                    invalid.tag, invalid.value, invalid.message
                )
            };
            report!(self, message, span, 2, "phpDoc.parseError");
        }
    }

    /// Checks a `@see` value shaped like a bare class name or a
    /// `Class::member()`/`Class::$prop`/`Class::CONST` reference against
    /// the registry. Anything else (a URL, free-text prose, a bare
    /// function name) is left alone - no upstream grammar exists to tell
    /// those apart from a real reference with any confidence.
    fn check_doc_see_tag_targets(&mut self, block: &docblock::DocBlock, span: Span) {
        for target in &block.see {
            let Some(reference) = docblock::see_reference(target) else {
                continue;
            };
            let trimmed = reference.class.trim_start_matches('\\');
            // `reference.class`, not `trimmed`, is resolved: a leading `\`
            // is what makes `NameContext::resolve` take the name as fully
            // qualified instead of qualifying it with the namespace again.
            let resolved = self.name_ctx.resolve(reference.class);
            if self.registry.lookup_class(&resolved).is_none() {
                report!(
                    self,
                    format!("PHPDoc tag @see contains unknown class {trimmed}"),
                    span,
                    2,
                    "phpDoc.seeTag",
                );
                continue;
            }
            let (found, member) = match reference.member {
                None | Some(docblock::SeeMember::Other) => continue,
                Some(docblock::SeeMember::Method(method)) => (
                    self.registry.lookup_method(&resolved, method).is_some(),
                    format!("{method}()"),
                ),
                Some(docblock::SeeMember::Property(property)) => (
                    self.registry.lookup_property(&resolved, property).is_some(),
                    format!("${property}"),
                ),
                Some(docblock::SeeMember::Constant(constant)) => (
                    self.registry.lookup_const(&resolved, constant).is_some(),
                    constant.to_string(),
                ),
            };
            if !found {
                report!(
                    self,
                    format!("PHPDoc tag @see contains unknown reference {trimmed}::{member}"),
                    span,
                    2,
                    "phpDoc.seeTag",
                );
            }
        }
    }

    /// Mirrors real PHPStan's `ClassConstantRule` typed-constant half:
    /// PHP 8.3's `const TYPE NAME = value;` syntax declares a real,
    /// unconditional native type for the constant - checked here the
    /// same "does the literal value actually satisfy the declared type"
    /// way a typed property's default value already is, matching real
    /// PHPStan's `classConstant.value` identifier. Untyped (`const NAME
    /// = value;`, the overwhelming majority) constants have nothing to
    /// check here. `self.infer_expr`, not the registry's already-
    /// resolved `ConstantReflection.ty`, so a value referencing another constant/
    /// enum case gets this checker's full expression inference rather
    /// than only ever seeing `infer_const_type`'s narrower literal-only
    /// view.
    ///
    /// The `@var` half: an untyped constant's `@var` type must be a
    /// supertype of its value (`classConstant.phpDocType`, level 2), and
    /// a typed constant's `@var` type must accept it
    /// (`classConstant.value`).
    pub(crate) fn check_typed_class_constant_values(&mut self, fqn: &str, consts: &[ClassConst]) {
        if !consts
            .iter()
            .any(|c| c.type_hint.is_some() || c.doc_comment.is_some())
            && !self.settings.constants.has_dynamic_constants()
        {
            return;
        }
        // `self::OTHER_CONST` inside a const's own value needs
        // `current_class` set the same way `check_function` sets it for
        // a method body: without it, `resolve_static_class_name("self")`
        // has no defining class to resolve against here, silently
        // reading every `self::`-qualified value as `Mixed`. This call
        // site runs outside any method, so it's otherwise left at
        // whatever it was (typically `None` here).
        let previous_class = self.current_class.take();
        self.current_class = Some(fqn.to_string());
        let syntax_owner = self
            .syntax_tree_owner()
            .expect("class-constant values retain their parsed file owner");
        let arenas = syntax_owner.arenas();
        for c in consts {
            let has_phpdoc_type = self
                .registry
                .lookup_const_with_owner(fqn, &c.name)
                .is_some_and(|(owner, sig)| owner.as_ref() == fqn && sig.has_phpdoc_type());
            let phpdoc_ty = has_phpdoc_type
                .then(|| self.registry.lookup_const_type(fqn, &c.name).cloned())
                .flatten();
            if phpdoc_ty.is_none() && c.type_hint.is_none() {
                let value = arenas
                    .expr(c.value)
                    .expect("class-constant value belongs to its parsed file owner");
                let value_ty = self.infer_expr(value, &self.new_locals());
                self.check_configured_constant_value(
                    Some(fqn),
                    &c.name,
                    &value_ty,
                    "classConstant.value",
                    c.span,
                );
                continue;
            }
            if let Some(phpdoc_ty) = &phpdoc_ty {
                let value = arenas
                    .expr(c.value)
                    .expect("class-constant value belongs to its parsed file owner");
                let value_ty = self.infer_expr(value, &self.new_locals());
                if !value_ty.is_mixed() {
                    self.check_constant_phpdoc_value(
                        fqn,
                        c,
                        phpdoc_ty,
                        &value_ty,
                        c.type_hint.is_some(),
                    );
                }
                if c.type_hint.is_some() {
                    continue;
                }
            }
            let Some(raw_hint) = &c.type_hint else {
                continue;
            };
            let declared_ty = self.resolve_type_hint(Some(raw_hint));
            let value = arenas
                .expr(c.value)
                .expect("class-constant value belongs to its parsed file owner");
            let value_ty = self.infer_expr(value, &self.new_locals());
            // `mixed` here almost always means "this checker
            // couldn't resolve it" (an unrecognized global constant,
            // e.g. `DEBUG_BACKTRACE_IGNORE_ARGS`, a real PHP builtin,
            // reads as `Mixed`), not a genuinely `mixed`-typed value -
            // the same "don't guess wrong on Mixed" leniency
            // `check_method_overrides`'s parameter-narrowing check
            // applies for the identical reason.
            if value_ty.is_mixed() {
                continue;
            }
            if !self.accepts(&declared_ty, &value_ty).yes() {
                report!(
                    self,
                    format!(
                        "constant {fqn}::{} ({declared_ty}) does not accept value {value_ty}",
                        c.name
                    ),
                    c.span,
                    0,
                    "classConstant.value",
                );
            }
        }
        self.current_class = previous_class;
    }

    fn check_constant_phpdoc_value(
        &mut self,
        fqn: &str,
        c: &ClassConst,
        phpdoc_ty: &Type,
        value_ty: &Type,
        has_native_type: bool,
    ) {
        if self.accepts(phpdoc_ty, value_ty).yes() {
            return;
        }
        let name = &c.name;
        if has_native_type {
            report!(
                self,
                format!("constant {fqn}::{name} ({phpdoc_ty}) does not accept value {value_ty}"),
                c.span,
                2,
                "classConstant.value",
            );
            return;
        }
        // A single known value's relation to another type is never "maybe".
        let definitely =
            value_ty.is_single_value() || self.relation().disjoint(value_ty, phpdoc_ty);
        let relation = if definitely {
            "is incompatible with"
        } else {
            "is not subtype of"
        };
        report!(
            self,
            format!(
                "PHPDoc tag @var for constant {fqn}::{name} with type {phpdoc_ty} {relation} value {value_ty}."
            ),
            c.span,
            2,
            "classConstant.phpDocType",
        );
    }

    /// The declaration-level checks for a class-like declared inside a
    /// block (a conditional or function-local declaration). Those keyed
    /// on the registry entry only run when that entry was built from
    /// this very declaration's `extends`/`implements`/`use` lists, since
    /// conditional duplicate declarations share one entry.
    pub(crate) fn check_nested_class_declaration(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        let Some(def) = arenas.class(c.def) else {
            return;
        };
        let methods = arenas.methods(def.methods.clone());
        self.check_readonly_class_version(def.is_readonly, false, c.name.span);
        self.check_method_declaration_sanity(fqn, def.kind, def.is_abstract, methods);
        self.check_property_declarations(
            fqn,
            fqn,
            def.kind,
            def.is_abstract,
            def.properties(arenas),
            methods,
        );
        self.check_class_constant_modifiers(fqn, def.kind, def.consts(arenas));
        let own = crate::registry::ClassReflection::from_decl(arenas, c, &self.name_ctx);
        let registry_matches =
            self.registry
                .class_references_by_name(fqn)
                .is_some_and(|references| {
                    references.extends == own.extends().as_slice()
                        && references.implements == own.implements().as_slice()
                        && references.uses == own.uses().as_slice()
                });
        if registry_matches {
            self.check_inheritance_target_kinds(fqn, false, def.span);
            self.check_magic_serialization_methods(fqn, fqn, c.name.span);
            self.check_constant_overrides(fqn, def.consts(arenas));
            self.check_trait_constant_conflicts(fqn, def.consts(arenas));
        }
    }

    /// `OverridingConstantRule`: a class constant redeclaring one from an
    /// immediate interface (checked first) or a non-private parent
    /// constant - it can't override a `final` one (`classConstant.final`)
    /// or narrow its visibility (`classConstant.visibility`), and, at
    /// level 3, must keep a covariant native type
    /// (`classConstant.nativeType`/`.missingNativeType`) and a covariant
    /// `@var` type when both sides declare one (`classConstant.type`).
    pub(crate) fn check_constant_overrides(&mut self, fqn: &str, consts: &[ClassConst]) {
        let Some(info) = self.class_info(fqn) else {
            return;
        };
        let Some(references) = self.registry.class_references_by_name(fqn) else {
            return;
        };
        let interfaces = references.implements.to_vec();
        let parent = references.extends.first().cloned();
        let is_interface = info.kind() == ClassKind::Interface;
        for cc in consts {
            let name = cc.name.as_ref();
            let Some((own_owner, own)) = self.registry.lookup_const_with_owner(fqn, name) else {
                continue;
            };
            if own_owner.as_ref() != fqn {
                continue;
            }
            // An interface's own `extends` list plays the immediate-
            // interface role.
            let immediate: Vec<_> = if is_interface {
                references.extends.to_vec()
            } else {
                interfaces.clone()
            };
            let prototype = immediate
                .iter()
                .find_map(|iface| self.registry.lookup_const_with_owner(iface, name))
                .or_else(|| {
                    if is_interface {
                        return None;
                    }
                    let parent = parent.as_deref()?;
                    self.registry
                        .lookup_const_with_owner(parent, name)
                        .filter(|(_, sig)| sig.visibility() != Visibility::Private)
                });
            let Some((proto_owner, proto)) = prototype else {
                continue;
            };
            let own = own.clone();
            let proto = proto.clone();
            let span = cc.name.span;
            if proto.is_final() {
                report_builder!(
                    self,
                    DiagnosticBuilder::new(
                        format!(
                            "Constant {fqn}::{name} overrides final constant {proto_owner}::{name}."
                        ),
                        span,
                        0,
                        "classConstant.final",
                    )
                    .non_ignorable(),
                );
            }
            let narrowed = match (&proto.visibility(), &own.visibility()) {
                (Visibility::Public, Visibility::Public) => None,
                (Visibility::Public, own_vis) => Some(format!(
                    "{} constant {fqn}::{name} overriding public constant {proto_owner}::{name} should also be public.",
                    if *own_vis == Visibility::Private {
                        "Private"
                    } else {
                        "Protected"
                    }
                )),
                (_, Visibility::Private) => Some(format!(
                    "Private constant {fqn}::{name} overriding protected constant {proto_owner}::{name} should be protected or public."
                )),
                _ => None,
            };
            if let Some(message) = narrowed {
                report_builder!(
                    self,
                    DiagnosticBuilder::new(message, span, 0, "classConstant.visibility")
                        .non_ignorable(),
                );
            }
            if let Some(proto_native) = &proto.native_ty() {
                match &own.native_ty() {
                    Some(own_native) if !self.accepts(proto_native, own_native).yes() => {
                        report_builder!(self, 
                            DiagnosticBuilder::new(format!(
                                    "Native type {own_native} of constant {fqn}::{name} is not covariant with native type {proto_native} of constant {proto_owner}::{name}."
                                ),
                                span,
                                3,
                                "classConstant.nativeType",
                            )
                            .non_ignorable(),
                        );
                    }
                    Some(_) => {}
                    None => {
                        report_builder!(self, 
                            DiagnosticBuilder::new(format!(
                                    "Constant {fqn}::{name} overriding constant {proto_owner}::{name} ({proto_native}) should also have native type {proto_native}."
                                ),
                                span,
                                3,
                                "classConstant.missingNativeType",
                            )
                            .non_ignorable(),
                        );
                    }
                }
            }
            let proto_ty = self
                .registry
                .lookup_const_type(&proto_owner, name)
                .unwrap_or_else(|| proto.ty());
            let own_ty = self
                .registry
                .lookup_const_type(fqn, name)
                .unwrap_or_else(|| own.ty());
            if proto.has_phpdoc_type()
                && own.has_phpdoc_type()
                && !self.accepts(proto_ty, own_ty).yes()
            {
                report!(
                    self,
                    format!(
                        "Type {} of constant {fqn}::{name} is not covariant with type {} of constant {proto_owner}::{name}.",
                        own_ty, proto_ty
                    ),
                    span,
                    3,
                    "classConstant.type",
                );
            }
        }
    }

    /// `ConflictingTraitConstantsRule`: a constant also supplied by a used
    /// trait (at any depth) must keep its visibility
    /// (`classConstant.visibility`), finality (`classConstant.nonFinal`/
    /// `.final`), and native type (`classConstant.nativeType`/
    /// `.missingNativeType`); against an immediately used trait it must
    /// also keep the same value (`classConstant.value`).
    pub(crate) fn check_trait_constant_conflicts(&mut self, fqn: &str, consts: &[ClassConst]) {
        let Some(references) = self.registry.class_references_by_name(fqn) else {
            return;
        };
        let immediate: Vec<Name> = references.uses.to_vec();
        if immediate.is_empty() {
            return;
        }
        let syntax_owner = self
            .syntax_tree_owner()
            .expect("trait-constant values retain their parsed file owner");
        let arenas = syntax_owner.arenas();
        let mut all_traits: Vec<Name> = Vec::new();
        let mut stack = immediate.clone();
        while let Some(t) = stack.pop() {
            if all_traits.contains(&t) {
                continue;
            }
            if let Some(references) = self.registry.class_references_by_name(&t) {
                stack.extend(references.uses.iter().cloned());
            }
            all_traits.push(t);
        }
        let previous_class = self.current_class.replace(fqn.to_string());
        for cc in consts {
            let name = cc.name.as_ref();
            let own_native = cc
                .type_hint
                .as_ref()
                .map(|h| self.resolve_type_hint(Some(h)));
            for trait_fqn in &all_traits {
                let Some(trait_const) = self
                    .registry
                    .lookup_const_with_owner(trait_fqn, name)
                    .filter(|(owner, _)| owner == trait_fqn)
                    .map(|(_, sig)| sig.clone())
                else {
                    continue;
                };
                self.report_trait_constant_conflict(
                    fqn,
                    trait_fqn,
                    cc,
                    &trait_const,
                    own_native.as_ref(),
                );
                if immediate.contains(trait_fqn) {
                    let own_value = arenas
                        .expr(cc.value)
                        .map(|value| self.infer_expr(value, &self.new_locals()))
                        .expect("trait-constant value belongs to its parsed file owner");
                    if !own_value.is_mixed() && own_value != *trait_const.value_ty() {
                        report_builder!(self, 
                            DiagnosticBuilder::new(format!(
                                    "Constant {fqn}::{name} with value {own_value} overriding constant {trait_fqn}::{name} with different value {} should have the same value.",
                                    trait_const.value_ty()
                                ),
                                cc.name.span,
                                0,
                                "classConstant.value",
                            )
                            .non_ignorable(),
                        );
                    }
                }
            }
        }
        self.current_class = previous_class;
    }

    fn report_trait_constant_conflict(
        &mut self,
        fqn: &str,
        trait_fqn: &str,
        cc: &ClassConst,
        trait_const: &crate::registry::ConstantReflection,
        own_native: Option<&Type>,
    ) {
        let name = cc.name.as_ref();
        let word = |v: &Visibility| match v {
            Visibility::Public => "public",
            Visibility::Protected => "protected",
            Visibility::Private => "private",
        };
        let mut errors: Vec<(String, &'static str)> = Vec::new();
        if cc.visibility != trait_const.visibility() {
            let own = word(&cc.visibility);
            let theirs = word(&trait_const.visibility());
            let mut own_cap = own.to_string();
            own_cap[..1].make_ascii_uppercase();
            errors.push((
                format!(
                    "{own_cap} constant {fqn}::{name} overriding {theirs} constant {trait_fqn}::{name} should also be {theirs}."
                ),
                "classConstant.visibility",
            ));
        }
        if trait_const.is_final() && !cc.is_final {
            errors.push((
                format!(
                    "Non-final constant {fqn}::{name} overriding final constant {trait_fqn}::{name} should also be final."
                ),
                "classConstant.nonFinal",
            ));
        } else if !trait_const.is_final() && cc.is_final {
            errors.push((
                format!(
                    "Final constant {fqn}::{name} overriding non-final constant {trait_fqn}::{name} should also be non-final."
                ),
                "classConstant.final",
            ));
        }
        match (&trait_const.native_ty(), own_native) {
            (None, Some(own)) => errors.push((
                format!(
                    "Constant {fqn}::{name} ({own}) overriding constant {trait_fqn}::{name} should not have a native type."
                ),
                "classConstant.nativeType",
            )),
            (Some(theirs), None) => errors.push((
                format!(
                    "Constant {fqn}::{name} overriding constant {trait_fqn}::{name} ({theirs}) should also have native type {theirs}."
                ),
                "classConstant.missingNativeType",
            )),
            (Some(theirs), Some(own)) if theirs != own => errors.push((
                format!(
                    "Constant {fqn}::{name} ({own}) overriding constant {trait_fqn}::{name} ({theirs}) should have the same native type {theirs}."
                ),
                "classConstant.nativeType",
            )),
            _ => {}
        }
        for (message, identifier) in errors {
            report_builder!(
                self,
                DiagnosticBuilder::new(message, cc.name.span, 0, identifier).non_ignorable(),
            );
        }
    }

    /// A concrete class implementing `Serializable` without also providing
    /// `__serialize()`/`__unserialize()` (`class.serializable`), which PHP
    /// 8.1+ deprecates. `registry_name` is the class's registry key,
    /// `display` how messages name it.
    pub(crate) fn check_magic_serialization_methods(
        &mut self,
        registry_name: &str,
        display: &str,
        span: Span,
    ) {
        if self.registry.php_supports_somewhere(
            types::php_version::PhpFeature::SerializableRequiresMagicMethods,
        ) == Some(false)
        {
            return;
        }
        let Some(info) = self.class_info(registry_name) else {
            return;
        };
        if info.kind() != ClassKind::Class
            || info.is_abstract()
            || !self.registry.is_subclass_of(registry_name, "Serializable")
        {
            return;
        }
        for method in ["__serialize", "__unserialize"] {
            let declared = self
                .registry
                .lookup_method(registry_name, method)
                .is_some_and(|sig| !sig.is_magic());
            if declared {
                continue;
            }
            report_builder!(self, 
                DiagnosticBuilder::new(format!(
                        "non-abstract class {display} implements the Serializable interface, but does not implement {method}()"
                    ),
                    span,
                    0,
                    "class.serializable",
                )
                .tip("See https://wiki.php.net/rfc/phase_out_serializable"),
            );
        }
    }

    /// `readonly class` needs PHP 8.2+, a `readonly` anonymous class PHP
    /// 8.3+ (`ReadOnlyClassRule`). Reported under real PHPStan's own
    /// identifier for this rule, `classConstant.nativeTypeNotSupported`.
    pub(crate) fn check_readonly_class_version(
        &mut self,
        is_readonly: bool,
        is_anonymous: bool,
        span: Span,
    ) {
        if !is_readonly {
            return;
        }
        let (feature, message) = if is_anonymous {
            (
                types::php_version::PhpFeature::ReadOnlyAnonymousClasses,
                "Anonymous readonly classes are supported only on PHP 8.3 and later.",
            )
        } else {
            (
                types::php_version::PhpFeature::ReadOnlyClasses,
                "Readonly classes are supported only on PHP 8.2 and later.",
            )
        };
        if self.registry.php_supports_everywhere(feature) != Some(false) {
            return;
        }
        report_builder!(
            self,
            DiagnosticBuilder::new(
                message.to_string(),
                span,
                0,
                "classConstant.nativeTypeNotSupported",
            )
            .non_ignorable(),
        );
    }

    /// Four small, independent PHP-version/modifier-consistency checks
    /// on a class constant's declaration, matching real PHPStan's
    /// identifiers:
    ///
    /// `classConstant.finalNotSupported` (`FinalConstantRule`) - `final
    /// const` needs PHP 8.1+; `classConstant.finalPrivate`
    /// (`FinalPrivateConstantRule`) - a `private` constant can never be
    /// `final` too, since nothing outside its declaring class could ever
    /// see it to override in the first place - unconditional, no
    /// PHP-version gate; `classConstant.nativeTypeNotSupported`
    /// (`NativeTypedClassConstantRule`) - a typed constant (`const TYPE
    /// NAME = value;`) needs PHP 8.3+; `classConstant.inTrait`
    /// (`ConstantsInTraitsRule`) - a trait's own `const` declaration
    /// needs PHP 8.2+.
    pub(crate) fn check_class_constant_modifiers(
        &mut self,
        fqn: &str,
        kind: ClassKind,
        consts: &[ClassConst],
    ) {
        for c in consts {
            let constant = format!("{fqn}::{}", c.name);
            self.check_type_declaration(
                c.type_hint.as_ref(),
                crate::checker::type_declarations::TypePosition::ClassConstant(&constant),
                c.type_hint.as_ref().map(|h| h.span).unwrap_or(c.span),
            );
            if kind == ClassKind::Trait
                && self
                    .registry
                    .php_supports_everywhere(types::php_version::PhpFeature::ConstantsInTraits)
                    == Some(false)
            {
                report_builder!(self, 
                    DiagnosticBuilder::new("Constant is declared inside a trait but is only supported on PHP 8.2 and later.".to_string(),
                        c.span,
                        0,
                        "classConstant.inTrait",
                    )
                    .non_ignorable(),
                );
            }
            if c.is_final
                && self
                    .registry
                    .php_supports_everywhere(types::php_version::PhpFeature::FinalClassConstants)
                    == Some(false)
            {
                report_builder!(
                    self,
                    DiagnosticBuilder::new(
                        "Final class constants are supported only on PHP 8.1 and later."
                            .to_string(),
                        c.span,
                        0,
                        "classConstant.finalNotSupported",
                    )
                    .non_ignorable(),
                );
            }
            if c.is_final && c.visibility == Visibility::Private {
                report_builder!(self, 
                    DiagnosticBuilder::new(format!(
                            "Private constant {fqn}::{} cannot be final as it is never overridden by other classes.",
                            c.name
                        ),
                        c.span,
                        0,
                        "classConstant.finalPrivate",
                    )
                    .non_ignorable(),
                );
            }
            if c.type_hint.is_some()
                && self.registry.php_supports_everywhere(
                    types::php_version::PhpFeature::NativeTypedClassConstants,
                ) == Some(false)
            {
                report_builder!(self, 
                    DiagnosticBuilder::new("Class constants with native types are supported only on PHP 8.3 and later.".to_string(),
                        c.span,
                        0,
                        "classConstant.nativeTypeNotSupported",
                    )
                    .non_ignorable(),
                );
            }
        }
    }

    /// A batch of small, independent method-declaration boolean-flag-
    /// combination checks real PHP enforces unconditionally at compile
    /// time - this project's parser is deliberately more lenient than
    /// the real engine (it has to tolerate parsing code that might never
    /// actually run), so nothing else would catch any of these before
    /// that code runs and fatals for real.
    ///
    /// `kind`/`is_class_abstract` describe the *declaring* class/enum
    /// (never `Trait` for `is_class_abstract` - a trait itself is never
    /// `abstract`/non-abstract in real PHP's own sense, and traits are
    /// deliberately exempt from the "abstract method needs an abstract
    /// container" check below the same way `check_class_completeness`
    /// already exempts them from "must implement every abstract
    /// method" - a trait's whole point is supplying a method the
    /// *using* class must complete).
    pub(crate) fn check_method_declaration_sanity(
        &mut self,
        fqn: &str,
        kind: ClassKind,
        is_class_abstract: bool,
        methods: &[MethodDecl],
    ) {
        for m in methods {
            // `method.abstract`: an abstract method inside a non-
            // abstract, non-interface, non-trait class/enum - real PHP:
            // "Class Foo declares abstract method bar() and must
            // therefore be declared abstract." An enum's built-in
            // `cases()`/`from()`/`tryFrom()` are exempt, mirroring real
            // PHPStan's identical exemption.
            if m.is_abstract
                && !is_class_abstract
                && matches!(kind, ClassKind::Class | ClassKind::Enum)
            {
                let lower = m.function.name.to_ascii_lowercase();
                let enum_builtin = kind == ClassKind::Enum
                    && matches!(lower.as_str(), "cases" | "from" | "tryfrom");
                if !enum_builtin {
                    let description = if kind == ClassKind::Enum {
                        "Enum"
                    } else {
                        "Non-abstract class"
                    };
                    report_builder!(
                        self,
                        DiagnosticBuilder::new(
                            format!(
                                "{description} {fqn} contains abstract method {}()",
                                m.function.name
                            ),
                            m.span,
                            0,
                            "method.abstract",
                        )
                        .non_ignorable(),
                    );
                }
            }
            // `method.nonAbstract`: a bodiless method that isn't abstract,
            // outside an interface - real PHP: "Non-abstract method
            // Foo::bar() must contain body."
            if !m.is_abstract && kind != ClassKind::Interface && m.function.body.is_none() {
                report_builder!(
                    self,
                    DiagnosticBuilder::new(
                        format!(
                            "Non-abstract method {fqn}::{}() must contain a body",
                            m.function.name
                        ),
                        m.span,
                        0,
                        "method.nonAbstract",
                    )
                    .non_ignorable(),
                );
            }
            // `method.abstractPrivate`: an abstract method can never be
            // `private` - nothing could ever provide the required
            // implementation, since a `private` method isn't visible to
            // (and so can't be overridden by) a subclass at all. Only
            // meaningful where an abstract method is legal to declare in
            // the first place (an abstract class or an interface);
            // elsewhere it's already `method.abstract`'s concern, and
            // traits are exempt for the same reason as above. Every
            // interface method is implicitly abstract regardless of the
            // literal `abstract` keyword - real PHP actually forbids
            // writing that keyword on one at all, so `m.is_abstract`
            // alone would never observe this shape; `kind ==
            // ClassKind::Interface` stands in for it here.
            if (m.is_abstract || kind == ClassKind::Interface)
                && m.visibility == Visibility::Private
                && kind != ClassKind::Trait
                && (is_class_abstract || kind == ClassKind::Interface)
            {
                report_builder!(
                    self,
                    DiagnosticBuilder::new(
                        format!(
                            "private method {fqn}::{}() cannot be abstract",
                            m.function.name
                        ),
                        m.span,
                        0,
                        "method.abstractPrivate",
                    )
                    .non_ignorable(),
                );
            }
            // `method.finalPrivate`: real PHP only warns here (not a
            // fatal error), since a `final` on a `private` method is
            // pointless rather than contradictory - a private method is
            // never overridden by anything, `final` or not.
            // `__construct` is deliberately exempt, mirroring real
            // PHPStan's identical exemption - a private final
            // constructor is a real, common, legitimate singleton-
            // pattern idiom, not the "forgot final does nothing here"
            // mistake this check otherwise exists to catch.
            if m.is_final && m.visibility == Visibility::Private && !m.function.is_constructor() {
                report!(
                    self,
                    format!(
                        "private method {fqn}::{}() cannot be final as it is never overridden by other classes",
                        m.function.name
                    ),
                    m.span,
                    1,
                    "method.finalPrivate",
                );
            }
            // `method.visibility`: every interface method is implicitly
            // `public` - real PHP: "Access type for interface method
            // Foo::bar() must be public." Reuses the exact same
            // identifier the override-visibility-narrowing check uses,
            // matching real PHPStan's deliberate choice to share it
            // between both shapes.
            if kind == ClassKind::Interface && m.visibility != Visibility::Public {
                report_builder!(
                    self,
                    DiagnosticBuilder::new(
                        format!(
                            "method {fqn}::{}() cannot use non-public visibility in interface",
                            m.function.name
                        ),
                        m.span,
                        0,
                        "method.visibility",
                    )
                    .non_ignorable(),
                );
            }
            // `constructor.returnType`: real PHP: "Method Foo::
            // __construct() cannot declare a return type." - a
            // constructor's own return value is never meaningful (it
            // always yields the new instance), so a native return type
            // is always a mistake, never legitimate.
            if m.function.is_constructor() && m.function.return_type.is_some() {
                report!(
                    self,
                    format!("constructor of class {fqn} has a return type"),
                    m.span,
                    0,
                    "constructor.returnType",
                );
            }
        }
    }

    /// A concrete class must provide a real implementation of every
    /// abstract method it inherits (from a parent class, an implemented
    /// interface, or a directly-`use`d trait) - real PHP raises a fatal
    /// error at declaration time otherwise, independent of whether
    /// anything ever calls the missing method. Interfaces, traits, and
    /// abstract classes are themselves exempt (a class that legitimately
    /// leaves a method unimplemented is required to declare itself
    /// `abstract`, and one that does is not itself required to be
    /// complete).
    pub(crate) fn check_class_completeness(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.class_info(fqn) else {
            return;
        };
        if info.kind() != ClassKind::Class && info.kind() != ClassKind::Enum {
            return;
        }
        if info.is_abstract() {
            return;
        }
        let required = self.registry.required_abstract_methods(fqn);
        let missing: Vec<&Name> = required
            .iter()
            .filter(|m| !self.registry.has_concrete_implementation(fqn, m))
            .collect();
        if missing.is_empty() {
            return;
        }
        let shown: Vec<&str> = missing.iter().take(3).map(|s| s.as_ref()).collect();
        let mut message = format!(
            "class {fqn} does not implement abstract method(s): {}",
            shown.join(", ")
        );
        if missing.len() > shown.len() {
            message.push_str(&format!(", and {} more", missing.len() - shown.len()));
        }
        report_builder!(
            self,
            DiagnosticBuilder::new(message, span, 0, "class.abstractMethodNotImplemented").tip(
                "either implement every method listed above, or declare this class itself abstract",
            ),
        );
    }

    /// Real `phpstan-strict-rules`' `RequireParentConstructCallRule`
    /// (`constructor.missingParentCall`) - a concrete class's own
    /// `__construct()` that never calls `parent::__construct()`
    /// anywhere in its body, when the nearest ancestor to actually
    /// declare its own constructor (not just inherit one further up)
    /// has one that's real, callable, and non-private.
    /// `lookup_ancestor_method_with_owner` already gets that "nearest
    /// real declaration" semantics for free.
    pub(crate) fn check_parent_constructor_call(&mut self, fqn: &str, c: &ClassDef) {
        if !self.settings.strict_rules.check_parent_constructor_call {
            return;
        }
        if c.kind == ClassKind::Trait || c.kind == ClassKind::Interface {
            return;
        }
        let Some(owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = owner.arenas();
        let Some(ctor) = c.constructor(arenas) else {
            return;
        };
        if ctor.is_abstract {
            return;
        }
        let Some((ancestor, ancestor_sig)) = self
            .registry
            .lookup_ancestor_method_with_owner(fqn, "__construct")
        else {
            return;
        };
        if ancestor_sig.visibility() == Visibility::Private {
            return;
        }
        if self.registry.method_is_abstract(&ancestor, "__construct") {
            return;
        }
        if body_calls_parent_construct(arenas, ctor.function.body_stmts(arenas)) {
            return;
        }
        if self.shows_level(4) {
            let mut builder = DiagnosticBuilder::new(
                format!("{fqn}::__construct() does not call parent constructor from {ancestor}."),
                ctor.function.span,
                4,
                "constructor.missingParentCall",
            );
            let needs_arguments = ancestor_sig
                .params()
                .iter()
                .any(|p| !p.has_default() && !p.variadic());
            if !needs_arguments
                && let Some((span, text)) = parent_call_insertion(arenas, ctor, &self.line_index)
            {
                builder = builder.fix(span, text, "Call parent::__construct()");
            }
            self.report_builder(builder);
        } else {
            self.note_below_level();
        }
    }

    /// Suggests constructor property promotion for a trivially-
    /// forwarding constructor: every parameter, in order, has exactly
    /// one matching `$this->name = $name;` statement and nothing else in
    /// the body, none of the parameters are already promoted, and each
    /// target property is a plain (non-static, non-hooked) declaration
    /// on this same class with the identical native type. Requires a
    /// configured PHP version >= 8.0.
    pub(crate) fn check_promoted_property_candidate(&mut self, fqn: &str, c: &ClassDef) {
        if !self.settings.extras.check_promoted_property_suggestions {
            return;
        }
        if self
            .registry
            .php_supports_everywhere(types::php_version::PhpFeature::PromotedProperties)
            != Some(true)
        {
            return;
        }
        let Some(arenas) = self.syntax_arenas() else {
            return;
        };
        let Some(ctor) = c.constructor(arenas) else {
            return;
        };
        let params = arenas.params(ctor.function.params.clone());
        if params.is_empty()
            || params
                .iter()
                .any(|p| p.by_ref || p.variadic || p.promotion.is_some())
        {
            return;
        }
        let body = ctor.function.body_stmts(arenas);
        if body.len() != params.len() {
            return;
        }
        for (param, stmt_id) in params.iter().zip(body.iter()) {
            let Some(stmt) = arenas.stmt(*stmt_id) else {
                return;
            };
            let StmtKind::Expression(e) = &stmt.kind else {
                return;
            };
            let Some(e) = arenas.peeled(*e).and_then(|id| arenas.expr(id)) else {
                return;
            };
            let ExprKind::Assign(lhs, rhs) = &e.kind else {
                return;
            };
            let Some(lhs) = arenas.peeled(*lhs).and_then(|id| arenas.expr(id)) else {
                return;
            };
            let ExprKind::PropertyAccess(recv, prop_name, ObjectOperator::Arrow) = &lhs.kind else {
                return;
            };
            let Some(recv) = arenas.peeled(*recv).and_then(|id| arenas.expr(id)) else {
                return;
            };
            if !matches!(&recv.kind, ExprKind::Variable(v) if **v == *"this") {
                return;
            }
            let Some(rhs) = arenas.peeled(*rhs).and_then(|id| arenas.expr(id)) else {
                return;
            };
            if !matches!(&rhs.kind, ExprKind::Variable(v) if v == &param.name) {
                return;
            }
            if **prop_name != *param.name {
                return;
            }
            let Some(prop) = c.properties(arenas).iter().find(|p| *p.name == **prop_name) else {
                return;
            };
            if prop.is_static
                || prop.hooks.is_some()
                || !TypeHint::same(prop.type_hint.as_ref(), param.type_hint.as_ref(), arenas)
            {
                return;
            }
        }
        report_builder!(self, 
            DiagnosticBuilder::new(format!(
                    "{fqn}::__construct() only forwards its parameters into matching properties - consider constructor property promotion instead"
                ),
                ctor.function.span,
                5,
                "constructor.promotionCandidate",
            )
            .tip("merge each property's own declaration into its constructor parameter"),
        );
    }

    /// `@phpstan-require-extends`/`@phpstan-require-implements`'s
    /// consumer half, mirroring real PHPStan's `RequireExtendsRule`/
    /// `RequireImplementsRule`. For every interface/trait `fqn`
    /// transitively pulls in (`Registry::all_ancestor_names`, both
    /// `extends`/`implements` for interfaces and `uses` for traits -
    /// both real rules are exactly this transitive) that declares one of
    /// these tags, `fqn` itself must satisfy it (`is_subclass_of`, since
    /// both "extend" and "implement" here mean "be a subtype of," not
    /// "declare directly").
    pub(crate) fn check_require_tags(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.class_info(fqn) else {
            return;
        };
        // Real PHPStan registers both consuming rules for `InClassNode`
        // only - a trait declaration gets its own separate `InTraitNode`
        // instead, so a trait is never itself "the checked class" here,
        // only an ancestor consulted for classes that use it. Without
        // this, a trait using another require-tagged trait would always
        // wrongly fail its own check - a trait can't `extends`/
        // `implements` anything, so `is_subclass_of`/membership would
        // never be satisfiable for one regardless of what any real
        // using class eventually does.
        if info.kind() == ClassKind::Trait {
            return;
        }
        // Real PHP: a checked class that is itself an interface is
        // exempt from require-extends (an interface can't `extends` a
        // concrete class the way a class does), mirroring
        // `RequireExtendsRule`'s top-level guard. Interfaces can't `use`
        // traits at all, so require-implements never has anything to
        // check for one either way.
        let is_interface = info.kind() == ClassKind::Interface;
        for ancestor_fqn in self.registry.all_ancestor_names(fqn) {
            let Some(ancestor_info) = self.registry.class_by_name(&ancestor_fqn) else {
                continue;
            };
            if !is_interface {
                for required in ancestor_info.require_extends().clone() {
                    if self.registry.is_subclass_of(fqn, &required) {
                        continue;
                    }
                    let (kind_word, relation_word) = if ancestor_info.kind() == ClassKind::Trait {
                        ("Trait", "using")
                    } else {
                        ("Interface", "implementing")
                    };
                    report!(
                        self,
                        format!(
                            "{kind_word} {ancestor_fqn} requires {relation_word} class to extend {required}, but {fqn} does not."
                        ),
                        span,
                        0,
                        "class.missingExtends",
                    );
                }
            }
            for required in ancestor_info.require_implements().clone() {
                if self.registry.is_subclass_of(fqn, &required) {
                    continue;
                }
                report!(
                    self,
                    format!(
                        "Trait {ancestor_fqn} requires using class to implement {required}, but {fqn} does not."
                    ),
                    span,
                    0,
                    "class.missingImplements",
                );
            }
        }
    }

    /// `@phpstan-sealed`'s consumer half, mirroring real PHPStan's
    /// `AllowedSubTypesRule` - the actual enforcement mechanism, not (as
    /// the tag's name might suggest) a standalone "illegal extends" rule
    /// of its own. Checked against immediate parents only (unlike
    /// `check_require_tags` above) - a sealed hierarchy's allowlist
    /// enumerates the complete, direct child set, not every transitive
    /// descendant.
    ///
    /// Real PHPStan's `getAllowedSubTypes` additionally allows a
    /// concrete (non-abstract) sealed class its own name for free, with
    /// no need to list itself - not modeled here, since that self-entry
    /// can only ever matter to this rule if the checked class were
    /// literally its own parent (impossible). The self-entry exists for
    /// `getAllowedSubTypes`'s other consumers (type-narrowing
    /// exhaustiveness over a sealed hierarchy), a separate, much larger
    /// feature this project doesn't attempt.
    pub(crate) fn check_sealed_hierarchy(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.class_info(fqn) else {
            return;
        };
        // Same `InClassNode`-only registration as `check_require_tags`
        // above - a trait is never itself the checked class. Real
        // PHPStan's `AllowedSubTypesRule` also never consults `uses` at
        // all (only `extends`/immediate `implements`), so a trait being
        // sealed is never actually enforced there either way.
        if info.kind() == ClassKind::Trait {
            return;
        }
        let kind = info.kind();
        let Some(references) = self.registry.class_references_by_name(fqn) else {
            return;
        };
        for parent_fqn in references.extends.iter().chain(references.implements) {
            let Some(parent_info) = self.registry.class_by_name(parent_fqn) else {
                continue;
            };
            let attribute_sealed: &[Name] = if self.settings.language_extensions {
                &parent_info.access().sealed
            } else {
                &[]
            };
            let permitted = || {
                parent_info
                    .sealed()
                    .iter()
                    .chain(attribute_sealed)
                    .any(|s| s.as_ref() == fqn)
            };
            if (parent_info.sealed().is_empty() && attribute_sealed.is_empty()) || permitted() {
                continue;
            }
            report!(
                self,
                format!("Type {fqn} is not allowed to be a subtype of {parent_fqn}."),
                span,
                0,
                format!("{}.disallowedSubtype", class_kind_identifier(&kind)),
            );
        }
    }
}

/// Whether `stmts` contains a `parent::__construct(...)` call as a
/// statement anywhere in the body, nested blocks (`if`/`else`/`try`/
/// `switch`/loops) included - real PHP call resolution is
/// case-insensitive, so the class/method names are compared that way too.
fn body_calls_parent_construct(arenas: &NodeArenas, stmts: &[StmtId]) -> bool {
    struct Search(bool);
    impl Visitor for Search {
        fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
            if self.0 {
                return;
            }
            let Some(stmt) = arenas.stmt(id) else {
                return;
            };
            if let StmtKind::Expression(expr) = &stmt.kind
                && let Some(expr) = arenas.peeled(*expr).and_then(|id| arenas.expr(id))
                && let ExprKind::StaticCall(class, method, _) = &expr.kind
                && class
                    .as_name()
                    .is_some_and(|name| name.is_keyword("parent"))
                && syntax::ast::is_constructor_name(method)
            {
                self.0 = true;
                return;
            }
            arena::walk_stmt(self, arenas, id);
        }

        fn visit_expr(&mut self, _: &NodeArenas, _: ExprId) {}
    }
    let mut search = Search(false);
    arena::walk_stmts(&mut search, arenas, stmts);
    search.0
}

pub(crate) fn visibility_word(v: &Visibility) -> &'static str {
    match v {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::Private => "private",
    }
}

/// Whether two trait methods with the same name are genuinely
/// incompatible, not just written slightly differently - deliberately
/// narrow (exact param count and exact per-position/return type
/// equality) rather than a real Liskov-style compatibility check: this
/// is comparing two unrelated trait declarations with no inheritance
/// relationship between them, so there's no contravariance/covariance
/// direction to apply - only "are these the same signature" makes sense
/// here.
pub(crate) fn signatures_compatible(a: &FunctionReflection, b: &FunctionReflection) -> bool {
    if a.params().len() != b.params().len() {
        return false;
    }
    if *a.return_type() != *b.return_type() {
        return false;
    }
    a.params()
        .iter()
        .zip(b.params())
        .all(|(pa, pb)| pa.ty() == pb.ty())
}

/// Where and what to insert so `ctor`'s body starts with
/// `parent::__construct();`: before its first statement at that
/// statement's indentation, or, for an empty body, the body rewritten
/// onto its own lines.
fn parent_call_insertion(
    arenas: &NodeArenas,
    ctor: &MethodDecl,
    index: &LineIndex,
) -> Option<(Span, String)> {
    const CALL: &str = "parent::__construct();";
    if let Some(first) = ctor.function.body_stmts(arenas).first() {
        let at = arenas
            .stmt_span(*first)
            .expect("constructor statement has a parallel span")
            .start;
        let indent = " ".repeat(index.line_col(at).col.saturating_sub(1) as usize);
        return Some((Span { start: at, end: at }, format!("{CALL}\n{indent}")));
    }
    let open = ctor.function.body.as_ref()?.open;
    let close = ctor.function.span.end;
    let member_indent = index.line_col(ctor.span.start).col.saturating_sub(1) as usize;
    let outer = " ".repeat(member_indent);
    let inner = " ".repeat(member_indent + 4);
    Some((
        Span {
            start: open.start,
            end: close,
        },
        format!("{{\n{inner}{CALL}\n{outer}}}"),
    ))
}

/// How a message names a declaration, written out only when a message is:
/// "anonymous class" without a word, otherwise "<word> <name>".
#[derive(Clone, Copy)]
struct DeclarationName<'a> {
    word: Option<&'static str>,
    name: &'a str,
}

impl std::fmt::Display for DeclarationName<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.word {
            Some(word) => write!(f, "{word} {}", self.name),
            None => f.write_str("anonymous class"),
        }
    }
}
