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
    If(IfStmt),
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
        subject: Box<Expr>,
        key: Option<Box<Expr>>,
        value: Box<Expr>,
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
    // `unset($a, $b[1], ...)`.
    Unset(Vec<Expr>),
    FunctionDecl(Box<FunctionDecl>),
    ClassDecl(Box<ClassDecl>),
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

/// `if (cond) then elseif (…) … else …`. An `elseif` is part of the chain;
/// `else if` is an `else` whose statement is another `if`.
#[derive(Debug, Clone)]
pub struct IfStmt {
    pub cond: Expr,
    pub then_branch: Box<Stmt>,
    pub elseifs: Vec<ElseIf>,
    pub else_branch: Option<Box<Stmt>>,
}

/// One `elseif (cond) body` of an `if` chain.
#[derive(Debug, Clone)]
pub struct ElseIf {
    pub id: NodeId,
    pub cond: Expr,
    pub body: Box<Stmt>,
    pub span: Span,
}

impl IfStmt {
    /// Every condition of the chain in order: the `if`'s, then each
    /// `elseif`'s.
    pub fn conditions(&self) -> impl Iterator<Item = &Expr> {
        std::iter::once(&self.cond).chain(self.elseifs.iter().map(|e| &e.cond))
    }

    /// Every branch body in order: the `if`'s, each `elseif`'s, then the
    /// `else`.
    pub fn bodies(&self) -> impl Iterator<Item = &Stmt> {
        std::iter::once(&*self.then_branch)
            .chain(self.elseifs.iter().map(|e| &*e.body))
            .chain(self.else_branch.as_deref())
    }
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
    // The bound variable without its `$`; `None` for `catch (Foo) {}`.
    pub var: Option<Ident>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// A constructor parameter promoted to a property (`private int $id`,
/// `readonly int $id`): the property's modifiers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Promotion {
    // Public when only `readonly`, `final` or a set visibility promotes.
    pub visibility: Visibility,
    // `private(set)`: who may write the property.
    pub set_visibility: Option<Visibility>,
    pub readonly: bool,
    // `final` - valid PHP, unlike `abstract`.
    pub is_final: bool,
}

#[derive(Debug, Clone)]
pub struct Param {
    // Moved out of the lexer's interned `Token::Variable`, without its `$`.
    pub name: Ident,
    pub type_hint: Option<TypeHint>,
    pub default: Option<Expr>,
    pub by_ref: bool,
    pub variadic: bool,
    // The property a constructor parameter promotes to, e.g.
    // `public function __construct(private int $id) {}`.
    pub promotion: Option<Promotion>,
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

/// A function's `{ ... }` body.
#[derive(Debug, Clone)]
pub struct FunctionBody {
    // The opening `{`.
    pub open: Span,
    pub stmts: Arc<Vec<Stmt>>,
}

#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub name: Ident,
    // The name with the file's namespace, for a function declared as a
    // statement; `None` for a method. Filled in by name resolution.
    pub namespaced_name: Option<Arc<str>>,
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
    // `None` for a bodiless `function f();` declaration (abstract and
    // interface methods), distinct from an empty `{}` body.
    pub body: Option<FunctionBody>,
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

impl FunctionDecl {
    /// The body's statements; none for a bodiless declaration.
    pub fn body_stmts(&self) -> &[Stmt] {
        self.body.as_ref().map_or(&[], |body| body.stmts.as_slice())
    }

    /// The fully-qualified name a function statement declares; a method's
    /// name.
    pub fn fq_name(&self) -> &str {
        self.namespaced_name.as_deref().unwrap_or(&self.name)
    }

    /// Whether this is a `__construct` method (PHP names are
    /// case-insensitive).
    pub fn is_constructor(&self) -> bool {
        is_constructor_name(&self.name)
    }
}

/// Whether a method name is `__construct`, compared as PHP does.
pub fn is_constructor_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("__construct")
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Visibility {
    Public,
    Private,
    Protected,
}

#[derive(Debug, Clone)]
pub struct PropertyDecl {
    // Without its `$`.
    pub name: Ident,
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
    pub get: Option<PropertyHook>,
    pub set: Option<PropertyHook>,
}

impl PropertyHooks {
    /// The hooks declared, `get` first.
    pub fn iter(&self) -> impl Iterator<Item = &PropertyHook> {
        self.get.iter().chain(self.set.iter())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut PropertyHook> {
        self.get.iter_mut().chain(self.set.iter_mut())
    }
}

/// One `get` or `set` hook of a property.
#[derive(Debug, Clone)]
pub struct PropertyHook {
    pub body: PropertyHookBody,
    // `&get`: the hook returns its value by reference.
    pub by_ref: bool,
    // `set(Type $value)`: the one parameter PHP allows there; `None` when
    // the hook omits it (`set { ... }` uses an implicit `$value`), and
    // always for `get`.
    pub param: Option<Param>,
    // The `#[Attr]` group(s) immediately preceding the hook's own
    // `get`/`set` keyword - see `PropertyHookAttributesRule`.
    pub attrs: Vec<Attribute>,
    // The `/** ... */` doc comment immediately preceding the hook's own
    // keyword (after any `#[Attr]` group(s)), so a hook can carry its own
    // `@throws` tag the way a method does.
    pub doc: Option<Arc<str>>,
    // The hook's own `final` modifier (independent of the property's or
    // its class's finality).
    pub is_final: bool,
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
    pub name: Ident,
    // The name with the file's namespace, for a global `const`; `None` in
    // a class. Filled in by name resolution.
    pub namespaced_name: Option<Arc<str>>,
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

impl ClassConst {
    /// The fully-qualified name a global `const` declares; a class
    /// constant's name.
    pub fn fq_name(&self) -> &str {
        self.namespaced_name.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ClassKind {
    Class,
    Interface,
    Trait,
    Enum,
}

/// A class, interface, trait or enum declaration statement: a name bound
/// to a class definition.
#[derive(Debug, Clone)]
pub struct ClassDecl {
    pub name: Ident,
    // The name with the file's namespace. The name as written until name
    // resolution fills it in.
    pub namespaced_name: Arc<str>,
    pub def: ClassDef,
}

impl ClassDecl {
    /// The fully-qualified name it declares.
    pub fn fq_name(&self) -> &str {
        &self.namespaced_name
    }
}

/// `new class(...) [extends X] [implements Y] { ... }`: a class
/// definition with no name in the source, and its constructor arguments.
#[derive(Debug, Clone)]
pub struct AnonymousClass {
    pub def: ClassDef,
    pub args: Vec<Arg>,
    // The name the analysis knows it by, derived from where it is; `None`
    // until the file's anonymous classes are named after parsing.
    pub name: Option<Name>,
}

/// A class-like's definition: everything but its name, shared by a
/// declaration statement and an anonymous class (`new class { ... }`).
#[derive(Debug, Clone)]
pub struct ClassDef {
    pub kind: ClassKind,
    // An enum's backing type (`int` or `string`); `None` for a pure enum
    // and every other class-like.
    pub backing_type: Option<TypeHint>,
    pub extends: Vec<QualifiedName>,
    pub implements: Vec<QualifiedName>,
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
    // Each `use TraitA, TraitB { ... }` statement in the body.
    pub trait_uses: Vec<TraitUse>,
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

impl ClassDef {
    /// Its `__construct` method, matched case-insensitively.
    pub fn constructor(&self) -> Option<&MethodDecl> {
        self.methods.iter().find(|m| m.function.is_constructor())
    }

    /// Every trait its `use` statements name, in source order.
    pub fn used_traits(&self) -> impl Iterator<Item = &QualifiedName> {
        self.trait_uses.iter().flat_map(|u| &u.traits)
    }

    /// Every `as` clause of its `use` statements.
    pub fn trait_aliases(&self) -> impl Iterator<Item = &TraitAlias> {
        self.trait_uses.iter().flat_map(|u| &u.adaptations.aliases)
    }

    /// Every `insteadof` clause of its `use` statements.
    pub fn trait_precedences(&self) -> impl Iterator<Item = &TraitPrecedence> {
        self.trait_uses
            .iter()
            .flat_map(|u| &u.adaptations.precedences)
    }

    /// Every method name an adaptation clause of its `use` statements
    /// mentions.
    pub fn resolved_trait_conflicts(&self) -> impl Iterator<Item = &Name> {
        self.trait_uses
            .iter()
            .flat_map(|u| u.adaptations.mentioned_methods())
    }
}

/// `use A, B;` or `use A, B { ... }` inside a class-like body.
#[derive(Debug, Clone)]
pub struct TraitUse {
    pub traits: Vec<QualifiedName>,
    // The statement's own doc comment, which carries `@use Trait<Args>`
    // (not the class's).
    pub doc_comment: Option<Arc<str>>,
    pub adaptations: TraitAdaptations,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumCase {
    pub name: Ident,
    // Some(...) for a backed enum (`case Hearts = 'H';`), None for a pure
    // one (`case Hearts;`).
    pub value: Option<Expr>,
    // `#[Attr]` groups preceding this case.
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

/// A trait-use block's adaptations: `B::hello as protected helloB;` and
/// `B::hello insteadof A;` clauses.
#[derive(Debug, Clone, Default)]
pub struct TraitAdaptations {
    pub aliases: Vec<TraitAlias>,
    pub precedences: Vec<TraitPrecedence>,
}

impl TraitAdaptations {
    /// Every method name a clause mentions.
    pub fn mentioned_methods(&self) -> impl Iterator<Item = &Name> {
        self.aliases
            .iter()
            .map(|a| &a.method)
            .chain(self.precedences.iter().map(|p| &p.method))
    }
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
    /// `&$x`: the element is a reference to the variable (created as
    /// `null` if it doesn't exist).
    pub by_ref: bool,
}

/// How a destructuring pattern is spelled. PHP doesn't allow the two
/// spellings to nest in each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestructureSyntax {
    /// `[$a, $b]`
    Short,
    /// `list($a, $b)`
    List,
}

/// One element of an `ExprKind::Destructure`: where the value at `key`
/// (or at the element's position, when there is no key) goes.
#[derive(Debug, Clone)]
pub struct DestructureItem {
    pub key: Option<Expr>,
    /// Where the value is written: a variable, property, element, or a
    /// nested pattern. `None` for a skipped slot (`[, $b]`).
    pub target: Option<Expr>,
    /// `&$x`: the target becomes a reference to the element.
    pub by_ref: bool,
}

#[derive(Debug, Clone)]
pub enum Literal {
    Int(i64),
    Float(f64),
    // The string's bytes: PHP strings aren't necessarily UTF-8.
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
    // The captured variable without its `$`, and its span: a `use (&$x)`
    // that auto-vivifies `$x` in the outer scope declares it there.
    pub name: Ident,
    pub by_ref: bool,
}

// The right-hand side of `instanceof`: almost always a static class name,
// but PHP also allows an arbitrary expression there (`$x instanceof
// $className`, `$x instanceof $obj`) for a runtime-determined class.
#[derive(Debug, Clone)]
pub enum InstanceofTarget {
    Name(QualifiedName),
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
    Name(Box<QualifiedName>),
    Expr(Box<Expr>),
}

impl StaticTarget {
    /// The literal class name, or `None` for a dynamic receiver.
    pub fn as_name(&self) -> Option<&QualifiedName> {
        match self {
            StaticTarget::Name(n) => Some(n),
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
            ExprKind::StaticPropertyFetch(StaticTarget::Name(class), property)
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
    pub span: Span,
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
    pub fn new(written: impl Into<Name>, span: Span) -> QualifiedName {
        QualifiedName {
            written: written.into(),
            resolved: ResolvedName::Unresolved,
            span,
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

    /// Whether this is `keyword` (`self`, `parent` or `static`), which
    /// PHP reads in any case.
    pub fn is_keyword(&self, keyword: &str) -> bool {
        self.written.eq_ignore_ascii_case(keyword)
    }

    /// The function or constant this names: the fully-qualified name to
    /// look up, and the global name PHP falls back to when that one isn't
    /// declared.
    pub fn candidates(&self) -> (&str, Option<&str>) {
        match &self.resolved {
            ResolvedName::Exact(name) => (name, None),
            ResolvedName::WithGlobalFallback(name) => (name, Some(&self.written)),
            ResolvedName::Unresolved => (self.written.trim_start_matches('\\'), None),
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

impl AsRef<str> for Ident {
    fn as_ref(&self) -> &str {
        &self.name
    }
}

impl PartialEq<Name> for Ident {
    fn eq(&self, other: &Name) -> bool {
        self.name == *other
    }
}

impl PartialEq<Ident> for Name {
    fn eq(&self, other: &Ident) -> bool {
        *self == other.name
    }
}

impl From<&Ident> for Name {
    fn from(ident: &Ident) -> Name {
        ident.name.clone()
    }
}

impl From<Ident> for Name {
    fn from(ident: Ident) -> Name {
        ident.name
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
    /// A native type keyword (`int`, `static`, `null`), with its spelling
    /// as written (`INT`).
    Keyword(NativeType, Arc<str>),
    /// A class name, as written (`\App\Foo`, `Foo`) - including a word
    /// PHP reads as a class in a native hint (`integer`, `resource`).
    Name(QualifiedName),
    /// `?T`.
    Nullable(Box<TypeHint>),
    /// `A|B`.
    Union(Vec<TypeHint>),
    /// `A&B`, or a parenthesized `(A&B)` term of a union.
    Intersection(Vec<TypeHint>),
}

/// A type keyword PHP recognizes in a native declaration, whatever its
/// case. Any other name in a type hint is a class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeType {
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
}

impl NativeType {
    /// The keyword `name` spells, compared case-insensitively.
    pub fn from_name(name: &str) -> Option<NativeType> {
        Some(match name.to_ascii_lowercase().as_str() {
            "int" => NativeType::Int,
            "float" => NativeType::Float,
            "string" => NativeType::String,
            "bool" => NativeType::Bool,
            "array" => NativeType::Array,
            "object" => NativeType::Object,
            "mixed" => NativeType::Mixed,
            "void" => NativeType::Void,
            "never" => NativeType::Never,
            "null" => NativeType::Null,
            "true" => NativeType::True,
            "false" => NativeType::False,
            "iterable" => NativeType::Iterable,
            "callable" => NativeType::Callable,
            "self" => NativeType::SelfType,
            "static" => NativeType::Static,
            "parent" => NativeType::Parent,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            NativeType::Int => "int",
            NativeType::Float => "float",
            NativeType::String => "string",
            NativeType::Bool => "bool",
            NativeType::Array => "array",
            NativeType::Object => "object",
            NativeType::Mixed => "mixed",
            NativeType::Void => "void",
            NativeType::Never => "never",
            NativeType::Null => "null",
            NativeType::True => "true",
            NativeType::False => "false",
            NativeType::Iterable => "iterable",
            NativeType::Callable => "callable",
            NativeType::SelfType => "self",
            NativeType::Static => "static",
            NativeType::Parent => "parent",
        }
    }
}

impl TypeHint {
    /// Whether two optional declarations are written the same (spans
    /// aside).
    pub fn same(a: Option<&TypeHint>, b: Option<&TypeHint>) -> bool {
        a.map(TypeHint::to_string) == b.map(TypeHint::to_string)
    }

    /// Whether this declares `mixed`.
    pub fn is_mixed(&self) -> bool {
        match &self.kind {
            TypeHintKind::Keyword(NativeType::Mixed, _) => true,
            TypeHintKind::Nullable(inner) => inner.is_mixed(),
            // A union holding `mixed` is `mixed`.
            TypeHintKind::Union(members) => members.iter().any(TypeHint::is_mixed),
            _ => false,
        }
    }

    /// Every class the declaration names (keyword types like `int` or
    /// `static` left out), as written, in source order.
    pub fn class_names(&self) -> Vec<&QualifiedName> {
        let mut out = Vec::new();
        self.collect_class_names(&mut out);
        out
    }

    fn collect_class_names<'a>(&'a self, out: &mut Vec<&'a QualifiedName>) {
        match &self.kind {
            TypeHintKind::Keyword(..) => {}
            TypeHintKind::Name(name) => out.push(name),
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
            TypeHintKind::Keyword(_, written) => f.write_str(written),
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
    pub fn name(&self) -> Option<&Name> {
        match self {
            GlobalVar::Name(ident) => Some(&ident.name),
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

/// The operator reaching a member: `->`, or `?->`, which gives `null`
/// instead when the object is `null`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectOperator {
    Arrow,
    NullsafeArrow,
}

impl ObjectOperator {
    pub fn from_nullsafe(nullsafe: bool) -> ObjectOperator {
        if nullsafe {
            ObjectOperator::NullsafeArrow
        } else {
            ObjectOperator::Arrow
        }
    }

    pub fn is_nullsafe(self) -> bool {
        self == ObjectOperator::NullsafeArrow
    }
}

/// `function (...) use (...) { ... }`.
#[derive(Debug, Clone)]
pub struct ClosureExpr {
    pub params: Vec<Param>,
    pub uses: Vec<ClosureUse>,
    pub body: Vec<Stmt>,
    pub is_static: bool,
    pub by_ref: bool,
    // Boxed: keeps `Expr` small, since expression parsing recurses.
    pub return_type: Option<Box<TypeHint>>,
    // `#[Attr]` groups on the closure expression itself
    // (`$f = #[Foo] function () {};`), not on any of its parameters.
    pub attrs: Vec<Attribute>,
}

/// `fn (...) => expr`.
#[derive(Debug, Clone)]
pub struct ArrowFnExpr {
    pub params: Vec<Param>,
    pub body: Box<Expr>,
    pub is_static: bool,
    // `fn &() => ...` - returns by reference.
    pub by_ref: bool,
    // See `ClosureExpr::return_type`.
    pub return_type: Option<Box<TypeHint>>,
    // See `ClosureExpr::attrs`.
    pub attrs: Vec<Attribute>,
}

/// An expression, plus the real source span it occupies - see `Stmt`'s
/// equivalent doc comment.
#[derive(Debug, Clone)]
pub struct Expr {
    pub id: NodeId,
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    /// This expression inside any parentheses and `@`s: what it
    /// evaluates to, the wrappers being transparent.
    pub fn peeled(&self) -> &Expr {
        crate::stack::guard(|| match &self.kind {
            ExprKind::Grouping(inner) | ExprKind::ErrorSuppress(inner) => inner.peeled(),
            _ => self,
        })
    }

    /// The expression's kind, taken out of it.
    pub fn into_kind(mut self) -> ExprKind {
        std::mem::replace(&mut self.kind, ExprKind::Literal(Literal::Null))
    }
}

impl Stmt {
    /// The statement's kind, taken out of it.
    pub fn into_kind(mut self) -> StmtKind {
        std::mem::replace(&mut self.kind, StmtKind::Block(Vec::new()))
    }
}

/// Dropping a tree goes one level at a time on a guarded stack, so a deep
/// tree (a long `.` chain is one) drops without overflowing it.
impl Drop for Expr {
    fn drop(&mut self) {
        let kind = std::mem::replace(&mut self.kind, ExprKind::Literal(Literal::Null));
        crate::stack::guard(|| drop(kind));
    }
}

impl Drop for Stmt {
    fn drop(&mut self) {
        let kind = std::mem::replace(&mut self.kind, StmtKind::Block(Vec::new()));
        crate::stack::guard(|| drop(kind));
    }
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Literal(Literal),
    // Moved out of the lexer's interned `Token::Variable` or a string
    // interpolation's variable, without its `$`.
    Variable(Name),
    // `${expr}` - the older "variable variable" syntax: the variable's own
    // name is computed at runtime by evaluating expr, so unlike Variable
    // there's no fixed name here to look up or bind in `locals`.
    DynamicVariable(Box<Expr>),
    ArrayLiteral(Vec<ArrayItem>),
    // `@expr`: the error-control operator, which silences the warnings
    // evaluating `expr` raises. Its value is `expr`'s.
    ErrorSuppress(Box<Expr>),
    // `[...]` or `list(...)` as the target of an assignment or a
    // `foreach`: takes the array apart into the item targets. Only ever
    // the left of an `Assign`, a `foreach` value, or an item target of
    // another pattern.
    Destructure(Vec<DestructureItem>, DestructureSyntax),
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
    // The property/method name: a `Name`, moved straight out of a
    // `Token::Identifier`/`Token::Keyword` (via `Parser::
    // take_name_segment`) - or, for a string-interpolation `$obj->prop`
    // occurrence, `StringPart::PropertyAccess::prop`. A `$obj->$prop` dynamic-by-variable-name
    // access is a `DynamicPropertyAccess`/`DynamicMethodCall` below, not
    // this variant - `$prop`'s own runtime value, not its declared name,
    // is the real member name.
    PropertyAccess(Box<Expr>, Ident, ObjectOperator),
    MethodCall(Box<Expr>, Ident, Vec<Arg>, ObjectOperator),
    // `$obj->{$name}` / `$obj->{$name}(...)` - the property/method name is
    // itself computed from an expression, not statically known, so it
    // can't share PropertyAccess/MethodCall's plain-String name. Still
    // worth its own base+name-expr(+args) checking (undefined variables
    // inside the {} can still be caught), even though the accessed
    // member's own type never can be.
    DynamicPropertyAccess(Box<Expr>, Box<Expr>, ObjectOperator),
    DynamicMethodCall(Box<Expr>, Box<Expr>, Vec<Arg>, ObjectOperator),
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
    // `isset($a, $b, ...)`: whether every operand is set and not null.
    Isset(Vec<Expr>),
    // `empty(expr)`: whether the operand is unset or falsy.
    Empty(Box<Expr>),
    // `print expr`, which evaluates to `1`.
    Print(Box<Expr>),
    // `exit`/`die`: `None` written bare, `Some` with its parenthesized
    // arguments (PHP 8.4 makes both functions of `string|int $status`).
    Exit(ExitKind, Option<Vec<Arg>>),
    // `eval(code)`.
    Eval(Box<Expr>),
    // `include`/`include_once`/`require`/`require_once path`.
    Include(IncludeKind, Box<Expr>),
    // `throw` as an expression (PHP 8.0+): valid in match arms, ternary
    // branches, ??'s right side, arrow function bodies, etc - as opposed
    // to StmtKind::Throw, a bare `throw expr;` statement, which is still
    // dispatched separately and takes priority at statement position.
    Throw(Box<Expr>),
    NewAnonymousClass(Box<AnonymousClass>),
    // `yield;` / `yield $v;` / `yield $k => $v;` - (key, value). Makes the
    // enclosing function a generator.
    Yield(Option<Box<Expr>>, Option<Box<Expr>>),
    // `yield from $generator;`
    YieldFrom(Box<Expr>),
    Interpolated(Vec<Expr>),
    // Boxed: keeps `Expr` small, since expression parsing recurses.
    Closure(Box<ClosureExpr>),
    ArrowFn(Box<ArrowFnExpr>),
    // The arms in written order, `default` among them.
    Match {
        subject: Box<Expr>,
        arms: Vec<MatchArm>,
    },
}

/// One arm of a `match`: `conditions => body`, or `default => body`.
#[derive(Debug, Clone)]
pub struct MatchArm {
    // `None` for the `default` arm.
    pub conditions: Option<Vec<Expr>>,
    pub body: Expr,
    pub span: Span,
}

impl MatchArm {
    /// A non-default arm's conditions and body.
    pub fn conditioned(&self) -> Option<(&Vec<Expr>, &Expr)> {
        Some((self.conditions.as_ref()?, &self.body))
    }
}

/// The body of a `match`'s `default` arm.
pub fn match_default(arms: &[MatchArm]) -> Option<&Expr> {
    arms.iter()
        .find(|arm| arm.conditions.is_none())
        .map(|arm| &arm.body)
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
    let e = e.peeled();
    crate::stack::guard(|| match &e.kind {
        ExprKind::Literal(Literal::String(s)) => s.as_str().map(std::borrow::Cow::Borrowed),
        ExprKind::Binary(BinOp::Concat, lhs, rhs) => {
            let l = literal_string_value(lhs)?;
            let r = literal_string_value(rhs)?;
            Some(std::borrow::Cow::Owned(format!("{l}{r}")))
        }
        _ => None,
    })
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
