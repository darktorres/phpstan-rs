//! A declaration's attributes, as reflection exposes them: each name
//! resolved to its class, each argument evaluated where it's a constant
//! expression.

use std::sync::Arc;

use crate::registry::name_context::class_ref;
use syntax::ast::{Arg, Attribute, BinOp, Expr, ExprKind, Literal, StaticTarget, UnaryOp};
use syntax::intern::intern;

/// One attribute on a declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ReflectionAttribute {
    name: Arc<str>,
    arguments: Vec<AttributeArgument>,
}

/// One argument an attribute is given: positional, or named.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeArgument {
    pub name: Option<Arc<str>>,
    pub value: AttributeValue,
}

/// An argument's value, as far as it's a constant expression.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(Arc<str>),
    /// `Foo::class`, the class resolved.
    ClassName(Arc<str>),
    /// `Foo::BAR`, the class resolved.
    ClassConstant {
        class: Arc<str>,
        name: Arc<str>,
    },
    /// A global constant, as written.
    Constant(Arc<str>),
    /// `[k => v, ...]`, each key `None` when positional.
    Array(Vec<(Option<AttributeValue>, AttributeValue)>),
    /// `A | B | ...`, flattened.
    BitOr(Vec<AttributeValue>),
    /// Anything else (a variable, a call, a spread argument).
    Unsupported,
}

impl ReflectionAttribute {
    /// `attribute` as written, its names resolved.
    pub(crate) fn from_ast(attribute: &Attribute) -> ReflectionAttribute {
        ReflectionAttribute {
            name: class_ref(&attribute.name),
            arguments: attribute.args.iter().map(argument).collect(),
        }
    }

    /// Every attribute in `attributes`.
    pub(crate) fn all(attributes: &[Attribute]) -> Vec<ReflectionAttribute> {
        attributes
            .iter()
            .map(ReflectionAttribute::from_ast)
            .collect()
    }

    /// The attribute class's fully qualified name.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn name_arc(&self) -> &Arc<str> {
        &self.name
    }

    /// The name's last segment.
    pub fn short_name(&self) -> &str {
        self.name.rsplit('\\').next().unwrap_or(&self.name)
    }

    pub fn arguments(&self) -> &[AttributeArgument] {
        &self.arguments
    }

    /// The argument for parameter `name`, at `position` when positional.
    pub fn argument(&self, position: usize, name: &str) -> Option<&AttributeValue> {
        self.arguments
            .iter()
            .find(|a| a.name.as_deref() == Some(name))
            .or_else(|| {
                self.arguments
                    .iter()
                    .take_while(|a| a.name.is_none())
                    .nth(position)
            })
            .map(|a| &a.value)
    }
}

fn argument(arg: &Arg) -> AttributeArgument {
    AttributeArgument {
        name: arg.name.as_deref().map(intern),
        value: if arg.spread {
            AttributeValue::Unsupported
        } else {
            value(&arg.value)
        },
    }
}

fn value(expr: &Expr) -> AttributeValue {
    let expr = expr.peeled();
    syntax::stack::guard(|| match &expr.kind {
        ExprKind::Literal(Literal::Null) => AttributeValue::Null,
        ExprKind::Literal(Literal::Bool(b)) => AttributeValue::Bool(*b),
        ExprKind::Literal(Literal::Int(i)) => AttributeValue::Int(*i),
        ExprKind::Literal(Literal::Float(f)) => AttributeValue::Float(*f),
        ExprKind::Literal(Literal::String(s)) => {
            AttributeValue::String(Arc::from(&*s.to_str_lossy()))
        }
        ExprKind::Unary(UnaryOp::Neg, inner) => match value(inner) {
            AttributeValue::Int(i) => AttributeValue::Int(i.wrapping_neg()),
            AttributeValue::Float(f) => AttributeValue::Float(-f),
            _ => AttributeValue::Unsupported,
        },
        ExprKind::Name(name) => match name.to_ascii_lowercase().trim_start_matches('\\') {
            "true" => AttributeValue::Bool(true),
            "false" => AttributeValue::Bool(false),
            "null" => AttributeValue::Null,
            _ => AttributeValue::Constant(intern(name)),
        },
        ExprKind::ClassNameFetch(StaticTarget::Name(class, _)) => {
            AttributeValue::ClassName(class_ref(class))
        }
        ExprKind::ClassConstFetch(StaticTarget::Name(class, _), name) => {
            AttributeValue::ClassConstant {
                class: class_ref(class),
                name: intern(name),
            }
        }
        ExprKind::ArrayLiteral(items) => {
            if items.iter().any(|i| i.spread) {
                return AttributeValue::Unsupported;
            }
            AttributeValue::Array(
                items
                    .iter()
                    .map(|i| (i.key.as_ref().map(value), value(&i.value)))
                    .collect(),
            )
        }
        ExprKind::Binary(BinOp::BitOr, left, right) => {
            let mut operands = Vec::new();
            for side in [left, right] {
                match value(side) {
                    AttributeValue::BitOr(inner) => operands.extend(inner),
                    other => operands.push(other),
                }
            }
            AttributeValue::BitOr(operands)
        }
        _ => AttributeValue::Unsupported,
    })
}
