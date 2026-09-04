use crate::token::Span;

/// Phase 37: one `Name(args)` entry inside an attribute group
/// (`#[Name(args), ...]`). `name` is the raw (possibly
/// namespaced/backslashed) class name exactly as `parse_qualified_name`
/// reads it, resolved against the
/// enclosing file's `use` imports the same way any other class-name
/// reference is (by the checker, not here - this node is deliberately as
/// dumb as `ClassConst`/`Param`, just structure). `args` reuses the exact
/// same `Arg` shape a real call's argument list uses (named arguments and
/// `...spread` included) - an attribute's own arguments are simply an
/// argument list applied to its constructor at compile time, real PHP's
/// own grammar doesn't distinguish the two syntactically at all.
#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<Arg>,
    pub span: Span,
}

/// A statement, plus the real source span it occupies - `kind` carries the
/// actual variant data (renamed from the old bare `Stmt` enum so the two
/// concerns - "what kind of statement" and "where it is" - stay separate).
#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
    // The raw text of a `/** ... */` doc comment immediately preceding this
    // statement, if any - lets the checker recognize an inline `@var`
    // assertion sitting on an arbitrary statement, not just on an actual
    // declaration (see `checker::stmt::inline_var_annotation`). A statement
    // that's also a declaration (e.g. a function) claiming its own doc
    // comment via `Parser::take_doc_comment` still gets the same raw text
    // here too - harmless, since a declaration's own docblock never has a
    // `@var $name` tag for that logic to act on.
    pub doc_comment: Option<String>,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
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
    // Every import a `use` statement brings in - a plain `use A, B;` list
    // and a group `use Foo\{A, B as C};` (each member's path prefixed
    // with the group path) both expand to one `UseImport` per name here,
    // rather than keeping only the first the way an earlier, unconsumed
    // version of this variant did.
    Use(Vec<UseImport>),
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
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CatchClause {
    pub types: Vec<String>,
    pub var: Option<String>,
    pub body: Vec<Stmt>,
    pub span: Span,
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
    // `public readonly int $id` in a promoted constructor parameter.
    // Meaningless when promoted_visibility is None.
    pub promoted_readonly: bool,
    // A promoted property's own hooks, defined inline in the constructor's
    // parameter list rather than on an ordinary property declaration.
    // Boxed because PropertyHooks::set itself embeds a Param - unboxed,
    // Param would have infinite size.
    pub hooks: Option<Box<PropertyHooks>>,
    // Phase 37: `#[Attr]` groups written directly on this parameter
    // (`function f(#[SensitiveParameter] string $password) {}`) - empty
    // for the overwhelming majority of parameters, which have none.
    pub attrs: Vec<Attribute>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<String>,
    pub body: Vec<Stmt>,
    // Raw `/** ... */` text immediately preceding this declaration, if any.
    pub doc_comment: Option<String>,
    // Phase 37: `#[Attr]` groups preceding this declaration - shared by a
    // top-level function and (via `MethodDecl.function`) a class method,
    // since real PHP attaches attributes to the same source position
    // either way (before any `public`/`static`/... modifiers).
    pub attrs: Vec<Attribute>,
    pub span: Span,
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
    pub is_readonly: bool,
    pub type_hint: Option<String>,
    pub default: Option<Expr>,
    pub doc_comment: Option<String>,
    // PHP 8.4 property hooks (`{ get => ...; set(Type $value) { ... } }`).
    // Parsed so the file doesn't fail to parse at all, but the hook bodies
    // aren't type-checked (they're arbitrary code referencing $this/$value
    // with no function-like registry entry to check them against) - out of
    // scope for now, same as FirstClassCallable's args.
    pub hooks: Option<PropertyHooks>,
    // Phase 37: `#[Attr]` groups preceding this property declaration.
    pub attrs: Vec<Attribute>,
    pub span: Span,
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
    pub is_final: bool,
    pub function: FunctionDecl,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct UseImport {
    pub path: String,
    pub alias: Option<String>,
    // Phase 48: which of PHP's three `use`-import namespaces this member
    // belongs to (`use Foo;` vs `use function foo;` vs `use const FOO;`) -
    // needed because only a class-shaped and function-shaped import are
    // ever existence-checkable against this project's own registry (a
    // plain `use Foo;` isn't checked for existence at all, matching real
    // PHPStan's own `ExistingNamesInUseRule`, which - confirmed directly
    // against its source - only ever calls `checkClassNames` with a `null`
    // usage location for the class case, i.e. only case-sensitivity/
    // forbidden-name checking, never a `hasClass` existence check; a class
    // reference is only ever existence-checked at the point it's actually
    // *used*, not merely imported). `use const` existence is deliberately
    // left unchecked here too - this project has no project-wide global-
    // constant registry to check it against at all (`ast::StmtKind::
    // GlobalConst` is currently only used to *infer a value's type*, not
    // indexed into any name table), and PHP's own huge, extension-
    // dependent built-in constant surface makes a half-built one worse
    // than none.
    pub kind: UseKind,
}

/// See `UseImport::kind`'s own doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind {
    Class,
    Function,
    Const,
}

#[derive(Debug, Clone)]
pub struct ClassConst {
    pub name: String,
    pub value: Expr,
    // Phase 59: the `public`/`private`/`protected` modifier a real class's
    // own `const` declaration can carry (PHP 7.1+) - parsed by both
    // `parser/classes.rs` call sites already (into a local `visibility`
    // that used to be discarded once `Const` was reached), but never kept
    // on the node itself until `UnusedPrivateConstantRule` needed to tell
    // a private constant candidate apart from a public/protected one.
    // `Visibility::Public` for the top-level (non-class) `const NAME =
    // value;` statement, which reuses this same struct shape (see
    // `StmtKind::GlobalConst`'s own doc comment) but has no visibility
    // concept of its own - a top-level const is never a legal "unused
    // private constant" candidate, so this default is never actually
    // read by that check.
    pub visibility: Visibility,
    // A real, common PHPDoc convention for pinning a `const array`'s own
    // element type more precisely than its native declaration ever can
    // (e.g. `@var list<class-string<MiddlewareInterface>>` on a `public
    // const array X = [...]`) - `None` for a const with no doc comment
    // directly above it (the common case) or one parsed via the rarer
    // top-level (non-class) `const` statement path, which doesn't thread
    // a doc comment through at all.
    pub doc_comment: Option<String>,
    // Phase 37: `#[Attr]` groups preceding this const declaration - the
    // same group applies to every name in a comma-separated
    // `const A = 1, B = 2;` list, mirroring how `visibility`/`doc_comment`
    // are already cloned onto each one.
    pub attrs: Vec<Attribute>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassKind {
    Class,
    Interface,
    Trait,
    Enum,
}

#[derive(Debug, Clone)]
pub struct ClassDecl {
    pub name: String,
    pub kind: ClassKind,
    pub extends: Vec<String>,
    pub implements: Vec<String>,
    pub is_abstract: bool,
    pub is_final: bool,
    pub properties: Vec<PropertyDecl>,
    pub methods: Vec<MethodDecl>,
    pub consts: Vec<ClassConst>,
    // `use TraitA, TraitB;` inside the class body.
    pub uses: Vec<String>,
    // `use A, B { B::hello as helloB; }` - each `as`-clause that
    // introduces a new name for an existing trait method (a
    // visibility-only `as public;` with no new name isn't captured, since
    // it doesn't need any registration). `insteadof` conflict-resolution
    // clauses are parsed (so the file doesn't fail to parse) but not
    // modeled here - not needed to fix Finding 30's own `as`-alias gap.
    pub trait_aliases: Vec<TraitAlias>,
    // Every method/property name mentioned in ANY adaptation clause at
    // all (`insteadof` or `as`, including a visibility-only `as public;`)
    // - Finding 58's own trait-conflict check needs this to know a name
    // the class author already explicitly addressed, even though this
    // project doesn't model *which* trait `insteadof` actually picked.
    pub resolved_trait_conflicts: Vec<String>,
    // The class's own `/** ... */` doc comment, if any - needed for its
    // own `@template T` declarations (class-level generics). Previously
    // never captured at all: the parser only called `take_doc_comment`
    // before a `function`/property declaration, never before
    // `class`/`interface`/`trait`.
    pub doc_comment: Option<String>,
    // Phase 37: `#[Attr]` groups preceding this class/interface/trait
    // declaration (e.g. `#[Attribute(Attribute::TARGET_CLASS)]`, marking
    // the class itself usable as an attribute).
    pub attrs: Vec<Attribute>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumCase {
    pub name: String,
    // Some(...) for a backed enum (`case Hearts = 'H';`), None for a pure
    // one (`case Hearts;`).
    pub value: Option<Expr>,
    // Phase 37: `#[Attr]` groups preceding this case.
    pub attrs: Vec<Attribute>,
    pub span: Span,
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
    pub trait_aliases: Vec<TraitAlias>,
    pub resolved_trait_conflicts: Vec<String>,
    // The enum's own `/** ... */` doc comment, if any (Phase 66) - needed
    // only to reject a stray `@template` tag (enums can't be generic).
    pub doc_comment: Option<String>,
    // Phase 37: `#[Attr]` groups preceding this enum declaration.
    pub attrs: Vec<Attribute>,
    pub span: Span,
}

// `use A, B { B::hello as helloB; }` - see `ClassDecl.trait_aliases`.
#[derive(Debug, Clone)]
pub struct TraitAlias {
    pub trait_name: Option<String>,
    pub method: String,
    pub alias: String,
}

/// One argument in a call's own argument list (`Call`/`New`/`MethodCall`/
/// `DynamicMethodCall`/`StaticCall`/`NewAnonymousClass` all hold
/// `Vec<Arg>`, not a bare `Vec<Expr>`) - replaces what used to be a plain
/// `Expr`, since a call's real argument-to-parameter mapping depends on
/// more than just each argument's own written order: `name` is
/// `Some("matches")` for a named argument (`matches: $m`), letting a
/// resolver match it against the right declared parameter regardless of
/// where it appears among the arguments (real PHP itself doesn't require
/// named arguments to appear in declaration order); `spread` is `true`
/// for `...$args` - once true, every argument at or after this one in the
/// call can't be resolved to a specific declared parameter at all (the
/// unpacked array's real contents aren't known statically). The
/// overwhelmingly common case (`name: None, spread: false`) is a plain
/// positional argument, unchanged in meaning from a bare `Expr`.
#[derive(Debug, Clone)]
pub struct Arg {
    pub name: Option<String>,
    pub spread: bool,
    pub value: Expr,
}

/// One element of an `ExprKind::ArrayLiteral` (`[...]`) - `key` is
/// `Some(...)` for a `key => value` pair, `None` for a plain positional
/// element. `spread` is `true` for `...$other` (PHP's own array-unpacking
/// syntax inside a literal, not the same construct as a call's own
/// spread argument even though it shares the same `...` token) -
/// Finding 48's own per-element shape inference (`infer_expr`'s
/// `ArrayLiteral` arm) needs to know this to avoid treating a spread
/// element's own expression as if it were meant to occupy one specific
/// key: real PHP inserts a variable number of the spread value's own
/// elements at that point, shifting every subsequent positional key by
/// an amount this checker has no way to know statically.
#[derive(Debug, Clone)]
pub struct ArrayItem {
    pub key: Option<Expr>,
    pub value: Expr,
    pub spread: bool,
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

/// An expression, plus the real source span it occupies - see `Stmt`'s
/// equivalent doc comment.
#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Literal(Literal),
    Variable(String),
    // `${expr}` - the older "variable variable" syntax: the variable's own
    // name is computed at runtime by evaluating expr, so unlike Variable
    // there's no fixed name here to look up or bind in `locals`.
    DynamicVariable(Box<Expr>),
    ArrayLiteral(Vec<ArrayItem>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    CompoundAssign(BinOp, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Option<Box<Expr>>, Box<Expr>),
    Call(Box<Expr>, Vec<Arg>),
    New(Box<Expr>, Vec<Arg>),
    // The trailing `bool` is `true` for a nullsafe access (`$x?->prop`),
    // `false` for a plain `$x->prop` - the two are otherwise identical
    // syntactically, but real PHP's `?->` implicitly makes the whole
    // access `null` when its base is `null`, which `infer_expr` needs to
    // know to union `Null` into the result.
    PropertyAccess(Box<Expr>, String, bool),
    MethodCall(Box<Expr>, String, Vec<Arg>, bool),
    // `$obj->{$name}` / `$obj->{$name}(...)` - the property/method name is
    // itself computed from an expression, not statically known, so it
    // can't share PropertyAccess/MethodCall's plain-String name. Still
    // worth its own base+name-expr(+args) checking (undefined variables
    // inside the {} can still be caught), even though the accessed
    // member's own type never can be.
    DynamicPropertyAccess(Box<Expr>, Box<Expr>),
    DynamicMethodCall(Box<Expr>, Box<Expr>, Vec<Arg>),
    StaticAccess(String, String),
    StaticCall(String, String, Vec<Arg>),
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
    // to StmtKind::Throw, a bare `throw expr;` statement, which is still
    // dispatched separately and takes priority at statement position.
    Throw(Box<Expr>),
    // `new class(...) [extends X] [implements Y] { ... }` - the anonymous
    // class's own declaration, inline, plus its constructor arguments.
    // Never given a stable name multiple such expressions could share, so
    // it isn't indexed into the registry the way a normal class is; its
    // methods are still type-checked (see checker::check_expr).
    NewAnonymousClass(Box<ClassDecl>, Vec<Arg>),
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
        // Phase 37: `#[Attr]` groups on the closure expression itself
        // (`$f = #[Foo] function () {};`), not on any of its parameters.
        attrs: Vec<Attribute>,
    },
    ArrowFn {
        params: Vec<Param>,
        body: Box<Expr>,
        is_static: bool,
        return_type: Option<String>,
        // Phase 37: see `Closure.attrs` above.
        attrs: Vec<Attribute>,
    },
    Match {
        subject: Box<Expr>,
        arms: Vec<(Vec<Expr>, Expr)>,
        default: Option<Box<Expr>>,
    },
}
