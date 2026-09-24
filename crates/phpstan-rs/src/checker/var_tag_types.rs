//! Statement-level `@var` tags: their resolved types, the native type of
//! an expression (what PHP itself guarantees, ignoring PHPDoc), and
//! `VarTagTypeRuleHelper`/`VarTagChangedExpressionTypeRule`'s checks of a
//! tag's type against the value it describes.

use super::*;
use crate::registry::{self, Trinary};

/// How the described value was written, for the literal leniency of
/// `VarTagTypeRuleHelper::isValidSuperTypeOfExpr`.
#[derive(Clone, Copy, PartialEq)]
enum ValueShape {
    /// A scalar literal or constant fetch - the tag only has to overlap.
    Literal,
    /// An array literal; an empty one widens to `array`.
    ArrayLiteral {
        empty: bool,
    },
    Other,
}

fn value_shape(e: &Expr) -> ValueShape {
    match &e.kind {
        ExprKind::Grouping(inner) => value_shape(inner),
        ExprKind::Literal(_) | ExprKind::Interpolated(_) | ExprKind::Name(_) => ValueShape::Literal,
        ExprKind::ArrayLiteral(items) => ValueShape::ArrayLiteral {
            empty: items.is_empty(),
        },
        _ => ValueShape::Other,
    }
}

/// The value a `@var` tag is checked against: its native and PHPDoc
/// types, and how it was written.
struct Described {
    native: Type,
    doc: Type,
    shape: ValueShape,
}

fn is_constant_value(ty: &Type) -> bool {
    match ty {
        Type::IntLiteral(_)
        | Type::StringLiteral(_)
        | Type::FloatLiteral(_)
        | Type::True
        | Type::False
        | Type::Null
        | Type::EnumCase(..) => true,
        Type::Union(members) => members.iter().all(is_constant_value),
        _ => false,
    }
}

fn is_array_like(ty: &Type) -> bool {
    match ty {
        Type::Array(..) | Type::ArrayShape(_) => true,
        Type::Intersection(members) => members.iter().any(is_array_like),
        _ => false,
    }
}

fn offset_value_type(ty: &Type, key: &Type) -> Type {
    match ty {
        Type::ArrayShape(fields) => {
            let name = match key {
                Type::IntLiteral(n) => n.to_string(),
                Type::StringLiteral(s) => s.clone(),
                _ => return Type::Mixed(false),
            };
            fields
                .iter()
                .find(|(k, _, _)| *k == name)
                .map_or(Type::Mixed(false), |(_, t, _)| t.clone())
        }
        other => Checker::array_element_type(other).unwrap_or(Type::Mixed(false)),
    }
}

/// A native iterable's element type - known only for an array; an
/// object's comes from PHPDoc generics.
fn native_iterable_value(ty: &Type) -> Type {
    if is_array_like(ty) {
        Checker::array_element_type(ty).unwrap_or(Type::Mixed(false))
    } else {
        Type::Mixed(false)
    }
}

fn native_iterable_key(ty: &Type) -> Type {
    if is_array_like(ty) {
        Checker::array_key_type(ty).unwrap_or(Type::Mixed(false))
    } else {
        Type::Mixed(false)
    }
}

/// `ty` with `static` bound to `class`.
fn bind_static(ty: &Type, class: &str) -> Type {
    match ty {
        Type::StaticRef => Type::Object(class.to_string(), Vec::new()),
        Type::Object(name, args) if name == "$$static" => Type::Object(
            class.to_string(),
            args.iter().map(|a| bind_static(a, class)).collect(),
        ),
        other => other.map_children(&mut |c| bind_static(c, class)),
    }
}

impl<'a> Checker<'a> {
    /// `e`'s value when it is a known string: literal syntax, or a
    /// constant (`self::PATTERN`) whose inferred type is one string.
    pub(crate) fn constant_string_value(&self, e: &Expr, locals: &Locals) -> Option<String> {
        if let Some(s) = super::assignment::literal_string_value(e) {
            return Some(s.into_owned());
        }
        match self.infer_expr(e, locals) {
            Type::StringLiteral(s) => Some(s),
            _ => None,
        }
    }

    /// A closure/arrow-fn parameter's native type: its hint alone, with
    /// a `= null` default's implicit nullability.
    pub(crate) fn closure_param_native(&self, p: &Param) -> Type {
        if p.type_hint.is_none() {
            return Type::Mixed(false);
        }
        let ty = self.resolve_type_hint(p.type_hint.as_deref());
        let ty = match &self.current_class {
            Some(c) => self.resolve_pseudo(&ty, c),
            None => ty,
        };
        let ty = registry::apply_implicit_nullable_default(ty, p.default.as_ref());
        if p.variadic {
            Type::Array(Box::new(Type::unknown_array_key()), Box::new(ty))
        } else {
            ty
        }
    }

    /// The native type of `e` - what PHP itself guarantees, ignoring
    /// PHPDoc - or `mixed` when it isn't known. Never narrower than the
    /// real native type, so a check against it can only miss.
    pub(crate) fn native_expr_type(&self, e: &Expr, locals: &Locals) -> Type {
        match &e.kind {
            ExprKind::Literal(_) => self.infer_expr(e, locals),
            ExprKind::Interpolated(_) => Type::String,
            ExprKind::Grouping(inner) | ExprKind::Clone(inner, _) => {
                self.native_expr_type(inner, locals)
            }
            ExprKind::Assign(_, rhs) => self.native_expr_type(rhs, locals),
            ExprKind::CompoundAssign(BinOp::Concat, ..) => Type::String,
            ExprKind::CompoundAssign(BinOp::Coalesce, lhs, rhs) => {
                let l = self.native_expr_type(lhs, locals);
                let r = self.native_expr_type(rhs, locals);
                if matches!(l, Type::Mixed(_)) || matches!(r, Type::Mixed(_)) {
                    Type::Mixed(false)
                } else {
                    union_types(l, r)
                }
            }
            ExprKind::ArrayLiteral(items) => {
                if items.is_empty() {
                    return self.infer_expr(e, locals);
                }
                let mut value: Option<Type> = None;
                for item in items {
                    let item_ty = if item.spread {
                        Type::Mixed(false)
                    } else {
                        self.native_expr_type(&item.value, locals)
                    };
                    value = Some(match value {
                        None => item_ty,
                        Some(prev) => union_types(prev, item_ty),
                    });
                }
                Type::Array(
                    Box::new(Type::unknown_array_key()),
                    Box::new(value.unwrap_or(Type::Mixed(false))),
                )
            }
            ExprKind::Variable(name) if &**name == "this" => self.infer_expr(e, locals),
            ExprKind::Variable(name) => match (locals.native(name), locals.get(name)) {
                // A by-reference write can leave a value the recorded
                // native type no longer describes.
                (Some(native), Some(current))
                    if self.registry.is_super_type_of(native, current) != Trinary::No =>
                {
                    native.clone()
                }
                _ => Type::Mixed(false),
            },
            ExprKind::Cast(kind, _, _) => match kind.as_str() {
                "int" => Type::Int,
                "float" => Type::Float,
                "string" => Type::String,
                "bool" => Type::Bool,
                "array" => Type::Array(
                    Box::new(Type::unknown_array_key()),
                    Box::new(Type::Mixed(false)),
                ),
                "unset" => Type::Null,
                _ => Type::Mixed(false),
            },
            ExprKind::New(..) => match self.infer_expr(e, locals) {
                Type::Object(class, _) => Type::Object(class, Vec::new()),
                _ => Type::Mixed(false),
            },
            ExprKind::Closure { .. } | ExprKind::ArrowFn { .. } => {
                Type::Object("Closure".to_string(), Vec::new())
            }
            ExprKind::Instanceof(..) | ExprKind::Unary(UnaryOp::Not, _) => Type::Bool,
            ExprKind::Binary(op, lhs, rhs) => match op {
                BinOp::Concat => Type::String,
                BinOp::Eq
                | BinOp::Identical
                | BinOp::NotEq
                | BinOp::NotIdentical
                | BinOp::Lt
                | BinOp::Gt
                | BinOp::LtEq
                | BinOp::GtEq
                | BinOp::And
                | BinOp::Or
                | BinOp::LogicalAnd
                | BinOp::LogicalOr
                | BinOp::LogicalXor => Type::Bool,
                BinOp::Spaceship => Type::Int,
                BinOp::Coalesce => {
                    let l = self.native_expr_type(lhs, locals);
                    let r = self.native_expr_type(rhs, locals);
                    if matches!(l, Type::Mixed(_)) || matches!(r, Type::Mixed(_)) {
                        Type::Mixed(false)
                    } else {
                        union_types(l, r)
                    }
                }
                _ => Type::Mixed(false),
            },
            ExprKind::Ternary(cond, then, otherwise) => {
                let t = self.native_expr_type(then.as_deref().unwrap_or(cond), locals);
                let o = self.native_expr_type(otherwise, locals);
                if matches!(t, Type::Mixed(_)) || matches!(o, Type::Mixed(_)) {
                    Type::Mixed(false)
                } else {
                    union_types(t, o)
                }
            }
            ExprKind::Call(callee, _) => {
                let ExprKind::Name(name) = &callee.kind else {
                    return Type::Mixed(false);
                };
                let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
                let sig = self.registry.lookup_function(&primary).or_else(|| {
                    fallback
                        .as_deref()
                        .and_then(|f| self.registry.lookup_function(f))
                });
                match sig {
                    Some(sig) if !sig.is_magic => sig.return_type_native.clone(),
                    _ => Type::Mixed(false),
                }
            }
            ExprKind::MethodCall(receiver, method, _, nullsafe) => {
                let Type::Object(class, _) = self.native_expr_type(receiver, locals) else {
                    return Type::Mixed(false);
                };
                let ty = self.native_method_return(&class, method);
                if *nullsafe && !matches!(ty, Type::Mixed(_)) {
                    union_types(ty, Type::Null)
                } else {
                    ty
                }
            }
            ExprKind::StaticCall(target, method, _) => {
                let class = match target {
                    StaticTarget::Name(name, _) => self.resolve_static_class_name(name),
                    StaticTarget::Expr(receiver) => match self.native_expr_type(receiver, locals) {
                        Type::Object(class, _) => Some(class),
                        _ => None,
                    },
                };
                match class {
                    Some(class) => self.native_method_return(&class, method),
                    None => Type::Mixed(false),
                }
            }
            ExprKind::PropertyAccess(receiver, prop, nullsafe) => {
                let Type::Object(class, _) = self.native_expr_type(receiver, locals) else {
                    return Type::Mixed(false);
                };
                let ty = self.native_property_type(&class, prop, false);
                if *nullsafe && !matches!(ty, Type::Mixed(_)) {
                    union_types(ty, Type::Null)
                } else {
                    ty
                }
            }
            ExprKind::StaticAccess(class_name, member) => match member.strip_prefix('$') {
                Some(prop) => match self.resolve_static_class_name(class_name) {
                    Some(class) => self.native_property_type(&class, prop, true),
                    None => Type::Mixed(false),
                },
                None => Type::Mixed(false),
            },
            _ => Type::Mixed(false),
        }
    }

    /// A method's native return type, `static` bound to `class` and
    /// `self` to the declaring class. `mixed` for a `@method` tag.
    fn native_method_return(&self, class: &str, method: &str) -> Type {
        let Some((owner, sig)) = self.registry.lookup_method_with_owner(class, method) else {
            return Type::Mixed(false);
        };
        if sig.is_magic {
            return Type::Mixed(false);
        }
        self.resolve_pseudo(&bind_static(&sig.return_type_native, class), &owner)
    }

    /// A property's native type, `mixed` when it has none (untyped, or a
    /// `@property` tag).
    fn native_property_type(&self, class: &str, prop: &str, want_static: bool) -> Type {
        match self.registry.lookup_property_with_owner(class, prop) {
            Some((owner, sig))
                if sig.has_native_type && !sig.is_virtual && sig.is_static == want_static =>
            {
                self.resolve_pseudo(&sig.ty_native, &owner)
            }
            _ => Type::Mixed(false),
        }
    }

    /// Every `@var` tag on a statement, resolved, in the
    /// `PhpDocNodeResolver::resolveVarTags` selection `var_tag_nodes`
    /// makes.
    pub(crate) fn statement_var_tags(&self, doc: &str) -> Vec<(Option<String>, Type)> {
        super::phpdoc_unresolvable::var_tag_nodes(doc)
            .into_iter()
            .map(|(name, node)| {
                let ty = self.resolve_type_hint(Some(&node.to_source_string()));
                let ty = match &self.current_class {
                    Some(class) => match self.registry.classes.get(class.as_str()) {
                        Some(info) if !info.type_aliases.is_empty() => {
                            registry::substitute_type_aliases(ty, &info.type_aliases)
                        }
                        _ => ty,
                    },
                    None => ty,
                };
                let ty = match &self.current_class {
                    Some(class) => self.resolve_pseudo(&ty, class),
                    None => ty,
                };
                (name, ty)
            })
            .collect()
    }

    /// `InvalidPhpDocVarTagTypeRule`'s class half for a statement's
    /// `@var` tags: `class.notFound` and `varTag.trait`.
    pub(crate) fn check_statement_var_tag_classes(
        &mut self,
        stmt: &Stmt,
        tags: &[(Option<String>, Type)],
        label: &str,
    ) {
        for (name, ty) in tags {
            let subject = match name {
                Some(n) => format!("PHPDoc tag @var for variable ${n}"),
                None => "PHPDoc tag @var".to_string(),
            };
            let mut unknown = Vec::new();
            self.collect_unknown_class_names(ty, &mut unknown);
            let mut traits = Vec::new();
            self.collect_trait_class_names(ty, &mut traits);
            let mut seen = rustc_hash::FxHashSet::default();
            for class in traits {
                if seen.insert(class.clone()) {
                    self.report(
                        label.to_string(),
                        format!("{subject} has invalid type {class}."),
                        stmt.span,
                        2,
                        "varTag.trait",
                    );
                }
            }
            self.check_type_prefixed_class_names(ty, stmt.span, label);
            for class in unknown {
                if seen.insert(class.clone()) {
                    self.report(
                        label.to_string(),
                        format!("{subject} contains unknown class {class}."),
                        stmt.span,
                        0,
                        "class.notFound",
                    );
                }
            }
        }
    }

    /// `VarTagTypeRuleHelper::checkVarType` for every value a statement's
    /// `@var` tags describe, and `VarTagChangedExpressionTypeRule` for a
    /// tag re-typing a variable that already exists. `locals` is the
    /// state before the statement; `names_ok` is whether the tags'
    /// variable names matched an assignment (checked only then).
    pub(crate) fn check_statement_var_tag_types(
        &mut self,
        stmt: &Stmt,
        tags: &[(Option<String>, Type)],
        names_ok: bool,
        locals: &Locals,
        label: &str,
    ) {
        let unnamed: Vec<&Type> = tags
            .iter()
            .filter(|(n, _)| n.is_none())
            .map(|(_, t)| t)
            .collect();
        let tag_for = |name: &str, candidates: usize| -> Option<&Type> {
            tags.iter()
                .find(|(n, _)| n.as_deref() == Some(name))
                .map(|(_, t)| t)
                .or_else(|| {
                    (candidates == 1)
                        .then(|| unnamed.first().copied())
                        .flatten()
                })
        };
        let mut checks: Vec<(Type, Described)> = Vec::new();
        // Variables whose tags describe the statement's own values, not
        // their current ones.
        let mut own: Vec<String> = Vec::new();
        match &stmt.kind {
            StmtKind::Expression(e) => {
                if let ExprKind::Assign(lhs, rhs)
                | ExprKind::AssignRef(lhs, rhs)
                | ExprKind::CompoundAssign(_, lhs, rhs) = &e.kind
                {
                    own = super::stmt::assigned_variable_names(lhs);
                    if names_ok {
                        let described = if matches!(e.kind, ExprKind::CompoundAssign(..)) {
                            Described {
                                native: self.native_expr_type(e, locals),
                                doc: self.infer_expr(e, locals),
                                shape: ValueShape::Other,
                            }
                        } else {
                            Described {
                                native: self.native_expr_type(rhs, locals),
                                doc: self.infer_expr(rhs, locals),
                                shape: value_shape(rhs),
                            }
                        };
                        let candidates = own.len();
                        self.collect_target_checks(
                            lhs,
                            described,
                            &tag_for,
                            candidates,
                            &mut checks,
                        );
                    }
                }
            }
            StmtKind::Foreach {
                subject,
                key,
                value,
                ..
            } => {
                let mut names = Vec::new();
                if let ExprKind::Variable(v) = &subject.kind {
                    names.push(v.to_string());
                }
                if let Some(k) = key
                    && let ExprKind::Variable(v) = &k.kind
                {
                    names.push(v.to_string());
                }
                names.extend(super::stmt::assigned_variable_names(value));
                let candidates = names.len();
                let native = self.native_expr_type(subject, locals);
                let doc = self.infer_expr(subject, locals);
                if let ExprKind::Variable(v) = &subject.kind
                    && let Some(tag) = tag_for(v, candidates)
                {
                    checks.push((
                        tag.clone(),
                        Described {
                            native: native.clone(),
                            doc: doc.clone(),
                            shape: ValueShape::Other,
                        },
                    ));
                }
                if let Some(k) = key {
                    let described = Described {
                        native: native_iterable_key(&native),
                        doc: self.iterable_key_of(&doc),
                        shape: ValueShape::Other,
                    };
                    self.collect_target_checks(k, described, &tag_for, candidates, &mut checks);
                }
                let described = Described {
                    native: native_iterable_value(&native),
                    doc: self.iterable_value_of(&doc),
                    shape: ValueShape::Other,
                };
                self.collect_target_checks(value, described, &tag_for, candidates, &mut checks);
                // Not re-typed by a tag's own annotation pass.
                own = names;
            }
            StmtKind::StaticVar(vars) => {
                for (name, default) in vars {
                    if let Some(default) = default
                        && let Some(tag) = tag_for(name, vars.len())
                    {
                        checks.push((
                            tag.clone(),
                            Described {
                                native: self.native_expr_type(default, locals),
                                doc: self.infer_expr(default, locals),
                                shape: value_shape(default),
                            },
                        ));
                    }
                }
                own = vars.iter().map(|(n, _)| n.to_string()).collect();
            }
            StmtKind::Global(names) => own = names.clone(),
            StmtKind::Return(Some(e)) | StmtKind::Throw(e) => {
                if let [tag] = unnamed.as_slice() {
                    checks.push((
                        (*tag).clone(),
                        Described {
                            native: self.native_expr_type(e, locals),
                            doc: self.infer_expr(e, locals),
                            shape: value_shape(e),
                        },
                    ));
                }
            }
            _ => {}
        }
        // A named tag re-typing a variable that already exists.
        for (name, tag) in tags {
            let Some(name) = name else { continue };
            if own.contains(name) {
                continue;
            }
            let Some(current) = locals.get(name) else {
                continue;
            };
            if current == tag {
                continue;
            }
            let variable = Expr {
                kind: ExprKind::Variable(name.as_str().into()),
                span: stmt.span,
            };
            checks.push((
                tag.clone(),
                Described {
                    native: self.native_expr_type(&variable, locals),
                    doc: current.clone(),
                    shape: ValueShape::Other,
                },
            ));
        }
        for (tag, described) in checks {
            self.check_var_tag_against(&tag, &described, stmt.span, label);
        }
    }

    /// `checkVarType`'s target walk: a plain variable takes its own tag
    /// (or the statement's single unnamed one), a destructuring target
    /// descends into each item's offset.
    fn collect_target_checks<'t>(
        &self,
        target: &Expr,
        described: Described,
        tag_for: &dyn Fn(&str, usize) -> Option<&'t Type>,
        candidates: usize,
        out: &mut Vec<(Type, Described)>,
    ) {
        match &target.kind {
            ExprKind::Variable(name) => {
                if let Some(tag) = tag_for(name, candidates) {
                    out.push((tag.clone(), described));
                }
            }
            ExprKind::ArrayLiteral(items) => {
                for (i, item) in items.iter().enumerate() {
                    let key = match &item.key {
                        Some(k) => match &k.kind {
                            ExprKind::Literal(Literal::Int(n)) => Type::IntLiteral(*n),
                            ExprKind::Literal(Literal::String(s)) => {
                                Type::StringLiteral(s.to_string())
                            }
                            _ => Type::Mixed(false),
                        },
                        None => Type::IntLiteral(i as i64),
                    };
                    let item_described = Described {
                        native: offset_value_type(&described.native, &key),
                        doc: offset_value_type(&described.doc, &key),
                        shape: ValueShape::Other,
                    };
                    self.collect_target_checks(
                        &item.value,
                        item_described,
                        tag_for,
                        candidates,
                        out,
                    );
                }
            }
            _ => {}
        }
    }

    fn iterable_value_of(&self, ty: &Type) -> Type {
        Self::array_element_type(ty)
            .or_else(|| self.iterable_element_type(ty).map(|(_, v)| v))
            .unwrap_or(Type::Mixed(false))
    }

    fn iterable_key_of(&self, ty: &Type) -> Type {
        if is_array_like(ty) {
            return Self::array_key_type(ty).unwrap_or(Type::Mixed(false));
        }
        self.iterable_element_type(ty)
            .map_or(Type::Mixed(false), |(k, _)| k)
    }

    /// `VarTagTypeRuleHelper::checkExprType`: `varTag.nativeType` when
    /// the tag isn't a subtype of the value's native type; otherwise
    /// `varTag.type` when it can't describe the PHPDoc type at all, and
    /// `phpstanApi.varTagAssumption` for a narrowed PHPStan `Type`.
    fn check_var_tag_against(&mut self, tag: &Type, value: &Described, span: Span, label: &str) {
        // An unknown class or a trait is reported on its own.
        let mut unusable = Vec::new();
        self.collect_unknown_class_names(tag, &mut unusable);
        self.collect_trait_class_names(tag, &mut unusable);
        if !unusable.is_empty() {
            return;
        }
        if !matches!(value.native, Type::Mixed(_))
            && !self.var_tag_fits_value(&value.native, tag, value.shape)
        {
            self.report(
                label.to_string(),
                format!(
                    "PHPDoc tag @var with type {tag} is not subtype of native type {}.",
                    value.native
                ),
                span,
                2,
                "varTag.nativeType",
            );
            return;
        }
        if matches!(value.doc, Type::Mixed(_)) || matches!(tag, Type::Mixed(_)) {
            return;
        }
        if self.contains_phpstan_type(tag) {
            if !self.var_tag_fits_value(&value.doc, tag, value.shape) {
                self.report(
                    label.to_string(),
                    format!(
                        "PHPDoc tag @var with type {tag} is not subtype of type {}.",
                        value.doc
                    ),
                    span,
                    2,
                    "varTag.type",
                );
            } else if value.doc != *tag {
                self.report(
                    label.to_string(),
                    format!(
                        "PHPDoc tag @var assumes the expression with type {} is always {tag} but it's error-prone and dangerous.",
                        value.doc
                    ),
                    span,
                    2,
                    "phpstanApi.varTagAssumption",
                );
            }
            return;
        }
        // A tag may refine the inferred type or widen it; only one with no
        // value in common with it describes some other value.
        if self.registry.is_super_type_of(&value.doc, tag) == Trinary::No
            && self.registry.is_super_type_of(tag, &value.doc) == Trinary::No
        {
            self.report(
                label.to_string(),
                format!(
                    "PHPDoc tag @var with type {tag} is incompatible with type {}.",
                    value.doc
                ),
                span,
                2,
                "varTag.type",
            );
        }
    }

    /// `isValidSuperTypeOfExpr`: literals and array literals only have to
    /// overlap the tag.
    fn var_tag_fits_value(&self, ty: &Type, tag: &Type, shape: ValueShape) -> bool {
        match shape {
            ValueShape::Literal => self.at_least_maybe_super_type(ty, tag),
            ValueShape::ArrayLiteral { empty } => {
                let ty = if empty {
                    Type::Array(
                        Box::new(Type::unknown_array_key()),
                        Box::new(Type::Mixed(false)),
                    )
                } else {
                    ty.clone()
                };
                self.at_least_maybe_super_type(&ty, tag)
            }
            ValueShape::Other => self.is_valid_var_tag_super_type(ty, tag, 0),
        }
    }

    /// `isValidSuperType` (the default, non-strict widening check).
    fn is_valid_var_tag_super_type(&self, ty: &Type, tag: &Type, depth: usize) -> bool {
        if matches!(ty, Type::Mixed(_)) {
            return true;
        }
        if matches!(ty, Type::ArrayShape(fields) if fields.is_empty()) {
            let array = Type::Array(
                Box::new(Type::unknown_array_key()),
                Box::new(Type::Mixed(false)),
            );
            return self.at_least_maybe_super_type(&array, tag);
        }
        if is_array_like(ty) && is_array_like(tag) {
            if !self.at_least_maybe_super_type(ty, tag) {
                return false;
            }
            let inner = self.iterable_value_of(ty);
            let inner_tag = self.iterable_value_of(tag);
            return self.is_valid_var_tag_super_type(&inner, &inner_tag, depth + 1);
        }
        if depth == 0 && is_constant_value(ty) {
            return self.at_least_maybe_super_type(ty, tag);
        }
        self.registry.is_super_type_of(ty, tag) == Trinary::Yes
    }

    fn at_least_maybe_super_type(&self, ty: &Type, tag: &Type) -> bool {
        self.registry.is_super_type_of(ty, tag) != Trinary::No
    }

    /// Whether `ty` names a subclass of `PHPStan\Type\Type`.
    fn contains_phpstan_type(&self, ty: &Type) -> bool {
        match ty {
            Type::Object(name, _) => {
                self.registry.classes.contains_key("PHPStan\\Type\\Type")
                    && self.registry.is_subclass_of(name, "PHPStan\\Type\\Type")
            }
            Type::Union(members) | Type::Intersection(members) => {
                members.iter().any(|m| self.contains_phpstan_type(m))
            }
            _ => false,
        }
    }
}
