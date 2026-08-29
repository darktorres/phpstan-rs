#[derive(Debug, Clone)]
pub enum Stmt {
    InlineHtml(String),
    Expression(Expr),
    Echo(Vec<Expr>),
    Return(Option<Expr>),
    Block(Vec<Stmt>),
    If {
        cond: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    While {
        cond: Expr,
        body: Box<Stmt>,
    },
    DoWhile {
        body: Box<Stmt>,
        cond: Expr,
    },
    For {
        init: Vec<Expr>,
        cond: Vec<Expr>,
        step: Vec<Expr>,
        body: Box<Stmt>,
    },
    Foreach {
        subject: Expr,
        key: Option<Expr>,
        value: Expr,
        by_ref: bool,
        body: Box<Stmt>,
    },
    Break(Option<i64>),
    Continue(Option<i64>),
    Global(Vec<String>),
    FunctionDecl(FunctionDecl),
    ClassDecl(ClassDecl),
    Namespace(String),
    // (imported path, optional `as Alias`)
    Use(String, Option<String>),
    // Top-level `const NAME = value, ...;`, distinct from a class's own
    // consts (ClassConst is reused for the shape, since it's identical).
    GlobalConst(Vec<ClassConst>),
    // `static $x = 1, $y;` - a function-local static variable declaration
    // (name, optional initializer). Persistence across calls isn't
    // modeled - only its scoping (the variable is defined from here on)
    // matters for a single-call static analysis pass.
    StaticVar(Vec<(String, Option<Expr>)>),
    EnumDecl(EnumDecl),
    Switch {
        subject: Expr,
        cases: Vec<SwitchCase>,
    },
    Try {
        body: Vec<Stmt>,
        catches: Vec<CatchClause>,
        finally: Option<Vec<Stmt>>,
    },
    Throw(Expr),
    Goto(String),
    // `label:` - a goto target. Control flow through goto/labels isn't
    // modeled (no effect on narrowing, terminal-statement detection,
    // etc.) - only enough structure to parse past them.
    Label(String),
}

#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub test: Option<Expr>, // None = `default:`
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub struct CatchClause {
    pub types: Vec<String>,
    pub var: Option<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub type_hint: Option<String>,
    pub default: Option<Expr>,
    pub by_ref: bool,
    pub variadic: bool,
    // Some(visibility) for constructor property promotion, e.g.
    // `public function __construct(private int $id) {}`.
    pub promoted_visibility: Option<Visibility>,
}

#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<String>,
    pub body: Vec<Stmt>,
    // Raw `/** ... */` text immediately preceding this declaration, if any.
    pub doc_comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Visibility {
    Public,
    Private,
    Protected,
}

#[derive(Debug, Clone)]
pub struct PropertyDecl {
    pub name: String,
    pub visibility: Visibility,
    pub is_static: bool,
    pub type_hint: Option<String>,
    pub default: Option<Expr>,
    pub doc_comment: Option<String>,
    // PHP 8.4 property hooks (`{ get => ...; set(Type $value) { ... } }`).
    // Parsed so the file doesn't fail to parse at all, but the hook bodies
    // aren't type-checked (they're arbitrary code referencing $this/$value
    // with no function-like registry entry to check them against) - out of
    // scope for now, same as FirstClassCallable's args.
    pub hooks: Option<PropertyHooks>,
}

#[derive(Debug, Clone)]
pub enum PropertyHookBody {
    // `get => expr;` / `set(...) => expr;`
    Arrow(Expr),
    // `get { ... }` / `set(...) { ... }`
    Block(Vec<Stmt>),
}

#[derive(Debug, Clone)]
pub struct PropertyHooks {
    pub get: Option<PropertyHookBody>,
    // The optional `(Type $value)` parameter list is just the one param
    // PHP allows there; None when the hook omits it (`set { ... }` uses an
    // implicit $value).
    pub set: Option<(Option<Param>, PropertyHookBody)>,
}

#[derive(Debug, Clone)]
pub struct MethodDecl {
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_abstract: bool,
    pub function: FunctionDecl,
}

#[derive(Debug, Clone)]
pub struct ClassConst {
    pub name: String,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct ClassDecl {
    pub name: String,
    pub extends: Vec<String>,
    pub implements: Vec<String>,
    pub is_abstract: bool,
    pub is_final: bool,
    pub properties: Vec<PropertyDecl>,
    pub methods: Vec<MethodDecl>,
    pub consts: Vec<ClassConst>,
    // `use TraitA, TraitB;` inside the class body.
    pub uses: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EnumCase {
    pub name: String,
    // Some(...) for a backed enum (`case Hearts = 'H';`), None for a pure
    // one (`case Hearts;`).
    pub value: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    // "int" | "string" for a backed enum, None for a pure one. Enums can't
    // extend anything or declare instance properties, unlike ClassDecl.
    pub backing_type: Option<String>,
    pub implements: Vec<String>,
    pub cases: Vec<EnumCase>,
    pub methods: Vec<MethodDecl>,
    pub consts: Vec<ClassConst>,
    pub uses: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Literal {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
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
    // The `xor` keyword operator - logical, not `^`'s bitwise BitXor. The
    // word forms of &&/|| (`and`/`or`) reuse And/Or directly since they're
    // the same operation at runtime, only their *precedence* differs,
    // which is purely a parse-time concern already resolved by the time
    // this tree exists.
    LogicalXor,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Coalesce,
    // PHP 8.5's `|>` pipe operator: `$value |> callable` is sugar for
    // `callable($value)`. Left-associative, and sits (empirically checked
    // against a real PHP 8.5 build) between concat/shift and the
    // relational operators - tighter than `<`, looser than `.`/`<<`.
    Pipe,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg,
    Plus,
    Not,
    BitNot,
    PreInc,
    PreDec,
    PostInc,
    PostDec,
}

#[derive(Debug, Clone)]
pub struct ClosureUse {
    pub name: String,
    pub by_ref: bool,
}

// The right-hand side of `instanceof`: almost always a static class name,
// but PHP also allows an arbitrary expression there (`$x instanceof
// $className`, `$x instanceof $obj`) for a runtime-determined class.
#[derive(Debug, Clone)]
pub enum InstanceofTarget {
    Name(String),
    Expr(Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Literal),
    Variable(String),
    // `${expr}` - the older "variable variable" syntax: the variable's own
    // name is computed at runtime by evaluating expr, so unlike Variable
    // there's no fixed name here to look up or bind in `locals`.
    DynamicVariable(Box<Expr>),
    ArrayLiteral(Vec<(Option<Expr>, Expr)>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    CompoundAssign(BinOp, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Option<Box<Expr>>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    New(Box<Expr>, Vec<Expr>),
    PropertyAccess(Box<Expr>, String),
    MethodCall(Box<Expr>, String, Vec<Expr>),
    // `$obj->{$name}` / `$obj->{$name}(...)` - the property/method name is
    // itself computed from an expression, not statically known, so it
    // can't share PropertyAccess/MethodCall's plain-String name. Still
    // worth its own base+name-expr(+args) checking (undefined variables
    // inside the {} can still be caught), even though the accessed
    // member's own type never can be.
    DynamicPropertyAccess(Box<Expr>, Box<Expr>),
    DynamicMethodCall(Box<Expr>, Box<Expr>, Vec<Expr>),
    StaticAccess(String, String),
    StaticCall(String, String, Vec<Expr>),
    Index(Box<Expr>, Option<Box<Expr>>),
    Name(String),
    Instanceof(Box<Expr>, InstanceofTarget),
    // Cast target is normalized to one of: int, float, string, bool,
    // array, object, unset.
    Cast(String, Box<Expr>),
    Grouping(Box<Expr>),
    // `clone $x` / PHP 8.5's `clone($x, $overrides)` (the second field is
    // the property-overrides array, when present). Both produce an object
    // of the same type as the first operand, so infer_expr treats them
    // identically - the overrides themselves aren't otherwise modeled.
    Clone(Box<Expr>, Option<Box<Expr>>),
    // `foo(...)` / `$obj->method(...)` (PHP 8.1): a reference to the
    // callable itself, not a call - wraps a Call/MethodCall/StaticCall
    // that was parsed with an empty argument list specifically because of
    // this syntax, so it's never mistaken for (and argument-count-checked
    // as) a real zero-argument call.
    FirstClassCallable(Box<Expr>),
    // `throw` as an expression (PHP 8.0+): valid in match arms, ternary
    // branches, ??'s right side, arrow function bodies, etc - as opposed
    // to Stmt::Throw, a bare `throw expr;` statement, which is still
    // dispatched separately and takes priority at statement position.
    Throw(Box<Expr>),
    // `new class(...) [extends X] [implements Y] { ... }` - the anonymous
    // class's own declaration, inline, plus its constructor arguments.
    // Never given a stable name multiple such expressions could share, so
    // it isn't indexed into the registry the way a normal class is; its
    // methods are still type-checked (see checker::check_expr).
    NewAnonymousClass(Box<ClassDecl>, Vec<Expr>),
    // `yield;` / `yield $v;` / `yield $k => $v;` - (key, value). Makes the
    // enclosing function a generator; that broader semantic isn't modeled,
    // only enough structure to parse and check the yielded expression(s).
    Yield(Option<Box<Expr>>, Option<Box<Expr>>),
    // `yield from $generator;`
    YieldFrom(Box<Expr>),
    Interpolated(Vec<Expr>),
    Closure {
        params: Vec<Param>,
        uses: Vec<ClosureUse>,
        body: Vec<Stmt>,
        is_static: bool,
        by_ref: bool,
        return_type: Option<String>,
    },
    ArrowFn {
        params: Vec<Param>,
        body: Box<Expr>,
        is_static: bool,
        return_type: Option<String>,
    },
    Match {
        subject: Box<Expr>,
        arms: Vec<(Vec<Expr>, Expr)>,
        default: Option<Box<Expr>>,
    },
}
