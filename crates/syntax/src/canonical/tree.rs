//! The canonical prints of the syntax tree. A statement and an expression
//! print as their span and their kind, without their node id, which only
//! numbers them within one parse; a name written in the source prints as
//! the name and its span, `name@10..14`.

use crate::ast::*;
use crate::canonical::{Canonical, Printer};
use crate::{canonical_enum, canonical_struct};

impl Canonical for Stmt {
    fn canonical(&self, p: &mut Printer) {
        let Stmt {
            id: _,
            kind,
            span,
            doc_comment,
            doc_line,
        } = self;
        span.canonical(p);
        p.atom(" ");
        if doc_comment.is_some() || doc_line.is_some() {
            p.tuple("doc", &[doc_line, doc_comment]);
            p.atom(" ");
        }
        kind.canonical(p);
    }
}

impl Canonical for ExprNode {
    fn canonical(&self, p: &mut Printer) {
        let ExprNode { id: _, kind, span } = self;
        span.canonical(p);
        p.atom(" ");
        crate::stack::guard(|| kind.canonical(p));
    }
}

impl Canonical for Ident {
    fn canonical(&self, p: &mut Printer) {
        let Ident { name, span, .. } = self;
        name.canonical(p);
        p.atom("@");
        span.canonical(p);
    }
}

/// `written@span resolved`.
impl Canonical for QualifiedName {
    fn canonical(&self, p: &mut Printer) {
        let QualifiedName {
            written,
            resolved,
            span,
        } = self;
        written.canonical(p);
        p.atom("@");
        span.canonical(p);
        p.atom(" ");
        resolved.canonical(p);
    }
}

canonical_enum!(ResolvedName {
    Unresolved,
    Exact(name),
    WithGlobalFallback(name),
});

canonical_struct!(Attribute {
    name,
    args,
    span,
    removal_span,
});

canonical_enum!(StmtKind {
    InlineHtml(text),
    Expression(expr),
    Echo(exprs),
    Return(expr),
    Block(stmts),
    If(stmt),
    While { cond, body },
    DoWhile { body, cond },
    For {
        init,
        cond,
        step,
        body,
    },
    Foreach {
        subject,
        key,
        value,
        by_ref,
        body,
    },
    Declare { directives, body },
    Break(levels),
    Continue(levels),
    Global(vars),
    Unset(exprs),
    FunctionDecl(decl),
    ClassDecl(decl),
    Namespace(name),
    Use(imports),
    GlobalConst(consts),
    StaticVar(vars),
    Switch { subject, cases },
    Try {
        body,
        catches,
        finally,
    },
    Throw(expr),
    Goto(label),
    Label(label),
});

canonical_struct!(IfStmt {
    cond,
    then_branch,
    elseifs,
    else_branch,
});

canonical_struct!(ElseIf { cond, body, span });

canonical_struct!(SwitchCase { test, body, span });

canonical_struct!(CatchClause {
    types,
    var,
    body,
    span,
});

canonical_struct!(Promotion {
    visibility,
    set_visibility,
    readonly,
    is_final,
});

canonical_struct!(Param {
    name,
    type_hint,
    default,
    by_ref,
    variadic,
    promotion,
    hooks,
    attrs,
    doc_comment,
    span,
});

canonical_struct!(GotoLabel { name, backward });

canonical_struct!(FunctionBody {
    open,
    stmts,
    goto_labels,
    is_generator,
});

canonical_struct!(FunctionDecl {
    name,
    namespaced_name,
    params,
    return_type,
    body,
    by_ref,
    doc_comment,
    attrs,
    span,
});

canonical_enum!(Visibility {
    Public,
    Private,
    Protected,
});

canonical_struct!(PropertyDecl {
    name,
    visibility,
    write_visibility,
    is_static,
    is_readonly,
    is_abstract,
    is_final,
    type_hint,
    default,
    doc_comment,
    hooks,
    attrs,
    span,
    removal_span,
});

canonical_enum!(PropertyHookBody {
    Arrow(expr),
    Block(stmts),
    Abstract,
});

canonical_struct!(PropertyHooks { get, set });

canonical_struct!(PropertyHook {
    body,
    goto_labels,
    is_generator,
    by_ref,
    param,
    attrs,
    doc,
    is_final,
});

canonical_struct!(MethodDecl {
    visibility,
    is_static,
    is_abstract,
    is_final,
    function,
    span,
    removal_span,
});

canonical_struct!(UseImport {
    path,
    written,
    alias,
    kind,
});

canonical_enum!(UseKind {
    Class,
    Function,
    Const,
});

canonical_struct!(ClassConst {
    name,
    namespaced_name,
    value,
    visibility,
    doc_comment,
    attrs,
    type_hint,
    is_final,
    span,
    removal_span,
});

canonical_enum!(ClassKind {
    Class,
    Interface,
    Trait,
    Enum,
});

canonical_struct!(ClassDecl {
    name,
    namespaced_name,
    def,
});

canonical_struct!(AnonymousClass { def, args, name });

canonical_struct!(ClassDef {
    kind,
    backing_type,
    extends,
    implements,
    is_abstract,
    is_final,
    is_readonly,
    properties,
    methods,
    consts,
    trait_uses,
    enum_cases,
    doc_comment,
    attrs,
    span,
});

canonical_struct!(TraitUse {
    traits,
    doc_comment,
    adaptations,
    span,
});

canonical_struct!(EnumCase {
    name,
    value,
    attrs,
    span,
});

canonical_struct!(TraitAlias {
    trait_name,
    method,
    visibility,
    alias,
});

canonical_struct!(TraitPrecedence {
    trait_name,
    method,
    instead_of,
});

canonical_struct!(TraitAdaptations {
    aliases,
    precedences,
});

canonical_struct!(Arg {
    name,
    spread,
    value
});

canonical_struct!(ArrayItem {
    key,
    value,
    spread,
    by_ref,
});

canonical_enum!(DestructureSyntax { Short, List });

canonical_struct!(DestructureItem {
    key,
    target,
    by_ref,
});

canonical_enum!(Literal {
    Int(value),
    Float(value),
    String(value),
    Bool(value),
    Null,
});

canonical_enum!(BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    Eq,
    Identical,
    NotEq,
    NotIdentical,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Spaceship,
    And,
    Or,
    LogicalAnd,
    LogicalOr,
    LogicalXor,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Coalesce,
    Pipe,
});

canonical_enum!(UnaryOp {
    Neg,
    Plus,
    Not,
    BitNot,
    PreInc,
    PreDec,
    PostInc,
    PostDec,
});

canonical_struct!(ClosureUse { name, by_ref });

canonical_enum!(InstanceofTarget {
    Name(name),
    Expr(expr),
});

canonical_enum!(StaticTarget {
    Name(name),
    Expr(expr),
});

canonical_struct!(TypeHint { kind, span });

canonical_enum!(TypeHintKind {
    Keyword(native, written),
    Name(name),
    Nullable(inner),
    Union(members),
    Intersection(members),
});

canonical_enum!(NativeType {
    Int,
    Float,
    String,
    Bool,
    Array,
    Object,
    Mixed,
    Void,
    Never,
    Null,
    True,
    False,
    Iterable,
    Callable,
    SelfType,
    Static,
    Parent,
});

canonical_enum!(GlobalVar {
    Name(name),
    Dynamic(expr),
});

canonical_enum!(ExitKind { Exit, Die });

canonical_enum!(IncludeKind {
    Include,
    IncludeOnce,
    Require,
    RequireOnce,
});

canonical_enum!(CastSpelling {
    Int,
    Integer,
    Float,
    Real,
    Double,
    String,
    Binary,
    Bool,
    Boolean,
    Array,
    Object,
    Unset,
    Void,
});

canonical_enum!(ObjectOperator {
    Arrow,
    NullsafeArrow,
});

canonical_struct!(ClosureExpr {
    params,
    uses,
    body,
    goto_labels,
    is_generator,
    is_static,
    by_ref,
    return_type,
    attrs,
});

canonical_struct!(ArrowFnExpr {
    params,
    body,
    is_generator,
    is_static,
    by_ref,
    return_type,
    attrs,
});

canonical_enum!(ExprKind {
    Literal(literal),
    Variable(name),
    DynamicVariable(name),
    ArrayLiteral(items),
    ErrorSuppress(expr),
    Destructure(items, syntax),
    Unary(op, operand),
    Binary(op, lhs, rhs),
    Assign(target, value),
    AssignRef(target, value),
    CompoundAssign(op, target, value),
    Ternary(cond, then, otherwise),
    Call(callee, args),
    New(class, args),
    PropertyAccess(base, name, operator),
    MethodCall(base, name, args, operator),
    DynamicPropertyAccess(base, name, operator),
    DynamicMethodCall(base, name, args, operator),
    ClassConstFetch(target, name),
    ClassNameFetch(target),
    StaticPropertyFetch(target, name),
    DynamicStaticPropertyFetch(target, name),
    DynamicClassConstFetch(target, name),
    StaticCall(target, name, args),
    DynamicStaticCall(target, name, args),
    Index(base, index),
    Name(name),
    Instanceof(subject, target),
    Cast(spelling, operand),
    Grouping(inner),
    Clone(subject, overrides),
    FirstClassCallable(call),
    ShellExec(command),
    Isset(exprs),
    Empty(expr),
    Print(expr),
    Exit(kind, args),
    Eval(code),
    Include(kind, path),
    Throw(expr),
    NewAnonymousClass(class),
    Yield(key, value),
    YieldFrom(expr),
    Interpolated(parts),
    Closure(closure),
    ArrowFn(arrow),
    Match { subject, arms },
});

canonical_struct!(MatchArm {
    conditions,
    body,
    span,
});
