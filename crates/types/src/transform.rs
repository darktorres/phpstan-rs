//! Types rewritten into related types: `static` bound, shapes unsealed,
//! a value stepped by `++`/`--`.

use crate::{Shape, Type, union_types};

impl Type {
    /// This type with `static` and `$this` as `class`, at any depth.
    pub fn bind_static(&self, class: &str) -> Type {
        match self {
            Type::StaticRef(args) => Type::Object(
                class.into(),
                args.iter().map(|a| a.bind_static(class)).collect(),
            ),
            Type::This => Type::Object(class.into(), Vec::new()),
            other => other.map_children(&mut |t| t.bind_static(class)),
        }
    }

    /// This type with each sealed array shape unsealed, so a key it
    /// doesn't list isn't proven absent.
    pub fn unsealed_shapes(&self) -> Type {
        match self {
            Type::ArrayShape(shape) if shape.is_sealed() => {
                Type::ArrayShape(Shape::unsealed(shape.fields.clone()))
            }
            Type::Union(members) => {
                Type::Union(members.iter().map(Type::unsealed_shapes).collect())
            }
            other => other.clone(),
        }
    }

    /// The type after `++`: `null` becomes `1` (an `int`), a literal or
    /// range moves up by one.
    pub fn increment_type(&self) -> Type {
        self.step(1)
    }

    /// The type after `--`: a literal or range moves down by one; `null`
    /// stays `null`.
    pub fn decrement_type(&self) -> Type {
        self.step(-1)
    }

    fn step(&self, by: i64) -> Type {
        match self {
            Type::Null if by > 0 => Type::Int,
            Type::IntLiteral(n) => Type::IntLiteral(n + by),
            Type::FloatLiteral(f) => Type::FloatLiteral(f + by as f64),
            Type::IntRange(min, max) => Type::IntRange(min.map(|v| v + by), max.map(|v| v + by)),
            Type::Union(members) => members
                .iter()
                .map(|m| m.step(by))
                .reduce(union_types)
                .unwrap_or(Type::Mixed(false)),
            other => other.clone(),
        }
    }
}
