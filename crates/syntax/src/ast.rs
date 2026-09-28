use std::sync::Arc;

use crate::name::Name;
use crate::php_string::PhpString;
use crate::token::Span;

/// A syntax node's identity within one parsed file: the parser numbers
/// every `Stmt` and `Expr` densely from 0 in the order it builds them, so a
/// side table can be a plain vector indexed by `NodeId::index`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl NodeId {
    /// The id of a node built outside the parser (a probe or a rewritten
    /// expression), which no side table has an entry for.
    pub const SYNTHETIC: NodeId = NodeId(u32::MAX);

    pub(crate) fn new(index: u32) -> NodeId {
        debug_assert!(index != u32::MAX, "node ids exhausted");
        NodeId(index)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// One `Name(args)` entry inside an attribute group
/// (`#[Name(args), ...]`). `name` is the raw class name as written,
/// resolved by the checker (not here). `args` reuses `Arg`'s call-argument
/// shape, since PHP's grammar doesn't distinguish the two.
#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: QualifiedName,
    pub args: Vec<Arg>,
    pub span: Span,
    // What deleting this attribute removes: its whole `#[...]` group up
    // to the next token when it's alone in the group, else its own entry
    // with the comma separating it from a neighbour.
    pub removal_span: Span,
}

/// A statement, plus the real source span it occupies - `kind` carries the
/// actual variant data (renamed from the old bare `Stmt` enum so the two
/// concerns - "what kind of statement" and "where it is" - stay separate).
#[derive(Debug, Clone)]
pub struct Stmt {
    pub id: NodeId,
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
    // The line `doc_comment` starts on - where real PHPStan reports a
    // finding about the comment itself (`varTag.unresolvableType`).
    pub doc_line: Option<u32>,
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
    // `declare(name=value, ...)`: each directive's name and value, and
    // the statements of a `declare(...) { ... }` or `declare(...): ...
    // enddeclare;` block.
    Declare {
        directives: Vec<(Ident, Expr)>,
        body: Option<Vec<Stmt>>,
    },
    Break(Option<i64>),
    Continue(Option<i64>),
    Global(Vec<GlobalVar>),
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
    // without its `$`.
    StaticVar(Vec<(Ident, Option<Expr>)>),
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
    Goto(Ident),
    // `label:` - a goto target. Control flow through goto/labels isn't
    // modeled (no effect on narrowing, terminal-statement detection,
    // etc.) - only enough structure to parse past them.
    Label(Ident),
}

#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub test: Option<Expr>, // None = `default:`
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CatchClause {
    pub types: Vec<QualifiedName>,
    // Each `types` entry's span (same index).
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
    // Moved out of the lexer's interned `Token::Variable`, without its `$`.
    pub name: Arc<str>,
    // This parameter's own `$name` token's span - populated by
    // the parser at the exact point it consumes it (`expect_variable`),
    // needed for local-variable-declaration navigation (a read inside the
    // function body jumping back to its own parameter).
    pub name_span: Span,
    pub type_hint: Option<TypeHint>,
    pub default: Option<Expr>,
    pub by_ref: bool,
    pub variadic: bool,
    // Some(visibility) for constructor property promotion, e.g.
    // `public function __construct(private int $id) {}`.
    pub promoted_visibility: Option<Visibility>,
    // `private(set)` on a promoted constructor parameter: who may write the
    // property. `None` without one.
    pub promoted_set_visibility: Option<Visibility>,
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
    pub name: Name,
    // The name with the file's namespace, for a function declared as a
    // statement; `None` for a method. Filled in by name resolution.
    pub namespaced_name: Option<Arc<str>>,
    // This function/method's own name token's span - `span`
    // below covers the *whole* declaration (from `function`/its
    // modifiers through the closing brace), which is where the LSP's own
    // go-to-definition would otherwise have to land instead of on the
    // identifier itself.
    pub name_span: Span,
    pub params: Vec<Param>,
    pub return_type: Option<TypeHint>,
    // `Arc`, not a plain `Vec` - `registry::FunctionReflection::from_decl` shares
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
    // The body's opening `{`, when it has one.
    pub body_open: Option<Span>,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Visibility {
    Public,
    Private,
    Protected,
}

#[derive(Debug, Clone)]
pub struct PropertyDecl {
    // Without its `$`.
    pub name: Name,
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
    pub type_hint: Option<TypeHint>,
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
    // What deleting this member removes (its doc comment and attributes
    // included, and its whole lines when it has them to itself); `None`
    // when it shares a declaration with other members.
    pub removal_span: Option<Span>,
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
    // `&get`: the hook returns its value by reference.
    pub get_by_ref: bool,
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
    // What deleting this member removes (its doc comment and attributes
    // included, and its whole lines when it has them to itself); `None`
    // when it shares a declaration with other members.
    pub removal_span: Option<Span>,
}

#[derive(Debug, Clone)]
pub struct UseImport {
    // The fully-qualified name imported, without a leading `\`.
    pub path: Name,
    // The name as written in source (a group member's own part only).
    pub written: Ident,
    // `as Alias`.
    pub alias: Option<Ident>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UseKind {
    Class,
    Function,
    Const,
}

#[derive(Debug, Clone)]
pub struct ClassConst {
    pub name: Name,
    // The name with the file's namespace, for a global `const`; `None` in
    // a class. Filled in by name resolution.
    pub namespaced_name: Option<Arc<str>>,
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
    // Cloned onto every name in a comma-separated list.
    pub type_hint: Option<TypeHint>,
    // PHP 8.1+ `final const`, needed by
    // `FinalConstantRule`/`FinalPrivateConstantRule` to tell a `final`
    // constant apart from an ordinary one. Always `false` for the
    // top-level (non-class) `const NAME = value;` statement - real PHP
    // has no `final` syntax there at all.
    pub is_final: bool,
    pub span: Span,
    // What deleting this member removes (its doc comment and attributes
    // included, and its whole lines when it has them to itself); `None`
    // when it shares a declaration with other members.
    pub removal_span: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassKind {
    Class,
    Interface,
    Trait,
    Enum,
}

#[derive(Debug, Clone)]
pub struct ClassDecl {
    // `None` for an anonymous class.
    pub name: Option<Name>,
    // The name with the file's namespace; `None` for an anonymous class.
    // Filled in by name resolution.
    pub namespaced_name: Option<Arc<str>>,
    // See `FunctionDecl::name_span`'s own doc comment. For an
    // anonymous class (which has no real name to navigate to at all -
    // `name` is the synthetic placeholder `"class@anonymous"`) this is
    // just `span` again, never actually looked up by name.
    pub name_span: Span,
    pub kind: ClassKind,
    pub extends: Vec<QualifiedName>,
    // Each `extends` entry's span (same index).
    pub extends_spans: Vec<Span>,
    pub implements: Vec<QualifiedName>,
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
    pub uses: Vec<QualifiedName>,
    pub uses_spans: Vec<Span>,
    // The doc comment of the `use` *statement* each `uses` entry came
    // from (not the class's own, separate `doc_comment` below) - same
    // index/shape as `uses_spans`. Needed for `@use Trait<Args>`: real
    // PHPStan resolves that tag from the `TraitUse` statement's own doc
    // comment, not the class's top-of-class docblock.
    pub uses_doc_comments: Vec<Option<Arc<str>>>,
    // `use A, B { B::hello as protected helloB; A::hi as private; }`: each
    // `as` clause, with its visibility and new name.
    pub trait_aliases: Vec<TraitAlias>,
    // `use A, B { B::hello insteadof A; }`: each `insteadof` clause.
    pub trait_precedences: Vec<TraitPrecedence>,
    // Every method name an adaptation clause mentions.
    pub resolved_trait_conflicts: Vec<Name>,
    // `case` members, which PHP parses in any class-like body but only
    // allows in an enum (`enum.caseOutsideOfEnum`).
    pub enum_cases: Vec<EnumCase>,
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
    pub name: Name,
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
    pub name: Name,
    // The name with the file's namespace. Filled in by name resolution.
    pub namespaced_name: Option<Arc<str>>,
    pub name_span: Span,
    // "int" | "string" for a backed enum, None for a pure one. Enums can't
    // extend anything or declare instance properties, unlike ClassDecl.
    pub backing_type: Option<TypeHint>,
    pub implements: Vec<QualifiedName>,
    pub implements_spans: Vec<Span>,
    pub cases: Vec<EnumCase>,
    pub methods: Vec<MethodDecl>,
    pub consts: Vec<ClassConst>,
    pub uses: Vec<QualifiedName>,
    pub uses_spans: Vec<Span>,
    // See `ClassDecl::uses_doc_comments`'s own doc comment.
    pub uses_doc_comments: Vec<Option<Arc<str>>>,
    pub trait_aliases: Vec<TraitAlias>,
    pub trait_precedences: Vec<TraitPrecedence>,
    pub resolved_trait_conflicts: Vec<Name>,
    // The enum's own `/** ... */` doc comment, if any - needed
    // only to reject a stray `@template` tag (enums can't be generic).
    pub doc_comment: Option<Arc<str>>,
    // `#[Attr]` groups preceding this enum declaration.
    pub attrs: Vec<Attribute>,
    pub span: Span,
}

/// `[Trait::]method as [visibility] [alias];` in a trait-use block.
#[derive(Debug, Clone)]
pub struct TraitAlias {
    pub trait_name: Option<QualifiedName>,
    pub method: Name,
    pub visibility: Option<Visibility>,
    pub alias: Option<Name>,
}

/// `Trait::method insteadof Other, ...;` in a trait-use block: `trait_name`'s
/// `method` wins over the one each trait in `instead_of` has.
#[derive(Debug, Clone)]
pub struct TraitPrecedence {
    pub trait_name: QualifiedName,
    pub method: Name,
    pub instead_of: Vec<QualifiedName>,
}

/// A trait-use block's adaptations.
#[derive(Debug, Clone, Default)]
pub struct TraitAdaptations {
    pub aliases: Vec<TraitAlias>,
    pub precedences: Vec<TraitPrecedence>,
    /// Every method name a clause mentions.
    pub mentioned: Vec<Name>,
}

/// One argument in a call's own argument list - `name` is
/// `Some("matches")` for a named argument (`matches: $m`), which real PHP
/// allows out of declaration order; `spread` is `true` for `...$args`,
/// after which no later argument can be resolved to a specific declared
/// parameter (the unpacked array's contents aren't known statically).
#[derive(Debug, Clone)]
pub struct Arg {
    // `name:` for a named argument.
    pub name: Option<Ident>,
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
    String(PhpString),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    // Moved out of the lexer's interned `Token::Variable`, without its `$`.
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
    Name(QualifiedName, Span),
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
    Name(QualifiedName, Span),
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
    pub fn as_name(&self) -> Option<&str> {
        match self {
            StaticTarget::Name(n, _) => Some(n),
            StaticTarget::Expr(_) => None,
        }
    }

    /// The receiver expression, or `None` for a literal class name.
    pub fn expr(&self) -> Option<&Expr> {
        match self {
            StaticTarget::Name(..) => None,
            StaticTarget::Expr(e) => Some(e),
        }
    }

    /// The property of a parent hook call's target, `parent::$property`
    /// in `parent::$property::get()`.
    pub fn parent_hook_property(&self) -> Option<&str> {
        match &self.expr()?.kind {
            ExprKind::StaticPropertyFetch(StaticTarget::Name(class, _), property)
                if class.eq_ignore_ascii_case("parent") =>
            {
                Some(property.as_ref())
            }
            _ => None,
        }
    }
}

/// A reference to a class, function or constant by name: as written, and
/// what it refers to given the file's `namespace` and `use` statements
/// (filled in by `name_resolution::resolve_names` after parsing). Derefs
/// to the name as written.
#[derive(Debug, Clone)]
pub struct QualifiedName {
    pub written: Name,
    pub resolved: ResolvedName,
}

/// What a [`QualifiedName`] refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedName {
    /// `self`, `parent` or `static`, a keyword type, or a name the
    /// resolution pass has not seen.
    Unresolved,
    /// The one fully-qualified name it refers to (no leading `\\`).
    Exact(Box<str>),
    /// An unqualified function or constant name in a namespace: this
    /// namespaced name if one is declared, else the global one, which is
    /// the name as written.
    WithGlobalFallback(Box<str>),
}

impl QualifiedName {
    pub fn new(written: impl Into<Name>) -> QualifiedName {
        QualifiedName {
            written: written.into(),
            resolved: ResolvedName::Unresolved,
        }
    }

    /// The fully-qualified class this names; `None` for `self`, `parent`
    /// and `static`, and for a name resolution has not seen.
    pub fn resolved_class(&self) -> Option<&str> {
        match &self.resolved {
            ResolvedName::Exact(name) => Some(name),
            _ => None,
        }
    }
}

impl std::ops::Deref for QualifiedName {
    type Target = str;

    fn deref(&self) -> &str {
        &self.written
    }
}

impl std::fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.written)
    }
}

impl PartialEq<str> for QualifiedName {
    fn eq(&self, other: &str) -> bool {
        self.written == *other
    }
}

impl PartialEq<&str> for QualifiedName {
    fn eq(&self, other: &&str) -> bool {
        self.written == **other
    }
}

/// A name written in the source - a member, method or constant name -
/// with its span. Derefs to the name.
#[derive(Debug, Clone)]
pub struct Ident {
    pub name: Name,
    pub span: Span,
}

impl std::ops::Deref for Ident {
    type Target = str;

    fn deref(&self) -> &str {
        &self.name
    }
}

impl PartialEq<str> for Ident {
    fn eq(&self, other: &str) -> bool {
        self.name == *other
    }
}

impl PartialEq<&str> for Ident {
    fn eq(&self, other: &&str) -> bool {
        self.name == **other
    }
}

impl std::fmt::Display for Ident {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// A native type declaration (`int`, `?Foo`, `A|B`, `(A&B)|null`) as
/// written, with the span of each part.
#[derive(Debug, Clone)]
pub struct TypeHint {
    pub kind: TypeHintKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeHintKind {
    /// A keyword type or a class name, as written (`int`, `static`,
    /// `\App\Foo`).
    Name(QualifiedName),
    /// `?T`.
    Nullable(Box<TypeHint>),
    /// `A|B`.
    Union(Vec<TypeHint>),
    /// `A&B`, or a parenthesized `(A&B)` term of a union.
    Intersection(Vec<TypeHint>),
}

impl TypeHint {
    /// Whether two optional declarations are written the same (spans
    /// aside).
    pub fn same(a: Option<&TypeHint>, b: Option<&TypeHint>) -> bool {
        a.map(TypeHint::to_string) == b.map(TypeHint::to_string)
    }

    /// Every class the declaration names (keyword types like `int` or
    /// `static` left out), as written, with its span, in source order.
    pub fn class_names(&self) -> Vec<(&str, Span)> {
        let mut out = Vec::new();
        self.collect_class_names(&mut out);
        out
    }

    fn collect_class_names<'a>(&'a self, out: &mut Vec<(&'a str, Span)>) {
        match &self.kind {
            TypeHintKind::Name(name) => {
                if !is_primitive_type_name(name.trim_start_matches('\\')) {
                    out.push((name, self.span));
                }
            }
            TypeHintKind::Nullable(inner) => inner.collect_class_names(out),
            TypeHintKind::Union(members) | TypeHintKind::Intersection(members) => {
                for member in members {
                    member.collect_class_names(out);
                }
            }
        }
    }
}

impl std::fmt::Display for TypeHint {
    /// The declaration's text, normalized: no whitespace, and a union's
    /// intersection terms parenthesized.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            TypeHintKind::Name(name) => f.write_str(name),
            TypeHintKind::Nullable(inner) => write!(f, "?{inner}"),
            TypeHintKind::Union(members) => {
                for (i, member) in members.iter().enumerate() {
                    if i > 0 {
                        f.write_str("|")?;
                    }
                    match member.kind {
                        TypeHintKind::Intersection(_) => write!(f, "({member})")?,
                        _ => write!(f, "{member}")?,
                    }
                }
                Ok(())
            }
            TypeHintKind::Intersection(members) => {
                for (i, member) in members.iter().enumerate() {
                    if i > 0 {
                        f.write_str("&")?;
                    }
                    write!(f, "{member}")?;
                }
                Ok(())
            }
        }
    }
}

/// One variable a `global` statement imports.
#[derive(Debug, Clone)]
pub enum GlobalVar {
    /// `global $name;` (the name is without its `$`).
    Name(Ident),
    /// `global ${expr};` / `global $$name;`: the name is computed.
    Dynamic(Expr),
}

impl GlobalVar {
    /// The variable's name, when it is written out.
    pub fn name(&self) -> Option<&str> {
        match self {
            GlobalVar::Name(name) => Some(name),
            GlobalVar::Dynamic(_) => None,
        }
    }
}

/// How an `ExprKind::Exit` is spelled; the two are the same construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKind {
    Exit,
    Die,
}

impl ExitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ExitKind::Exit => "exit",
            ExitKind::Die => "die",
        }
    }
}

/// Which of the four file-inclusion constructs an `ExprKind::Include` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncludeKind {
    Include,
    IncludeOnce,
    Require,
    RequireOnce,
}

impl IncludeKind {
    /// The keyword, as written in lowercase.
    pub fn as_str(self) -> &'static str {
        match self {
            IncludeKind::Include => "include",
            IncludeKind::IncludeOnce => "include_once",
            IncludeKind::Require => "require",
            IncludeKind::RequireOnce => "require_once",
        }
    }

    /// `require`/`require_once`: a missing file is a fatal error, not a
    /// warning.
    pub fn is_require(self) -> bool {
        matches!(self, IncludeKind::Require | IncludeKind::RequireOnce)
    }
}

/// A cast's target, as the type it converts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastKind {
    Int,
    Float,
    String,
    Bool,
    Array,
    Object,
    Unset,
    /// `(void)` (PHP 8.5): discards the value.
    Void,
}

impl CastKind {
    /// The canonical spelling, as written between the parentheses.
    pub fn keyword(self) -> &'static str {
        match self {
            CastKind::Int => "int",
            CastKind::Float => "float",
            CastKind::String => "string",
            CastKind::Bool => "bool",
            CastKind::Array => "array",
            CastKind::Object => "object",
            CastKind::Unset => "unset",
            CastKind::Void => "void",
        }
    }
}

/// A cast exactly as spelled (`(integer)` and `(int)` are the same cast).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastSpelling {
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
}

impl CastSpelling {
    /// Recognizes a cast name (any case), `None` if it isn't one.
    pub fn from_name(name: &str) -> Option<CastSpelling> {
        Some(match name.to_ascii_lowercase().as_str() {
            "int" => CastSpelling::Int,
            "integer" => CastSpelling::Integer,
            "float" => CastSpelling::Float,
            "real" => CastSpelling::Real,
            "double" => CastSpelling::Double,
            "string" => CastSpelling::String,
            "binary" => CastSpelling::Binary,
            "bool" => CastSpelling::Bool,
            "boolean" => CastSpelling::Boolean,
            "array" => CastSpelling::Array,
            "object" => CastSpelling::Object,
            "unset" => CastSpelling::Unset,
            "void" => CastSpelling::Void,
            _ => return None,
        })
    }

    pub fn kind(self) -> CastKind {
        match self {
            CastSpelling::Int | CastSpelling::Integer => CastKind::Int,
            CastSpelling::Float | CastSpelling::Real | CastSpelling::Double => CastKind::Float,
            CastSpelling::String | CastSpelling::Binary => CastKind::String,
            CastSpelling::Bool | CastSpelling::Boolean => CastKind::Bool,
            CastSpelling::Array => CastKind::Array,
            CastSpelling::Object => CastKind::Object,
            CastSpelling::Unset => CastKind::Unset,
            CastSpelling::Void => CastKind::Void,
        }
    }

    /// The spelling as written between the parentheses, lowercased.
    pub fn keyword(self) -> &'static str {
        match self {
            CastSpelling::Integer => "integer",
            CastSpelling::Real => "real",
            CastSpelling::Double => "double",
            CastSpelling::Binary => "binary",
            CastSpelling::Boolean => "boolean",
            other => other.kind().keyword(),
        }
    }
}

/// An expression, plus the real source span it occupies - see `Stmt`'s
/// equivalent doc comment.
#[derive(Debug, Clone)]
pub struct Expr {
    pub id: NodeId,
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Literal(Literal),
    // Moved out of the lexer's interned `Token::Variable` or a string
    // interpolation's variable, without its `$`.
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
    PropertyAccess(Box<Expr>, Ident, bool),
    MethodCall(Box<Expr>, Ident, Vec<Arg>, bool),
    // `$obj->{$name}` / `$obj->{$name}(...)` - the property/method name is
    // itself computed from an expression, not statically known, so it
    // can't share PropertyAccess/MethodCall's plain-String name. Still
    // worth its own base+name-expr(+args) checking (undefined variables
    // inside the {} can still be caught), even though the accessed
    // member's own type never can be.
    DynamicPropertyAccess(Box<Expr>, Box<Expr>),
    DynamicMethodCall(Box<Expr>, Box<Expr>, Vec<Arg>),
    /// `Foo::BAR`.
    ClassConstFetch(StaticTarget, Ident),
    /// `Foo::class` (any case).
    ClassNameFetch(StaticTarget),
    /// `Foo::$bar`; the name is without its `$`.
    StaticPropertyFetch(StaticTarget, Ident),
    /// `Foo::${expr}` / `Foo::$$name`: the property name is computed.
    DynamicStaticPropertyFetch(StaticTarget, Box<Expr>),
    /// `Foo::{$expr}` (PHP 8.3): the constant name is computed.
    DynamicClassConstFetch(StaticTarget, Box<Expr>),
    // `Name` mirrors `InstanceofTarget::Name`'s own (name, name-span)
    // shape exactly - a `Foo::method()` receiver, unchanged from before
    // this variant existed. `Expr` is a real, non-`Name` receiver
    // expression (`$obj::method()`) - previously discarded into a
    // permanent `"?"` sentinel string at parse time; now the real,
    // already-parsed receiver `Expr` survives into the AST, letting
    // `check_static_call` resolve its inferred type and check the call
    // against it, the same way a literal class name already is.
    StaticCall(StaticTarget, Ident, Vec<Arg>),
    /// `Foo::$name()` / `Foo::{$expr}()`: the method name is computed.
    DynamicStaticCall(StaticTarget, Box<Expr>, Vec<Arg>),
    Index(Box<Expr>, Option<Box<Expr>>),
    Name(QualifiedName),
    Instanceof(Box<Expr>, InstanceofTarget),
    Cast(CastSpelling, Box<Expr>),
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
    // `` `cmd $interpolated` ``: runs the command, as `shell_exec()` does.
    // Holds the command string.
    ShellExec(Box<Expr>),
    // `print expr`, which evaluates to `1`.
    Print(Box<Expr>),
    // `exit`/`die`, with its status or message when one is given.
    Exit(ExitKind, Option<Box<Expr>>),
    // `eval(code)`.
    Eval(Box<Expr>),
    // `include`/`include_once`/`require`/`require_once path`.
    Include(IncludeKind, Box<Expr>),
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
        // Boxed: keeps `Expr` small, since expression parsing recurses.
        return_type: Option<Box<TypeHint>>,
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
        // See `Closure.return_type`.
        return_type: Option<Box<TypeHint>>,
        // See `Closure.attrs` above.
        attrs: Vec<Attribute>,
    },
    Match {
        subject: Box<Expr>,
        arms: Vec<(Vec<Expr>, Expr)>,
        default: Option<Box<Expr>>,
    },
}

/// Peels through any `(...)`-grouping wrapper(s) to the real expression
/// underneath - `ExprKind::Grouping` is a genuine, distinct AST node,
/// not something parens collapse away during parsing, so a closure-
/// literal-shaped check like the `Closure::bind`/`->bindTo()` special-
/// casing needs to see through `(function () {...})` the same as a bare
/// `function () {...}`.
pub fn unwrap_grouping(e: &Expr) -> &Expr {
    match &e.kind {
        ExprKind::Grouping(inner) => unwrap_grouping(inner),
        _ => e,
    }
}

/// A literal string's text, or a statically-foldable `.`-concatenation of
/// nothing but literal strings (and parenthesized groupings of the same),
/// folded into one owned value. `None` for anything not fully foldable
/// this way (a variable anywhere in the chain, a non-string literal) -
/// unlike `literal_string_key`, this doesn't also accept an int literal,
/// since a format string is never legitimately written as a bare number.
/// Purely syntactic: a concatenation with a variable operand is never
/// resolved here even if a caller's type tracking knows it's a literal
/// value, since this function only sees the raw `Expr`.
pub fn literal_string_value(e: &Expr) -> Option<std::borrow::Cow<'_, str>> {
    match &e.kind {
        ExprKind::Literal(Literal::String(s)) => s.as_str().map(std::borrow::Cow::Borrowed),
        ExprKind::Grouping(inner) => literal_string_value(inner),
        ExprKind::Binary(BinOp::Concat, lhs, rhs) => {
            let l = literal_string_value(lhs)?;
            let r = literal_string_value(rhs)?;
            Some(std::borrow::Cow::Owned(format!("{l}{r}")))
        }
        _ => None,
    }
}

/// Whether `name` (without its `$`) is one of PHP's superglobals.
pub fn is_superglobal(name: &str) -> bool {
    matches!(
        name,
        "GLOBALS"
            | "_GET"
            | "_POST"
            | "_REQUEST"
            | "_SERVER"
            | "_SESSION"
            | "_COOKIE"
            | "_FILES"
            | "_ENV"
            | "argv"
            | "argc"
    )
}

/// Whether `name` is a PHP or PHPDoc keyword type rather than a class name.
pub fn is_primitive_type_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "int"
            | "integer"
            | "float"
            | "double"
            | "real"
            | "string"
            | "bool"
            | "boolean"
            | "array"
            | "object"
            | "mixed"
            | "void"
            | "never"
            | "noreturn"
            | "never-return"
            | "never-returns"
            | "no-return"
            | "null"
            | "true"
            | "false"
            | "self"
            | "static"
            | "parent"
            | "callable"
            | "iterable"
    )
}
