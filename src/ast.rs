use std::sync::Arc;

use crate::token::Span;

/// One `Name(args)` entry inside an attribute group
/// (`#[Name(args), ...]`). `name` is the raw class name as written,
/// resolved by the checker (not here). `args` reuses `Arg`'s call-argument
/// shape, since PHP's grammar doesn't distinguish the two.
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
    // `@var $name` tag for that logic to act on. `Arc<str>`, not `String`:
    // moved straight out of the lexer's own interned `Token::DocComment`
    // (via `Parser`'s `doc_comments` map), and cloned onto more than one
    // statement/declaration when several share one preceding doc comment
    // (e.g. a comma-separated `use`/`const` list) - a refcount bump instead
    // of a full text copy each time.
    pub doc_comment: Option<Arc<str>>,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    InlineHtml(Arc<str>),
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
    // `declare(name=value, ...);` - each directive's name, its span, and
    // its value. The block form (`declare(...) { ... }`/`: ...
    // enddeclare;`) parses as a `Block` whose first statement is this
    // node with `block_mode` set, followed by the body's statements.
    Declare {
        directives: Vec<(Arc<str>, Span, Expr)>,
        block_mode: bool,
    },
    Break(Option<i64>),
    Continue(Option<i64>),
    Global(Vec<String>),
    FunctionDecl(FunctionDecl),
    ClassDecl(ClassDecl),
    Namespace(String),
    // Every import a `use` statement brings in - a plain `use A, B;`
    // list and a group `use Foo\{A, B as C};` both expand to one
    // `UseImport` per name here.
    Use(Vec<UseImport>),
    // Top-level `const NAME = value, ...;`, distinct from a class's own
    // consts (ClassConst is reused for the shape, since it's identical).
    GlobalConst(Vec<ClassConst>),
    // `static $x = 1, $y;` - a function-local static variable declaration
    // (name, optional initializer). Persistence across calls isn't
    // modeled - only its scoping (the variable is defined from here on)
    // matters for a single-call static analysis pass. The name is
    // `Arc<str>`, moved straight out of the lexer's own interned
    // `Token::Variable`, same as `ExprKind::Variable`.
    StaticVar(Vec<(Arc<str>, Option<Expr>)>),
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
    // `Arc<str>`, moved straight out of the lexer's own interned
    // `Token::Identifier` - see `FunctionDecl::name`'s own doc comment.
    Goto(Arc<str>),
    // `label:` - a goto target. Control flow through goto/labels isn't
    // modeled (no effect on narrowing, terminal-statement detection,
    // etc.) - only enough structure to parse past them.
    Label(Arc<str>),
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
    // See `FunctionDecl::return_type_span`'s own doc comment, one span
    // per `types` entry (same index).
    pub types_spans: Vec<Span>,
    // Taken via `Parser::expect_variable` - see `FunctionDecl::name`'s
    // own doc comment.
    pub var: Option<Arc<str>>,
    // `var`'s own `$name` token span, populated at the same
    // parse point `var` itself is consumed - `None` exactly when `var` is
    // (a catch clause with no bound variable, `catch (Foo) {}`).
    pub var_span: Option<Span>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    // `Arc<str>`, moved straight out of the lexer's own interned
    // `Token::Variable` - see `FunctionDecl::name`'s own doc comment for
    // why this preserves (rather than throws away) the lexer's own
    // interning.
    pub name: Arc<str>,
    // This parameter's own `$name` token's span - populated by
    // the parser at the exact point it consumes it (`expect_variable`),
    // needed for local-variable-declaration navigation (a read inside the
    // function body jumping back to its own parameter).
    pub name_span: Span,
    pub type_hint: Option<String>,
    // See `FunctionDecl::return_type_span`'s own doc comment - identical
    // shape and purpose, one level down.
    pub type_hint_span: Option<Span>,
    pub default: Option<Expr>,
    pub by_ref: bool,
    pub variadic: bool,
    // Some(visibility) for constructor property promotion, e.g.
    // `public function __construct(private int $id) {}`.
    pub promoted_visibility: Option<Visibility>,
    // `public readonly int $id` in a promoted constructor parameter.
    // Meaningless when promoted_visibility is None.
    pub promoted_readonly: bool,
    // `public final int $id` in a promoted constructor parameter -
    // real, valid PHP, unlike `abstract` (an abstract constructor can't
    // declare any promoted property at all - real fatal error).
    // Meaningless when promoted_visibility is None.
    pub promoted_final: bool,
    // A promoted property's own hooks, defined inline in the constructor's
    // parameter list rather than on an ordinary property declaration.
    // Boxed because PropertyHooks::set itself embeds a Param - unboxed,
    // Param would have infinite size.
    pub hooks: Option<Box<PropertyHooks>>,
    // `#[Attr]` groups written directly on this parameter
    // (`function f(#[SensitiveParameter] string $password) {}`) - empty
    // for the overwhelming majority of parameters, which have none.
    pub attrs: Vec<Attribute>,
    // Raw `/** ... */` text immediately preceding this
    // parameter, if any - real, idiomatic PHP for a promoted
    // constructor property (`/** @var array<string, mixed>|null */
    // public ?array $details,`), since it doubles as that property's own
    // docblock. `None` for the overwhelming majority of parameters.
    pub doc_comment: Option<Arc<str>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FunctionDecl {
    // `Arc<str>`, moved straight out of the lexer's own interned
    // `Token::Identifier` (`Parser::expect_ident`/`take_name_segment`,
    // `Parser::take_token`'s existing move-based construction) rather than
    // re-materializing a fresh `String` at this token->AST boundary - see
    // Fix 10 Stage 1's own design note (the "wash trap": a partial
    // `Token`-only conversion that still re-allocates here would gain
    // nothing).
    pub name: Arc<str>,
    // This function/method's own name token's span - `span`
    // below covers the *whole* declaration (from `function`/its
    // modifiers through the closing brace), which is where the LSP's own
    // go-to-definition would otherwise have to land instead of on the
    // identifier itself.
    pub name_span: Span,
    pub params: Vec<Param>,
    pub return_type: Option<String>,
    // The return type hint's own span (covering the whole
    // hint text - `?Foo`, `Foo|Bar`, ... - not a per-identifier span;
    // the LSP's own text-split utility recovers those on demand from
    // this span's source text). `None` exactly when `return_type` is.
    pub return_type_span: Option<Span>,
    // `Arc`, not a plain `Vec` - `registry::FunctionSig::from_decl` shares
    // this exact allocation into its own `body: Option<Arc<Vec<Stmt>>>`
    // field (an `Arc` clone, not a deep AST clone) rather than
    // re-materializing an owned copy at that boundary, the same
    // move/share-not-reallocate reasoning as `name`'s own doc comment
    // above. Never mutated after `parse_function_decl` builds it, so
    // sharing it needs no `Arc::make_mut` anywhere. `Arc`, not `Rc`: like
    // `src/intern.rs`'s own table, parsing/registry-building runs in
    // parallel across files via a real `rayon` thread pool.
    pub body: Arc<Vec<Stmt>>,
    // `false` for a bodiless `function f();` declaration (abstract and
    // interface methods), distinct from an empty `{}` body.
    pub has_body: bool,
    // `function &f()` - returns by reference.
    pub by_ref: bool,
    // Raw `/** ... */` text immediately preceding this declaration, if any.
    pub doc_comment: Option<Arc<str>>,
    // `#[Attr]` groups preceding this declaration - shared by a
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
    // See `FunctionDecl::name`'s own doc comment - moved straight out of
    // the lexer's own interned `Token::Variable`.
    pub name: Arc<str>,
    // See `FunctionDecl::name_span`'s own doc comment.
    pub name_span: Span,
    pub visibility: Visibility,
    // PHP 8.4 asymmetric visibility (`public private(set) string $x;`) -
    // the narrower write-side visibility, when one was declared
    // separately from `visibility` above (which always stays the
    // *read*-side visibility). `None` for an ordinary, symmetric
    // property, in which case a write is checked against `visibility`
    // itself.
    pub write_visibility: Option<Visibility>,
    pub is_static: bool,
    pub is_readonly: bool,
    // PHP 8.4 `final`/`abstract` property modifiers -
    // `is_abstract_member`/`is_final_member` are also parsed for
    // `MethodDecl`/`ClassConst`, threaded through here for a plain
    // property declaration too.
    pub is_abstract: bool,
    pub is_final: bool,
    pub type_hint: Option<String>,
    // See `FunctionDecl::return_type_span`'s own doc comment.
    pub type_hint_span: Option<Span>,
    pub default: Option<Expr>,
    pub doc_comment: Option<Arc<str>>,
    // PHP 8.4 property hooks (`{ get => ...; set(Type $value) { ... } }`).
    // Parsed so the file doesn't fail to parse at all, but the hook bodies
    // aren't type-checked (they're arbitrary code referencing $this/$value
    // with no function-like registry entry to check them against) - out of
    // scope for now, same as FirstClassCallable's args.
    pub hooks: Option<PropertyHooks>,
    // `#[Attr]` groups preceding this property declaration.
    pub attrs: Vec<Attribute>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PropertyHookBody {
    // `get => expr;` / `set(...) => expr;`
    Arrow(Expr),
    // `get { ... }` / `set(...) { ... }`
    Block(Vec<Stmt>),
    // `get;` / `set(...);` - a hook declared, with no
    // body at all (only legal inside an abstract class/interface). Real
    // PHP distinguishes this from "no hook declared" (`PropertyHooks::
    // get`/`set` staying `None`) - `hasHook()` needs to answer "is a
    // hook declared at all," independent of whether it has a body.
    Abstract,
}

#[derive(Debug, Clone)]
pub struct PropertyHooks {
    pub get: Option<PropertyHookBody>,
    // The optional `(Type $value)` parameter list is just the one param
    // PHP allows there; None when the hook omits it (`set { ... }` uses an
    // implicit $value).
    pub set: Option<(Option<Param>, PropertyHookBody)>,
    // The `#[Attr]` group(s) immediately preceding this hook's
    // own `get`/`set` keyword - see `PropertyHookAttributesRule`. Empty
    // (not `None`) when the hook has no attributes, matching every other
    // `attrs: Vec<Attribute>` field in this file.
    pub get_attrs: Vec<Attribute>,
    pub set_attrs: Vec<Attribute>,
    // The `/** ... */` doc comment immediately preceding this
    // hook's own `get`/`set` keyword (after any `#[Attr]` group(s)) -
    // mirrors `FunctionDecl::doc_comment` exactly, needed so a hook can
    // carry its own `@throws` tag the same way an ordinary method does.
    pub get_doc: Option<Arc<str>>,
    pub set_doc: Option<Arc<str>>,
    // Whether this hook itself carries its own `final` modifier
    // (independent of the property's own/declaring class's finality) -
    // see `TooWideMethodThrowTypeRule`'s property-hook sibling's own gate.
    pub get_final: bool,
    pub set_final: bool,
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
    // Always taken via `Parser::expect_ident` (a single-token name, never
    // namespace-qualified) - see `FunctionDecl::name`'s own doc comment.
    pub alias: Option<Arc<str>>,
    // Which of PHP's three `use`-import namespaces this member belongs
    // to (`use Foo;` vs `use function foo;` vs `use const FOO;`) - a
    // plain `use Foo;` isn't checked for existence at all, matching real
    // PHPStan's own `ExistingNamesInUseRule` (a class reference is only
    // existence-checked where it's actually *used*, not merely
    // imported). `use const` existence is also left unchecked - this
    // project has no project-wide global-constant registry to check it
    // against.
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
    // See `FunctionDecl::name`'s own doc comment - always taken via
    // `Parser::expect_ident`.
    pub name: Arc<str>,
    // See `FunctionDecl::name_span`'s own doc comment.
    pub name_span: Span,
    pub value: Expr,
    // The `public`/`private`/`protected` modifier a real class's own
    // `const` declaration can carry (PHP 7.1+), needed by
    // `UnusedPrivateConstantRule` to tell a private constant candidate
    // apart from a public/protected one.
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
    pub doc_comment: Option<Arc<str>>,
    // `#[Attr]` groups preceding this const declaration - the
    // same group applies to every name in a comma-separated
    // `const A = 1, B = 2;` list, mirroring how `visibility`/`doc_comment`
    // are already cloned onto each one.
    pub attrs: Vec<Attribute>,
    // PHP 8.3 typed class constants (`const TYPE NAME = value;`) - same
    // "one type applies to every name in a comma-separated list" cloning
    // as `visibility`/`doc_comment`/`attrs` above.
    pub type_hint: Option<String>,
    // See `FunctionDecl::return_type_span`'s own doc comment - also
    // cloned onto every name in a comma-separated list.
    pub type_hint_span: Option<Span>,
    // PHP 8.1+ `final const`, needed by
    // `FinalConstantRule`/`FinalPrivateConstantRule` to tell a `final`
    // constant apart from an ordinary one. Always `false` for the
    // top-level (non-class) `const NAME = value;` statement - real PHP
    // has no `final` syntax there at all.
    pub is_final: bool,
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
    // See `FunctionDecl::name`'s own doc comment - always taken via
    // `Parser::expect_ident`, except an anonymous class's own synthetic
    // placeholder (`"class@anonymous"`, interned like any other name -
    // real corpora declare many, so it dedups the same way a repeated
    // real class name would).
    pub name: Arc<str>,
    // See `FunctionDecl::name_span`'s own doc comment. For an
    // anonymous class (which has no real name to navigate to at all -
    // `name` is the synthetic placeholder `"class@anonymous"`) this is
    // just `span` again, never actually looked up by name.
    pub name_span: Span,
    pub kind: ClassKind,
    pub extends: Vec<String>,
    // One span per `extends` entry, same index, same purpose
    // as `FunctionDecl::return_type_span`.
    pub extends_spans: Vec<Span>,
    pub implements: Vec<String>,
    pub implements_spans: Vec<Span>,
    pub is_abstract: bool,
    pub is_final: bool,
    // `readonly class` (PHP 8.2), surfaced at the class level (in
    // addition to being spread onto each own property in
    // `parse_class_body`) for `check_class_like_marker_attributes`'s own
    // `#[\AllowDynamicProperties]`-on-a-readonly-class check (real
    // PHPStan's own `ClassAttributesRule`).
    pub is_readonly: bool,
    pub properties: Vec<PropertyDecl>,
    pub methods: Vec<MethodDecl>,
    pub consts: Vec<ClassConst>,
    // `use TraitA, TraitB;` inside the class body.
    pub uses: Vec<String>,
    pub uses_spans: Vec<Span>,
    // The doc comment of the `use` *statement* each `uses` entry came
    // from (not the class's own, separate `doc_comment` below) - same
    // index/shape as `uses_spans`. Needed for `@use Trait<Args>`: real
    // PHPStan resolves that tag from the `TraitUse` statement's own doc
    // comment, not the class's top-of-class docblock.
    pub uses_doc_comments: Vec<Option<Arc<str>>>,
    // `use A, B { B::hello as helloB; }` - each `as`-clause that
    // introduces a new name for an existing trait method (a
    // visibility-only `as public;` with no new name isn't captured, since
    // it doesn't need any registration). `insteadof` conflict-resolution
    // clauses are parsed (so the file doesn't fail to parse) but not
    // modeled here.
    pub trait_aliases: Vec<TraitAlias>,
    // Every method/property name mentioned in ANY adaptation clause at
    // all (`insteadof` or `as`, including a visibility-only `as public;`)
    // - the trait-conflict check needs this to know a name the class
    // author already explicitly addressed, even though this project
    // doesn't model *which* trait `insteadof` actually picked.
    pub resolved_trait_conflicts: Vec<Arc<str>>,
    // The class's own `/** ... */` doc comment, if any - needed for its
    // own `@template T` declarations (class-level generics).
    pub doc_comment: Option<Arc<str>>,
    // `#[Attr]` groups preceding this class/interface/trait
    // declaration (e.g. `#[Attribute(Attribute::TARGET_CLASS)]`, marking
    // the class itself usable as an attribute).
    pub attrs: Vec<Attribute>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumCase {
    // See `FunctionDecl::name`'s own doc comment.
    pub name: Arc<str>,
    // See `FunctionDecl::name_span`'s own doc comment.
    pub name_span: Span,
    // Some(...) for a backed enum (`case Hearts = 'H';`), None for a pure
    // one (`case Hearts;`).
    pub value: Option<Expr>,
    // `#[Attr]` groups preceding this case.
    pub attrs: Vec<Attribute>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    // See `FunctionDecl::name`'s own doc comment.
    pub name: Arc<str>,
    pub name_span: Span,
    // "int" | "string" for a backed enum, None for a pure one. Enums can't
    // extend anything or declare instance properties, unlike ClassDecl.
    pub backing_type: Option<String>,
    pub implements: Vec<String>,
    pub implements_spans: Vec<Span>,
    pub cases: Vec<EnumCase>,
    pub methods: Vec<MethodDecl>,
    pub consts: Vec<ClassConst>,
    pub uses: Vec<String>,
    pub uses_spans: Vec<Span>,
    // See `ClassDecl::uses_doc_comments`'s own doc comment.
    pub uses_doc_comments: Vec<Option<Arc<str>>>,
    pub trait_aliases: Vec<TraitAlias>,
    pub resolved_trait_conflicts: Vec<Arc<str>>,
    // The enum's own `/** ... */` doc comment, if any - needed
    // only to reject a stray `@template` tag (enums can't be generic).
    pub doc_comment: Option<Arc<str>>,
    // `#[Attr]` groups preceding this enum declaration.
    pub attrs: Vec<Attribute>,
    pub span: Span,
}

// `use A, B { B::hello as helloB; }` - see `ClassDecl.trait_aliases`.
#[derive(Debug, Clone)]
pub struct TraitAlias {
    pub trait_name: Option<Arc<str>>,
    pub method: Arc<str>,
    pub alias: Arc<str>,
}

/// One argument in a call's own argument list - `name` is
/// `Some("matches")` for a named argument (`matches: $m`), which real PHP
/// allows out of declaration order; `spread` is `true` for `...$args`,
/// after which no later argument can be resolved to a specific declared
/// parameter (the unpacked array's contents aren't known statically).
#[derive(Debug, Clone)]
pub struct Arg {
    pub name: Option<String>,
    pub spread: bool,
    pub value: Expr,
}

/// One element of an `ExprKind::ArrayLiteral` (`[...]`) - `key` is
/// `Some(...)` for a `key => value` pair, `None` for a plain positional
/// element. `spread` is `true` for `...$other` (PHP's own array-unpacking
/// syntax) - a spread inserts an unknown number of elements, shifting
/// every subsequent positional key by an amount this checker can't know
/// statically.
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
    // `Arc<str>`, not `String`: fed both by an ordinary `Token::StringLit`
    // (plain `String`, converted at that one construction site - a string
    // literal's own contents have low interning payoff, same reasoning as
    // `Token::StringLit` itself staying `String`) and by
    // `StringPart::Literal`/`SimpleIndexKey::Str` (already interned
    // `Arc<str>` - an interpolated string's own literal chunks, moved
    // straight through with no extra allocation). Keeping this `String`
    // would make the latter, explicitly-interned path a wash (the
    // interned value immediately re-copied into a fresh `String` at its
    // very next - and only - consumer).
    String(Arc<str>),
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
    // The word forms of &&/|| - runtime-identical to And/Or (only their
    // *precedence* differs, a parse-time concern already resolved by the
    // time this tree exists), kept as their own variants (not folded into
    // And/Or the way this comment used to argue for) because real
    // PHPStan's own identifier/message text genuinely differs by spelling
    // (`booleanAnd.*`/`logicalAnd.*`, `"&&"` vs `"and"` in the rendered
    // message) - every consumer that only cares about runtime semantics
    // matches `BinOp::And | BinOp::LogicalAnd` (`BinOp::Or | BinOp::
    // LogicalOr`) together; only the diagnostic-identifier/message sites
    // distinguish them.
    LogicalAnd,
    LogicalOr,
    // The `xor` keyword operator - logical, not `^`'s bitwise BitXor. Has
    // no symbol counterpart at all, so it never needed an And/Or-style
    // split.
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
    // See `FunctionDecl::name`'s own doc comment - taken via
    // `Parser::expect_variable`.
    pub name: Arc<str>,
    pub by_ref: bool,
    // The captured variable's own name span - lets a `use (&$x)`
    // capture that auto-vivifies `$x` in the outer scope (see
    // `checker::calls::check_closure_literal_with_this_and_scope`'s sibling
    // write-back logic) declare a real, navigable definition site for it,
    // the same way `Param::name_span` already does for a parameter.
    pub span: Span,
}

// The right-hand side of `instanceof`: almost always a static class name,
// but PHP also allows an arbitrary expression there (`$x instanceof
// $className`, `$x instanceof $obj`) for a runtime-determined class.
#[derive(Debug, Clone)]
pub enum InstanceofTarget {
    // The `Span` is the class-name token's own span - see
    // `FunctionDecl::name_span`'s own doc comment for why this differs
    // from `Instanceof`'s own (whole-expression) span.
    Name(String, Span),
    Expr(Box<Expr>),
}

/// `ExprKind::StaticCall`'s own receiver - `InstanceofTarget`'s identical
/// shape, reused rather than duplicated with different field names: a
/// literal class name (`Foo::method()`, still the overwhelming common
/// case) plus its own name span, or a genuinely dynamic receiver
/// expression (`$obj::method()`, `(new Foo())::method()`, `self::$
/// method()` when `$method` isn't itself the receiver but the member
/// name - only the *receiver* side is ever `Expr` here).
#[derive(Debug, Clone)]
pub enum StaticTarget {
    Name(String, Span),
    Expr(Box<Expr>),
}

impl StaticTarget {
    /// The literal class name, or `None` for a genuinely dynamic
    /// receiver - the drop-in replacement for every consumer that only
    /// ever needs `StaticCall`'s old permanent `"?"` sentinel string:
    /// `target.as_name().unwrap_or("?")` reproduces that exact prior
    /// behavior byte-for-byte (a real class registry/pseudo-name lookup
    /// against the literal `"?"` already always failed/mismatched, the
    /// same outcome `None` naturally produces here too), for every site
    /// that isn't `check_static_call`'s own new receiver-type resolution.
    pub(crate) fn as_name(&self) -> Option<&str> {
        match self {
            StaticTarget::Name(n, _) => Some(n),
            StaticTarget::Expr(_) => None,
        }
    }
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
    // `Arc<str>`, moved straight out of the lexer's own interned
    // `Token::Variable` (or, for a string-interpolation `$var`/`$var->
    // prop`/`$var[key]` occurrence, the identically-interned
    // `StringPart::SimpleVar`/`PropertyAccess::var`/`Index::var`/
    // `SimpleIndexKey::Var`) - see `FunctionDecl::name`'s own doc comment
    // for why this preserves rather than discards that interning.
    Variable(Arc<str>),
    // `${expr}` - the older "variable variable" syntax: the variable's own
    // name is computed at runtime by evaluating expr, so unlike Variable
    // there's no fixed name here to look up or bind in `locals`.
    DynamicVariable(Box<Expr>),
    ArrayLiteral(Vec<ArrayItem>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    // `$a = &$b;` - reference assignment, only ever valid on a plain
    // `=` (never a compound one). A genuinely distinct variant, not a
    // flag on `Assign`, specifically so a `nullsafe.byRef` check
    // (`$a = &$b?->c;`) can tell this RHS position apart from an
    // ordinary read - aliasing itself still isn't modeled beyond that
    // (nothing tracks that two variables now share storage; the RHS's
    // own type still just flows through normally, identically to
    // `Assign`). Every other consumer that doesn't care about the
    // by-ref distinction folds this into the same arm as `Assign` via
    // an or-pattern.
    AssignRef(Box<Expr>, Box<Expr>),
    CompoundAssign(BinOp, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Option<Box<Expr>>, Box<Expr>),
    Call(Box<Expr>, Vec<Arg>),
    New(Box<Expr>, Vec<Arg>),
    // The trailing `bool` is `true` for a nullsafe access (`$x?->prop`),
    // `false` for a plain `$x->prop` - the two are otherwise identical
    // syntactically, but real PHP's `?->` implicitly makes the whole
    // access `null` when its base is `null`, which `infer_expr` needs to
    // know to union `Null` into the result.
    // The property/method name: `Arc<str>`, moved straight out of an
    // interned `Token::Identifier`/`Token::Keyword` (via `Parser::
    // take_name_segment`) - or, for a string-interpolation `$obj->prop`
    // occurrence, the identically-interned `StringPart::
    // PropertyAccess::prop`. A `$obj->$prop` dynamic-by-variable-name
    // access is a `DynamicPropertyAccess`/`DynamicMethodCall` below, not
    // this variant - `$prop`'s own runtime value, not its declared name,
    // is the real member name.
    PropertyAccess(Box<Expr>, Arc<str>, bool),
    MethodCall(Box<Expr>, Arc<str>, Vec<Arg>, bool),
    // `$obj->{$name}` / `$obj->{$name}(...)` - the property/method name is
    // itself computed from an expression, not statically known, so it
    // can't share PropertyAccess/MethodCall's plain-String name. Still
    // worth its own base+name-expr(+args) checking (undefined variables
    // inside the {} can still be caught), even though the accessed
    // member's own type never can be.
    DynamicPropertyAccess(Box<Expr>, Box<Expr>),
    DynamicMethodCall(Box<Expr>, Box<Expr>, Vec<Arg>),
    StaticAccess(String, String),
    // `Foo::{$expr}` - PHP 8.3's dynamic class-constant fetch; the class
    // is as in `StaticAccess`, the second field the name expression.
    DynamicClassConstFetch(String, Box<Expr>),
    // `Name` mirrors `InstanceofTarget::Name`'s own (name, name-span)
    // shape exactly - a `Foo::method()` receiver, unchanged from before
    // this variant existed. `Expr` is a real, non-`Name` receiver
    // expression (`$obj::method()`) - previously discarded into a
    // permanent `"?"` sentinel string at parse time; now the real,
    // already-parsed receiver `Expr` survives into the AST, letting
    // `check_static_call` resolve its inferred type and check the call
    // against it, the same way a literal class name already is.
    StaticCall(StaticTarget, String, Vec<Arg>),
    Index(Box<Expr>, Option<Box<Expr>>),
    Name(String),
    Instanceof(Box<Expr>, InstanceofTarget),
    // Cast target is normalized to one of: int, float, string, bool,
    // array, object, unset. The trailing `bool` is `DeprecatedCastRule`'s
    // own concern only: true for the four non-canonical spellings PHP
    // 8.5 deprecates - `(integer)`, `(boolean)`, `(double)`, `(binary)` -
    // false for every canonical spelling (including `(real)`, which real
    // PHPStan's own `DeprecatedCastRule` never flags either, a faithful
    // gap - see its own `KIND_FLOAT`-vs-`KIND_DOUBLE`-only check).
    Cast(String, Box<Expr>, bool),
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
    // `` `cmd $interpolated` `` - shell-exec shorthand. Wraps the
    // identically-shaped `Call(Name("shell_exec"), [cmd])` a plain
    // `shell_exec($cmd)` call would itself produce (built once, at parse
    // time, by the exact same code) purely so `BacktickRule`'s own
    // `backtick.deprecated` check can tell "this was literally backtick
    // syntax" apart from "the author wrote shell_exec() themselves" -
    // every other consumer (type inference, argument checking, ...) just
    // delegates straight through to the wrapped `Call`, unchanged.
    ShellExec(Box<Expr>),
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
        // `#[Attr]` groups on the closure expression itself
        // (`$f = #[Foo] function () {};`), not on any of its parameters.
        attrs: Vec<Attribute>,
    },
    ArrowFn {
        params: Vec<Param>,
        body: Box<Expr>,
        is_static: bool,
        // `fn &() => ...` - returns by reference.
        by_ref: bool,
        return_type: Option<String>,
        // See `Closure.attrs` above.
        attrs: Vec<Attribute>,
    },
    Match {
        subject: Box<Expr>,
        arms: Vec<(Vec<Expr>, Expr)>,
        default: Option<Box<Expr>>,
    },
}
