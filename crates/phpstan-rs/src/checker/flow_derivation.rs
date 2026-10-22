//! Where a derived value comes from: an expression that computes its value
//! from its operands' values (arithmetic, a cast, a string built from
//! parts, a builtin that passes its argument's value through) comes from
//! wherever they came from. Its type is given their data-flow sets here,
//! where the checker infers it, so no type operation has to.

use syntax::ast::*;
use syntax::span::Span;
use types::{Flow, Type};

use crate::checker::flow_nodes::{FlowNode, Owner, Site, flow_of};
use crate::checker::{Checker, Locals};

/// Builtins whose result is made of their arguments' values: converted,
/// trimmed, cut, combined, decoded. One that measures its argument
/// (`strlen`, `count`) is not here: a size is bounded by what the process
/// holds, not chosen by where the value came from.
const PASS_THROUGH_BUILTINS: &[&str] = &[
    "abs",
    "array_product",
    "array_sum",
    "base64_decode",
    "bin2hex",
    "bindec",
    "boolval",
    "ceil",
    "explode",
    "floatval",
    "floor",
    "fmod",
    "hex2bin",
    "hexdec",
    "html_entity_decode",
    "htmlspecialchars_decode",
    "implode",
    "intdiv",
    "intval",
    "join",
    "json_decode",
    "lcfirst",
    "ltrim",
    "max",
    "min",
    "octdec",
    "rawurldecode",
    "round",
    "rtrim",
    "sprintf",
    "str_pad",
    "str_repeat",
    "str_replace",
    "str_split",
    "strrev",
    "strtolower",
    "strtoupper",
    "strval",
    "substr",
    "trim",
    "ucfirst",
    "ucwords",
    "urldecode",
    "vsprintf",
];

impl Checker<'_> {
    /// `ty`, the type of `e`, with the data-flow sets of the operands `e`
    /// derives its value from, or of the place it reads it from.
    pub(crate) fn with_derived_flow(&self, e: &Expr, ty: Type, locals: &Locals) -> Type {
        let flow = self.derived_flow(e, &ty, locals);
        if flow.is_empty() {
            ty
        } else {
            ty.with_flow(&flow)
        }
    }

    fn derived_flow(&self, e: &Expr, ty: &Type, locals: &Locals) -> Flow {
        let of = |operand: &Expr| self.infer_expr(operand, locals).deep_flow();
        let of_args = |args: &[Arg]| {
            args.iter()
                .fold(Flow::default(), |acc, arg| acc.union(&of(&arg.value)))
        };
        match &e.kind {
            ExprKind::Binary(op, l, r) if derives(*op) => of(l).union(&of(r)),
            ExprKind::Unary(op, inner) if *op != UnaryOp::Not => of(inner),
            ExprKind::Cast(spelling, inner)
                if !matches!(
                    spelling,
                    CastSpelling::Bool
                        | CastSpelling::Boolean
                        | CastSpelling::Unset
                        | CastSpelling::Void
                        | CastSpelling::Object
                ) =>
            {
                of(inner)
            }
            ExprKind::Interpolated(parts) => parts
                .iter()
                .fold(Flow::default(), |acc, part| acc.union(&of(part))),
            ExprKind::CompoundAssign(op, target, value) if derives(*op) => {
                of(target).union(&of(value))
            }
            ExprKind::Call(callee, args) => match &callee.kind {
                ExprKind::Name(name) => match self.registry.resolve_function(name) {
                    Some(_) if self.passes_values_through(name) => of_args(args),
                    Some((_, sig)) if sig.is_stub() => Flow::default(),
                    Some((resolved, _)) => {
                        self.call_flow(e.span, format!("{}()", resolved.trim_start_matches('\\')))
                    }
                    None => self.call_flow(e.span, format!("{name}()")),
                },
                _ => self.call_flow(e.span, "{dynamic}()".to_string()),
            },
            ExprKind::MethodCall(base, method, ..) => {
                let classes = self.infer_expr(base, locals);
                let builtin = !classes.object_class_names().is_empty()
                    && classes.object_class_names().iter().all(|class| {
                        self.registry
                            .lookup_method_with_owner(class, &method.name)
                            .is_some_and(|(owner, _)| self.registry.is_builtin_class(&owner))
                    });
                if builtin {
                    Flow::default()
                } else {
                    self.call_flow(e.span, format!("->{}()", method.name))
                }
            }
            ExprKind::DynamicMethodCall(..) => self.call_flow(e.span, "->{dynamic}()".to_string()),
            ExprKind::StaticCall(target, method, _) => {
                match self.infer_static_target_class(target, locals) {
                    Some(class)
                        if self
                            .registry
                            .lookup_method_with_owner(&class, &method.name)
                            .is_some_and(|(owner, _)| self.registry.is_builtin_class(&owner)) =>
                    {
                        Flow::default()
                    }
                    class => self.call_flow(
                        e.span,
                        format!(
                            "{}::{}()",
                            class.as_deref().unwrap_or("{dynamic}"),
                            method.name
                        ),
                    ),
                }
            }
            ExprKind::DynamicStaticCall(..) => {
                self.call_flow(e.span, "{dynamic}::{dynamic}()".to_string())
            }
            ExprKind::PropertyAccess(base, prop, _) if !ty.has_flow() => self
                .infer_expr(base, locals)
                .object_class_names()
                .iter()
                .filter_map(|class| self.property_flow(class, &prop.name))
                .fold(Flow::default(), |acc, flow| acc.union(&flow)),
            ExprKind::StaticPropertyFetch(target, prop) if !ty.has_flow() => self
                .infer_static_target_class(target, locals)
                .and_then(|class| self.property_flow(&class, &prop.name))
                .unwrap_or_default(),
            _ => Flow::default(),
        }
    }

    /// Where `span` is in the file being checked.
    pub(crate) fn flow_site(&self, span: Span) -> Site {
        let file = self
            .current_file_absolute
            .as_ref()
            .or(self.current_file.as_ref())
            .map(|path| path.to_string_lossy().into())
            .unwrap_or_else(|| "".into());
        Site {
            file,
            offset: span.start,
        }
    }

    /// The set of the parameter `name` of `owner`.
    pub(crate) fn param_flow(&self, owner: Owner, name: &str) -> Flow {
        flow_of(FlowNode::Param {
            owner,
            name: name.into(),
        })
    }

    fn call_flow(&self, span: Span, callee: String) -> Flow {
        flow_of(FlowNode::Call {
            site: self.flow_site(span),
            callee: callee.into(),
        })
    }

    /// The set of the property `name` read from a `class`: the class that
    /// declares it, unless a builtin class does.
    fn property_flow(&self, class: &str, name: &str) -> Option<Flow> {
        let (owner, _) = self.registry.lookup_property_with_owner(class, name)?;
        (!self.registry.is_builtin_class(&owner)).then(|| {
            flow_of(FlowNode::Property {
                class: owner.as_ref().into(),
                name: name.into(),
            })
        })
    }

    /// Whether `name` calls a builtin whose result is made of its
    /// arguments' values.
    fn passes_values_through(&self, name: &QualifiedName) -> bool {
        let Some((resolved, sig)) = self.registry.resolve_function(name) else {
            return false;
        };
        sig.is_stub()
            && PASS_THROUGH_BUILTINS
                .binary_search(
                    &resolved
                        .trim_start_matches('\\')
                        .to_ascii_lowercase()
                        .as_str(),
                )
                .is_ok()
    }
}

/// Whether `op` computes its value from its operands' values: arithmetic,
/// bitwise, a shift, concatenation. A comparison gives a `bool`, and `??`
/// one of its operands, which keeps its own set.
fn derives(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Mod
            | BinOp::Pow
            | BinOp::Concat
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
    )
}
