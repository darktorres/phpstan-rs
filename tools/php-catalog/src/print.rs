//! The catalog's functions as text, for reading the lowering.

use std::fmt::Write;

use crate::ir::{BinOp, Callee, Expr, ExprKind, Function, Global, Init, Place, Stmt, Term, UnOp};

pub(crate) fn function(function: &Function) -> String {
    let mut out = String::new();
    let params: Vec<String> = function.vars[..function.params as usize]
        .iter()
        .map(|var| format!("{}: {}", var.name, var.ty))
        .collect();
    let _ = writeln!(
        out,
        "fn {}({}{}) -> {}  [{}:{}]",
        function.name,
        params.join(", "),
        if function.is_variadic { ", ..." } else { "" },
        function.ret,
        function.file,
        function.line
    );
    for what in &function.unread {
        let _ = writeln!(out, "  unread: {what}");
    }
    for what in &function.approximated {
        let _ = writeln!(out, "  approximated: {what}");
    }
    for (i, var) in function
        .vars
        .iter()
        .enumerate()
        .skip(function.params as usize)
    {
        let _ = writeln!(out, "  var {} {}: {}", i, var.name, var.ty);
    }
    let printer = Printer {
        function: Some(function),
    };
    for (i, block) in function.blocks.iter().enumerate() {
        let _ = writeln!(out, "  b{i}:");
        for stmt in &block.stmts {
            let _ = writeln!(out, "    {}", printer.stmt(stmt));
        }
        let _ = writeln!(out, "    {}", printer.term(&block.term));
    }
    out
}

pub(crate) fn global(global: &Global) -> String {
    let mut out = format!("global {}: {}", global.name, global.ty);
    if let Some(init) = &global.init {
        out.push_str(" = ");
        let printer = Printer { function: None };
        printer.init(init, 1, &mut out);
    }
    out.push('\n');
    out
}

struct Printer<'f> {
    /// The function whose variables the printed code names.
    function: Option<&'f Function>,
}

impl Printer<'_> {
    fn init(&self, init: &Init, depth: usize, out: &mut String) {
        match init {
            Init::Expr(expr) => out.push_str(&self.expr(expr)),
            Init::Zero => out.push_str("zero"),
            Init::List {
                items,
                filler,
                element,
            } => {
                out.push('{');
                for item in items {
                    out.push('\n');
                    out.push_str(&"  ".repeat(depth));
                    if let Some(field) = &item.field {
                        let _ = write!(out, ".{} ", field.name);
                    }
                    match item.offset {
                        Some(offset) => {
                            let _ = write!(out, "+b {offset} = ");
                        }
                        None => out.push_str("+b ? = "),
                    }
                    self.init(&item.init, depth + 1, out);
                    out.push(',');
                }
                if let Some(filler) = filler {
                    out.push('\n');
                    out.push_str(&"  ".repeat(depth));
                    match element {
                        Some(size) => {
                            let _ = write!(out, "filler, each {size} bytes = ");
                        }
                        None => out.push_str("filler, each ? bytes = "),
                    }
                    self.init(filler, depth + 1, out);
                }
                out.push('\n');
                out.push_str(&"  ".repeat(depth.saturating_sub(1)));
                out.push('}');
            }
        }
    }

    fn stmt(&self, stmt: &Stmt) -> String {
        match stmt {
            Stmt::Assign { dst, src, line } => {
                format!("{} = {}  @{line}", self.place(dst), self.expr(src))
            }
            Stmt::Call {
                dst,
                callee,
                args,
                line,
            } => {
                let callee = match callee {
                    Callee::Direct(name) => name.to_string(),
                    Callee::Indirect(expr) => format!("({})", self.expr(expr)),
                };
                let args: Vec<String> = args.iter().map(|arg| self.expr(arg)).collect();
                match dst {
                    Some(dst) => format!(
                        "{} = call {callee}({})  @{line}",
                        self.place(dst),
                        args.join(", ")
                    ),
                    None => format!("call {callee}({})  @{line}", args.join(", ")),
                }
            }
            Stmt::Asm { writes, line } => {
                let writes: Vec<String> = writes.iter().map(|place| self.place(place)).collect();
                format!("asm writes [{}]  @{line}", writes.join(", "))
            }
        }
    }

    fn term(&self, term: &Term) -> String {
        match term {
            Term::Goto(target) => format!("goto b{target}"),
            Term::Branch { cond, then, els } => {
                format!("if {} then b{then} else b{els}", self.expr(cond))
            }
            Term::Switch { on, cases, default } => {
                let cases: Vec<String> = cases
                    .iter()
                    .map(|(low, high, target)| {
                        if low == high {
                            format!("{low} => b{target}")
                        } else {
                            format!("{low}..={high} => b{target}")
                        }
                    })
                    .collect();
                format!(
                    "switch {} {{ {}, _ => b{default} }}",
                    self.expr(on),
                    cases.join(", ")
                )
            }
            Term::Return(Some(value)) => format!("return {}", self.expr(value)),
            Term::Return(None) => "return".to_owned(),
            Term::Unreachable => "unreachable".to_owned(),
            Term::IndirectGoto(targets) => {
                let targets: Vec<String> = targets.iter().map(|t| format!("b{t}")).collect();
                format!("goto * [{}]", targets.join(", "))
            }
        }
    }

    fn place(&self, place: &Place) -> String {
        match place {
            Place::Var(id) => match self.function.and_then(|f| f.vars.get(*id as usize)) {
                Some(var) if !var.name.is_empty() => var.name.to_string(),
                _ => format!("${id}"),
            },
            Place::Global(name) => format!("@{name}"),
            Place::Deref(pointer) => format!("*{}", self.operand(pointer)),
            Place::Field(base, field) => {
                format!(
                    "{}.{}{{{}{}}}",
                    self.place_operand(base),
                    field.name,
                    field.record,
                    field
                        .offset
                        .map_or_else(|| " +b ?".to_owned(), |o| format!(" +b {o}"))
                )
            }
            Place::Index(base, index) => {
                format!("{}[b {}]", self.place_operand(base), self.expr(index))
            }
        }
    }

    fn place_operand(&self, place: &Place) -> String {
        match place {
            Place::Deref(_) => format!("({})", self.place(place)),
            _ => self.place(place),
        }
    }

    fn operand(&self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Use(_) | ExprKind::Int(_) | ExprKind::Func(_) | ExprKind::Str(_) => {
                self.expr(expr)
            }
            _ => format!("({})", self.expr(expr)),
        }
    }

    fn expr(&self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Use(place) => self.place(place),
            ExprKind::AddrOf(place) => format!("&{}", self.place_operand(place)),
            ExprKind::Int(value) => value.to_string(),
            ExprKind::Float => "<float>".to_owned(),
            ExprKind::Str(bytes) => {
                let text: String = bytes.escape_ascii().to_string();
                let short: String = text.chars().take(60).collect();
                if short.len() < text.len() {
                    format!("\"{short}...\"")
                } else {
                    format!("\"{short}\"")
                }
            }
            ExprKind::Func(name) => format!("fn {name}"),
            ExprKind::Unary(op, value) => {
                let op = match op {
                    UnOp::Neg => "-",
                    UnOp::Not => "!",
                    UnOp::BitNot => "~",
                };
                format!("{op}{}", self.operand(value))
            }
            ExprKind::Binary(op, left, right) => format!(
                "{} {} {}",
                self.operand(left),
                binop(*op),
                self.operand(right)
            ),
            ExprKind::Offset(pointer, bytes) => {
                format!("{} +b {}", self.operand(pointer), self.operand(bytes))
            }
            ExprKind::Cast(value) => format!("({}) {}", expr.ty, self.operand(value)),
            ExprKind::VaArg(list) => format!("va_arg({}, {})", self.expr(list), expr.ty),
            ExprKind::SizeOf(ty) => format!("sizeof({ty})"),
            ExprKind::AlignOf(ty) => format!("alignof({ty})"),
            ExprKind::Unknown(why) => format!("?{why}"),
        }
    }
}

fn binop(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
    }
}
