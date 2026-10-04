//! A declaration's attributes, as reflection exposes them: each name
//! resolved to its class, each argument evaluated where it's a constant
//! expression.

use syntax::name::Name;

use crate::registry::name_context::class_ref;
use syntax::ast::{Arg, Attribute, BinOp, Expr, ExprKind, Literal, StaticTarget, UnaryOp};

/// One attribute on a declaration.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReflectionAttribute {
    name: Name,
    arguments: Vec<AttributeArgument>,
}

/// One argument an attribute is given: positional, or named.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AttributeArgument {
    pub name: Option<Name>,
    pub value: AttributeValue,
}

/// An argument's value, as far as it's a constant expression.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AttributeValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(Name),
    /// `Foo::class`, the class resolved.
    ClassName(Name),
    /// `Foo::BAR`, the class resolved.
    ClassConstant {
        class: Name,
        name: Name,
    },
    /// A global constant, as written.
    Constant(Name),
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

    pub(crate) fn name_arc(&self) -> &Name {
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
        name: arg.name.as_deref().map(Name::from),
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
            AttributeValue::String(Name::from(&*s.to_str_lossy()))
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
            _ => AttributeValue::Constant(name.written.clone()),
        },
        ExprKind::ClassNameFetch(StaticTarget::Name(class)) => {
            AttributeValue::ClassName(class_ref(class))
        }
        ExprKind::ClassConstFetch(StaticTarget::Name(class), name) => {
            AttributeValue::ClassConstant {
                class: class_ref(class),
                name: name.name.clone(),
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

syntax::canonical_struct!(ReflectionAttribute { name, arguments });

syntax::canonical_struct!(AttributeArgument { name, value });

syntax::canonical_enum!(AttributeValue {
    Null,
    Bool(value),
    Int(value),
    Float(value),
    String(value),
    ClassName(class),
    ClassConstant { class, name },
    Constant(name),
    Array(entries),
    BitOr(members),
    Unsupported,
});
