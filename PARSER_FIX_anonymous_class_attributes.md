# Fix: `#[Attr]` group between `new` and `class` for anonymous classes

## Context / how this was found

Investigating why `check-project` skipped 1 file scanning the real Piwigo
corpus (`/home/torres/piwigo17-frozen`, `--config phpstan-rs.toml`) turned up
a genuine parser gap. The skipped file was:

```
tests/Unit/Template/RendererTest.php:78
```

with this line:

```php
$view = new #[TemplateAttr('renderer-test.latte')] class implements View, HasPageAssets {
```

The exact parser error (reproduced by adding a temporary `eprintln!` at the
`.ok()?` in `read_and_parse_file`, `src/main/check_project.rs`):

```
PARSE ERROR in tests/Unit/Template/RendererTest.php: ParseError { message: "parse error: expected Semicolon, found AttributeStart at token 436", span: Span { start: Position { line: 78, col: 17 }, end: Position { line: 78, col: 19 } } }
```

## Root cause

`Parser::at_anonymous_class_start()` (`src/parser/classes.rs:11-22`) is the
lookahead that decides whether a `new` expression is about to declare an
anonymous class. It only skips past `readonly`/`final`/`abstract` modifiers
before checking for the `class` keyword:

```rust
pub(crate) fn at_anonymous_class_start(&self) -> bool {
    let mut offset = 0;
    loop {
        match self.peek_at(offset) {
            Token::Keyword(Keyword::Readonly)
            | Token::Keyword(Keyword::Final)
            | Token::Keyword(Keyword::Abstract) => offset += 1,
            Token::Keyword(Keyword::Class) => return true,
            _ => return false,
        }
    }
}
```

It has no case for a leading `#[Attr]` group, so for `new #[Attr] class {}`
it returns `false` at the first token, and `primary.rs`'s `Keyword::New` arm
falls through to the named-class-reference parsing path instead, which then
chokes on the unexpected `#[`.

**This is real, valid PHP** — not an edge case to reject. The PHP manual's
own attributes documentation gives this exact example:

```php
new #[ExampleAttribute] class () {}
```

`parse_anonymous_class_new` (`src/parser/classes.rs:27-127`) has the same
gap on the *consuming* side too: even if detection were fixed, it never
calls anything that would consume the attribute group, and it currently
hard-codes `attrs: Vec::new()` on the constructed `ClassDecl`, with a doc
comment that's simply wrong:

```rust
// An anonymous class expression has no declaration position of
// its own for a `#[Attr]` group to precede - not valid PHP
// syntax at all (`new #[Attr] class {}` doesn't parse in real
// PHP either).
attrs: Vec::new(),
```

## The fix

Three small, contained edits, all in `src/parser/`. No other call sites
reference `at_anonymous_class_start`/`parse_anonymous_class_new` (verified
via `grep -rn "at_anonymous_class_start\|parse_anonymous_class_new" src/` —
only the two definitions in `classes.rs` and the one call site in
`primary.rs`), so the blast radius is fully contained to these three spots.

### 1. `src/parser/primary.rs` — trigger the anonymous-class path on `#[` too

Current (`Keyword::New` arm, near the top of `parse_primary`):

```rust
Token::Keyword(Keyword::New) => {
    self.skip();
    if self.at_anonymous_class_start() {
        return self.parse_anonymous_class_new(start);
    }
```

Change to:

```rust
Token::Keyword(Keyword::New) => {
    self.skip();
    // A `#[Attr]` group can only start a `new` expression for
    // an anonymous class (`new #[Attr] class {}`) - no other
    // `new` shape (`new Foo(...)`, `new $var(...)`, `new
    // (expr)(...)`) allows one here, so seeing one is
    // unambiguous even before `at_anonymous_class_start`'s own
    // (attribute-unaware) modifiers/`class` lookahead applies.
    if self.check(&Token::AttributeStart) || self.at_anonymous_class_start() {
        return self.parse_anonymous_class_new(start);
    }
```

(Rationale for checking at the call site instead of teaching
`at_anonymous_class_start` to look past `#[...]` itself: that function does
a **pure lookahead** via `peek_at(offset)`, no token consumption. Attribute
arguments can contain their own `[`/`]` — e.g. `#[Attr(['a', 'b'])]` — so a
correct lookahead would need real bracket-nesting-depth tracking, which is
more machinery than this one-line dispatch check is worth. `Token::
AttributeStart` immediately after `new` is unambiguous on its own, so no
lookahead is even needed for the dispatch decision.)

### 2. `src/parser/classes.rs` — consume and attach the attribute group

Current start of `parse_anonymous_class_new`:

```rust
/// `new [readonly] class(args) [extends X] [implements Y, ...] { ... }`.
/// Reuses parse_class_body for the `{ ... }` part, the same as a named
/// class/interface/trait declaration.
pub(crate) fn parse_anonymous_class_new(
    &mut self,
    start: Position,
) -> Result<Expr, ParseError> {
    let decl_start = self.current_pos();
    let mut class_is_readonly = false;
```

Change to:

```rust
/// `new [#[Attr]...] [readonly] class(args) [extends X] [implements Y,
/// ...] { ... }`. Real PHP does allow an attribute group here (confirmed
/// directly against the PHP manual's own `new #[ExampleAttribute] class
/// () {}` example, and against a real corpus file that uses exactly this
/// shape) - attributes precede any `readonly`/`final`/`abstract`
/// modifier, the same order `parse_statement`'s own leading-attributes
/// handling already establishes for every other declaration. Reuses
/// parse_class_body for the `{ ... }` part, the same as a named
/// class/interface/trait declaration.
pub(crate) fn parse_anonymous_class_new(
    &mut self,
    start: Position,
) -> Result<Expr, ParseError> {
    let decl_start = self.current_pos();
    let attrs = self.parse_attributes()?;
    let mut class_is_readonly = false;
```

(`decl_start` stays captured *before* parsing the attributes, matching how
`parse_statement`'s own `start` is captured before its `parse_leading_
attributes()?` call, so the anonymous class's own span includes the
attribute group when present. `parse_attributes()` already exists and is
used the same way elsewhere in this file — see `parse_class_body`'s own
`self.parse_leading_attributes()?` call for class members — it returns
`Result<Vec<Attribute>, ParseError>`, loops over possibly-multiple `#[...]`
groups, and handles trailing commas; nothing new to write.)

Then, further down in the same function, where the `ClassDecl` literal is
built:

```rust
            doc_comment: None,
            // An anonymous class expression has no declaration position of
            // its own for a `#[Attr]` group to precede - not valid PHP
            // syntax at all (`new #[Attr] class {}` doesn't parse in real
            // PHP either).
            attrs: Vec::new(),
            span: decl_span,
```

Change to:

```rust
            doc_comment: None,
            attrs,
            span: decl_span,
```

Optionally (not required, just keeps the reader from wondering later),
`at_anonymous_class_start`'s own doc comment can note that it's
attribute-unaware by design:

```rust
    /// True when, starting right after `new`, the upcoming tokens are
    /// `[readonly|final|abstract]* class` - an anonymous class, as opposed
    /// to `new SomeExistingClass(...)`. Doesn't itself look past a leading
    /// `#[Attr]` group - the call site in `primary.rs` checks for
    /// `Token::AttributeStart` separately before falling back to this,
    /// since a bracket-aware lookahead here (attribute arguments can
    /// contain their own `[`/`]`, e.g. an array literal) would need real
    /// nesting-depth tracking this function's simple linear scan doesn't
    /// have.
    pub(crate) fn at_anonymous_class_start(&self) -> bool {
```

### 3. `src/parser/tests/declarations.rs` — regression test

Add right after the existing `anonymous_class_parses_with_interface_and_constructor_args`
test (so it sits next to its sibling):

```rust
#[test]
fn anonymous_class_with_a_leading_attribute_group_parses() {
    // Regression test: real PHP allows `#[Attr]` between `new` and `class`
    // for an anonymous class (confirmed against the PHP manual's own
    // `new #[ExampleAttribute] class () {}` example, and against a real
    // corpus file - Piwigo's tests/Unit/Template/RendererTest.php - that
    // uses exactly this shape and used to fail to parse entirely).
    let program = parse(
        r#"<?php
            $x = new #[TemplateAttr('a.latte')] class implements Countable {
                public function count(): int { return 0; }
            };
        "#,
    );
    let StmtKind::Expression(Expr {
        kind: ExprKind::Assign(_, rhs),
        ..
    }) = &program[0].kind
    else {
        panic!("expected an assignment");
    };
    let ExprKind::NewAnonymousClass(decl, _args) = &rhs.kind else {
        panic!("expected NewAnonymousClass");
    };
    assert_eq!(decl.attrs.len(), 1);
    assert_eq!(decl.attrs[0].name, "TemplateAttr");
    assert_eq!(decl.implements, vec!["Countable".to_string()]);
}
```

(`Attribute`'s field is named `name` — confirmed against `parse_attributes`'s
own construction in `classes.rs`... actually in `tokens.rs`/wherever
`parse_attributes` lives in this checkout; grep for `struct Attribute` if the
field name has moved.)

## Verification already done (before this worktree was created)

This exact fix was implemented and fully verified earlier in the session
that wrote this doc (in the main repo, before its changes were discarded due
to a worktree mixup — the fix itself was correct and tested, just needs
redoing here):

1. `cargo build --release --bin phpstan-rs` — clean.
2. New regression test passed on its own.
3. Full `cargo test --release --lib` — **3005/3005 passing** (3004 existing +
   the 1 new test), zero regressions.
4. Re-ran `check-project --config phpstan-rs.toml` against
   `/home/torres/piwigo17-frozen` — the "1 file(s) skipped: parse error"
   note disappeared entirely; files-checked went from 2791 to 2792; the
   358 diagnostic count was unchanged (the file itself produces no new
   diagnostics, it just parses now).

## Suggested commit message

```
Support a #[Attr] group between new and class for anonymous classes

at_anonymous_class_start's lookahead (readonly/final/abstract then
class) never accounted for a leading attribute group, so `new #[Attr]
class {}` fell through to the named-class-reference path and failed to
parse - confirmed a genuine gap, not an invalid-PHP case: the PHP
manual's own attributes documentation gives `new #[ExampleAttribute]
class () {}` as a valid example, and a real Piwigo corpus file
(tests/Unit/Template/RendererTest.php) uses exactly this shape.

`#[Attr]` right after `new` is unambiguous (no other `new` form starts
with one), so the primary.rs call site checks for it directly rather
than teaching at_anonymous_class_start's simple linear scan to be
bracket-aware (attribute arguments can contain their own `[`/`]`).
parse_anonymous_class_new now consumes the group via the existing
parse_attributes() and attaches it to the ClassDecl, instead of always
emitting an empty attrs list under a doc comment that (incorrectly)
called this invalid PHP.

Found while investigating why check-project skipped 1 file scanning
the Piwigo corpus. Added a regression test.
```

## Notes for whoever picks this up

- Do this work in this worktree (`/home/torres/projects/phpstan-rs-parser-fix`,
  branch `fix-anonymous-class-attributes`) — the main repo worktree
  (`/home/torres/projects/phpstan-rs`, `master`) has another session's
  in-progress uncommitted work (a `DatePeriod` constructor-merge fix
  touching `src/registry/class_info.rs`, `src/checker/tests/
  arity_and_signatures.rs`, `src/registry/tests/
  stubs_and_conditional_declarations.rs`) — don't touch those files or that
  worktree.
- This worktree was branched from `master` at commit `d118301`.
- After committing here, this'll need merging/rebasing back onto whatever
  `master` has moved to by then, same as any other worktree-based change in
  this repo's established workflow (see the existing `.corpus-runs/README.md`
  and this session's own earlier `parallelize-check-project` rebase for the
  pattern: `git worktree add`, symlink `vendor/*` submodules from the main
  checkout when a build needs real corpus content, `git rebase master`,
  resolve conflicts, verify, fast-forward `master` once confirmed).
