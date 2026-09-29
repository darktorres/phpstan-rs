use rustc_hash::FxHashMap;

use crate::{CallableParam, CallableSig, ConditionalSubject, Relation, Repr, TemplateParam, Type};

impl Relation<'_> {
    /// PHPStan's `inferTemplateTypes`: the bindings a parameter of type
    /// `param` implies for its templates when given an argument of type
    /// `arg`, added to `bindings`. Every position is read structurally: a
    /// bare `T`, an array's key and value, a shape's fields, a generic
    /// object's arguments (through the argument's ancestors), a
    /// `class-string<T>`, a callable's parameters and return. A template
    /// binds only an argument its bound surely holds, and bindings of one
    /// template from several positions join (`pick(1, 'a')` binds `1|'a'`).
    /// A callable type's own templates bind when it is called
    /// (`CallableSig::instantiated_return`), not here.
    pub fn infer_template_bindings(
        &self,
        param: &Type,
        arg: &Type,
        bindings: &mut FxHashMap<String, Type>,
    ) {
        syntax::stack::guard(|| {
            if matches!(arg, Type(Repr::Never)) {
                return;
            }
            // A union argument binds member by member, unless the parameter
            // itself is a union or a bare template to match it whole.
            if let Type(Repr::Union(members)) | Type(Repr::BenevolentUnion(members)) = arg
                && !matches!(param, Type(Repr::Union(_)) | Type(Repr::TemplateParam(_)))
            {
                for member in members {
                    self.infer_template_bindings(param, member, bindings);
                }
                return;
            }
            match param {
                Type(Repr::TemplateParam(tp))
                    if tp.scope.as_deref() == Some(crate::CALLABLE_TEMPLATE_SCOPE) => {}
                Type(Repr::TemplateParam(tp)) => {
                    // An argument without type information binds nothing.
                    if arg.is_mixed() {
                        return;
                    }
                    // `T of array<K, V>` binds `K` and `V` too.
                    self.infer_template_bindings(&tp.bound, arg, bindings);
                    if matches!(*tp.bound, Type(Repr::Mixed(_)))
                        || self.is_super_type_of(&tp.bound, arg).yes()
                    {
                        bind(bindings, &tp.name, arg.clone());
                    }
                }
                // `T|null` given `int|null` binds `T` to `int`: an argument
                // member a parameter member without templates surely holds
                // is that member's, and the rest binds the templates.
                Type(Repr::Union(param_members)) => {
                    let mut remaining: Vec<Type> = Vec::new();
                    'members: for arg_member in arg.union_members() {
                        for param_member in param_members {
                            if self.is_super_type_of(param_member, arg_member).yes() {
                                self.infer_template_bindings(param_member, arg_member, bindings);
                                continue 'members;
                            }
                        }
                        remaining.push(arg_member.clone());
                    }
                    remaining.retain(|arg_member| {
                        !param_members.iter().any(|param_member| {
                            !param_member.references_template()
                                && self.is_super_type_of(param_member, arg_member).yes()
                        })
                    });
                    if remaining.is_empty() {
                        return;
                    }
                    let rest = Type::union_of(remaining);
                    for param_member in param_members {
                        self.infer_template_bindings(param_member, &rest, bindings);
                    }
                }
                Type(Repr::Intersection(members)) => {
                    for member in members {
                        self.infer_template_bindings(member, arg, bindings);
                    }
                }
                Type(Repr::Array(key, value)) => {
                    if let Some(arg_key) = arg.array_key_type() {
                        self.infer_template_bindings(key, &arg_key, bindings);
                    }
                    if let Some(arg_value) = arg.array_value_type() {
                        self.infer_template_bindings(value, &arg_value, bindings);
                    }
                }
                Type(Repr::ArrayShape(shape)) => {
                    for field in &shape.fields {
                        if let Some(arg_value) = arg.offset_value_type(Some(&field.key)) {
                            self.infer_template_bindings(&field.ty, &arg_value, bindings);
                        }
                    }
                    if let Some((key, value)) = shape.unsealed.as_deref() {
                        if let Some(arg_key) = arg.array_key_type() {
                            self.infer_template_bindings(key, &arg_key, bindings);
                        }
                        if let Some(arg_value) = arg.array_value_type() {
                            self.infer_template_bindings(value, &arg_value, bindings);
                        }
                    }
                }
                Type(Repr::ClassString(inner)) => {
                    if let Type(Repr::ClassString(arg_inner)) = arg {
                        self.infer_template_bindings(inner, arg_inner, bindings);
                    }
                }
                Type(Repr::Object(class, params)) if !params.is_empty() => {
                    let Some(args) = self.object_arguments_as(arg, class) else {
                        return;
                    };
                    for (param, arg) in params.iter().zip(&args) {
                        let param = param.without_call_site_variance().unwrap_or(param);
                        self.infer_template_bindings(param, arg, bindings);
                    }
                }
                Type(Repr::Callable(Some(sig))) => {
                    if let Some(arg_sig) = arg.callable_sig() {
                        for (param, arg_param) in sig.params.iter().zip(&arg_sig.params) {
                            self.infer_template_bindings(&param.ty, &arg_param.ty, bindings);
                        }
                        self.infer_template_bindings(
                            &sig.return_type,
                            &arg_sig.return_type,
                            bindings,
                        );
                    } else if let Some((class, _)) = arg.as_object()
                        && let Some(return_type) =
                            self.hierarchy.method_return_type(class, "__invoke")
                    {
                        self.infer_template_bindings(&sig.return_type, &return_type, bindings);
                    }
                }
                Type(Repr::VarianceArg(_, inner)) => {
                    self.infer_template_bindings(inner, arg, bindings)
                }
                _ => {}
            }
        })
    }

    /// `arg`'s generic arguments as seen from `class` (itself, or an
    /// ancestor of `arg`'s class): `Box<int>` read as its interface
    /// `Container<int>` through its `@implements`. `None` when `arg` isn't
    /// an object of `class`.
    fn object_arguments_as(&self, arg: &Type, class: &str) -> Option<Vec<Type>> {
        match arg {
            Type(Repr::Object(arg_class, args)) if arg_class.eq_ignore_ascii_case(class) => {
                Some(args.clone())
            }
            Type(Repr::Object(arg_class, args))
                if self.hierarchy.is_subclass_of(arg_class, class) =>
            {
                let bound = self
                    .hierarchy
                    .ancestor_template_bindings(arg_class, args, class);
                let templates = self.hierarchy.find_class(class)?.templates;
                Some(
                    templates
                        .iter()
                        .map(|name| bound.get(name.as_ref()).cloned().unwrap_or(Type::mixed()))
                        .collect(),
                )
            }
            Type(Repr::Intersection(members)) => members
                .iter()
                .find_map(|member| self.object_arguments_as(member, class)),
            Type(Repr::TemplateParam(tp)) => self.object_arguments_as(&tp.bound, class),
            _ => None,
        }
    }
}

/// Adds `ty` to `name`'s binding: a template bound twice is bound to both.
fn bind(bindings: &mut FxHashMap<String, Type>, name: &str, ty: Type) {
    match bindings.get_mut(name) {
        Some(existing) => *existing = Type::union(existing.clone(), ty),
        None => {
            bindings.insert(name.to_string(), ty);
        }
    }
}

impl Type {
    /// `arg_ty` with each string literal in a `class-string<T>` position of
    /// this parameter type read as the class it names, when
    /// `resolve_class` finds one (PHPStan's
    /// `ConstantStringType::isClassString()`): `'Foo'` binds `T` as
    /// `Foo::class` does, through arrays and unions too. A bare `T`
    /// position keeps a string a string.
    pub fn promote_class_string_literals(
        &self,
        arg_ty: &Type,
        resolve_class: &impl Fn(&str) -> Option<String>,
    ) -> Type {
        syntax::stack::guard(|| match (self, arg_ty) {
            (Type(Repr::ClassString(_)), Type(Repr::StringLiteral(s))) => {
                match s.as_str().and_then(&resolve_class) {
                    Some(canonical) => {
                        Type::class_string(Type(Repr::Object(canonical.into(), Vec::new())))
                    }
                    None => arg_ty.clone(),
                }
            }
            (Type(Repr::Array(_, inner_p)), Type(Repr::Array(key, inner_a))) => Type(Repr::Array(
                key.clone(),
                Box::new(inner_p.promote_class_string_literals(inner_a, resolve_class)),
            )),
            (Type(Repr::Union(members)), _) => members
                .iter()
                .find_map(|m| {
                    let promoted = m.promote_class_string_literals(arg_ty, resolve_class);
                    (promoted != *arg_ty).then_some(promoted)
                })
                .unwrap_or_else(|| arg_ty.clone()),
            _ => arg_ty.clone(),
        })
    }
}

/// Applies a set of template bindings through a type tree, replacing every
/// `TemplateParam` with its bound type. A name left unbound falls back
/// to that template's own declared `@template T = Default` when it has
/// one, rather than unconditionally `Mixed` - e.g. a function-own
/// `@template T = int` whose `T` never actually gets bound from any real
/// call-site argument (used only in the return type, say) resolves to
/// `int` instead of `Mixed`. `checker::generics::
/// substitute_scoped` mirrors this same fallback for the separate
/// class-level substitution path (an "erased" - too-few-arguments, or
/// bare - generic class reference).
impl CallableSig {
    /// The return type of calling a value of this callable type with
    /// arguments of `arg_types`: its own templates bound from them, an
    /// unbound one falling back to its bound.
    pub fn instantiated_return(&self, arg_types: &[Type], relation: &Relation) -> Type {
        if self.templates.is_empty() {
            return (*self.return_type).clone();
        }
        fn open(ty: &Type) -> Type {
            match ty {
                Type(Repr::TemplateParam(tp))
                    if tp.scope.as_deref() == Some(crate::CALLABLE_TEMPLATE_SCOPE) =>
                {
                    Type::template_param(TemplateParam {
                        scope: None,
                        ..tp.clone()
                    })
                }
                other => other.map_children(&mut open),
            }
        }
        let mut bindings = FxHashMap::default();
        for (i, arg) in arg_types.iter().enumerate() {
            let param = self
                .params
                .get(i)
                .or_else(|| self.params.last().filter(|p| p.variadic));
            if let Some(param) = param {
                relation.infer_template_bindings(&open(&param.ty), arg, &mut bindings);
            }
        }
        substitute(
            &open(&self.return_type),
            &bindings,
            &outside_callable,
            &|_| true,
            relation,
        )
    }
}

impl CallableSig {
    /// This signature with its own templates replaced by their bounds:
    /// what it must work for whatever they are bound to.
    pub(crate) fn own_templates_as_bounds(&self, relation: &Relation) -> CallableSig {
        if self.templates.is_empty() {
            return self.clone();
        }
        let own = |tp: &TemplateParam| tp.scope.as_deref() == Some(crate::CALLABLE_TEMPLATE_SCOPE);
        self.map_types(&mut |ty| substitute(ty, &FxHashMap::default(), &own, &|_| true, relation))
    }

    /// This signature with its own templates bound as far as `other`'s
    /// parameter and return types bind them, an unbound one falling back
    /// to its bound: the instance of a generic callable that stands in
    /// for `other`.
    pub(crate) fn instantiated_against(
        &self,
        other: &CallableSig,
        relation: &Relation,
    ) -> CallableSig {
        if self.templates.is_empty() {
            return self.clone();
        }
        const OPEN: &str = "callable-open";
        fn open(ty: &Type) -> Type {
            match ty {
                Type(Repr::TemplateParam(tp))
                    if tp.scope.as_deref() == Some(crate::CALLABLE_TEMPLATE_SCOPE) =>
                {
                    Type::template_param(TemplateParam {
                        scope: Some(OPEN.into()),
                        ..tp.clone()
                    })
                }
                other => other.map_children(&mut open),
            }
        }
        let opened = self.map_types(&mut open);
        let mut bindings = FxHashMap::default();
        for (mine, theirs) in opened.params.iter().zip(&other.params) {
            relation.infer_template_bindings(&mine.ty, &theirs.ty, &mut bindings);
        }
        relation.infer_template_bindings(&opened.return_type, &other.return_type, &mut bindings);
        let is_open = |tp: &TemplateParam| tp.scope.as_deref() == Some(OPEN);
        let mut instance =
            opened.map_types(&mut |ty| substitute(ty, &bindings, &is_open, &|_| true, relation));
        instance.templates.clear();
        instance
    }

    /// This signature with `f` applied to each parameter type and the
    /// return type.
    fn map_types(&self, f: &mut dyn FnMut(&Type) -> Type) -> CallableSig {
        CallableSig {
            params: self
                .params
                .iter()
                .map(|p| CallableParam {
                    ty: f(&p.ty),
                    ..p.clone()
                })
                .collect(),
            return_type: Box::new(f(&self.return_type)),
            templates: self.templates.clone(),
        }
    }
}

impl Type {
    /// This type with each template `replace` answers for replaced by
    /// that answer, and each late type over one settled.
    pub fn replace_templates(
        &self,
        replace: &mut dyn FnMut(&TemplateParam) -> Option<Type>,
    ) -> Type {
        syntax::stack::guard(|| match self {
            Type(Repr::TemplateParam(tp)) => replace(tp).unwrap_or_else(|| self.clone()),
            _ => super::late::settle(self.map_children(&mut |t| t.replace_templates(replace))),
        })
    }
}

impl Type {
    /// This type with each template `bindings` binds replaced, an unbound
    /// one by its default or `mixed`; a conditional over a template
    /// resolves by `relation`.
    pub fn substitute_templates(
        &self,
        bindings: &FxHashMap<String, Type>,
        relation: &Relation,
    ) -> Type {
        substitute(self, bindings, &outside_callable, &|_| false, relation)
    }
}

impl Type {
    /// `Type::substitute_templates` for the templates `scope` declares only: every
    /// other template stays as it is.
    pub fn substitute_templates_of(
        &self,
        bindings: &FxHashMap<String, Type>,
        scope: &str,
        relation: &Relation,
    ) -> Type {
        substitute(
            self,
            bindings,
            &|tp| tp.scope.as_deref() == Some(scope),
            &|_| false,
            relation,
        )
    }
}

/// Whether a template belongs to something other than a callable type,
/// whose own templates stay until that callable is itself called.
fn outside_callable(tp: &TemplateParam) -> bool {
    tp.scope.as_deref() != Some(crate::CALLABLE_TEMPLATE_SCOPE)
}

impl Type {
    /// `Type::substitute_templates`, except that an unbound template `to_bound`
    /// selects falls back to its declared bound when it has no default
    /// (`@template T of Connection` gives `Connection`) - real PHPStan's
    /// `resolveToBounds` for a call's own templates that no argument
    /// inferred, e.g. Doctrine DBAL's `DriverManager::getConnection`
    /// conditional return `T` when no `wrapperClass` is passed. Class-level
    /// templates of an erased receiver stay `Mixed`, the wildcard argument
    /// an erased class's arguments compare as.
    pub fn substitute_templates_resolving_to_bounds(
        &self,
        bindings: &FxHashMap<String, Type>,
        to_bound: &dyn Fn(&TemplateParam) -> bool,
        relation: &Relation,
    ) -> Type {
        substitute(self, bindings, &outside_callable, to_bound, relation)
    }
}

/// `ty` with each template `substituted` selects replaced by its binding,
/// else its default, else (when `to_bound` selects it) its bound, else
/// `mixed`.
fn substitute(
    ty: &Type,
    bindings: &FxHashMap<String, Type>,
    substituted: &dyn Fn(&TemplateParam) -> bool,
    to_bound: &dyn Fn(&TemplateParam) -> bool,
    relation: &Relation,
) -> Type {
    syntax::stack::guard(|| {
        match ty {
            Type(Repr::TemplateParam(tp)) if !substituted(tp) => ty.clone(),
            Type(Repr::TemplateParam(tp)) => bindings.get(&tp.name).cloned().unwrap_or_else(|| {
                let fallback = match tp.default.as_deref() {
                    Some(default) => default,
                    None if to_bound(tp) && !matches!(*tp.bound, Type(Repr::Mixed(_))) => &tp.bound,
                    None => return Type::mixed(),
                };
                // The template's own name is bound to `Mixed` while its
                // fallback is substituted, so a self-referential bound
                // (`@template T of Foo<T>`), or two templates bounded by each
                // other, can't recurse forever.
                let mut guarded = bindings.clone();
                guarded.insert(tp.name.clone(), Type::mixed());
                substitute(fallback, &guarded, substituted, to_bound, relation)
            }),
            // A template-subject conditional (`(T is int ? A : B)`) resolves
            // once `T` is bound, the way real PHPStan's `ConditionalType`
            // does; otherwise both branches stay.
            Type(Repr::Conditional {
                subject: ConditionalSubject::Template(subject),
                target,
                if_true,
                if_false,
                negated,
            }) if bindings
                .get(subject.as_str())
                .is_some_and(|b| !matches!(b, Type(Repr::Mixed(_)))) =>
            {
                let target = substitute(target, bindings, substituted, to_bound, relation);
                match super::conditional::conditional_branch(
                    relation,
                    &bindings[subject.as_str()],
                    &target,
                    *negated,
                ) {
                    Some(true) => substitute(if_true, bindings, substituted, to_bound, relation),
                    Some(false) => substitute(if_false, bindings, substituted, to_bound, relation),
                    None => Type::union(
                        substitute(if_true, bindings, substituted, to_bound, relation),
                        substitute(if_false, bindings, substituted, to_bound, relation),
                    ),
                }
            }
            // Every nested position - a generic argument (`Box<T>` becomes
            // `Box<int>`), a shape field, a `Closure(T): U` shape, a
            // conditional return type's branches (`subject` is a raw
            // parameter name, not a `Type`, so there's nothing to substitute
            // there).
            _ => {
                super::late::settle(ty.map_children(&mut |t| {
                    substitute(t, bindings, substituted, to_bound, relation)
                }))
            }
        }
    })
}
