//! The native declaration a type can be written as: PHP's own type
//! syntax, as far as the configured version allows.

use crate::{PhpVersion, Repr, Type};

impl Type {
    /// The native declaration this type can be written as on `version` (a
    /// return type when `for_return`, where `void`, `never` and `static` are
    /// allowed); `None` when some part has no native spelling or the
    /// version can't declare it.
    pub fn native_declaration(
        &self,
        for_return: bool,
        version: Option<PhpVersion>,
    ) -> Option<String> {
        let position = if for_return {
            Position::Return
        } else {
            Position::Parameter
        };
        let mut native = Native::default();
        if !collect(self, position, &mut native) {
            return None;
        }
        render(native, version)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Parameter,
    Return,
}

/// A native type as a set of atoms: keywords and class names, with
/// intersections as `A&B`.
#[derive(Default)]
struct Native {
    atoms: Vec<String>,
}

impl Native {
    fn add(&mut self, atom: &str) {
        if !self.atoms.iter().any(|a| a.eq_ignore_ascii_case(atom)) {
            self.atoms.push(atom.to_string());
        }
    }
}

/// Collects `ty`'s native atoms; `false` when some part has no native
/// spelling.
fn collect(ty: &Type, position: Position, out: &mut Native) -> bool {
    syntax::stack::guard(|| {
        match ty.repr() {
            Repr::Int | Repr::IntLiteral(_) | Repr::IntRange(..) => out.add("int"),
            Repr::Float | Repr::FloatLiteral(_) => out.add("float"),
            Repr::String
            | Repr::StringLiteral(_)
            | Repr::ClassString(_)
            | Repr::ClassKindString(_) => out.add("string"),
            Repr::Bool => out.add("bool"),
            Repr::True => out.add("true"),
            Repr::False => out.add("false"),
            Repr::Null => out.add("null"),
            Repr::Array(..) | Repr::ArrayShape(_) => out.add("array"),
            Repr::Callable(_) => out.add("callable"),
            Repr::ObjectShape(_) | Repr::ObjectWithoutClass => out.add("object"),
            Repr::Mixed(..) => out.add("mixed"),
            Repr::Void if position == Position::Return => out.add("void"),
            Repr::Never if position == Position::Return => out.add("never"),
            Repr::StaticRef(_) | Repr::This if position == Position::Return => out.add("static"),
            Repr::SelfRef(_) => out.add("self"),
            Repr::ParentRef => out.add("parent"),
            // Fully qualified, so the declaration names the same class
            // wherever it is written.
            Repr::EnumCase(class, _) | Repr::Object(class, _) => out.add(&fully_qualified(class)),
            Repr::TemplateParam(param) => return collect(&param.bound, position, out),
            Repr::Union(members) | Repr::BenevolentUnion(members) => {
                return members.iter().all(|m| collect(m, position, out));
            }
            Repr::Intersection(members) => {
                let bases: Vec<&Type> = members.iter().filter(|m| !m.is_accessory()).collect();
                if bases.len() == 1 {
                    return bases.iter().all(|b| collect(b, position, out));
                }
                let mut classes = Vec::new();
                for base in bases {
                    let Repr::Object(class, _) = base.repr() else {
                        return false;
                    };
                    classes.push(fully_qualified(class));
                }
                out.add(&classes.join("&"));
            }
            _ => return false,
        }
        true
    })
}

/// `class` with a leading `\`.
fn fully_qualified(class: &str) -> String {
    format!("\\{}", class.trim_start_matches('\\'))
}

/// Renders `native` for `version`, or `None` when that version can't
/// declare it.
fn render(mut native: Native, version: Option<PhpVersion>) -> Option<String> {
    let at_least = |v: PhpVersion| version.is_some_and(|configured| configured >= v);
    let atoms = &mut native.atoms;
    if atoms.iter().any(|a| a == "mixed") {
        return at_least(PhpVersion::new(8, 0)).then(|| "mixed".to_string());
    }
    if atoms.len() > 1 && atoms.iter().any(|a| a == "void" || a == "never") {
        return None;
    }
    if atoms.iter().any(|a| a == "true") && atoms.iter().any(|a| a == "false") {
        atoms.retain(|a| a != "true" && a != "false");
        atoms.push("bool".to_string());
    }
    if atoms.iter().any(|a| a == "bool") {
        atoms.retain(|a| a != "true" && a != "false");
    }
    if atoms.iter().any(|a| a == "array") && atoms.iter().any(|a| a == "\\Traversable") {
        atoms.retain(|a| a != "array" && a != "\\Traversable");
        atoms.push("iterable".to_string());
    }
    if atoms.iter().any(|a| a == "object") {
        atoms.retain(|a| {
            matches!(
                a.as_str(),
                "object"
                    | "int"
                    | "float"
                    | "string"
                    | "bool"
                    | "true"
                    | "false"
                    | "null"
                    | "array"
                    | "iterable"
                    | "callable"
            )
        });
    }
    let nullable = atoms.iter().any(|a| a == "null");
    let others: Vec<&String> = atoms.iter().filter(|a| *a != "null").collect();
    let has_intersection = others.iter().any(|a| a.contains('&'));
    let needs = |feature: PhpVersion| at_least(feature);
    match others.as_slice() {
        [] => needs(PhpVersion::new(8, 2)).then(|| "null".to_string()),
        [only] => {
            let standalone_literal = matches!(only.as_str(), "true" | "false");
            if has_intersection {
                if nullable {
                    return needs(PhpVersion::new(8, 2)).then(|| format!("({only})|null"));
                }
                return needs(PhpVersion::new(8, 1)).then(|| only.to_string());
            }
            if (standalone_literal && !needs(PhpVersion::new(8, 2)))
                || (only.as_str() == "static" && !needs(PhpVersion::new(8, 0)))
                || (only.as_str() == "never" && !needs(PhpVersion::new(8, 1)))
                || (only.as_str() == "object" && !needs(PhpVersion::new(7, 2)))
            {
                return None;
            }
            Some(if nullable {
                format!("?{only}")
            } else {
                only.to_string()
            })
        }
        many => {
            if !needs(PhpVersion::new(8, 0)) || (has_intersection && !needs(PhpVersion::new(8, 2)))
            {
                return None;
            }
            if many.iter().any(|a| a.as_str() == "true") && !needs(PhpVersion::new(8, 2)) {
                return None;
            }
            let mut parts: Vec<String> = many
                .iter()
                .map(|a| {
                    if a.contains('&') {
                        format!("({a})")
                    } else {
                        a.to_string()
                    }
                })
                .collect();
            if nullable {
                parts.push("null".to_string());
            }
            Some(parts.join("|"))
        }
    }
}
