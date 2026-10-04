//! A type's canonical print (`syntax::canonical`): one line that shows
//! every part of its representation, where `Display` shows what PHPStan's
//! messages show. A union's and an intersection's members are sorted by
//! their prints, since their order is not part of the type, and nothing
//! is read from a node's interning: equal representations print alike,
//! whichever nodes hold them.
//!
//! - `mixed(explicit)`, `mixed(implicit)`, with `, minus T` for what is
//!   subtracted; `*error*`;
//! - keywords for the plain types, a literal int as its digits, a literal
//!   string quoted with its bytes (`syntax::canonical::quote_bytes`), a
//!   literal float as `float(1.5)`;
//! - a class's name after a `\`, so that no class prints as a keyword;
//! - a template as `template(T, scope: S, bound: B, variance: V, default: D)`;
//! - a shape with each field's optionality, and the tail of an unsealed one
//!   as `...<K, V>`.

#![deny(clippy::wildcard_enum_match_arm)]

use std::fmt::Write as _;

use syntax::canonical::{Canonical, Printer, quote_bytes, write_name};
use syntax::name::Name;

use crate::{
    AccessoryKind, CallSiteVariance, CallableParam, CallableSig, ClassKind, ConditionalSubject,
    Repr, ShapeKey, TemplateParam, TrinaryLogic, Type, Variance,
};

impl Type {
    /// The type's canonical print; see the module's doc comment.
    pub fn canonical_text(&self) -> String {
        let mut out = String::new();
        write_type(self, &mut out);
        out
    }
}

impl Canonical for Type {
    fn canonical(&self, p: &mut Printer) {
        p.atom(&self.canonical_text());
    }
}

impl Canonical for TemplateParam {
    fn canonical(&self, p: &mut Printer) {
        let mut out = String::new();
        write_template(self, &mut out);
        p.atom(&out);
    }
}

impl Canonical for ShapeKey {
    fn canonical(&self, p: &mut Printer) {
        let mut out = String::new();
        self.write_canonical(&mut out);
        p.atom(&out);
    }
}

impl Canonical for Variance {
    fn canonical(&self, p: &mut Printer) {
        p.atom(variance_name(*self));
    }
}

impl Canonical for TrinaryLogic {
    fn canonical(&self, p: &mut Printer) {
        p.atom(trinary_name(*self));
    }
}

pub(crate) fn write_type(ty: &Type, out: &mut String) {
    syntax::stack::guard(|| write_repr(ty.repr(), out));
}

fn write_repr(repr: &Repr, out: &mut String) {
    match repr {
        Repr::Mixed(explicit, subtracted) => {
            out.push_str(if *explicit {
                "mixed(explicit"
            } else {
                "mixed(implicit"
            });
            if let Some(subtracted) = subtracted {
                out.push_str(", minus ");
                write_type(subtracted, out);
            }
            out.push(')');
        }
        Repr::Error => out.push_str("*error*"),
        Repr::Never => out.push_str("never"),
        Repr::Void => out.push_str("void"),
        Repr::Null => out.push_str("null"),
        Repr::Bool => out.push_str("bool"),
        Repr::True => out.push_str("true"),
        Repr::False => out.push_str("false"),
        Repr::Int => out.push_str("int"),
        Repr::IntLiteral(n) => {
            let _ = write!(out, "{n}");
        }
        Repr::Float => out.push_str("float"),
        Repr::String => out.push_str("string"),
        Repr::StringLiteral(s) => quote_bytes(s.as_bytes(), out),
        Repr::Array(key, value) => {
            out.push_str("array<");
            write_type(key, out);
            out.push_str(", ");
            write_type(value, out);
            out.push('>');
        }
        Repr::ClassString(inner) => {
            out.push_str("class-string<");
            write_type(inner, out);
            out.push('>');
        }
        Repr::ClassKindString(kind) => {
            out.push_str("kind-string(");
            out.push_str(match kind {
                ClassKind::Class => "class",
                ClassKind::Interface => "interface",
                ClassKind::Trait => "trait",
                ClassKind::Enum => "enum",
            });
            out.push(')');
        }
        Repr::ArrayShape(shape) => shape.write_canonical("array", out),
        Repr::ObjectShape(shape) => shape.write_canonical("object", out),
        Repr::Object(name, args) => {
            write_class(name, out);
            write_args(args, out);
        }
        Repr::ObjectWithoutClass => out.push_str("object"),
        Repr::Resource { closed } => out.push_str(if *closed {
            "closed-resource"
        } else {
            "resource"
        }),
        Repr::EnumCase(class, case) => {
            write_class(class, out);
            out.push_str("::");
            write_name(case, out);
        }
        Repr::Callable(None) => out.push_str("callable"),
        Repr::Callable(Some(sig)) => write_callable(sig, out),
        Repr::SelfRef(args) => {
            out.push_str("self");
            write_args(args, out);
        }
        Repr::ParentRef => out.push_str("parent"),
        Repr::StaticRef(args) => {
            out.push_str("static");
            write_args(args, out);
        }
        Repr::This => out.push_str("$this"),
        Repr::TemplateParam(tp) => write_template(tp, out),
        Repr::VarianceArg(variance, inner) => {
            out.push_str(match variance {
                CallSiteVariance::Covariant => "covariant ",
                CallSiteVariance::Contravariant => "contravariant ",
                CallSiteVariance::Bivariant => "bivariant ",
            });
            write_type(inner, out);
        }
        Repr::Union(members) => write_members("union", "(", " | ", members, out),
        Repr::BenevolentUnion(members) => {
            write_members("benevolent", "benevolent(", " | ", members, out)
        }
        Repr::Intersection(members) => write_members("intersection", "(", " & ", members, out),
        Repr::IntRange(min, max) => {
            out.push_str("int<");
            match min {
                Some(min) => {
                    let _ = write!(out, "{min}");
                }
                None => out.push_str("min"),
            }
            out.push_str(", ");
            match max {
                Some(max) => {
                    let _ = write!(out, "{max}");
                }
                None => out.push_str("max"),
            }
            out.push('>');
        }
        Repr::FloatLiteral(f) => {
            // `{:?}` prints the shortest text that reads back as the same
            // bits, `-0.0` and the infinities included; a NaN's payload
            // only its bits show.
            if f.is_nan() {
                let _ = write!(out, "float(NaN 0x{:016x})", f.to_bits());
            } else {
                let _ = write!(out, "float({f:?})");
            }
        }
        Repr::Accessory(kind) => out.push_str(accessory_name(*kind)),
        Repr::HasOffsetValue(key, value) => {
            out.push_str("has-offset-value(");
            key.write_canonical(out);
            out.push_str(", ");
            write_type(value, out);
            out.push(')');
        }
        Repr::HasOffset(key) => {
            out.push_str("has-offset(");
            key.write_canonical(out);
            out.push(')');
        }
        Repr::HasMethod(name) => {
            out.push_str("has-method(");
            write_name(name, out);
            out.push(')');
        }
        Repr::HasProperty(name) => {
            out.push_str("has-property(");
            write_name(name, out);
            out.push(')');
        }
        Repr::ReflectedMember(owner, member) => {
            out.push_str("reflected-member(");
            write_type(owner, out);
            out.push_str(", ");
            write_name(member, out);
            out.push(')');
        }
        Repr::ValueOf(inner) => {
            out.push_str("value-of<");
            write_type(inner, out);
            out.push('>');
        }
        Repr::KeyOf(inner) => {
            out.push_str("key-of<");
            write_type(inner, out);
            out.push('>');
        }
        Repr::OffsetAccess(base, offset) => {
            out.push_str("offset-access(");
            write_type(base, out);
            out.push_str(", ");
            write_type(offset, out);
            out.push(')');
        }
        Repr::ClassConstant { class, pattern } => {
            out.push_str("class-constant(");
            write_type(class, out);
            out.push_str(", ");
            write_name(pattern, out);
            out.push(')');
        }
        Repr::Conditional {
            subject,
            target,
            if_true,
            if_false,
            negated,
        } => {
            out.push('(');
            match subject {
                ConditionalSubject::Parameter(name) => {
                    out.push_str("parameter $");
                    write_name(name, out);
                }
                ConditionalSubject::Template(name) => {
                    out.push_str("template ");
                    write_name(name, out);
                }
            }
            out.push_str(if *negated { " is not " } else { " is " });
            write_type(target, out);
            out.push_str(" ? ");
            write_type(if_true, out);
            out.push_str(" : ");
            write_type(if_false, out);
            out.push(')');
        }
        Repr::Extension(data) => {
            out.push_str("extension(");
            data.write_canonical(out);
            out.push(')');
        }
    }
}

/// A class's name, after a `\`.
fn write_class(name: &Name, out: &mut String) {
    out.push('\\');
    write_name(name, out);
}

/// `<A, B>` for a non-empty list of type arguments.
fn write_args(args: &[Type], out: &mut String) {
    if args.is_empty() {
        return;
    }
    out.push('<');
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_type(arg, out);
    }
    out.push('>');
}

/// `members` between `open` and `)`, sorted by their prints; with fewer
/// than two, which no separator then tells apart, between `kind(` and `)`.
fn write_members(kind: &str, open: &str, separator: &str, members: &[Type], out: &mut String) {
    let mut printed: Vec<String> = members.iter().map(Type::canonical_text).collect();
    printed.sort_unstable();
    if printed.len() < 2 {
        out.push_str(kind);
        out.push('(');
    } else {
        out.push_str(open);
    }
    out.push_str(&printed.join(separator));
    out.push(')');
}

fn write_template(tp: &TemplateParam, out: &mut String) {
    let TemplateParam {
        name,
        bound,
        variance,
        scope,
        default,
    } = tp;
    out.push_str("template(");
    write_name(name, out);
    out.push_str(", scope: ");
    match scope {
        Some(scope) => write_name(scope, out),
        None => out.push('-'),
    }
    out.push_str(", bound: ");
    write_type(bound, out);
    out.push_str(", variance: ");
    out.push_str(variance_name(*variance));
    out.push_str(", default: ");
    match default {
        Some(default) => write_type(default, out),
        None => out.push('-'),
    }
    out.push(')');
}

/// `callable<templates>(params): return`, each parameter with `&` before
/// it when by reference, `...` after it when variadic and `=` when
/// optional.
fn write_callable(sig: &CallableSig, out: &mut String) {
    let CallableSig {
        params,
        return_type,
        templates,
    } = sig;
    out.push_str("callable");
    if !templates.is_empty() {
        out.push('<');
        for (i, tp) in templates.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            write_template(tp, out);
        }
        out.push('>');
    }
    out.push('(');
    for (i, param) in params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let CallableParam {
            ty,
            variadic,
            optional,
            by_ref,
        } = param;
        if *by_ref {
            out.push('&');
        }
        write_type(ty, out);
        if *variadic {
            out.push_str("...");
        }
        if *optional {
            out.push('=');
        }
    }
    out.push_str("): ");
    write_type(return_type, out);
}

fn variance_name(variance: Variance) -> &'static str {
    match variance {
        Variance::Invariant => "invariant",
        Variance::Covariant => "covariant",
        Variance::Contravariant => "contravariant",
    }
}

pub(crate) fn trinary_name(value: TrinaryLogic) -> &'static str {
    match value {
        TrinaryLogic::Yes => "yes",
        TrinaryLogic::Maybe => "maybe",
        TrinaryLogic::No => "no",
    }
}

fn accessory_name(kind: AccessoryKind) -> &'static str {
    match kind {
        AccessoryKind::NonEmptyString => "non-empty-string",
        AccessoryKind::NonFalsyString => "non-falsy-string",
        AccessoryKind::LiteralString => "literal-string",
        AccessoryKind::NumericString => "numeric-string",
        AccessoryKind::ArrayList => "list",
        AccessoryKind::NonEmptyArray => "non-empty-array",
        AccessoryKind::LowercaseString => "lowercase-string",
        AccessoryKind::UppercaseString => "uppercase-string",
        AccessoryKind::DecimalIntegerString => "decimal-int-string",
        AccessoryKind::NonDecimalIntegerString => "non-decimal-int-string",
    }
}
