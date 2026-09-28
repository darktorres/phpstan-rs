//! A call's arguments against the parameters they bind to: PHPStan's
//! `FunctionCallParametersCheck`, which every call rule (functions,
//! methods, static methods, constructors, attributes, callables) runs
//! with its own wording of the callee.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::checker::calls::{contains_nullsafe_access, is_valid_byref_argument_expr};
use crate::checker::{Checker, Locals};
use crate::registry::{FunctionReflection, ParameterReflection};
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use syntax::token::Span;
use types::php_version::PhpFeature;
use types::{Shape, ShapeKey, Type};

/// The callee as a call rule's messages name it.
#[derive(Clone, Copy)]
pub(crate) enum Callee<'a> {
    /// `CallToFunctionParametersRule`: the function's declared name.
    Function(&'a str),
    /// `CallMethodsRule`: the declaring class and the method's declared
    /// name.
    Method { class: &'a str, name: &'a str },
    /// `CallStaticMethodsRule`, which also covers a non-static method
    /// called statically (`parent::foo()`).
    StaticMethod {
        class: &'a str,
        name: &'a str,
        is_static: bool,
    },
    /// `InstantiationRule`.
    Constructor(&'a str),
    /// `AttributesCheck`.
    AttributeConstructor(&'a str),
    /// `CallCallablesRule`/`CallUserFuncRule`: `closure`, `callable
    /// 'strlen'`, `callable passed to call_user_func()`.
    Callable(&'a str),
}

impl Callee<'_> {
    /// The subject of `... invoked with 2 parameters, 1 required.`.
    fn invoked(&self) -> String {
        match self {
            Callee::Function(name) => format!("Function {name}"),
            Callee::Method { class, name } => format!("Method {class}::{name}()"),
            Callee::StaticMethod {
                class,
                name,
                is_static,
            } => format!(
                "{} {class}::{name}()",
                if *is_static {
                    "Static method"
                } else {
                    "Method"
                }
            ),
            Callee::Constructor(class) => format!("Class {class} constructor"),
            Callee::AttributeConstructor(class) => format!("Attribute class {class} constructor"),
            Callee::Callable(description) => upper_first(description),
        }
    }

    /// The object of `Parameter #1 $x of ... expects int, string given.`.
    fn of(&self) -> String {
        match self {
            Callee::Function(name) => format!("function {name}"),
            Callee::Method { class, name } => format!("method {class}::{name}()"),
            Callee::StaticMethod {
                class,
                name,
                is_static,
            } => format!(
                "{} {class}::{name}()",
                if *is_static {
                    "static method"
                } else {
                    "method"
                }
            ),
            Callee::Constructor(class) => format!("class {class} constructor"),
            Callee::AttributeConstructor(class) => format!("attribute class {class} constructor"),
            Callee::Callable(description) => (*description).to_string(),
        }
    }

    /// The object of `Missing parameter $x in call to ...`.
    fn target(&self) -> String {
        match self {
            Callee::Constructor(class) | Callee::AttributeConstructor(class) => {
                format!("{class} constructor")
            }
            _ => self.of(),
        }
    }

    /// `... is passed by reference, so it expects variables only.` -
    /// verbatim, including the two constructor rules' leading space and
    /// missing full stop.
    fn by_ref_message(&self, parameter: &str) -> String {
        match self {
            Callee::Constructor(_) | Callee::AttributeConstructor(_) => format!(
                " {parameter} of {} is passed by reference, so it expects variables only",
                self.of()
            ),
            _ => format!(
                "{parameter} of {} is passed by reference, so it expects variables only.",
                self.of()
            ),
        }
    }
}

fn upper_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// One argument as the check sees it: a written argument, or one
/// element of a constant array unpacked into the call.
struct Argument<'e> {
    value: &'e Expr,
    /// Known up front for an unpacked element; inferred otherwise.
    ty: Option<Type>,
    unpack: bool,
    name: Option<String>,
    span: Span,
}

/// How an argument names its parameter in a message
/// (`FunctionCallParametersCheck::describeParameter`).
enum Position<'s> {
    Index(usize),
    Name(&'s str),
    Unnamed,
}

fn describe_parameter(param: &ParameterReflection, position: Position) -> String {
    let dollar = if param.variadic() { "...$" } else { "$" };
    match position {
        Position::Index(i) => format!("Parameter #{i} {dollar}{}", param.name()),
        Position::Name(name) if param.variadic() => {
            format!(
                "Named argument {name} for variadic parameter {dollar}{}",
                param.name()
            )
        }
        _ => format!("Parameter {dollar}{}", param.name()),
    }
}

pub(crate) struct ParametersCheck<'a> {
    pub callee: Callee<'a>,
    pub sig: &'a FunctionReflection,
    pub args: &'a [Arg],
    pub locals: &'a Locals,
    /// The class `self`/`static` in parameter types resolve against.
    pub defining_class: Option<&'a str>,
    /// Whether the callee is a PHP builtin: a builtin's variadic takes no
    /// unknown named argument.
    pub is_builtin: bool,
    pub span: Span,
}

impl<'a> Checker<'a> {
    /// `FunctionCallParametersCheck::check`.
    pub(crate) fn check_parameters(&mut self, check: ParametersCheck) {
        let ParametersCheck {
            callee,
            sig,
            args,
            locals,
            defining_class,
            is_builtin,
            span,
        } = check;
        let params = sig.params();
        let variadic = params.last().is_some_and(|p| p.variadic());
        let min = params
            .iter()
            .filter(|p| !p.has_default() && !p.variadic())
            .count();
        let max = if variadic { None } else { Some(params.len()) };
        let supports =
            |feature: PhpFeature| self.registry.php_supports_everywhere(feature) != Some(false);
        let named_after_unpack = supports(PhpFeature::NamedArgumentAfterUnpackedArgument);
        let named_supported = supports(PhpFeature::NamedArguments);

        let mut arguments: Vec<Argument> = Vec::new();
        let mut has_named = false;
        let mut has_unpacked = false;
        for arg in args {
            if arg.name.is_some() {
                has_named = true;
            }
            if has_named && arg.spread {
                self.report_non_ignorable(
                    "Named argument cannot be followed by an unpacked (...) argument.".to_string(),
                    arg.value.span,
                    "argument.unpackAfterNamed",
                );
            }
            if has_unpacked && !arg.spread && (arg.name.is_none() || !named_after_unpack) {
                self.report_non_ignorable(
                    "Unpacked argument (...) cannot be followed by a non-unpacked argument."
                        .to_string(),
                    arg.value.span,
                    "argument.nonUnpackAfterUnpacked",
                );
            }
            if arg.spread {
                has_unpacked = true;
                let ty = self.infer_expr(&arg.value, locals);
                let unpacked = Argument {
                    value: &arg.value,
                    ty: Some(ty.array_value_type().unwrap_or(Type::mixed())),
                    unpack: true,
                    name: None,
                    span: arg.value.span,
                };
                match self.unpacked_constant_array_arguments(arg, &ty, &mut has_named) {
                    Some(items) => {
                        arguments.extend(items);
                        // Only when no argument at all precedes it, as in
                        // PHPStan.
                        if arguments.is_empty() && ty.is_non_empty_array().yes() {
                            arguments.push(unpacked);
                        }
                    }
                    None => arguments.push(unpacked),
                }
                continue;
            }
            arguments.push(Argument {
                value: &arg.value,
                ty: None,
                unpack: false,
                name: arg.name.as_ref().map(|n| n.to_string()),
                span: arg.value.span,
            });
        }

        if has_named && !named_supported && !matches!(callee, Callee::AttributeConstructor(_)) {
            self.report_non_ignorable(
                "Named arguments are supported only on PHP 8.0 and later.".to_string(),
                span,
                "argument.namedNotSupported",
            );
        }

        if !has_named {
            let mut invoked = arguments.len();
            if arguments.iter().any(|a| a.unpack) {
                invoked = max.map_or(min, |m| m.max(min));
            }
            let too_few = invoked < min;
            let too_many = max.is_some_and(|m| invoked > m);
            if too_few || too_many {
                let parameters = if invoked == 1 {
                    "parameter"
                } else {
                    "parameters"
                };
                let required = match max {
                    Some(m) if m == min => Some(format!("{min} required")),
                    None if too_few => Some(format!("at least {min} required")),
                    Some(m) => Some(format!("{min}-{m} required")),
                    None => None,
                };
                if let Some(required) = required {
                    // Too many arguments is `checkExtraArguments`, from
                    // level 1; too few is level 0.
                    self.report(
                        format!(
                            "{} invoked with {invoked} {parameters}, {required}.",
                            callee.invoked()
                        ),
                        span,
                        if too_few { 0 } else { 1 },
                        "arguments.count",
                    );
                }
            }
        }

        let bound = self.bind_arguments(callee, sig, &arguments, has_named, is_builtin, span);

        for (i, (argument, param)) in arguments.iter().zip(bound).enumerate() {
            let Some(param) = param else { continue };
            let arg_ty = match &argument.ty {
                Some(ty) => ty.clone(),
                None => self
                    .infer_closure_literal_in_context(argument.value, param.ty(), locals)
                    .unwrap_or_else(|| self.infer_expr(argument.value, locals)),
            };
            let param_ty = match defining_class {
                Some(c) => self.resolve_pseudo(param.ty(), c),
                None => param.ty().clone(),
            };
            if !self.rule_level_accepts(&param_ty, &arg_ty).yes() {
                let position = match &argument.name {
                    Some(name) => Position::Name(name),
                    None => Position::Index(i + 1),
                };
                self.report(
                    format!(
                        "{} of {} expects {param_ty}, {arg_ty} given.",
                        describe_parameter(param, position),
                        callee.of()
                    ),
                    argument.span,
                    5,
                    "argument.type",
                );
            }
            if !param.by_ref() {
                continue;
            }
            let position = match argument.name {
                Some(_) => Position::Unnamed,
                None => Position::Index(i + 1),
            };
            if contains_nullsafe_access(argument.value)
                || (!argument.unpack && !is_valid_byref_argument_expr(argument.value))
            {
                self.report(
                    callee.by_ref_message(&describe_parameter(param, position)),
                    argument.span,
                    5,
                    "argument.byRef",
                );
            }
        }
    }

    /// The arguments a constant array unpacked into a call stands for
    /// (`FunctionCallParametersCheck::check`): one per position every
    /// array the type can be has, its string key naming the parameter;
    /// an optional position with no string key is skipped. `None` when
    /// the type is not constant arrays.
    fn unpacked_constant_array_arguments<'e>(
        &self,
        arg: &'e Arg,
        ty: &Type,
        has_named: &mut bool,
    ) -> Option<Vec<Argument<'e>>> {
        let shapes: Vec<&Shape> = ty
            .union_members()
            .iter()
            .map(Type::as_array_shape)
            .collect::<Option<Vec<_>>>()?;
        let unsealed = shapes.iter().any(|shape| !shape.is_sealed());
        let size = if unsealed {
            0
        } else {
            shapes
                .iter()
                .map(|shape| shape.fields.len())
                .min()
                .unwrap_or(0)
        };
        let mut out = Vec::new();
        for j in 0..size {
            let mut value_ty: Option<Type> = None;
            let mut common_key: Option<Option<&ShapeKey>> = None;
            let mut optional = false;
            for shape in &shapes {
                let field = &shape.fields[j];
                optional = field.optional;
                value_ty = Some(match value_ty {
                    Some(t) => Type::union(t, field.ty.clone()),
                    None => field.ty.clone(),
                });
                common_key = match common_key {
                    None => Some(Some(&field.key)),
                    Some(Some(k)) if *k == field.key => Some(Some(k)),
                    _ => Some(None),
                };
            }
            let name = match common_key.flatten() {
                Some(ShapeKey::Str(name)) => Some(name.clone()),
                _ => None,
            };
            if name.is_some() {
                *has_named = true;
            }
            if optional && name.is_none() {
                continue;
            }
            out.push(Argument {
                value: &arg.value,
                ty: value_ty,
                unpack: false,
                name,
                span: arg.value.span,
            });
        }
        Some(out)
    }

    /// `FunctionCallParametersCheck::processArguments`: the parameter each
    /// argument binds to (`None` for one that binds to nothing), reporting
    /// unknown, duplicate, misplaced and missing arguments.
    #[allow(clippy::too_many_arguments)]
    fn bind_arguments<'s>(
        &mut self,
        callee: Callee,
        sig: &'s FunctionReflection,
        arguments: &[Argument],
        has_named: bool,
        is_builtin: bool,
        span: Span,
    ) -> Vec<Option<&'s ParameterReflection>> {
        let params = sig.params();
        let variadic = params.last().is_some_and(|p| p.variadic());
        let natively_variadic = params.iter().any(|p| p.variadic());
        let by_name: FxHashMap<&str, &ParameterReflection> =
            params.iter().map(|p| (p.name().as_ref(), p)).collect();
        let mut unused: Vec<&ParameterReflection> =
            params.iter().filter(|p| !p.variadic()).collect();
        let mut variadic_names: FxHashSet<&str> = FxHashSet::default();
        let mut named_occurred = false;
        let mut bound = Vec::with_capacity(arguments.len());
        for (i, argument) in arguments.iter().enumerate() {
            let param = match &argument.name {
                None => match params.get(i) {
                    Some(p) => p,
                    None => match params.last() {
                        Some(last) if variadic && last.variadic() => last,
                        _ => {
                            bound.push(None);
                            bound.resize(arguments.len(), None);
                            break;
                        }
                    },
                },
                Some(name) => {
                    named_occurred = true;
                    match by_name.get(name.as_str()) {
                        Some(p) => p,
                        None => {
                            if !natively_variadic || is_builtin {
                                self.report(
                                    format!(
                                        "Unknown parameter ${name} in call to {}.",
                                        callee.target()
                                    ),
                                    argument.span,
                                    0,
                                    "argument.unknown",
                                );
                                bound.push(None);
                                continue;
                            }
                            if !variadic_names.insert(name.as_str()) {
                                self.report(
                                    format!(
                                        "Named parameter ${name} overwrites previous argument."
                                    ),
                                    argument.span,
                                    0,
                                    "argument.duplicate",
                                );
                            }
                            &params[params.len() - 1]
                        }
                    }
                }
            };
            if named_occurred && argument.name.is_none() && !argument.unpack {
                self.report_non_ignorable(
                    "Named argument cannot be followed by a positional argument.".to_string(),
                    argument.span,
                    "argument.positionalAfterNamed",
                );
                bound.push(None);
                continue;
            }
            bound.push(Some(param));
            if !param.variadic() {
                match unused.iter().position(|p| std::ptr::eq(*p, param)) {
                    Some(at) => {
                        unused.remove(at);
                    }
                    None if has_named => {
                        self.report(
                            format!(
                                "Argument for parameter ${} has already been passed.",
                                param.name()
                            ),
                            argument.span,
                            0,
                            "argument.duplicate",
                        );
                    }
                    None => {}
                }
            }
        }
        if has_named {
            for param in unused {
                if param.has_default() {
                    continue;
                }
                self.report(
                    format!(
                        "Missing parameter ${} ({}) in call to {}.",
                        param.name(),
                        param.ty().describe_type_only(),
                        callee.target()
                    ),
                    span,
                    0,
                    "argument.missing",
                );
            }
        }
        bound
    }

    fn report_non_ignorable(&mut self, message: String, span: Span, identifier: &'static str) {
        self.report_builder(DiagnosticBuilder::new(message, span, 0, identifier).non_ignorable());
    }
}
