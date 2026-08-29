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
    Use(String),
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
}

#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<String>,
    pub body: Vec<Stmt>,
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
    pub extends: Option<String>,
    pub implements: Vec<String>,
    pub is_abstract: bool,
    pub is_final: bool,
    pub properties: Vec<PropertyDecl>,
    pub methods: Vec<MethodDecl>,
    pub consts: Vec<ClassConst>,
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
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Coalesce,
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

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Literal),
    Variable(String),
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
    StaticAccess(String, String),
    StaticCall(String, String, Vec<Expr>),
    Index(Box<Expr>, Option<Box<Expr>>),
    Name(String),
    Instanceof(Box<Expr>, String),
    Grouping(Box<Expr>),
    Clone(Box<Expr>),
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
