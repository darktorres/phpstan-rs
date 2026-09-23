use std::collections::HashMap;

use crate::ast::*;
use crate::ast_walk::any_stmt_child;
use crate::checker::{Checker, DiagnosticBuilder, Locals, class_kind_identifier, unwrap_grouping};
use crate::docblock;
use crate::registry::{
    FunctionSig, PropertySig, collect_class_name_case_refs, object_class_names,
    resolve_names_in_type,
};
use crate::token::Span;
use crate::types::{self, Type};

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
        label: &str,
        span: Span,
    ) {
        let Some(info) = self.registry.classes.get(registry_name) else {
            return;
        };
        let kind = info.kind;
        let subject = if is_anonymous {
            "anonymous class".to_string()
        } else {
            format!("{} {registry_name}", class_kind_identifier(&kind))
        };
        let bare_name = if is_anonymous {
            "anonymous class".to_string()
        } else {
            format!("class {registry_name}")
        };
        let is_readonly = info.is_readonly;
        let target_kind = |name: &str| {
            self.registry
                .lookup_class(name)
                .map(|(fqn, i)| (fqn.to_string(), i.kind))
        };
        let mut reports: Vec<(String, &'static str)> = Vec::new();

        for parent in &info.extends {
            let Some((parent_fqn, parent_info)) = self.registry.lookup_class(parent) else {
                continue;
            };
            let parent_kind = class_kind_identifier(&parent_info.kind);
            if kind == ClassKind::Interface {
                if parent_info.kind != ClassKind::Interface {
                    reports.push((
                        format!("{subject} cannot extend {parent_kind} {parent_fqn}"),
                        match parent_info.kind {
                            ClassKind::Trait => "interfaceExtends.trait",
                            ClassKind::Enum => "interfaceExtends.enum",
                            _ => "interfaceExtends.class",
                        },
                    ));
                }
                continue;
            }
            match parent_info.kind {
                ClassKind::Interface | ClassKind::Trait | ClassKind::Enum => reports.push((
                    format!("{subject} cannot extend {parent_kind} {parent_fqn}"),
                    match parent_info.kind {
                        ClassKind::Interface => "class.extendsInterface",
                        ClassKind::Trait => "class.extendsTrait",
                        _ => "class.extendsEnum",
                    },
                )),
                ClassKind::Class => {
                    if parent_info.is_final {
                        reports.push((
                            format!("{subject} cannot extend final class {parent_fqn}"),
                            "class.extendsFinal",
                        ));
                    } else if parent_info.is_final_by_phpdoc {
                        reports.push((
                            format!("{subject} extends @final class {parent_fqn}"),
                            "class.extendsFinalByPhpDoc",
                        ));
                    }
                    if is_readonly && !parent_info.is_readonly {
                        reports.push((
                            format!(
                                "readonly {bare_name} cannot extend non-readonly class {parent_fqn}"
                            ),
                            "class.readOnly",
                        ));
                    } else if !is_readonly && parent_info.is_readonly {
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

        for iface in &info.implements {
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

        for used in &info.uses {
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
            self.report(label.to_string(), message, span, 0, identifier);
        }
    }

    /// The class/enum-declaration-level half of the "does this name
    /// actually resolve" existence pipeline - every `extends` target (a
    /// class's single parent, or an interface's possibly-multiple parent
    /// interfaces), every `implements` target, and every `use`d trait.
    /// Checks every entry - an interface's `extends A, B;` is real,
    /// valid PHP with more than one target. Operates purely on the
    /// already-indexed, already name-resolved `ClassInfo`, so this needs
    /// no raw `ClassDecl`/`EnumDecl` at all and covers both uniformly
    /// from one call site. Existence-only - kind compatibility (a
    /// class `implements`-ing another class instead of an interface) is
    /// `check_inheritance_target_kinds`'.
    pub(crate) fn check_class_references_exist(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for parent in &info.extends {
            if !self.class_name_exists(parent) {
                let verb = if info.kind == ClassKind::Interface {
                    "extends unknown interface"
                } else {
                    "extends unknown class"
                };
                self.report(
                    fqn.to_string(),
                    format!("{fqn} {verb} {parent}"),
                    span,
                    0,
                    "class.notFound",
                );
            }
        }
        for iface in &info.implements {
            if !self.class_name_exists(iface) {
                self.report(
                    fqn.to_string(),
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
        for used_trait in &info.uses {
            if !self.class_name_exists(used_trait) {
                self.report(
                    fqn.to_string(),
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
    /// `FunctionSig` (`ClassInfo::method_tags`) but never existence-
    /// checked. Pure reuse of `check_type_class_names_exist` - no new
    /// existence machinery. `methodTag.trait` (the type resolves but is
    /// a trait, invalid there) is deliberately not built - no analog
    /// exists anywhere else in this project for any typehint location.
    pub(crate) fn check_method_tag_types(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        // `Registry::resolve_method_tags` drains `ClassInfo::method_tags`
        // into `info.methods` before this checker pass runs, so this
        // class's `@method` tags are already merged and indistinguishable
        // there from a real declared method by name alone.
        // `FunctionSig::is_magic` is the one field `method_tag_sig` sets
        // that a real declared method's `FunctionSig::from_decl` never
        // does, so it's the correct filter here. Also matches enum
        // `cases()`/`from()`/`tryFrom()` (the other `is_magic: true`
        // producer) harmlessly - their synthesized types never reference
        // an unresolvable class.
        for (name, sig) in info.methods.iter().filter(|(_, sig)| sig.is_magic) {
            for param in &sig.params {
                self.check_type_class_names_exist(
                    &param.ty,
                    &format!("PHPDoc tag @method for method {fqn}::{name}()"),
                    span,
                    fqn,
                );
                // `class.nameCase` for this `@method`-tag parameter.
                self.check_type_class_name_case(&param.case_refs, fqn, span);
                self.check_type_internal_usage(&param.case_refs, fqn, span);
            }
            self.check_type_class_names_exist(
                &sig.return_type,
                &format!("PHPDoc tag @method for method {fqn}::{name}()"),
                span,
                fqn,
            );
            self.check_type_class_name_case(&sig.return_case_refs, fqn, span);
            self.check_type_internal_usage(&sig.return_case_refs, fqn, span);
        }
    }

    /// Mirrors real PHPStan's `MixinCheck` - `@mixin Type` is already
    /// resolved to a bare FQN string (`ClassInfo::mixins`) and consumed
    /// for its real semantic effect (the mixin's methods/properties
    /// become available on the host class), but its own existence needs
    /// checking too - an unresolvable `@mixin` target would otherwise
    /// silently contribute nothing instead of surfacing a real typo.
    /// `mixin.nonObject`/`mixin.trait` are deliberately not built, same
    /// scope as `check_method_tag_types`.
    pub(crate) fn check_mixin_types(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for mixin in &info.mixins {
            if !self.class_name_exists(mixin) {
                self.report(
                    fqn.to_string(),
                    format!("PHPDoc tag @mixin references unknown class {mixin}."),
                    span,
                    0,
                    "class.notFound",
                );
            }
        }
        // `class.nameCase` for an `@mixin` target - `check_mixin_types`
        // doesn't route through `check_type_class_names_exist` at all
        // (its bare `class_name_exists` check above), so this is a
        // parallel loop rather than a rewire of an existing one.
        self.check_type_class_name_case(&info.mixin_case_refs, fqn, span);
        self.check_type_internal_usage(&info.mixin_case_refs, fqn, span);
    }

    /// Mirrors real PHPStan's `RequireExtendsCheck`/
    /// `RequireImplementsDefinitionTraitRule`'s full declaration-validity
    /// family, not just the existence half - `@phpstan-require-extends`/
    /// `-implements`'s RHS is re-parsed directly from `c`'s own doc
    /// comment (rather than read back from `ClassInfo::require_extends`/
    /// `require_implements`, which is already flattened across every tag
    /// into one combined `Vec<String>` of only the successfully-
    /// extracted object class names - exactly the information a
    /// `.nonObject`/`.duplicate` check needs to tell apart from "no tag
    /// at all"), mirroring real PHPStan's `getObjectClassNames()`/
    /// `RequireExtendsTag` resolution pipeline. The RHS's own case-
    /// sensitivity re-check is deliberately not built here - that's the
    /// dedicated `class.nameCase` pipeline-wide expansion, not
    /// re-litigated per tag family.
    pub(crate) fn check_require_tag_types(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(doc) = c.doc_comment.as_deref().map(docblock::parse) else {
            return;
        };
        if doc.require_extends.len() > 1 {
            self.report(
                fqn.to_string(),
                "PHPDoc tag @phpstan-require-extends can only be used once.".to_string(),
                c.span,
                0,
                "requireExtends.duplicate",
            );
        }
        for raw in &doc.require_extends {
            let parsed = types::parse_type_string(raw);
            let mut case_refs = Vec::new();
            collect_class_name_case_refs(&parsed, &self.name_ctx, &mut case_refs);
            self.check_type_class_name_case(&case_refs, fqn, c.span);
            let ty = resolve_names_in_type(parsed, &self.name_ctx);
            let names = object_class_names(ty.clone());
            if names.is_empty() {
                self.report(
                    fqn.to_string(),
                    format!("PHPDoc tag @phpstan-require-extends contains non-object type {ty}."),
                    c.span,
                    0,
                    "requireExtends.nonObject",
                );
                continue;
            }
            for name in names {
                if !self.class_name_exists(&name) {
                    self.report(
                        fqn.to_string(),
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
                match info.kind {
                    ClassKind::Interface => {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                fqn.to_string(),
                                format!(
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
                        self.report(
                            fqn.to_string(),
                            format!(
                                "PHPDoc tag @phpstan-require-extends cannot contain non-class type {name}."
                            ),
                            c.span,
                            0,
                            &format!("requireExtends.{}", class_kind_identifier(&info.kind)),
                        );
                    }
                    ClassKind::Class if info.is_final => {
                        self.report(
                            fqn.to_string(),
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
            let parsed = types::parse_type_string(raw);
            let mut case_refs = Vec::new();
            collect_class_name_case_refs(&parsed, &self.name_ctx, &mut case_refs);
            self.check_type_class_name_case(&case_refs, fqn, c.span);
            let ty = resolve_names_in_type(parsed, &self.name_ctx);
            let names = object_class_names(ty.clone());
            if names.is_empty() {
                self.report(
                    fqn.to_string(),
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
                    self.report(
                        fqn.to_string(),
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
                if info.kind != ClassKind::Interface {
                    self.report(
                        fqn.to_string(),
                        format!(
                            "PHPDoc tag @phpstan-require-implements cannot contain non-interface type {name}."
                        ),
                        c.span,
                        0,
                        &format!("requireImplements.{}", class_kind_identifier(&info.kind)),
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
    /// `ClassInfo::sealed`. Takes scalars rather than a whole
    /// `&ClassDecl` since `@phpstan-sealed`'s `sealed.onEnum` check needs
    /// a real `StmtKind::EnumDecl` call site too - `EnumDecl` and
    /// `ClassDecl` are different AST types with no shared struct to
    /// borrow fields from.
    pub(crate) fn check_sealed_tag_types(
        &mut self,
        fqn: &str,
        doc_comment: Option<&str>,
        span: Span,
        is_enum: bool,
    ) {
        let Some(doc) = doc_comment.map(docblock::parse) else {
            return;
        };
        if doc.sealed.is_empty() {
            return;
        }
        if is_enum {
            self.report(
                fqn.to_string(),
                "PHPDoc tag @phpstan-sealed is only valid on class or interface.".to_string(),
                span,
                0,
                "sealed.onEnum",
            );
            return;
        }
        for raw in &doc.sealed {
            let parsed = types::parse_type_string(raw);
            let mut case_refs = Vec::new();
            collect_class_name_case_refs(&parsed, &self.name_ctx, &mut case_refs);
            self.check_type_class_name_case(&case_refs, fqn, span);
            let ty = resolve_names_in_type(parsed, &self.name_ctx);
            let names = object_class_names(ty.clone());
            if names.is_empty() {
                self.report(
                    fqn.to_string(),
                    format!("PHPDoc tag @phpstan-sealed contains non-object type {ty}."),
                    span,
                    0,
                    "sealed.nonObject",
                );
                continue;
            }
            for name in names {
                if !self.class_name_exists(&name) {
                    self.report(
                        fqn.to_string(),
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
    /// (`ClassInfo::type_aliases`) but never existence-checked. Only the
    /// alias-RHS half is built here, not `@phpstan-import-type`'s
    /// `from Class` existence: that class is commonly only reachable via
    /// Composer autoloading this registry may legitimately have no data
    /// for in a partial-analysis run.
    pub(crate) fn check_type_alias_types(&mut self, fqn: &str, span: Span) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for (name, ty) in &info.type_aliases {
            self.check_type_class_names_exist(
                ty,
                &format!("PHPDoc tag @phpstan-type {name} for class {fqn}"),
                span,
                fqn,
            );
            // `class.nameCase` for this alias's RHS - looked up by the
            // same key already in scope here.
            if let Some(case_refs) = info.type_alias_case_refs.get(name) {
                self.check_type_class_name_case(case_refs, fqn, span);
                self.check_type_internal_usage(case_refs, fqn, span);
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
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        if info.uses.len() < 2 {
            return;
        }

        let mut seen_props: HashMap<&str, (&str, &PropertySig)> = HashMap::new();
        let mut seen_methods: HashMap<&str, (&str, &FunctionSig)> = HashMap::new();
        for trait_name in &info.uses {
            let Some(trait_info) = self.registry.classes.get(trait_name) else {
                continue;
            };
            for (prop_name, sig) in &trait_info.properties {
                if info.resolved_trait_conflicts.contains(prop_name) {
                    continue;
                }
                match seen_props.get(prop_name.as_ref()) {
                    Some((first_trait, first_sig)) if first_sig.ty != sig.ty => {
                        self.report(fqn.to_string(), format!(
                                "{first_trait} and {trait_name} define the same property (${prop_name}) with incompatible types in the composition of {fqn}"
                            ), span, 0, "trait.propertyConflict");
                    }
                    Some(_) => {}
                    None => {
                        seen_props.insert(prop_name, (trait_name, sig));
                    }
                }
            }
            for (method_name, sig) in &trait_info.methods {
                if info.resolved_trait_conflicts.contains(method_name) {
                    continue;
                }
                match seen_methods.get(method_name.as_ref()) {
                    Some((first_trait, first_sig)) if !signatures_compatible(first_sig, sig) => {
                        self.report(fqn.to_string(), format!(
                                "{first_trait} and {trait_name} define the same method ({method_name}) with an incompatible signature in the composition of {fqn}"
                            ), span, 0, "trait.methodConflict");
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
    /// `EnumDecl`'s) rather than living inside that function itself -
    /// unrelated to methods.
    pub(crate) fn check_reserved_class_constant_name(&mut self, fqn: &str, consts: &[ClassConst]) {
        for c in consts {
            if c.name == "class".into() {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        "a class constant must not be called 'class'; it is reserved for class name fetching".to_string(),
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
    pub(crate) fn check_unknown_phpstan_tags_in_class(&mut self, fqn: &str, c: &ClassDecl) {
        self.check_unknown_phpstan_tags_on(fqn, c.doc_comment.as_deref(), c.name_span);
        for m in &c.methods {
            self.check_unknown_phpstan_tags_on(
                fqn,
                m.function.doc_comment.as_deref(),
                m.function.name_span,
            );
        }
        for p in &c.properties {
            self.check_unknown_phpstan_tags_on(fqn, p.doc_comment.as_deref(), p.span);
        }
        for cc in &c.consts {
            self.check_unknown_phpstan_tags_on(fqn, cc.doc_comment.as_deref(), cc.name_span);
        }
    }

    /// See `check_unknown_phpstan_tags_in_class`'s doc comment - the
    /// one-declaration-at-a-time worker both that function and
    /// `check_function`'s top-level-function call site share.
    pub(crate) fn check_unknown_phpstan_tags_on(
        &mut self,
        label: &str,
        doc_comment: Option<&str>,
        span: Span,
    ) {
        let Some(doc) = doc_comment else {
            return;
        };
        for tag in docblock::unknown_phpstan_tags(doc) {
            self.report(
                label.to_string(),
                format!("Unknown PHPDoc tag: @phpstan-{tag}"),
                span,
                2,
                "phpDoc.phpstanTag",
            );
        }
        self.report_invalid_doc_tag_values(label, doc, span);
        self.check_doc_see_tag_targets(label, doc, span);
    }

    /// `InvalidPhpDocTagValueRule` (`phpDoc.parseError`), level 2: a tag
    /// whose value this project's PHPDoc grammar couldn't parse. A
    /// type-alias tag names its alias instead of repeating the whole value,
    /// matching real PHPStan's own two message shapes.
    pub(crate) fn report_invalid_doc_tag_values(&mut self, label: &str, doc: &str, span: Span) {
        for invalid in docblock::parse(doc).invalid_tag_values {
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
            self.report(label.to_string(), message, span, 2, "phpDoc.parseError");
        }
    }

    /// Checks a `@see` value shaped like a bare class name or a
    /// `Class::member()`/`Class::$prop`/`Class::CONST` reference against
    /// the registry. Anything else (a URL, free-text prose, a bare
    /// function name) is left alone - no upstream grammar exists to tell
    /// those apart from a real reference with any confidence.
    fn check_doc_see_tag_targets(&mut self, label: &str, doc: &str, span: Span) {
        for target in &docblock::parse(doc).see {
            let (class_part, member) = match target.split_once("::") {
                Some((c, m)) => (c, Some(m)),
                None => (target.as_str(), None),
            };
            // A bare (no `::`) target only counts as a class reference
            // when it starts uppercase/backslash, PHP's own class-naming
            // convention - a lowercase bare word is far more likely the
            // first word of a free-text description (`@see the return
            // value of parseFoo`), which real PHPStan's own missing
            // `SeeTagValueNode` grammar can't tell apart from a real
            // reference either - erring towards not guessing here.
            if member.is_none()
                && !class_part.starts_with(|c: char| c.is_ascii_uppercase() || c == '\\')
            {
                continue;
            }
            let trimmed = class_part.trim_start_matches('\\');
            if trimmed.is_empty()
                || !trimmed
                    .split('\\')
                    .all(|seg| is_php_identifier(seg) && !seg.is_empty())
            {
                continue;
            }
            // `class_part`, not `trimmed`, is passed here - a leading `\`
            // means "already fully qualified" to `NameContext::resolve`
            // (`resolve_as`'s own `strip_prefix('\\')` check), matching
            // real PHP name-resolution rules. Resolving the already-
            // stripped form re-qualified an FQN `@see` target against the
            // current namespace a second time, corrupting it into a name
            // that could never resolve (a real, confirmed false positive:
            // `\Piwigo\Controller\Projection\PictureElement` from a
            // `Piwigo\Controller\Projection`-namespaced file).
            let resolved = self.name_ctx.resolve(class_part);
            if self.registry.lookup_class(&resolved).is_none() {
                self.report(
                    label.to_string(),
                    format!("PHPDoc tag @see contains unknown class {trimmed}"),
                    span,
                    2,
                    "phpDoc.seeTag",
                );
                continue;
            }
            let Some(member) = member else { continue };
            let found = if let Some(method) = member.strip_suffix("()") {
                self.registry.lookup_method(&resolved, method).is_some()
            } else if let Some(prop) = member.strip_prefix('$') {
                self.registry.lookup_property(&resolved, prop).is_some()
            } else if is_php_identifier(member) {
                self.registry.lookup_const(&resolved, member).is_some()
            } else {
                // An unrecognized member shape (stray punctuation, a
                // description that happened to contain "::") - don't
                // guess.
                true
            };
            if !found {
                self.report(
                    label.to_string(),
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
    /// resolved `ConstSig.ty`, so a value referencing another constant/
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
            && self.registry.dynamic_constant_names.is_empty()
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
        for c in consts {
            let phpdoc_ty = self
                .registry
                .lookup_const_with_owner(fqn, &c.name)
                .filter(|(owner, sig)| owner.as_ref() == fqn && sig.has_phpdoc_type)
                .map(|(_, sig)| sig.ty.clone());
            if phpdoc_ty.is_none() && c.type_hint.is_none() {
                let value_ty = self.infer_expr(&c.value, &Locals::default());
                self.check_configured_constant_value(
                    Some(fqn),
                    &c.name,
                    &value_ty,
                    "classConstant.value",
                    c.span,
                    fqn,
                );
                continue;
            }
            if let Some(phpdoc_ty) = &phpdoc_ty {
                let value_ty = self.infer_expr(&c.value, &Locals::default());
                if value_ty != Type::Mixed(false) {
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
            let value_ty = self.infer_expr(&c.value, &Locals::default());
            // `Type::Mixed(false)` here almost always means "this checker
            // couldn't resolve it" (an unrecognized global constant,
            // e.g. `DEBUG_BACKTRACE_IGNORE_ARGS`, a real PHP builtin,
            // reads as `Mixed`), not a genuinely `mixed`-typed value -
            // the same "don't guess wrong on Mixed" leniency
            // `check_method_overrides`'s parameter-narrowing check
            // applies for the identical reason.
            if value_ty == Type::Mixed(false) {
                continue;
            }
            if !self.registry.is_assignable(&value_ty, &declared_ty) {
                self.report(
                    fqn.to_string(),
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
        if self.registry.is_assignable(value_ty, phpdoc_ty) {
            return;
        }
        let name = &c.name;
        if has_native_type {
            self.report(
                fqn.to_string(),
                format!("constant {fqn}::{name} ({phpdoc_ty}) does not accept value {value_ty}"),
                c.span,
                2,
                "classConstant.value",
            );
            return;
        }
        let definitely = value_type_is_constant(value_ty)
            || crate::types::types_definitely_disjoint(value_ty, phpdoc_ty);
        let relation = if definitely {
            "is incompatible with"
        } else {
            "is not subtype of"
        };
        self.report(
            fqn.to_string(),
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
        if self.facts_only {
            return;
        }
        self.check_readonly_class_version(c.is_readonly, false, fqn, c.name_span);
        self.check_method_declaration_sanity(fqn, c.kind, c.is_abstract, &c.methods);
        self.check_property_declarations(
            fqn,
            fqn,
            c.kind,
            c.is_abstract,
            &c.properties,
            &c.methods,
        );
        self.check_class_constant_modifiers(fqn, c.kind, &c.consts);
        let own = crate::registry::ClassInfo::from_decl(c, &self.name_ctx);
        let registry_matches = self.registry.classes.get(fqn).is_some_and(|info| {
            info.extends == own.extends
                && info.implements == own.implements
                && info.uses == own.uses
        });
        if registry_matches {
            self.check_inheritance_target_kinds(fqn, false, fqn, c.span);
            self.check_magic_serialization_methods(fqn, fqn, fqn, c.name_span);
            self.check_constant_overrides(fqn, &c.consts);
            self.check_trait_constant_conflicts(fqn, &c.consts);
        }
    }

    /// `check_nested_class_declaration`'s enum counterpart.
    pub(crate) fn check_nested_enum_declaration(&mut self, fqn: &str, e: &EnumDecl) {
        if self.facts_only {
            return;
        }
        self.check_method_declaration_sanity(fqn, ClassKind::Enum, false, &e.methods);
        self.check_class_constant_modifiers(fqn, ClassKind::Enum, &e.consts);
        let own = crate::registry::ClassInfo::from_enum_decl(e, &self.name_ctx);
        let registry_matches = self
            .registry
            .classes
            .get(fqn)
            .is_some_and(|info| info.implements == own.implements && info.uses == own.uses);
        if registry_matches {
            self.check_inheritance_target_kinds(fqn, false, fqn, e.span);
            self.check_constant_overrides(fqn, &e.consts);
            self.check_trait_constant_conflicts(fqn, &e.consts);
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
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let interfaces = info.implements.clone();
        let parent = info.extends.first().cloned();
        let is_interface = info.kind == ClassKind::Interface;
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
                self.registry
                    .classes
                    .get(fqn)
                    .map(|i| i.extends.clone())
                    .unwrap_or_default()
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
                        .filter(|(_, sig)| sig.visibility != Visibility::Private)
                });
            let Some((proto_owner, proto)) = prototype else {
                continue;
            };
            let own = own.clone();
            let proto = proto.clone();
            let span = cc.name_span;
            if proto.is_final {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
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
            let narrowed = match (&proto.visibility, &own.visibility) {
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
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        message,
                        span,
                        0,
                        "classConstant.visibility",
                    )
                    .non_ignorable(),
                );
            }
            if let Some(proto_native) = &proto.native_ty {
                match &own.native_ty {
                    Some(own_native) if !self.registry.is_assignable(own_native, proto_native) => {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                fqn.to_string(),
                                format!(
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
                        self.report_builder(
                            DiagnosticBuilder::new(
                                fqn.to_string(),
                                format!(
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
            if proto.has_phpdoc_type
                && own.has_phpdoc_type
                && !self.registry.is_assignable(&own.ty, &proto.ty)
            {
                self.report(
                    fqn.to_string(),
                    format!(
                        "Type {} of constant {fqn}::{name} is not covariant with type {} of constant {proto_owner}::{name}.",
                        own.ty, proto.ty
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
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let immediate: Vec<std::sync::Arc<str>> = info.uses.clone();
        if immediate.is_empty() {
            return;
        }
        let mut all_traits: Vec<std::sync::Arc<str>> = Vec::new();
        let mut stack = immediate.clone();
        while let Some(t) = stack.pop() {
            if all_traits.contains(&t) {
                continue;
            }
            if let Some(trait_info) = self.registry.classes.get(&t) {
                stack.extend(trait_info.uses.iter().cloned());
            }
            all_traits.push(t);
        }
        let previous_class = self.current_class.replace(fqn.to_string());
        for cc in consts {
            let name = cc.name.as_ref();
            let own_native = cc
                .type_hint
                .as_deref()
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
                    let own_value = self.infer_expr(&cc.value, &Locals::default());
                    if own_value != Type::Mixed(false) && own_value != trait_const.value_ty {
                        self.report_builder(
                            DiagnosticBuilder::new(
                                fqn.to_string(),
                                format!(
                                    "Constant {fqn}::{name} with value {own_value} overriding constant {trait_fqn}::{name} with different value {} should have the same value.",
                                    trait_const.value_ty
                                ),
                                cc.name_span,
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
        trait_const: &crate::registry::ConstSig,
        own_native: Option<&Type>,
    ) {
        let name = cc.name.as_ref();
        let word = |v: &Visibility| match v {
            Visibility::Public => "public",
            Visibility::Protected => "protected",
            Visibility::Private => "private",
        };
        let mut errors: Vec<(String, &'static str)> = Vec::new();
        if cc.visibility != trait_const.visibility {
            let own = word(&cc.visibility);
            let theirs = word(&trait_const.visibility);
            let mut own_cap = own.to_string();
            own_cap[..1].make_ascii_uppercase();
            errors.push((
                format!(
                    "{own_cap} constant {fqn}::{name} overriding {theirs} constant {trait_fqn}::{name} should also be {theirs}."
                ),
                "classConstant.visibility",
            ));
        }
        if trait_const.is_final && !cc.is_final {
            errors.push((
                format!(
                    "Non-final constant {fqn}::{name} overriding final constant {trait_fqn}::{name} should also be final."
                ),
                "classConstant.nonFinal",
            ));
        } else if !trait_const.is_final && cc.is_final {
            errors.push((
                format!(
                    "Final constant {fqn}::{name} overriding non-final constant {trait_fqn}::{name} should also be non-final."
                ),
                "classConstant.final",
            ));
        }
        match (&trait_const.native_ty, own_native) {
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
            self.report_builder(
                DiagnosticBuilder::new(fqn.to_string(), message, cc.name_span, 0, identifier)
                    .non_ignorable(),
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
        label: &str,
        span: Span,
    ) {
        if self.registry.php_version.is_some_and(|v| {
            !crate::php_version::PhpFeature::SerializableRequiresMagicMethods.is_available(v)
        }) {
            return;
        }
        let Some(info) = self.registry.classes.get(registry_name) else {
            return;
        };
        if info.kind != ClassKind::Class
            || info.is_abstract
            || !self.registry.is_subclass_of(registry_name, "Serializable")
        {
            return;
        }
        for method in ["__serialize", "__unserialize"] {
            let declared = self
                .registry
                .lookup_method(registry_name, method)
                .is_some_and(|sig| !sig.is_magic);
            if declared {
                continue;
            }
            self.report_builder(
                DiagnosticBuilder::new(
                    label.to_string(),
                    format!(
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
        label: &str,
        span: Span,
    ) {
        if !is_readonly {
            return;
        }
        let Some(version) = self.registry.php_version else {
            return;
        };
        let (feature, message) = if is_anonymous {
            (
                crate::php_version::PhpFeature::ReadOnlyAnonymousClasses,
                "Anonymous readonly classes are supported only on PHP 8.3 and later.",
            )
        } else {
            (
                crate::php_version::PhpFeature::ReadOnlyClasses,
                "Readonly classes are supported only on PHP 8.2 and later.",
            )
        };
        if feature.is_available(version) {
            return;
        }
        self.report_builder(
            DiagnosticBuilder::new(
                label.to_string(),
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
        let version = self.registry.php_version;
        for c in consts {
            let constant = format!("{fqn}::{}", c.name);
            self.check_type_declaration(
                c.type_hint.as_deref(),
                crate::checker::type_declarations::TypePosition::ClassConstant(&constant),
                c.type_hint_span.unwrap_or(c.span),
                fqn,
            );
            if kind == ClassKind::Trait
                && let Some(version) = version
                && !crate::php_version::PhpFeature::ConstantsInTraits.is_available(version)
            {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        "Constant is declared inside a trait but is only supported on PHP 8.2 and later.".to_string(),
                        c.span,
                        0,
                        "classConstant.inTrait",
                    )
                    .non_ignorable(),
                );
            }
            if c.is_final
                && let Some(version) = version
                && !crate::php_version::PhpFeature::FinalClassConstants.is_available(version)
            {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
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
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        format!(
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
                && let Some(version) = version
                && !crate::php_version::PhpFeature::NativeTypedClassConstants.is_available(version)
            {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
                        "Class constants with native types are supported only on PHP 8.3 and later.".to_string(),
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
                    self.report_builder(
                        DiagnosticBuilder::new(
                            fqn.to_string(),
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
            if !m.is_abstract && kind != ClassKind::Interface && !m.function.has_body {
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
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
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
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
            if m.is_final
                && m.visibility == Visibility::Private
                && m.function.name != "__construct".into()
            {
                self.report(
                    fqn.to_string(),
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
                self.report_builder(
                    DiagnosticBuilder::new(
                        fqn.to_string(),
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
            if m.function.name.eq_ignore_ascii_case("__construct")
                && m.function.return_type.is_some()
            {
                self.report(
                    fqn.to_string(),
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
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        if info.kind != ClassKind::Class && info.kind != ClassKind::Enum {
            return;
        }
        if info.is_abstract {
            return;
        }
        let required = self.registry.required_abstract_methods(fqn);
        let missing: Vec<&std::sync::Arc<str>> = required
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
        self.report_builder(
            DiagnosticBuilder::new(
                fqn.to_string(),
                message,
                span,
                0,
                "class.abstractMethodNotImplemented",
            )
            .tip(
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
    pub(crate) fn check_parent_constructor_call(&mut self, fqn: &str, c: &ClassDecl) {
        if !self.registry.check_parent_constructor_call {
            return;
        }
        if c.kind == ClassKind::Trait || c.kind == ClassKind::Interface {
            return;
        }
        let Some(ctor) = c
            .methods
            .iter()
            .find(|m| m.function.name == "__construct".into())
        else {
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
        if ancestor_sig.visibility == Visibility::Private {
            return;
        }
        if self
            .registry
            .classes
            .get(&ancestor)
            .is_some_and(|info| info.abstract_methods.contains("__construct"))
        {
            return;
        }
        if body_calls_parent_construct(&ctor.function.body) {
            return;
        }
        self.report(
            fqn.to_string(),
            format!("{fqn}::__construct() does not call parent constructor from {ancestor}."),
            ctor.function.span,
            4,
            "constructor.missingParentCall",
        );
    }

    /// Suggests constructor property promotion for a trivially-
    /// forwarding constructor: every parameter, in order, has exactly
    /// one matching `$this->name = $name;` statement and nothing else in
    /// the body, none of the parameters are already promoted, and each
    /// target property is a plain (non-static, non-hooked) declaration
    /// on this same class with the identical native type. Requires a
    /// configured PHP version >= 8.0.
    pub(crate) fn check_promoted_property_candidate(&mut self, fqn: &str, c: &ClassDecl) {
        if !self.registry.check_promoted_property_suggestions {
            return;
        }
        if !self
            .registry
            .php_version
            .is_some_and(|v| crate::php_version::PhpFeature::PromotedProperties.is_available(v))
        {
            return;
        }
        let Some(ctor) = c
            .methods
            .iter()
            .find(|m| m.function.name == "__construct".into())
        else {
            return;
        };
        let params = &ctor.function.params;
        if params.is_empty()
            || params
                .iter()
                .any(|p| p.by_ref || p.variadic || p.promoted_visibility.is_some())
        {
            return;
        }
        let body = &ctor.function.body;
        if body.len() != params.len() {
            return;
        }
        for (param, stmt) in params.iter().zip(body.iter()) {
            let StmtKind::Expression(e) = &stmt.kind else {
                return;
            };
            let ExprKind::Assign(lhs, rhs) = &unwrap_grouping(e).kind else {
                return;
            };
            let ExprKind::PropertyAccess(recv, prop_name, false) = &unwrap_grouping(lhs).kind
            else {
                return;
            };
            if !matches!(&unwrap_grouping(recv).kind, ExprKind::Variable(v) if **v == *"this") {
                return;
            }
            if prop_name != &param.name {
                return;
            }
            if !matches!(&unwrap_grouping(rhs).kind, ExprKind::Variable(v) if v == &param.name) {
                return;
            }
            let Some(prop) = c.properties.iter().find(|p| &p.name == prop_name) else {
                return;
            };
            if prop.is_static || prop.hooks.is_some() || prop.type_hint != param.type_hint {
                return;
            }
        }
        self.report_builder(
            DiagnosticBuilder::new(
                fqn.to_string(),
                format!(
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
        let Some(info) = self.registry.classes.get(fqn) else {
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
        if info.kind == ClassKind::Trait {
            return;
        }
        // Real PHP: a checked class that is itself an interface is
        // exempt from require-extends (an interface can't `extends` a
        // concrete class the way a class does), mirroring
        // `RequireExtendsRule`'s top-level guard. Interfaces can't `use`
        // traits at all, so require-implements never has anything to
        // check for one either way.
        let is_interface = info.kind == ClassKind::Interface;
        for ancestor_fqn in self.registry.all_ancestor_names(fqn) {
            let Some(ancestor_info) = self.registry.classes.get(&ancestor_fqn) else {
                continue;
            };
            if !is_interface {
                for required in ancestor_info.require_extends.clone() {
                    if self.registry.is_subclass_of(fqn, &required) {
                        continue;
                    }
                    let (kind_word, relation_word) = if ancestor_info.kind == ClassKind::Trait {
                        ("Trait", "using")
                    } else {
                        ("Interface", "implementing")
                    };
                    self.report(
                        fqn.to_string(),
                        format!(
                            "{kind_word} {ancestor_fqn} requires {relation_word} class to extend {required}, but {fqn} does not."
                        ),
                        span,
                        0,
                        "class.missingExtends",
                    );
                }
            }
            for required in ancestor_info.require_implements.clone() {
                if self.registry.is_subclass_of(fqn, &required) {
                    continue;
                }
                self.report(
                    fqn.to_string(),
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
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        // Same `InClassNode`-only registration as `check_require_tags`
        // above - a trait is never itself the checked class. Real
        // PHPStan's `AllowedSubTypesRule` also never consults `uses` at
        // all (only `extends`/immediate `implements`), so a trait being
        // sealed is never actually enforced there either way.
        if info.kind == ClassKind::Trait {
            return;
        }
        let kind = info.kind;
        for parent_fqn in info.extends.iter().chain(info.implements.iter()) {
            let Some(parent_info) = self.registry.classes.get(parent_fqn) else {
                continue;
            };
            if parent_info.sealed.is_empty() || parent_info.sealed.iter().any(|s| s.as_ref() == fqn)
            {
                continue;
            }
            self.report(
                fqn.to_string(),
                format!("Type {fqn} is not allowed to be a subtype of {parent_fqn}."),
                span,
                0,
                &format!("{}.disallowedSubtype", class_kind_identifier(&kind)),
            );
        }
    }
}

/// Whether `stmts` contains a `parent::__construct(...)` call as a
/// direct statement anywhere in the body, recursing into nested blocks
/// (`if`/`else`/`try`/`switch`/loops) the same way `any_stmt_child`'s
/// other consumers do - real PHP call resolution is case-insensitive,
/// so the class/method names are compared that way too.
fn body_calls_parent_construct(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_calls_parent_construct)
}

fn stmt_calls_parent_construct(stmt: &Stmt) -> bool {
    if let StmtKind::Expression(e) = &stmt.kind
        && let ExprKind::StaticCall(class, method, _) = &unwrap_grouping(e).kind
        && class
            .as_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("parent"))
        && method.eq_ignore_ascii_case("__construct")
    {
        return true;
    }
    any_stmt_child(stmt, &mut stmt_calls_parent_construct)
}

fn is_php_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
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
pub(crate) fn signatures_compatible(a: &FunctionSig, b: &FunctionSig) -> bool {
    if a.params.len() != b.params.len() {
        return false;
    }
    if a.return_type != b.return_type {
        return false;
    }
    a.params
        .iter()
        .zip(&b.params)
        .all(|(pa, pb)| pa.ty == pb.ty)
}

/// A value type fully known down to its value (a literal, or a shape of
/// them), whose supertype relation to another type is never "maybe".
fn value_type_is_constant(ty: &Type) -> bool {
    match ty {
        Type::IntLiteral(_)
        | Type::StringLiteral(_)
        | Type::FloatLiteral(_)
        | Type::True
        | Type::False
        | Type::Null
        | Type::EnumCase(..) => true,
        Type::ArrayShape(fields) => fields.iter().all(|(_, t, _)| value_type_is_constant(t)),
        _ => false,
    }
}
