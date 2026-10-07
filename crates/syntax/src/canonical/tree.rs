//! The canonical prints of the syntax tree. A statement and an expression
//! print as their span and their kind, without their node id, which only
//! numbers them within one parse; a name written in the source prints as
//! the name and its span, `name@10..14`.

use crate::ast::*;
use crate::canonical::{Canonical, Printer};
use crate::{canonical_enum, canonical_struct};

impl Canonical for DocCommentRanges {
    fn canonical(&self, p: &mut Printer) {
        match self {
            DocCommentRanges::One(span) => p.tuple("doc_range", &[span]),
            DocCommentRanges::Group(spans) => p.tuple("doc_group_ranges", &[spans]),
            DocCommentRanges::Appended { spans, text } => p.tuple("doc_appended", &[spans, text]),
        }
    }
}

impl Canonical for StmtNode {
    fn canonical(&self, p: &mut Printer) {
        let StmtNode { id: _, kind, span } = self;
        span.canonical(p);
        p.atom(" ");
        kind.canonical(p);
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        let StmtNode { id: _, kind, span } = self;
        span.canonical(p);
        p.atom(" ");
        if let Some(site) = arenas.doc_site(span.start) {
            p.tuple("doc", &[&site.line, &site.text]);
            p.atom(" ");
        }
        kind.canonical_with(p, arenas);
    }
}

impl Canonical for ExprNode {
    fn canonical(&self, p: &mut Printer) {
        let ExprNode { id: _, kind, span } = self;
        span.canonical(p);
        p.atom(" ");
        crate::stack::guard(|| kind.canonical(p));
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        let ExprNode { id: _, kind, span } = self;
        span.canonical(p);
        p.atom(" ");
        crate::stack::guard(|| kind.canonical_with(p, arenas));
    }
}

impl Canonical for ExprId {
    fn canonical(&self, p: &mut Printer) {
        p.atom(&u32::from((*self).into_raw()).to_string());
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        arenas
            .expr(*self)
            .expect("canonical expression id belongs to its arena owner")
            .canonical_with(p, arenas);
    }
}

impl Canonical for StmtId {
    fn canonical(&self, p: &mut Printer) {
        p.atom(&u32::from((*self).into_raw()).to_string());
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        arenas
            .stmt(*self)
            .expect("canonical statement id belongs to its arena owner")
            .canonical_with(p, arenas);
    }
}

macro_rules! canonical_arena_id {
    ($id:ty, $get:ident, $what:literal) => {
        impl Canonical for $id {
            fn canonical(&self, p: &mut Printer) {
                p.atom(&u32::from((*self).into_raw()).to_string());
            }

            fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
                arenas
                    .$get(*self)
                    .expect(concat!(
                        "canonical ",
                        $what,
                        " id belongs to its arena owner"
                    ))
                    .canonical_with(p, arenas);
            }
        }
    };
}

canonical_arena_id!(FunctionId, function, "function");
canonical_arena_id!(ClassId, class, "class");
canonical_arena_id!(ClosureId, closure, "closure");
canonical_arena_id!(AnonymousClassId, anonymous_class, "anonymous-class");

macro_rules! canonical_arena_range {
    ($range:ty, $element:ty, $get:ident, $what:literal) => {
        impl Canonical for $range {
            fn canonical(&self, p: &mut Printer) {
                let start = u32::from(self.start().into_raw());
                let end = u32::from(self.end().into_raw());
                p.tuple("range", &[&start, &end]);
            }

            fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
                p.list_with(arenas.$get(*self).iter(), arenas);
            }
        }
    };
}

canonical_arena_range!(ArgRange, Arg, args, "argument");
canonical_arena_range!(StmtIdRange, StmtId, stmt_ids, "statement");
impl Canonical for ExprIdRange {
    fn canonical(&self, p: &mut Printer) {
        let start = u32::from(self.start().into_raw());
        let end = u32::from(self.end().into_raw());
        p.tuple("range", &[&start, &end]);
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        p.list_with(arenas.expr_ids(self).iter(), arenas);
    }
}
impl Canonical for DestructureItemRange {
    fn canonical(&self, p: &mut Printer) {
        let start = u32::from(self.start().into_raw());
        let end = u32::from(self.end().into_raw());
        p.tuple("range", &[&start, &end]);
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        p.list_with(arenas.destructure_items(self).iter(), arenas);
    }
}
canonical_arena_range!(ArrayItemRange, ArrayItem, array_items, "array-item");
canonical_arena_range!(ParamRange, Param, params, "parameter");
canonical_arena_range!(MatchArmRange, MatchArm, match_arms, "match-arm");
canonical_arena_range!(AttributeRange, Attribute, attributes, "attribute");
canonical_arena_range!(MethodRange, MethodDecl, methods, "method");

macro_rules! canonical_list_arena_range {
    ($range:ty, $get:ident) => {
        impl Canonical for $range {
            fn canonical(&self, p: &mut Printer) {
                let start = u32::from(self.start().into_raw());
                let end = u32::from(self.end().into_raw());
                p.tuple("range", &[&start, &end]);
            }

            fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
                p.list_with(arenas.$get(self).iter(), arenas);
            }
        }
    };
}

canonical_list_arena_range!(QualifiedNameRange, qualified_names);
canonical_list_arena_range!(PropertyRange, properties);
canonical_list_arena_range!(ClassConstRange, class_consts);
canonical_list_arena_range!(TraitUseRange, trait_uses);
canonical_list_arena_range!(EnumCaseRange, enum_cases);
canonical_list_arena_range!(TraitAliasRange, trait_aliases);
canonical_list_arena_range!(TraitPrecedenceRange, trait_precedences);
canonical_list_arena_range!(UseImportRange, use_imports);
canonical_list_arena_range!(ClosureUseRange, closure_uses);
canonical_list_arena_range!(TypeHintRange, type_hints);

impl Canonical for crate::parse::ParsedFile {
    fn canonical(&self, p: &mut Printer) {
        p.list_with(self.statements().iter(), &self.arenas);
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        p.list_with(arenas.stmt_ids(&self.stmts).iter(), arenas);
    }
}

impl Canonical for crate::parser::ParsedProgram {
    fn canonical(&self, p: &mut Printer) {
        p.list_with(self.statements().iter(), &self.arenas);
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        p.list_with(arenas.stmt_ids(&self.stmts).iter(), arenas);
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

impl Canonical for Param {
    fn canonical(&self, p: &mut Printer) {
        p.node(
            "Param",
            &[
                ("name", &self.name),
                ("type_hint", &self.type_hint),
                ("default", &self.default),
                ("by_ref", &self.by_ref),
                ("variadic", &self.variadic),
                ("promotion", &self.promotion),
                ("hooks", &self.hooks),
                ("attrs", &self.attrs),
                ("span", &self.span),
            ],
        );
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        let mut fields: Vec<(&str, &dyn Canonical)> = vec![
            ("name", &self.name),
            ("type_hint", &self.type_hint),
            ("default", &self.default),
            ("by_ref", &self.by_ref),
            ("variadic", &self.variadic),
            ("promotion", &self.promotion),
            ("hooks", &self.hooks),
            ("attrs", &self.attrs),
            ("span", &self.span),
        ];
        if let Some(site) = arenas.doc_site(self.span.start) {
            fields.push(("doc_comment", &site.text));
        }
        p.node_with("Param", &fields, arenas);
    }
}

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

canonical_struct!(AnonymousClass { def, args });

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

impl Canonical for Literal {
    fn canonical(&self, p: &mut Printer) {
        match self {
            Literal::Int(value) => p.tuple("Int", &[value]),
            Literal::Float(value) => p.tuple("Float", &[value]),
            Literal::String(value) => p.tuple("String", &[value]),
            Literal::Bool(value) => p.tuple("Bool", &[value]),
            Literal::Null => p.atom("Null"),
        }
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        match self {
            Literal::Int(value) => p.tuple("Int", &[value]),
            Literal::Float(value) => p.tuple("Float", &[value]),
            Literal::String(value) => {
                if let Some(bytes) = arenas.string_bytes(value) {
                    p.tuple(
                        "String",
                        &[&crate::php_string::PhpString::from_bytes(std::sync::Arc::<
                            [u8],
                        >::from(
                            bytes
                        ))],
                    );
                } else {
                    p.tuple("String", &[value]);
                }
            }
            Literal::Bool(value) => p.tuple("Bool", &[value]),
            Literal::Null => p.atom("Null"),
        }
    }
}

impl Canonical for StringPayload {
    fn canonical(&self, p: &mut Printer) {
        match self {
            StringPayload::Source(span) => p.tuple("source", &[span]),
            StringPayload::Decoded(range) => p.tuple("decoded", &[range.start(), range.end()]),
            StringPayload::Owned(value) => p.tuple("owned", &[value]),
        }
    }
}

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
    ExprNode(expr),
});

canonical_enum!(StaticTarget {
    Name(name),
    ExprNode(expr),
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

canonical_enum!(ClosurePayload {
    Closure(closure),
    ArrowFn(arrow),
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
