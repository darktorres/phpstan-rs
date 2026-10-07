//! Renders an expression back to PHP source for diagnostic messages, in
//! the same style as nikic/php-parser's standard pretty printer (what
//! PHPStan's `ExprPrinter` uses): single-quoted strings, spaced binary
//! operators, `[...]` arrays, and multi-line constructs (closure bodies,
//! anonymous classes, `match` arms) on their own lines.

use crate::ast::*;

pub fn print_expr(arenas: &NodeArenas, id: ExprId) -> String {
    let mut out = String::new();
    write_expr(&mut out, arenas, id);
    out
}

/// `print_expr`, cut to its first line with a trailing `…` when the
/// rendering spans several.
pub fn print_expr_first_line(arenas: &NodeArenas, id: ExprId) -> String {
    let printed = print_expr(arenas, id);
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
fn write_php_string(out: &mut String, bytes: &[u8]) {
    match std::str::from_utf8(bytes) {
        Ok(text) => write_single_quoted(out, text),
        Err(_) => {
            out.push('"');
            write_double_quoted_bytes(out, bytes, b'"');
            out.push('"');
        }
    }
}

/// The body of a double-quoted string: valid UTF-8 as text, every other
/// byte as `\xNN`.
/// `bytes` between `delimiter` quotes (`"` or `` ` ``), escaped as that
/// string reads them: the delimiter itself, `\\`, `$` and the control
/// characters.
fn write_double_quoted_bytes(out: &mut String, bytes: &[u8], delimiter: u8) {
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            match c {
                c if c == char::from(delimiter) => {
                    out.push('\\');
                    out.push(c);
                }
                '\\' => out.push_str("\\\\"),
                '$' => out.push_str("\\$"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                '\u{0B}' => out.push_str("\\v"),
                '\u{1B}' => out.push_str("\\e"),
                '\u{0C}' => out.push_str("\\f"),
                c if c.is_ascii_control() => out.push_str(&format!("\\x{:02x}", c as u8)),
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
    out.push_str(&crate::php_float::var_export_float(f));
}

fn write_list<T>(out: &mut String, items: &[T], mut write: impl FnMut(&mut String, &T)) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write(out, item);
    }
}

fn write_args(out: &mut String, arenas: &NodeArenas, range: ArgRange) {
    let args = arenas.args(range);
    out.push('(');
    write_list(out, args, |out, arg| {
        if let Some(name) = &arg.name {
            out.push_str(name);
            out.push_str(": ");
        }
        if arg.spread {
            out.push_str("...");
        }
        write_expr(out, arenas, arg.value);
    });
    out.push(')');
}

fn write_params(out: &mut String, arenas: &NodeArenas, params: &[Param]) {
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
            write_expr(out, arenas, *default);
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
fn write_member_name(out: &mut String, arenas: &NodeArenas, id: impl ExprIndex) {
    let id = id.expr_index();
    match arenas.expr(id).map(|name| &name.kind) {
        Some(ExprKind::Variable(_)) => write_expr(out, arenas, id),
        _ => {
            out.push('{');
            write_expr(out, arenas, id);
            out.push('}');
        }
    }
}

fn write_interpolated_parts(
    out: &mut String,
    arenas: &NodeArenas,
    parts: &ExprIdRange,
    delimiter: u8,
) {
    for &part in arenas.expr_ids(parts) {
        match arenas.expr(part).map(|node| &node.kind) {
            Some(ExprKind::Literal(Literal::String(s))) => {
                if let Some(bytes) = arenas.string_bytes(s) {
                    write_double_quoted_bytes(out, bytes, delimiter)
                }
            }
            _ => {
                out.push('{');
                write_expr(out, arenas, part);
                out.push('}');
            }
        }
    }
}

fn write_static_target(out: &mut String, arenas: &NodeArenas, target: &StaticTarget) {
    match target {
        StaticTarget::Name(n) => out.push_str(n),
        StaticTarget::Expr(inner) => write_expr(out, arenas, *inner),
    }
}

trait ExprIndex {
    fn expr_index(self) -> ExprId;
}

impl ExprIndex for ExprId {
    fn expr_index(self) -> ExprId {
        self
    }
}

impl ExprIndex for &ExprId {
    fn expr_index(self) -> ExprId {
        *self
    }
}

fn write_expr(out: &mut String, arenas: &NodeArenas, id: impl ExprIndex) {
    let id = id.expr_index();
    crate::stack::guard(|| {
        if let Some(expr) = arenas.expr(id) {
            write_expr_unguarded(out, arenas, expr);
        }
    });
}

fn write_expr_unguarded(out: &mut String, arenas: &NodeArenas, e: &ExprNode) {
    match &e.kind {
        ExprKind::Literal(lit) => match lit {
            Literal::Int(n) => out.push_str(&n.to_string()),
            Literal::Float(f) => write_float(out, *f),
            Literal::String(s) => {
                if let Some(bytes) = arenas.string_bytes(s) {
                    write_php_string(out, bytes)
                }
            }
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
            write_expr(out, arenas, inner);
            out.push('}');
        }
        ExprKind::ArrayLiteral(items) => {
            out.push('[');
            write_list(out, arenas.array_items(*items), |out, item| {
                if item.spread {
                    out.push_str("...");
                }
                if let Some(key) = &item.key {
                    write_expr(out, arenas, key);
                    out.push_str(" => ");
                }
                if item.by_ref {
                    out.push('&');
                }
                write_expr(out, arenas, &item.value);
            });
            out.push(']');
        }
        ExprKind::Destructure(items, syntax) => {
            let is_list = *syntax == DestructureSyntax::List;
            let items = arenas.destructure_items(items);
            out.push_str(if is_list { "list(" } else { "[" });
            write_list(out, items, |out, item| {
                if let Some(key) = &item.key {
                    write_expr(out, arenas, key);
                    out.push_str(" => ");
                }
                if item.by_ref {
                    out.push('&');
                }
                if let Some(target) = &item.target {
                    write_expr(out, arenas, target);
                }
            });
            // `[$a, ]` has one item, so a skipped last slot needs the
            // comma that marks it.
            if items.last().is_some_and(|item| item.target.is_none()) {
                out.push(',');
            }
            out.push(if is_list { ')' } else { ']' });
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
            write_expr(out, arenas, operand);
            out.push_str(suffix);
        }
        ExprKind::Binary(op, l, r) => {
            write_expr(out, arenas, l);
            out.push(' ');
            out.push_str(binary_operator(op));
            out.push(' ');
            write_expr(out, arenas, r);
        }
        ExprKind::Assign(target, value) => {
            write_expr(out, arenas, target);
            out.push_str(" = ");
            write_expr(out, arenas, value);
        }
        ExprKind::AssignRef(target, value) => {
            write_expr(out, arenas, target);
            out.push_str(" =& ");
            write_expr(out, arenas, value);
        }
        ExprKind::CompoundAssign(op, target, value) => {
            write_expr(out, arenas, target);
            out.push(' ');
            out.push_str(binary_operator(op));
            out.push_str("= ");
            write_expr(out, arenas, value);
        }
        ExprKind::Ternary(cond, then, otherwise) => {
            write_expr(out, arenas, cond);
            match then {
                Some(then) => {
                    out.push_str(" ? ");
                    write_expr(out, arenas, then);
                    out.push_str(" : ");
                }
                None => out.push_str(" ?: "),
            }
            write_expr(out, arenas, otherwise);
        }
        ExprKind::Call(callee, args) => {
            write_expr(out, arenas, callee);
            write_args(out, arenas, *args);
        }
        ExprKind::Isset(operands) => {
            out.push_str("isset(");
            for (i, operand) in arenas.expr_ids(operands).iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_expr(out, arenas, operand);
            }
            out.push(')');
        }
        ExprKind::Empty(operand) => {
            out.push_str("empty(");
            write_expr(out, arenas, operand);
            out.push(')');
        }
        ExprKind::Print(inner) => {
            out.push_str("print ");
            write_expr(out, arenas, inner);
        }
        ExprKind::Include(kind, path) => {
            out.push_str(kind.as_str());
            out.push(' ');
            write_expr(out, arenas, path);
        }
        ExprKind::Exit(kind, args) => {
            out.push_str(kind.as_str());
            if let Some(args) = args {
                write_args(out, arenas, *args);
            }
        }
        ExprKind::Eval(code) => {
            out.push_str("eval(");
            write_expr(out, arenas, code);
            out.push(')');
        }
        ExprKind::New(class, args) => {
            out.push_str("new ");
            write_expr(out, arenas, class);
            write_args(out, arenas, *args);
        }
        ExprKind::PropertyAccess(object, name, nullsafe) => {
            write_expr(out, arenas, object);
            out.push_str(if nullsafe.is_nullsafe() { "?->" } else { "->" });
            out.push_str(name);
        }
        ExprKind::MethodCall(object, name, args, nullsafe) => {
            write_expr(out, arenas, object);
            out.push_str(if nullsafe.is_nullsafe() { "?->" } else { "->" });
            out.push_str(name);
            write_args(out, arenas, *args);
        }
        ExprKind::DynamicPropertyAccess(object, name, operator) => {
            write_expr(out, arenas, object);
            out.push_str(if operator.is_nullsafe() { "?->" } else { "->" });
            write_member_name(out, arenas, name);
        }
        ExprKind::DynamicMethodCall(object, name, args, operator) => {
            write_expr(out, arenas, object);
            out.push_str(if operator.is_nullsafe() { "?->" } else { "->" });
            write_member_name(out, arenas, name);
            write_args(out, arenas, *args);
        }
        ExprKind::ClassConstFetch(target, name) => {
            write_static_target(out, arenas, target);
            out.push_str("::");
            out.push_str(name);
        }
        ExprKind::ClassNameFetch(target) => {
            write_static_target(out, arenas, target);
            out.push_str("::class");
        }
        ExprKind::StaticPropertyFetch(target, name) => {
            write_static_target(out, arenas, target);
            out.push_str("::$");
            out.push_str(name);
        }
        ExprKind::DynamicStaticPropertyFetch(target, name) => {
            write_static_target(out, arenas, target);
            out.push_str("::${");
            write_expr(out, arenas, name);
            out.push('}');
        }
        ExprKind::DynamicClassConstFetch(target, name) => {
            write_static_target(out, arenas, target);
            out.push_str("::{");
            write_expr(out, arenas, name);
            out.push('}');
        }
        ExprKind::DynamicStaticCall(target, name, args) => {
            write_static_target(out, arenas, target);
            out.push_str("::");
            write_member_name(out, arenas, name);
            write_args(out, arenas, *args);
        }
        ExprKind::StaticCall(target, member, args) => {
            write_static_target(out, arenas, target);
            out.push_str("::");
            out.push_str(member);
            write_args(out, arenas, *args);
        }
        ExprKind::Index(base, index) => {
            write_expr(out, arenas, base);
            out.push('[');
            if let Some(index) = index {
                write_expr(out, arenas, index);
            }
            out.push(']');
        }
        ExprKind::Name(n) => out.push_str(n),
        ExprKind::Instanceof(value, target) => {
            write_expr(out, arenas, value);
            out.push_str(" instanceof ");
            match target {
                InstanceofTarget::Name(n) => out.push_str(n),
                InstanceofTarget::Expr(inner) => write_expr(out, arenas, inner),
            }
        }
        ExprKind::Cast(cast, operand) => {
            out.push('(');
            out.push_str(cast.keyword());
            out.push_str(") ");
            write_expr(out, arenas, operand);
        }
        ExprKind::Grouping(inner) => {
            out.push('(');
            write_expr(out, arenas, inner);
            out.push(')');
        }
        ExprKind::ErrorSuppress(inner) => {
            out.push('@');
            write_expr(out, arenas, inner);
        }
        ExprKind::Clone(object, overrides) => match overrides {
            Some(overrides) => {
                out.push_str("clone(");
                write_expr(out, arenas, object);
                out.push_str(", ");
                write_expr(out, arenas, overrides);
                out.push(')');
            }
            None => {
                out.push_str("clone ");
                write_expr(out, arenas, object);
            }
        },
        ExprKind::FirstClassCallable(call) => {
            let printed = print_expr(arenas, *call);
            out.push_str(printed.strip_suffix("()").unwrap_or(&printed));
            out.push_str("(...)");
        }
        ExprKind::ShellExec(cmd) => {
            out.push('`');
            match arenas.expr(*cmd).map(|cmd| &cmd.kind) {
                Some(ExprKind::Interpolated(parts)) => {
                    write_interpolated_parts(out, arenas, parts, b'`')
                }
                Some(ExprKind::Literal(Literal::String(s))) => {
                    if let Some(bytes) = arenas.string_bytes(s) {
                        write_double_quoted_bytes(out, bytes, b'`')
                    }
                }
                _ => write_expr(out, arenas, cmd),
            }
            out.push('`');
        }
        ExprKind::Throw(inner) => {
            out.push_str("throw ");
            write_expr(out, arenas, inner);
        }
        ExprKind::NewAnonymousClass(class) => {
            out.push_str("new class");
            if let Some(class) = arenas.anonymous_class(*class) {
                if !arenas.args(class.args).is_empty() {
                    write_args(out, arenas, class.args);
                }
            }
            out.push_str("\n{\n}");
        }
        ExprKind::Yield(key, value) => {
            out.push_str("yield");
            if let Some(key) = key {
                out.push(' ');
                write_expr(out, arenas, key);
                out.push_str(" =>");
            }
            if let Some(value) = value {
                out.push(' ');
                write_expr(out, arenas, value);
            }
        }
        ExprKind::YieldFrom(inner) => {
            out.push_str("yield from ");
            write_expr(out, arenas, inner);
        }
        ExprKind::Interpolated(parts) => {
            out.push('"');
            write_interpolated_parts(out, arenas, parts, b'"');
            out.push('"');
        }
        ExprKind::Closure(closure) => {
            let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure) else {
                unreachable!("closure expression ID must address a closure payload")
            };
            let ClosureExpr {
                params,
                uses,
                is_static,
                by_ref,
                return_type,
                ..
            } = closure;
            let uses = arenas.closure_uses(uses);
            if *is_static {
                out.push_str("static ");
            }
            out.push_str("function ");
            if *by_ref {
                out.push('&');
            }
            write_params(out, arenas, arenas.params(*params));
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
        ExprKind::ArrowFn(arrow) => {
            let Some(ClosurePayload::ArrowFn(arrow)) = arenas.closure(*arrow) else {
                unreachable!("arrow-function expression ID must address an arrow payload")
            };
            let ArrowFnExpr {
                params,
                body,
                is_static,
                by_ref,
                return_type,
                ..
            } = arrow;
            if *is_static {
                out.push_str("static ");
            }
            out.push_str("fn");
            if *by_ref {
                out.push('&');
            }
            write_params(out, arenas, arenas.params(*params));
            write_return_type(out, return_type.as_deref());
            out.push_str(" => ");
            write_expr(out, arenas, body);
        }
        ExprKind::Match { subject, .. } => {
            out.push_str("match (");
            write_expr(out, arenas, subject);
            out.push_str(") {\n}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printed(src: &str) -> String {
        let parsed = crate::parse(format!("<?php {src};").as_bytes()).unwrap();
        let [stmt] = parsed.statements() else {
            panic!("expected one statement");
        };
        let stmt = parsed.arenas.stmt(*stmt).expect("parsed statement");
        let StmtKind::Expression(e) = &stmt.kind else {
            panic!("expected an expression statement");
        };
        print_expr_first_line(&parsed.arenas, *e)
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
