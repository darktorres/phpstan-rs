//! Renders an expression back to PHP source for diagnostic messages, in
//! the same style as nikic/php-parser's standard pretty printer (what
//! PHPStan's `ExprPrinter` uses): single-quoted strings, spaced binary
//! operators, `[...]` arrays, and multi-line constructs (closure bodies,
//! anonymous classes, `match` arms) on their own lines.

use crate::ast::*;
use crate::php_string::PhpString;

pub fn print_expr(e: &Expr) -> String {
    let mut out = String::new();
    write_expr(&mut out, e);
    out
}

/// `print_expr`, cut to its first line with a trailing `…` when the
/// rendering spans several.
pub fn print_expr_first_line(e: &Expr) -> String {
    let printed = print_expr(e);
    match printed.split_once('\n') {
        Some((first, _)) => format!("{first}…"),
        None => printed,
    }
}

fn binary_operator(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::Concat => ".",
        BinOp::Eq => "==",
        BinOp::Identical => "===",
        BinOp::NotEq => "!=",
        BinOp::NotIdentical => "!==",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::LtEq => "<=",
        BinOp::GtEq => ">=",
        BinOp::Spaceship => "<=>",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::LogicalAnd => "and",
        BinOp::LogicalOr => "or",
        BinOp::LogicalXor => "xor",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Coalesce => "??",
        BinOp::Pipe => "|>",
    }
}

/// A string literal: single-quoted when it is text, double-quoted with
/// `\xNN` escapes for the bytes that aren't UTF-8.
fn write_php_string(out: &mut String, s: &PhpString) {
    match s.as_str() {
        Some(text) => write_single_quoted(out, text),
        None => {
            out.push('"');
            write_double_quoted_bytes(out, s.as_bytes());
            out.push('"');
        }
    }
}

/// The body of a double-quoted string: valid UTF-8 as text, every other
/// byte as `\xNN`.
fn write_double_quoted_bytes(out: &mut String, bytes: &[u8]) {
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '$' => out.push_str("\\$"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                c => out.push(c),
            }
        }
        for b in chunk.invalid() {
            out.push_str(&format!("\\x{b:02x}"));
        }
    }
}

fn write_single_quoted(out: &mut String, s: &str) {
    out.push('\'');
    for c in s.chars() {
        if c == '\'' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('\'');
}

fn write_float(out: &mut String, f: f64) {
    if f.is_infinite() {
        out.push_str(if f > 0.0 { "INF" } else { "-INF" });
    } else if f.is_nan() {
        out.push_str("NAN");
    } else if f.fract() == 0.0 && f.abs() < 1e15 {
        out.push_str(&format!("{f:.1}"));
    } else {
        out.push_str(&f.to_string());
    }
}

fn write_list<T>(out: &mut String, items: &[T], mut write: impl FnMut(&mut String, &T)) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write(out, item);
    }
}

fn write_args(out: &mut String, args: &[Arg]) {
    out.push('(');
    write_list(out, args, |out, arg| {
        if let Some(name) = &arg.name {
            out.push_str(name);
            out.push_str(": ");
        }
        if arg.spread {
            out.push_str("...");
        }
        write_expr(out, &arg.value);
    });
    out.push(')');
}

fn write_params(out: &mut String, params: &[Param]) {
    out.push('(');
    write_list(out, params, |out, p| {
        if let Some(ty) = &p.type_hint {
            out.push_str(&ty.to_string());
            out.push(' ');
        }
        if p.by_ref {
            out.push('&');
        }
        if p.variadic {
            out.push_str("...");
        }
        out.push('$');
        out.push_str(&p.name);
        if let Some(default) = &p.default {
            out.push_str(" = ");
            write_expr(out, default);
        }
    });
    out.push(')');
}

fn write_return_type(out: &mut String, return_type: Option<&TypeHint>) {
    if let Some(ty) = return_type {
        out.push_str(": ");
        out.push_str(&ty.to_string());
    }
}

/// A member name after `->`/`?->`: bare when it's a plain name, `$x` for
/// a variable, braced otherwise.
fn write_member_name(out: &mut String, name: &Expr) {
    match &name.kind {
        ExprKind::Variable(_) => write_expr(out, name),
        _ => {
            out.push('{');
            write_expr(out, name);
            out.push('}');
        }
    }
}

fn write_interpolated_parts(out: &mut String, parts: &[Expr]) {
    for part in parts {
        match &part.kind {
            ExprKind::Literal(Literal::String(s)) => write_double_quoted_bytes(out, s.as_bytes()),
            _ => {
                out.push('{');
                write_expr(out, part);
                out.push('}');
            }
        }
    }
}

fn write_static_target(out: &mut String, target: &StaticTarget) {
    match target {
        StaticTarget::Name(n, _) => out.push_str(n),
        StaticTarget::Expr(inner) => write_expr(out, inner),
    }
}

fn write_expr(out: &mut String, e: &Expr) {
    match &e.kind {
        ExprKind::Literal(lit) => match lit {
            Literal::Int(n) => out.push_str(&n.to_string()),
            Literal::Float(f) => write_float(out, *f),
            Literal::String(s) => write_php_string(out, s),
            Literal::Bool(true) => out.push_str("true"),
            Literal::Bool(false) => out.push_str("false"),
            Literal::Null => out.push_str("null"),
        },
        ExprKind::Variable(name) => {
            out.push('$');
            out.push_str(name);
        }
        ExprKind::DynamicVariable(inner) => {
            out.push_str("${");
            write_expr(out, inner);
            out.push('}');
        }
        ExprKind::ArrayLiteral(items) => {
            out.push('[');
            write_list(out, items, |out, item| {
                if item.spread {
                    out.push_str("...");
                }
                if let Some(key) = &item.key {
                    write_expr(out, key);
                    out.push_str(" => ");
                }
                write_expr(out, &item.value);
            });
            out.push(']');
        }
        ExprKind::Unary(op, operand) => {
            let (prefix, suffix) = match op {
                UnaryOp::Neg => ("-", ""),
                UnaryOp::Plus => ("+", ""),
                UnaryOp::Not => ("!", ""),
                UnaryOp::BitNot => ("~", ""),
                UnaryOp::PreInc => ("++", ""),
                UnaryOp::PreDec => ("--", ""),
                UnaryOp::PostInc => ("", "++"),
                UnaryOp::PostDec => ("", "--"),
            };
            out.push_str(prefix);
            write_expr(out, operand);
            out.push_str(suffix);
        }
        ExprKind::Binary(op, l, r) => {
            write_expr(out, l);
            out.push(' ');
            out.push_str(binary_operator(op));
            out.push(' ');
            write_expr(out, r);
        }
        ExprKind::Assign(target, value) => {
            write_expr(out, target);
            out.push_str(" = ");
            write_expr(out, value);
        }
        ExprKind::AssignRef(target, value) => {
            write_expr(out, target);
            out.push_str(" =& ");
            write_expr(out, value);
        }
        ExprKind::CompoundAssign(op, target, value) => {
            write_expr(out, target);
            out.push(' ');
            out.push_str(binary_operator(op));
            out.push_str("= ");
            write_expr(out, value);
        }
        ExprKind::Ternary(cond, then, otherwise) => {
            write_expr(out, cond);
            match then {
                Some(then) => {
                    out.push_str(" ? ");
                    write_expr(out, then);
                    out.push_str(" : ");
                }
                None => out.push_str(" ?: "),
            }
            write_expr(out, otherwise);
        }
        ExprKind::Call(callee, args) => {
            write_expr(out, callee);
            write_args(out, args);
        }
        ExprKind::Print(inner) => {
            out.push_str("print ");
            write_expr(out, inner);
        }
        ExprKind::Include(kind, path) => {
            out.push_str(kind.as_str());
            out.push(' ');
            write_expr(out, path);
        }
        ExprKind::Exit(kind, arg) => {
            out.push_str(kind.as_str());
            if let Some(arg) = arg {
                out.push('(');
                write_expr(out, arg);
                out.push(')');
            }
        }
        ExprKind::Eval(code) => {
            out.push_str("eval(");
            write_expr(out, code);
            out.push(')');
        }
        ExprKind::New(class, args) => {
            out.push_str("new ");
            write_expr(out, class);
            write_args(out, args);
        }
        ExprKind::PropertyAccess(object, name, nullsafe) => {
            write_expr(out, object);
            out.push_str(if *nullsafe { "?->" } else { "->" });
            out.push_str(name);
        }
        ExprKind::MethodCall(object, name, args, nullsafe) => {
            write_expr(out, object);
            out.push_str(if *nullsafe { "?->" } else { "->" });
            out.push_str(name);
            write_args(out, args);
        }
        ExprKind::DynamicPropertyAccess(object, name) => {
            write_expr(out, object);
            out.push_str("->");
            write_member_name(out, name);
        }
        ExprKind::DynamicMethodCall(object, name, args) => {
            write_expr(out, object);
            out.push_str("->");
            write_member_name(out, name);
            write_args(out, args);
        }
        ExprKind::ClassConstFetch(target, name) => {
            write_static_target(out, target);
            out.push_str("::");
            out.push_str(name);
        }
        ExprKind::ClassNameFetch(target) => {
            write_static_target(out, target);
            out.push_str("::class");
        }
        ExprKind::StaticPropertyFetch(target, name) => {
            write_static_target(out, target);
            out.push_str("::$");
            out.push_str(name);
        }
        ExprKind::DynamicStaticPropertyFetch(target, name) => {
            write_static_target(out, target);
            out.push_str("::${");
            write_expr(out, name);
            out.push('}');
        }
        ExprKind::DynamicClassConstFetch(target, name) => {
            write_static_target(out, target);
            out.push_str("::{");
            write_expr(out, name);
            out.push('}');
        }
        ExprKind::DynamicStaticCall(target, name, args) => {
            write_static_target(out, target);
            out.push_str("::");
            write_member_name(out, name);
            write_args(out, args);
        }
        ExprKind::StaticCall(target, member, args) => {
            write_static_target(out, target);
            out.push_str("::");
            out.push_str(member);
            write_args(out, args);
        }
        ExprKind::Index(base, index) => {
            write_expr(out, base);
            out.push('[');
            if let Some(index) = index {
                write_expr(out, index);
            }
            out.push(']');
        }
        ExprKind::Name(n) => out.push_str(n),
        ExprKind::Instanceof(value, target) => {
            write_expr(out, value);
            out.push_str(" instanceof ");
            match target {
                InstanceofTarget::Name(n, _) => out.push_str(n),
                InstanceofTarget::Expr(inner) => write_expr(out, inner),
            }
        }
        ExprKind::Cast(cast, operand) => {
            out.push('(');
            out.push_str(cast.keyword());
            out.push_str(") ");
            write_expr(out, operand);
        }
        ExprKind::Grouping(inner) => {
            out.push('(');
            write_expr(out, inner);
            out.push(')');
        }
        ExprKind::Clone(object, overrides) => match overrides {
            Some(overrides) => {
                out.push_str("clone(");
                write_expr(out, object);
                out.push_str(", ");
                write_expr(out, overrides);
                out.push(')');
            }
            None => {
                out.push_str("clone ");
                write_expr(out, object);
            }
        },
        ExprKind::FirstClassCallable(call) => {
            let printed = print_expr(call);
            out.push_str(printed.strip_suffix("()").unwrap_or(&printed));
            out.push_str("(...)");
        }
        ExprKind::ShellExec(cmd) => {
            out.push('`');
            match &cmd.kind {
                ExprKind::Interpolated(parts) => write_interpolated_parts(out, parts),
                ExprKind::Literal(Literal::String(s)) => {
                    out.push_str(&s.to_str_lossy());
                }
                _ => write_expr(out, cmd),
            }
            out.push('`');
        }
        ExprKind::Throw(inner) => {
            out.push_str("throw ");
            write_expr(out, inner);
        }
        ExprKind::NewAnonymousClass(_, args) => {
            out.push_str("new class");
            if !args.is_empty() {
                write_args(out, args);
            }
            out.push_str("\n{\n}");
        }
        ExprKind::Yield(key, value) => {
            out.push_str("yield");
            if let Some(key) = key {
                out.push(' ');
                write_expr(out, key);
                out.push_str(" =>");
            }
            if let Some(value) = value {
                out.push(' ');
                write_expr(out, value);
            }
        }
        ExprKind::YieldFrom(inner) => {
            out.push_str("yield from ");
            write_expr(out, inner);
        }
        ExprKind::Interpolated(parts) => {
            out.push('"');
            write_interpolated_parts(out, parts);
            out.push('"');
        }
        ExprKind::Closure {
            params,
            uses,
            is_static,
            by_ref,
            return_type,
            ..
        } => {
            if *is_static {
                out.push_str("static ");
            }
            out.push_str("function ");
            if *by_ref {
                out.push('&');
            }
            write_params(out, params);
            if !uses.is_empty() {
                out.push_str(" use (");
                write_list(out, uses, |out, u| {
                    if u.by_ref {
                        out.push('&');
                    }
                    out.push('$');
                    out.push_str(&u.name);
                });
                out.push(')');
            }
            write_return_type(out, return_type.as_deref());
            out.push_str(" {\n}");
        }
        ExprKind::ArrowFn {
            params,
            body,
            is_static,
            by_ref,
            return_type,
            ..
        } => {
            if *is_static {
                out.push_str("static ");
            }
            out.push_str("fn");
            if *by_ref {
                out.push('&');
            }
            write_params(out, params);
            write_return_type(out, return_type.as_deref());
            out.push_str(" => ");
            write_expr(out, body);
        }
        ExprKind::Match { subject, .. } => {
            out.push_str("match (");
            write_expr(out, subject);
            out.push_str(") {\n}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn printed(src: &str) -> String {
        let tokens = Lexer::new(&format!("<?php {src};")).tokenize_with_spans();
        let program = Parser::new(tokens).parse_program().unwrap();
        let [stmt] = program.as_slice() else {
            panic!("expected one statement");
        };
        let StmtKind::Expression(e) = &stmt.kind else {
            panic!("expected an expression statement");
        };
        print_expr_first_line(e)
    }

    #[test]
    fn prints_in_php_parser_style() {
        for (src, expected) in [
            ("$a", "$a"),
            ("$a+$b*2", "$a + $b * 2"),
            ("'it\\'s'", "'it\\'s'"),
            ("\"x\"", "'x'"),
            ("1.0", "1.0"),
            ("0.5", "0.5"),
            ("TRUE", "true"),
            ("array(1, 'k' => $v, ...$rest)", "[1, 'k' => $v, ...$rest]"),
            ("!$a && -$b", "!$a && -$b"),
            ("$a ?: $b", "$a ?: $b"),
            ("$x?->y->z", "$x?->y->z"),
            ("$x->$name", "$x->$name"),
            ("$x->{'a' . 'b'}", "$x->{'a' . 'b'}"),
            ("Foo::BAR", "Foo::BAR"),
            ("Foo::$bar", "Foo::$bar"),
            ("$a[0][]", "$a[0][]"),
            ("$a instanceof \\Foo\\Bar", "$a instanceof \\Foo\\Bar"),
            ("(int)$a", "(int) $a"),
            ("($a + $b) * $c", "($a + $b) * $c"),
            ("f(named: 1, ...$args)", "f(named: 1, ...$args)"),
            ("strlen(...)", "strlen(...)"),
            ("\"a{$b}c\\n\"", "\"a{$b}c\\n\""),
            ("fn(int $x) => $x + 1", "fn(int $x) => $x + 1"),
            (
                "function ($a) use (&$b) { return 1; }",
                "function ($a) use (&$b) {…",
            ),
            ("match ($x) { 1 => 2 }", "match ($x) {…"),
            ("new class { }", "new class…"),
            ("print $a", "print $a"),
            ("isset($a, $b['c'])", "isset($a, $b['c'])"),
            ("$a <=> $b", "$a <=> $b"),
            ("$a and $b", "$a and $b"),
        ] {
            assert_eq!(printed(src), expected, "{src}");
        }
    }
}
