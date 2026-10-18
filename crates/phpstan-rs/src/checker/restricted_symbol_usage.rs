//! Checker-side consumption of a project's own configured
//! `restricted_symbols` list (`crate::restricted_symbols::Restricted
//! Symbols`) - reuses the same call sites as `internal_usage.rs`'s
//! `@internal` checks, one function per event, each opening with a fast
//! `is_empty()` bail.

use crate::checker::internal_usage::class_type_word;
use crate::checker::internal_usage::collect_referenced_class_names;
use crate::checker::{Checker, Locals};
use crate::ignore_errors::glob_matches;
use crate::restricted_symbols::{
    ParamCondition, ParamFlagsCondition, ParamRef, ParamScalarValue, RestrictedSymbolEntry,
};
use diagnostics::DiagnosticBuilder;
use syntax::ast::TypeHint;
use syntax::ast::{Arg, ClassKind, Expr, FunctionDecl, Param};
use syntax::span::Span;
use types::Name;
use types::Type;

/// Where a `Class`-kind ban's own referenced-class check fires -
/// `allowInParamTypes`/`allowInReturnType` and their `allowExceptIn*`
/// inverses (`Class`-kind only) need to distinguish exactly these two
/// positions from every other detection surface (`new`/`instanceof`/
/// `catch`/`extends`/`implements`, none of which either flag ever
/// touches - confirmed against the real docs' own worked examples).
/// `use`-import context is deliberately not a variant here: real
/// PHPStan's own docs confirm a `use` import is never affected by
/// either flag at all (`allowInUse` is its own, wholly separate
/// mechanism - see `check_restricted_class_use_import`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClassUsageLocation {
    ParamType,
    ReturnType,
    Other,
}

/// `allowInParamTypes`/`allowExceptInParamTypes` (`disallowInParamTypes`)
/// and their `*ReturnType` siblings. Unlike every other allow mechanism
/// in this module (purely additive on top of "disallowed everywhere"),
/// `allow_except_in_*` inverts the *entry's own default* entirely:
/// "disallow a class only when it appears as a parameter type hint,
/// allowing it everywhere else" (confirmed against the real docs' own
/// worked example - a `use` of the same class, `new`-ing it, and using
/// it as a *return* type all become allowed once `disallowInParamTypes`
/// is set; only the parameter-type-hint position stays reported).
/// Evaluated before `report_restricted`'s own attribute/path checks,
/// which still apply on top once this returns `false` - matching how
/// every allow directive here combines ("the item is allowed as soon as
/// one of the allow directives matches").
fn is_allowed_by_position(entry: &RestrictedSymbolEntry, location: ClassUsageLocation) -> bool {
    if entry.allow_in_param_types && location == ClassUsageLocation::ParamType {
        return true;
    }
    if entry.allow_in_return_type && location == ClassUsageLocation::ReturnType {
        return true;
    }
    if entry.allow_except_in_param_types || entry.allow_except_in_return_type {
        let disallowed_here = (entry.allow_except_in_param_types
            && location == ClassUsageLocation::ParamType)
            || (entry.allow_except_in_return_type && location == ClassUsageLocation::ReturnType);
        return !disallowed_here;
    }
    false
}

/// Resolves one `{position, name}` pair against the real call-site
/// argument list - matches the real package's own "positional or
/// named" resolution directly (confirmed against its docs): a named
/// condition looks for a literally-named argument; a positional one
/// (1-indexed) counts the Nth non-named, non-spread argument. Named
/// resolution is tried first, matching the real docs' own combined
/// `position`+`name` worked example (`json_decode()`'s `flags`
/// parameter, matched whether called positionally or by name).
/// Independent of the callee's own declared parameter names - unlike
/// `checker::calls::resolve_call_args`, which needs a `ParameterReflection` list
/// to resolve a named argument, a named condition here needs nothing
/// more than `arg.name == configured_name`. A spread/unpack argument
/// makes every subsequent positional slot statically unknown, so
/// positional resolution stops (returns `None`) once one is seen -
/// this project's own conservative posture, matching how every other
/// spread-aware check here already treats an unpack as "can't know."
fn resolve_arg<'a>(args: &'a [Arg], position: Option<u32>, name: Option<&str>) -> Option<&'a Expr> {
    if let Some(name) = name
        && let Some(found) = args
            .iter()
            .find(|a| !a.spread && a.name.as_deref() == Some(name))
    {
        return Some(&found.value);
    }
    let pos = position?;
    let idx = pos.checked_sub(1)?;
    let mut count = 0u32;
    for a in args {
        if a.spread {
            return None;
        }
        if a.name.is_some() {
            continue;
        }
        if count == idx {
            return Some(&a.value);
        }
        count += 1;
    }
    None
}

/// `value:`'s own scalar-literal matching - the argument's own already-
/// inferred `Type` must be the exact literal variant configured (real
/// docs: "only scalar values are supported"). `case_insensitive` only
/// affects a `Str` comparison (real `allowExceptCaseInsensitiveParams`);
/// the docs confirm this doesn't extend to `typeString`/`classPattern`
/// matching, which never reach this function at all.
fn scalar_value_matches(ty: &Type, value: &ParamScalarValue, case_insensitive: bool) -> bool {
    match value {
        ParamScalarValue::Bool(v) => ty.bool_literal_value() == Some(*v),
        ParamScalarValue::Int(v) => ty.int_literal_value() == Some(*v),
        ParamScalarValue::Float(v) => *ty == Type::float_literal(*v),
        ParamScalarValue::Str(v) => ty.string_literal_value().is_some_and(|s| {
            if case_insensitive {
                s.as_bytes().eq_ignore_ascii_case(v.as_bytes())
            } else {
                s == v
            }
        }),
    }
}

impl<'a> Checker<'a> {
    /// One `ParamFlagsCondition`'s own match - the argument's own
    /// resolved integer value must have every bit of `value` (a single
    /// flag) set, or every bit of *any one* member of `type_string`'s
    /// own parsed union set (real docs: "if you want to (dis)allow a
    /// parameter when either the flag `1` or `2` is set, use
    /// `typeString: 1 | 2`" - this `|` is a PHPDoc union, not PHP's own
    /// bitwise-or). Every integer the argument's type allows must match;
    /// an argument that isn't there, or whose type isn't a set of integer
    /// values, never matches.
    fn param_flags_condition_matches(
        &self,
        cond: &ParamFlagsCondition,
        args: &[Arg],
        locals: &Locals,
    ) -> bool {
        let Some(expr) = resolve_arg(args, cond.position, cond.name.as_deref()) else {
            return false;
        };
        let actual_values = self.infer_expr(expr, locals).constant_ints();
        let matches = |actual: i64| {
            if let Some(type_string) = &cond.type_string {
                return types::parse_type_string(type_string)
                    .constant_ints()
                    .iter()
                    .any(|flag| (actual & flag) == *flag);
            }
            cond.value.is_some_and(|value| (actual & value) == value)
        };
        !actual_values.is_empty() && actual_values.into_iter().all(matches)
    }

    /// `all_conditions_match`'s own `ParamFlagsCondition` sibling.
    fn all_flags_conditions_match(
        &self,
        conditions: &[ParamFlagsCondition],
        args: &[Arg],
        locals: &Locals,
    ) -> bool {
        !conditions.is_empty()
            && conditions
                .iter()
                .all(|c| self.param_flags_condition_matches(c, args, locals))
    }

    /// One `ParamCondition`'s own match against the real call site -
    /// `class_pattern` takes precedence over `type_string`, which takes
    /// precedence over `value` (confirmed against the real docs: "If
    /// both `classPattern` and `typeString` are specified, `classPattern`
    /// takes precedence"; "If both `typeString` and `value` directives
    /// are specified, the `value` directive is ignored"). An argument
    /// that can't be resolved at all (missing, or past an unresolvable
    /// spread) never matches, regardless of which directive is
    /// configured.
    fn param_condition_matches(
        &self,
        cond: &ParamCondition,
        args: &[Arg],
        locals: &Locals,
        case_insensitive: bool,
    ) -> bool {
        let Some(expr) = resolve_arg(args, cond.position, cond.name.as_deref()) else {
            return false;
        };
        let ty = self.infer_expr(expr, locals);
        if let Some(pattern) = &cond.class_pattern {
            return ty.as_object().is_some_and(|(name, _)| {
                glob_matches(&pattern.to_lowercase(), &name.to_lowercase())
            });
        }
        if let Some(type_string) = &cond.type_string {
            let configured = types::parse_type_string(type_string);
            return self.accepts(&configured, &ty).yes();
        }
        if let Some(value) = &cond.value {
            return scalar_value_matches(&ty, value, case_insensitive);
        }
        false
    }

    /// `*AnyValue` siblings' own per-parameter check - present at all,
    /// regardless of value.
    fn param_ref_present(&self, r: &ParamRef, args: &[Arg]) -> bool {
        resolve_arg(args, r.position, r.name.as_deref()).is_some()
    }

    /// Every real `*Params*` directive is a *list* of conditions
    /// evaluated together as one AND-group (confirmed against the real
    /// docs' own `log()` worked example: "calls are allowed when called
    /// with all parameters listed"), not "any one matches." An empty
    /// list never matches anything (there's nothing to be an unconfigured
    /// directive's own vacuous truth here - the caller's own `is_empty()`
    /// gate is what decides whether this directive applies at all).
    fn all_conditions_match(
        &self,
        conditions: &[ParamCondition],
        args: &[Arg],
        locals: &Locals,
        case_insensitive: bool,
    ) -> bool {
        !conditions.is_empty()
            && conditions
                .iter()
                .all(|c| self.param_condition_matches(c, args, locals, case_insensitive))
    }

    /// `all_conditions_match`'s presence-only sibling.
    fn all_refs_present(&self, refs: &[ParamRef], args: &[Arg]) -> bool {
        !refs.is_empty() && refs.iter().all(|r| self.param_ref_present(r, args))
    }

    /// `allowParamsAnywhere`/`allowParamsAnywhereAnyValue` (purely
    /// additive, on top of "disallowed everywhere") and
    /// `allowExceptParamsAnywhere`/`allowExceptParamsAnyValue`/
    /// `allowExceptCaseInsensitiveParams` (default-inverting, the same
    /// shape `is_allowed_by_position`/`is_allowed_by_calling_context`
    /// already established) - all location-independent, unlike the
    /// `*InAllowed` family's own zone-combined siblings (not yet built).
    /// A first-class-callable call site (no real arguments) naturally
    /// falls out of this correctly with zero special-casing: every
    /// condition/ref fails to resolve against an empty `args`, so the
    /// additive family never grants an allowance (matching the real
    /// docs: "always reported") and the inverting family never finds a
    /// disallow-match either (matching the real docs: "never reported").
    fn is_allowed_by_params_anywhere_context(
        &self,
        entry: &RestrictedSymbolEntry,
        args: &[Arg],
        locals: &Locals,
    ) -> bool {
        let has_allow = !entry.allow_params_anywhere.is_empty()
            || !entry.allow_params_anywhere_any_value.is_empty()
            || !entry.allow_param_flags_anywhere.is_empty();
        if has_allow
            && (self.all_conditions_match(&entry.allow_params_anywhere, args, locals, false)
                || self.all_refs_present(&entry.allow_params_anywhere_any_value, args)
                || self.all_flags_conditions_match(&entry.allow_param_flags_anywhere, args, locals))
        {
            return true;
        }
        let has_disallow_gate = !entry.allow_except_params_anywhere.is_empty()
            || !entry.allow_except_params_any_value.is_empty()
            || !entry.allow_except_case_insensitive_params.is_empty()
            || !entry.allow_except_param_flags.is_empty();
        if has_disallow_gate {
            let disallowed_here =
                self.all_conditions_match(&entry.allow_except_params_anywhere, args, locals, false)
                    || self.all_refs_present(&entry.allow_except_params_any_value, args)
                    || self.all_conditions_match(
                        &entry.allow_except_case_insensitive_params,
                        args,
                        locals,
                        true,
                    )
                    || self.all_flags_conditions_match(
                        &entry.allow_except_param_flags,
                        args,
                        locals,
                    );
            return !disallowed_here;
        }
        false
    }

    /// The single unified verdict `allow_params_in_allowed`/
    /// `allow_except_params_in_allowed` contribute, independent of
    /// which of the entry's own zone directives (`allow_in_instance_of`/
    /// `allow_in_methods`/`allow_in`/their `allow_except_in_*` siblings)
    /// they're paired with: `true` when the param condition favors
    /// allowing (the additive family matched, or the inverting family
    /// did *not* match), `false` when it favors reporting. `None` when
    /// neither field is configured on this entry at all - the caller
    /// then knows this whole mechanism doesn't apply here.
    fn params_in_allowed_gate(
        &self,
        entry: &RestrictedSymbolEntry,
        args: &[Arg],
        locals: &Locals,
    ) -> Option<bool> {
        if !entry.allow_except_params_in_allowed.is_empty()
            || !entry.allow_except_param_flags_in_allowed.is_empty()
        {
            let disallow_matched = self.all_conditions_match(
                &entry.allow_except_params_in_allowed,
                args,
                locals,
                false,
            ) || self.all_flags_conditions_match(
                &entry.allow_except_param_flags_in_allowed,
                args,
                locals,
            );
            return Some(!disallow_matched);
        }
        if !entry.allow_params_in_allowed.is_empty()
            || !entry.allow_params_in_allowed_any_value.is_empty()
            || !entry.allow_param_flags_in_allowed.is_empty()
        {
            return Some(
                self.all_conditions_match(&entry.allow_params_in_allowed, args, locals, false)
                    || self.all_refs_present(&entry.allow_params_in_allowed_any_value, args)
                    || self.all_flags_conditions_match(
                        &entry.allow_param_flags_in_allowed,
                        args,
                        locals,
                    ),
            );
        }
        None
    }

    /// Combines `params_in_allowed_gate`'s own verdict with whichever
    /// of `allow_in_instance_of`/`allow_in_methods`/their `allow_
    /// except_in_*` siblings this entry also configures (real docs:
    /// "the parameter condition is applied to the location matched by
    /// the accompanying allow directive"). An `allow_in_*`-style zone
    /// is purely additive - not matching it means "not allowed via this
    /// mechanism" regardless of `gate`; an `allow_except_in_*`-style
    /// zone inverts the default - not matching it means "allowed,
    /// params irrelevant" (matching the real docs' own "outside the
    /// zone, the zone rule alone determines" FCC-section wording,
    /// generalized to the ordinary case too). Returns `None` when
    /// neither zone type is configured on this entry - the path-scoped
    /// `allow_in`/`allow_except_in` zone is handled separately (see
    /// `report_restricted_with_params_gate`'s own doc comment), since
    /// it has no call-site context available inline at all.
    fn is_allowed_by_params_in_allowed_context(
        &self,
        entry: &RestrictedSymbolEntry,
        gate: Option<bool>,
    ) -> Option<bool> {
        let gate = gate?;
        if !entry.allow_in_instance_of.is_empty() || !entry.allow_in_methods.is_empty() {
            let zone_matched = self.matches_instance_of_patterns(&entry.allow_in_instance_of)
                || self.matches_calling_context_patterns(&entry.allow_in_methods);
            return Some(zone_matched && gate);
        }
        if !entry.allow_except_in_instance_of.is_empty()
            || !entry.allow_except_in_methods.is_empty()
        {
            let zone_matched = self
                .matches_instance_of_patterns(&entry.allow_except_in_instance_of)
                || self.matches_calling_context_patterns(&entry.allow_except_in_methods);
            if !zone_matched {
                return Some(true);
            }
            return Some(gate);
        }
        None
    }

    /// Every `check_restricted_*` report call routes through here: an
    /// entry allowed in the current context (attributes, calling method,
    /// instance-of, or the file's path zone) isn't reported.
    fn report_restricted(
        &mut self,
        entry: &RestrictedSymbolEntry,
        message: String,
        span: Span,
        identifier: &'static str,
    ) {
        self.report_restricted_with_params_gate(entry, message, span, identifier, None);
    }

    /// `report_restricted`, with `params_gate` - the call site's
    /// `Self::params_in_allowed_gate` verdict, which only
    /// `Function`/`Method`/`StaticMethod` call sites have - narrowing the
    /// path zone the way `is_allowed_by_params_in_allowed_context`
    /// narrows the instance-of/methods zones.
    fn report_restricted_with_params_gate(
        &mut self,
        entry: &RestrictedSymbolEntry,
        message: String,
        span: Span,
        identifier: &'static str,
        params_gate: Option<bool>,
    ) {
        if self.is_allowed_by_attribute_context(entry)
            || self.is_allowed_by_calling_context(entry)
            || self.is_allowed_by_instance_of_context(entry)
            || self.project_relative_path().is_some_and(|path| {
                entry.allowed_at_path(&path, self.settings.path_case, params_gate)
            })
        {
            return;
        }
        self.report_builder(DiagnosticBuilder::new(message, span, 0, identifier));
    }

    /// The checked file's path as path-scoped config spells it: relative to
    /// the project's working directory when it lies below it, as given
    /// otherwise, with `/` between its names on every platform
    /// (`diagnostics::report_path`). `None` when no file is being checked.
    fn project_relative_path(&self) -> Option<String> {
        let shown = self.current_file_shown.as_deref()?;
        Some(diagnostics::report_path(shown).into_owned())
    }

    /// `allow_in_class_with_attributes`/`allow_except_in_class_with_
    /// attributes` and their method-level siblings, evaluated in the
    /// same "first genuine allow wins" order the real package's own
    /// broader DSL uses (see `restricted_symbols.rs`'s own module doc
    /// comment) - class-level checked before method-level, `allow_in_*`
    /// before its own `allow_except_in_*` inverse. Attribute-name
    /// matching is case-insensitive (`glob_matches` itself isn't, so both
    /// sides are lowercased first), matching PHP's own class-name
    /// case-insensitivity - an attribute name is, after all, a class
    /// name.
    fn is_allowed_by_attribute_context(&self, entry: &RestrictedSymbolEntry) -> bool {
        let matches_any = |patterns: &[String], attrs: &[Name]| {
            patterns.iter().any(|p| {
                let p = p.to_lowercase();
                attrs.iter().any(|a| glob_matches(&p, &a.to_lowercase()))
            })
        };
        if matches_any(
            &entry.allow_in_class_with_attributes,
            &self.current_class_attrs,
        ) {
            return true;
        }
        if !entry.allow_except_in_class_with_attributes.is_empty()
            && !matches_any(
                &entry.allow_except_in_class_with_attributes,
                &self.current_class_attrs,
            )
        {
            return true;
        }
        if matches_any(
            &entry.allow_in_methods_with_attributes,
            &self.current_method_attrs,
        ) {
            return true;
        }
        if !entry.allow_except_in_methods_with_attributes.is_empty()
            && !matches_any(
                &entry.allow_except_in_methods_with_attributes,
                &self.current_method_attrs,
            )
        {
            return true;
        }
        if !entry.allow_in_class_with_method_attributes.is_empty()
            || !entry
                .allow_except_in_class_with_method_attributes
                .is_empty()
        {
            let any_method_attrs = self.current_class_any_method_attrs();
            if matches_any(
                &entry.allow_in_class_with_method_attributes,
                &any_method_attrs,
            ) {
                return true;
            }
            if !entry
                .allow_except_in_class_with_method_attributes
                .is_empty()
                && !matches_any(
                    &entry.allow_except_in_class_with_method_attributes,
                    &any_method_attrs,
                )
            {
                return true;
            }
        }
        false
    }

    /// `allow_in_class_with_method_attributes`'s own data source - every
    /// attribute on *any* method of the current class (the enclosing
    /// method's own attrs included, via `ClassReflection::methods`, never the
    /// class's own attrs) - distinct from `current_method_attrs`, which
    /// only ever holds the single currently-executing method's own.
    /// Collected fresh per call rather than threaded as another
    /// `Checker` field: only consulted when an entry actually configures
    /// this one specific allow mechanism (the `is_empty()` guard just
    /// above), unlike `current_class_attrs`/`current_method_attrs`,
    /// which every restricted-symbol report reads.
    fn current_class_any_method_attrs(&self) -> Vec<Name> {
        let Some(class) = &self.current_class else {
            return Vec::new();
        };
        let Some(info) = self.registry.classes.get(class.as_str()) else {
            return Vec::new();
        };
        info.methods()
            .values()
            .flat_map(|sig| sig.attributes().iter().map(|a| a.name_arc().clone()))
            .collect()
    }

    /// `allow_in_methods`/`allow_except_in_methods` - matches the
    /// *calling* function/method's own name, unlike `allow_in_methods_
    /// with_attributes` (attribute-context, a wholly separate mechanism
    /// already built above). `allow_in_methods` is purely additive
    /// (allowed on top of the ordinary "disallowed everywhere" default);
    /// `allow_except_in_methods` inverts the entry's own default
    /// entirely, the same shape `is_allowed_by_position` already
    /// established for `allow_except_in_param_types`.
    fn is_allowed_by_calling_context(&self, entry: &RestrictedSymbolEntry) -> bool {
        if entry.allow_in_methods.is_empty() && entry.allow_except_in_methods.is_empty() {
            return false;
        }
        if entry.has_params_in_allowed_directive() {
            // Evaluated together with the param condition instead (see
            // `is_allowed_by_params_in_allowed_context`, which reads
            // this same zone via `matches_calling_context_patterns`) -
            // this unconditional check would otherwise wrongly grant
            // the allowance without ever consulting the param
            // condition the entry also configures.
            return false;
        }
        if self.matches_calling_context_patterns(&entry.allow_in_methods) {
            return true;
        }
        if !entry.allow_except_in_methods.is_empty()
            && !self.matches_calling_context_patterns(&entry.allow_except_in_methods)
        {
            return true;
        }
        false
    }

    /// The raw pattern match `is_allowed_by_calling_context` and
    /// `is_allowed_by_params_in_allowed_context` both need - factored
    /// out so the latter can combine it with a param condition instead
    /// of the former's own unconditional "matched, therefore allowed"
    /// shortcut.
    fn matches_calling_context_patterns(&self, patterns: &[String]) -> bool {
        if patterns.is_empty() {
            return false;
        }
        let current = self.current_calling_context_label().to_lowercase();
        patterns
            .iter()
            .any(|p| glob_matches(&p.to_lowercase(), &current))
    }

    /// The calling function/method's own `Class::method()`/`function()`-
    /// shaped label `is_allowed_by_calling_context` matches patterns
    /// against - `current_method` alone (bare name, no parens) for a
    /// top-level function, `current_class`+`current_method` for a
    /// method. Empty when neither is set (top-level file scope), which
    /// correctly never matches any real pattern.
    fn current_calling_context_label(&self) -> String {
        match (&self.current_class, &self.current_method) {
            (Some(class), Some(method)) => format!("{class}::{method}()"),
            (None, Some(method)) => format!("{method}()"),
            _ => String::new(),
        }
    }

    /// `allow_in_instance_of`/`allow_except_in_instance_of` - matches
    /// the current class's own name *and* every transitive `extends`/
    /// `implements` ancestor's name (real docs: "matched against the
    /// current class name as well as all its parent class and
    /// interface names transitively"). Reuses `Registry::ancestor_names`
    /// directly - the exact precomputed closure `is_subclass_of` itself
    /// already reads - rather than a fresh hierarchy walk.
    /// `allow_in_instance_of` is purely additive; `allow_except_in_
    /// instance_of` inverts the entry's own default, the same shape
    /// `is_allowed_by_calling_context` already established.
    fn is_allowed_by_instance_of_context(&self, entry: &RestrictedSymbolEntry) -> bool {
        if entry.allow_in_instance_of.is_empty() && entry.allow_except_in_instance_of.is_empty() {
            return false;
        }
        if entry.has_params_in_allowed_directive() {
            // Same reasoning as `is_allowed_by_calling_context`'s own
            // identical guard just above.
            return false;
        }
        if self.matches_instance_of_patterns(&entry.allow_in_instance_of) {
            return true;
        }
        if !entry.allow_except_in_instance_of.is_empty()
            && !self.matches_instance_of_patterns(&entry.allow_except_in_instance_of)
        {
            return true;
        }
        false
    }

    /// The raw pattern match `is_allowed_by_instance_of_context` and
    /// `is_allowed_by_params_in_allowed_context` both need - see
    /// `matches_calling_context_patterns`'s own identical rationale.
    fn matches_instance_of_patterns(&self, patterns: &[String]) -> bool {
        if patterns.is_empty() {
            return false;
        }
        let Some(class) = &self.current_class else {
            return false;
        };
        patterns.iter().any(|p| {
            let p = p.to_lowercase();
            glob_matches(&p, &class.to_lowercase())
                || self
                    .registry
                    .ancestor_names
                    .get(class.as_str())
                    .is_some_and(|ancestors| {
                        ancestors
                            .iter()
                            .any(|a| glob_matches(&p, &a.to_lowercase()))
                    })
        })
    }

    pub(crate) fn check_restricted_function_call(
        &mut self,
        fqn: &str,
        args: &[Arg],
        locals: &Locals,
        span: Span,
    ) {
        if self.settings.restricted_symbols.is_empty() {
            return;
        }
        let bare = fqn.rsplit('\\').next().unwrap_or(fqn);
        let Some(entry) = self.settings.restricted_symbols.function(bare) else {
            return;
        };
        if self.is_allowed_by_params_anywhere_context(entry, args, locals) {
            return;
        }
        let gate = self.params_in_allowed_gate(entry, args, locals);
        if let Some(allowed) = self.is_allowed_by_params_in_allowed_context(entry, gate)
            && allowed
        {
            return;
        }
        let entry = entry.clone();
        self.report_restricted_with_params_gate(
            &entry,
            format!("Call to restricted function {bare}(): {}", entry.message),
            span,
            "restrictedSymbol.function",
            gate,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_restricted_method_call(
        &mut self,
        owner: &str,
        method: &str,
        is_static: bool,
        args: &[Arg],
        locals: &Locals,
        span: Span,
    ) {
        if self.settings.restricted_symbols.is_empty() {
            return;
        }
        let entry = if is_static {
            self.settings
                .restricted_symbols
                .static_method(owner, method)
        } else {
            self.settings.restricted_symbols.method(owner, method)
        };
        let Some(entry) = entry else { return };
        if self.is_allowed_by_params_anywhere_context(entry, args, locals) {
            return;
        }
        let gate = self.params_in_allowed_gate(entry, args, locals);
        if let Some(allowed) = self.is_allowed_by_params_in_allowed_context(entry, gate)
            && allowed
        {
            return;
        }
        let entry = entry.clone();
        let static_word = if is_static { "static " } else { "" };
        let identifier = if is_static {
            "restrictedSymbol.staticMethod"
        } else {
            "restrictedSymbol.method"
        };
        self.report_restricted_with_params_gate(
            &entry,
            format!(
                "Call to restricted {static_word}method {owner}::{method}(): {}",
                entry.message
            ),
            span,
            identifier,
            gate,
        );
    }

    pub(crate) fn check_restricted_property_access(
        &mut self,
        owner: &str,
        prop: &str,
        is_static: bool,
        span: Span,
    ) {
        if self.settings.restricted_symbols.is_empty() {
            return;
        }
        let entry = if is_static {
            self.settings
                .restricted_symbols
                .static_property(owner, prop)
        } else {
            self.settings.restricted_symbols.property(owner, prop)
        };
        let Some(entry) = entry else { return };
        let entry = entry.clone();
        let static_word = if is_static { "static " } else { "" };
        let identifier = if is_static {
            "restrictedSymbol.staticProperty"
        } else {
            "restrictedSymbol.property"
        };
        self.report_restricted(
            &entry,
            format!(
                "Access to restricted {static_word}property {owner}::${prop}: {}",
                entry.message
            ),
            span,
            identifier,
        );
    }

    pub(crate) fn check_restricted_class_constant_access(
        &mut self,
        owner: &str,
        const_name: &str,
        span: Span,
    ) {
        if self.settings.restricted_symbols.is_empty() {
            return;
        }
        let Some(entry) = self
            .settings
            .restricted_symbols
            .class_constant(owner, const_name)
        else {
            return;
        };
        let entry = entry.clone();
        self.report_restricted(
            &entry,
            format!(
                "Access to restricted constant {owner}::{const_name}: {}",
                entry.message
            ),
            span,
            "restrictedSymbol.classConstant",
        );
    }

    /// Shared by every native class-name-usage location a `Class`-kind
    /// ban covers (instantiation/`instanceof`/`catch`/`extends`/
    /// `implements`/param-and-return-type-hints). `location` is only
    /// ever meaningfully distinguished by `is_allowed_by_position` -
    /// every non-type-hint call site passes `ClassUsageLocation::Other`.
    pub(crate) fn check_restricted_class_name_usage(
        &mut self,
        class: &str,
        verb_phrase: &str,
        span: Span,
        location: ClassUsageLocation,
    ) {
        if self.settings.restricted_symbols.is_empty() {
            return;
        }
        let Some(entry) = self.settings.restricted_symbols.class(class) else {
            return;
        };
        if is_allowed_by_position(entry, location) {
            return;
        }
        let entry = entry.clone();
        let type_word = self
            .registry
            .classes
            .get(class)
            .map(|info| class_type_word(info.kind()))
            .unwrap_or_else(|| class_type_word(ClassKind::Class));
        self.report_restricted(
            &entry,
            format!(
                "{verb_phrase} restricted {} {class}: {}",
                type_word.to_lowercase(),
                entry.message
            ),
            span,
            "restrictedSymbol.class",
        );
    }

    /// The `use`-import detection surface a `Class`-kind ban's own
    /// listed locations never covered at all until now - a real,
    /// confirmed gap: `checker::imports::check_use_class_imports`
    /// already checks a class-shaped `use Foo;` for `class.prefixed`,
    /// but never for a restricted-symbols ban. Kept separate from
    /// `check_restricted_class_name_usage` (rather than threading a new
    /// parameter through its 6 other call sites) since `allow_in_use`
    /// is the one allow-mechanism scoped to exactly this single
    /// location, not shared by any of the others.
    pub(crate) fn check_restricted_class_use_import(&mut self, class: &str, span: Span) {
        if self.settings.restricted_symbols.is_empty() {
            return;
        }
        let Some(entry) = self.settings.restricted_symbols.class(class) else {
            return;
        };
        if entry.allow_in_use {
            return;
        }
        let entry = entry.clone();
        let type_word = self
            .registry
            .classes
            .get(class)
            .map(|info| class_type_word(info.kind()))
            .unwrap_or_else(|| class_type_word(ClassKind::Class));
        self.report_restricted(
            &entry,
            format!(
                "Importing restricted {} {class}: {}",
                type_word.to_lowercase(),
                entry.message
            ),
            span,
            "restrictedSymbol.class",
        );
    }

    /// Closes a real, live enforcement gap: the `Class` kind's own ban
    /// was never actually checked against an `extends`/`implements`
    /// clause at all (only `instanceof`/`new` called `check_restricted_
    /// class_name_usage`) - mirrors `check_internal_inheritance`'s own
    /// shape exactly (same already-resolved `ClassReflection::extends`/
    /// `implements` fields, same whole-class-declaration span, same two
    /// call sites), minus its trait-`use` half - a banned class named in
    /// a trait's own `use` clause isn't one of the `Class`-kind ban's own
    /// listed detection surfaces.
    pub(crate) fn check_restricted_class_inheritance(&mut self, fqn: &str, span: Span) {
        if self.settings.restricted_symbols.is_empty() {
            return;
        }
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let (extends, implements) = (info.extends().clone(), info.implements().clone());
        for parent in &extends {
            self.check_restricted_class_name_usage(
                parent,
                &format!("{fqn} extends"),
                span,
                ClassUsageLocation::Other,
            );
        }
        for iface in &implements {
            self.check_restricted_class_name_usage(
                iface,
                &format!("{fqn} implements"),
                span,
                ClassUsageLocation::Other,
            );
        }
    }

    /// `check_internal_param_and_return_types`'s own restricted-symbols
    /// sibling - named functions/methods only, delegating to `check_
    /// restricted_param_and_return_types_raw` below.
    pub(crate) fn check_restricted_param_and_return_types(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
    ) {
        self.check_restricted_param_and_return_types_raw(
            &f.params,
            f.return_type.as_ref(),
            f.return_type.as_ref().map(|h| h.span).unwrap_or(f.span),
            class_name,
        );
    }

    /// `param`/`return` native type-hint coverage - the third and last
    /// missing piece of the `Class` kind's own listed detection surface.
    /// Mirrors `check_internal_param_and_return_types_raw`'s exact
    /// shape (same native-type-hint-only resolution, same recursive
    /// `Union`/`Intersection` class-name collection via the shared
    /// `collect_referenced_class_names`), called from the identical two
    /// call sites (`checker/expr.rs`'s `Closure`/`ArrowFn` literal-
    /// signature checking and the named-declaration path both already
    /// reach through `check_internal_param_and_return_types_raw`).
    pub(crate) fn check_restricted_param_and_return_types_raw(
        &mut self,
        params: &[Param],
        return_type: Option<&TypeHint>,
        return_type_fallback_span: Span,
        class_name: Option<&str>,
    ) {
        if self.settings.restricted_symbols.is_empty() {
            return;
        }
        for p in params {
            let ty = self.resolve_type_hint(p.type_hint.as_ref());
            let ty = match class_name {
                Some(c) => self.resolve_pseudo(&ty, c),
                None => ty,
            };
            let mut classes = Vec::new();
            collect_referenced_class_names(&ty, &mut classes);
            for class in classes {
                self.check_restricted_class_name_usage(
                    &class,
                    &format!("Parameter ${} references", p.name),
                    p.type_hint.as_ref().map(|h| h.span).unwrap_or(p.name.span),
                    ClassUsageLocation::ParamType,
                );
            }
        }
        if return_type.is_some() {
            let ty = self.resolve_type_hint(return_type);
            let ty = match class_name {
                Some(c) => self.resolve_pseudo(&ty, c),
                None => ty,
            };
            let mut classes = Vec::new();
            collect_referenced_class_names(&ty, &mut classes);
            for class in classes {
                self.check_restricted_class_name_usage(
                    &class,
                    "Return type references",
                    return_type_fallback_span,
                    ClassUsageLocation::ReturnType,
                );
            }
        }
    }
}
