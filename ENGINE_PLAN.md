# The engine: Part H (the engine cluster) and Part J (the wall's later designs)

Document three of four; see `POST_LEVEL_9_PLAN.md` for the table of where each part of the plan went, and for the sequence.
Part H designs the identity model, the engine, REG-7 and REG-8 on it, and the pipeline; Part J the data model of the
version ranges, the C6 to C9 rows and the reconciliation of Stages D to G.

---

# Part H. The engine cluster: identity, the engine, REG-7 and REG-8 on it, the pipeline

Part F rows 10, 13, 14 and 15, designed together because each constrains the next. Written from the
code at `8a1ca1e0` and from the source of **salsa 0.28.5**, the version REG-16.1 measured
(`~/.cargo/registry/src/index.crates.io-*/salsa-0.28.5`). The ledger's prototype directory
(`~/.cache/prs-ledger/reg161`) is gone, so everything said here about salsa was read from the crate and
none of it was run; H5 lists what a run must settle. **H1 is what batch 4 is built to.** H2 to H4 are what
W2 and W3 build. They are written as a contract (what the code above the engine may rely on), so that
REG-16.1's choice (done: salsa) stays reversible, with S-2 and S-3 re-testing it and an engine of our own with the
same contract as the fallback: only the engine module names salsa (a crate from G1: Part E 33).

## H1. Identity (Part F 10)

### H1.0 What the code and the crate showed

1. **Built already:** one process-wide interner, `Name` equality by pointer, no `Borrow<str>`, `FoldedMap`
   with a stored folded hash (`syntax/src/name.rs`; 25 G C3's steps 1 to 3). 25 G's text still says `Project`
   owns an interner; it does not. **Not built:** `FoldedName`, `ConstName`, the one-allocation name
   (NAME-2.1), `Name::short`.
2. **REG-8.6 cannot be built as written.** "Snapshots own their names ... the project's interner ... `apply`
   sweeps the session interner" needs a second interner, and a second interner makes two allocations of one
   text, which `Name`'s pointer equality forbids (the module's own doc says so). H1.3 re-specifies it.
3. **`ArrayKey` exists:** `types::ShapeKey` (`shape.rs:14`) is PHP's array key, `Int(i64)` or `Str(PhpString)`,
   a decimal-int string canonicalized by `canonical_int_key`; 229 uses in 38 files (242 lines in 42 files with `phpdoc`'s own `ShapeKey`). 25 G's new `ArrayKey`
   would be a second type for it.
4. **25 G C3 and NAME-2.1 disagree** on where a name's lowercase form lives: a second interned `Name`
   (`NameData.folded`), or inline in the one allocation. H1.2 takes NAME-2.1.
5. **A position inside a value defeats early cutoff.** `Provenance { origin, span, declaring_class }`
   (`registry/provenance.rs`) derives `PartialEq` and sits on every class, function and member, so a class is a
   different value after a line is inserted above it. Today that makes `changed_since` call it changed and
   recheck its readers; on the engine it would invalidate every reader of every class below an edit.
6. **`Type`'s `PartialEq` is semantic and its interner is exact.** `repr_eq` ignores a class name's case and the
   explicit-`mixed` flag (`types/src/lib.rs:601-640`), while "a shared node never changes how a type prints"
   (`types/src/intern.rs`). salsa backdates on the result's `PartialEq` by two paths and both go stale on the
   semantic equality: a tracked struct's field that compares equal **keeps the old value**
   (`update_fields`), so a reader gets the *old* node, which prints the old way; a tracked function's result
   that compares equal is stored **as the new value with the old memo's `changed_at`**
   (`function/backdate.rs`), so dependents that were verified and not re-run keep results computed from the old
   input while a re-run reader sees the new memo, and two readers in one revision can see different
   spellings. A warm run then differs from a cold one either way.
7. **salsa's interned structs are collected and their slots reused** with a new generation (`interned.rs:93`;
   `REVISIONS` defaults to 3; `revisions = usize::MAX` turns collection off). Only a value interned under a
   `LOW`-durability query is reusable (`:244-257`; a value interned outside any query, or under a query that
   read only higher-durability inputs, is never collected), and the query that *interns* it records an edge to
   it (`:263-290`: creation records the edge, reading does not), so a reuse invalidates that query. A reused
   slot is checked on a read only by `assert_validated`, which fails for a slot not re-interned this revision,
   and by a `debug_assert_eq!` on the id ("refers to a reused slot", `:932`): in a release build a stale id
   of a slot re-interned this revision reads the new occupant's fields and memos. What keeps a collectable id
   honest is H1.6's rule that none outlives its query or leaves the engine, not salsa; `DeclKey` is immortal
   (Part E 38), so it needs no such discipline.
8. **salsa's macros emit `unsafe impl` into the using crate** (`salsa-macros/src/db.rs:94`), and the
   workspace has `unsafe_code = "deny"` with `allow`s for PCRE and the allocator only.
9. **A write needs `&mut` on the database,** cancels every other handle's running queries by unwinding
   (`Cancelled::PendingWrite`) and waits until those handles are dropped (`database.rs:30-80`). The database
   is `Send`, not `Sync`: a handle per thread, made by `clone`.
10. **Persistence is a whole-database serde** (`#[salsa::input(persist)]`, `interned(persist)`,
    `tracked(persist)`; `<dyn Database>::as_serialize` / `deserialize`, `database.rs:166`). It needs `&mut`
    (it cancels other workers), restores into a later revision (`tests/persistence_stale_interned.rs`), and **a
    non-persisted dependency is replaced by the base-input leaves under it** (`function.rs:602-634`, `:880-896`):
    the nested walk descends through every edge, persisted tracked functions included (it never inserts such an
    edge), inserts only input fields and persisted collectable interned structs, and drops tracked-field edges;
    it asserts that every input field a persisted memo depends on is itself `persist`
    (`input/input_field.rs:68-82`). `deserialize` changes the live database in place and is not atomic
    (`database.rs:186-193`, `:360-377`: it removes an ingredient from the database's `Vec`, decodes into it and
    puts it back only if the decode succeeded), and it expects a database with no inputs (a singleton that
    exists already panics, and input slots are re-allocated in the saved order under an assertion;
    `input/singleton.rs`, `input.rs:640-658`). An input, an interned struct and a tracked struct may name their
    own `serialize` and `deserialize` functions; a tracked function's memo is written by its result type's serde
    (`options.rs:318-333`, `tracked_fn.rs:66`), and each memo key is read as a borrowed `&str`
    (`function.rs:939`), which needs a format decoded from one whole in-memory buffer.

### H1.1 The identities

| identity | type | equal when | hash | made by | lives until | may be named by |
|---|---|---|---|---|---|---|
| a spelling | `Name` | same allocation: one per text while any holder lives | stored `hash` | `Interner::global().name`, the lexer | the last holder drops; the table sweeps | anything `'static` |
| a case-insensitive identifier (class-like, function, method, namespace) | `FoldedName(Name)` | `folded_hash` equal, then the same allocation, else the lowercase bytes equal | stored `folded_hash` | `Name::folded()`, a clone | as its name | as a `Name` |
| a global constant | `ConstName` | namespace equal ignoring case, last segment exact | stored | `ConstName::of(&Name)` | a value | as a `Name` |
| an array key | `ArrayKey` (`ShapeKey` renamed) | same variant and value; string bytes interned | stored | `ShapeKey::from_bytes` | weak table | as a `Name` |
| a file | `FileId(u32)` | number | itself | the project, at the path's first sight | the process; a persisted database restores it | anywhere, meaningful in its project |
| a tree and its nodes | serial, `Expr<'a>` handles (B4, G1) | (serial, id) | | `parse` | the `Arc<Ast>` | G1's list |
| a declaration | `DeclRef` (value) / `DeclKey` (engine) | file, kind, folded name, occurrence (an anonymous class: its ordinal in the file) | | the declaration index | a value / an immortal interned slot | `DeclRef` anywhere; `DeclKey` in the engine module |
| a global variable | `Name`, exact (PHP variable names are case-sensitive) | the pointer | stored `hash` | the lexer | as a name | as a `Name` |
| a member | `MemberRef` = (`DeclRef`, `MemberName`): a method by `FoldedName`, a property by `Name` (exact), a class constant by `Name` (exact; an enum case is a class constant, one namespace) | those | | a lookup | a value | anywhere |
| a type | `Type` (`Arc<Node>`, hash-consed) | exactly: the node; semantically: `PartialEq` | stored | the type interner | weak | anywhere; a result compares it by node (H1.5b) |
| a local | `LocalKey` | id | | one check's key table | the check | the check |
| a revision | salsa `Revision` | | | a write | | clears TYPE-4's memo |

### H1.2 The name types

```rust
// syntax::name. The header is NAME-2.1's, with the fields NAME-1.4 needs.
struct Header { hash: u64, folded_hash: u64, len: u32, last_segment: u32 }   // 24 B, no padding
pub struct Name(ThinArc<Header, u8>);        // the text, then its lowercase form when that differs (the slice is 2 * len long then)
pub struct FoldedName(Name);                 // equal ignoring ASCII case; offers no spelling
pub struct ConstName { name: Name, hash: u64 }
// ArrayKey: types::ShapeKey under its proper name. The rename takes its 229 uses in 38 files (242 lines in 42 files with `phpdoc`'s own `ShapeKey`); the old name is
// deleted, not aliased.
```

- **`Name`** keeps what it has: equality by pointer (with the debug assertion that two allocations differ in
  text), hash read from the header, `Deref<Target = str>`, serde as text re-interned into the one interner.
  New: `short() -> &str` (the last segment, from `last_segment`), `namespace() -> &str`, `lower()` from the
  slice (whether a lowercase form follows is the slice's length: `lower_at` would be `len` or 0, so it is not
  stored). One allocation replaces `Arc` + `Box<str>` and, for a name with capitals, an `Option<Box<str>>` (a
  lowercase name takes two allocations today and a temporary lowercase `String`). **How a `&str` is read back is
  S-1's:** triomphe's thin pointer exists only for `[T]`, so `Deref<Target = str>` over a `ThinArc<Header, u8>`
  costs a UTF-8 validation on every access unless a crate supplies a thin header-plus-`str` type; S-1 measures
  it, and H1.3's fallback has no such cost. Identity is `ThinArc::as_ptr()` and uniqueness is
  `strong_count(..) == 1` (triomphe has no `ptr_eq` or `is_unique`). **A PHP identifier is a byte string,** and
  the lexer reads identifier bytes lossily when the source is not UTF-8, so two classes that differ in a byte
  above 0x7f become one `Name` today and after: a known gap, which the lexer reports as a warning when it
  decodes an identifier lossily (a test); `ArrayKey` is the one place the bytes are kept.
- **`FoldedName`** is `Eq` when `folded_hash` is equal and either the allocations are the same or `lower()`
  is byte-equal; `Hash` is `folded_hash`; `Ord` is by lowercase text and agrees with `Eq` (a property test).
  `Name::folded()` is a clone, with no allocation. **Its spelling is unobtainable:** it offers `lower()` and a
  `Display` of the lowercase text, no `name()` and no `Deref` to a spelling. salsa keeps the fields of the first
  equal intern and the old value of a field that compares equal (H1.0 6 and 7), so after `class Foo` becomes
  `class FOO` a stored `FoldedName` would otherwise hand out `Foo`; its `Eq` is exact for everything it can
  show (H1.5b). A map that keeps and shows the declared spelling (`Members::iter` yields every member with its
  declared name and the LSP shows it as the symbol) is keyed by `Name` with folded probes, as `FoldedMap` is
  today, or stores the declared `Name` in its entry; a map that only answers "is there one" is keyed by
  `FoldedName`. Probes by `&Name` read the stored hash; probes from text keep `FoldKey` (the text hashed eight
  bytes at a time, `eq_lowercased` against the stored lowercase form).
- **Why the lowercase form stays inline** (NAME-2.1) and not as 25 G C3's second interned `Name`: a mixed-case
  name would then cost a second header and allocation, an interning order (the lowercase form first, outside
  the shard lock, since creating one must not hold one shard's lock while taking another's) and a strong edge
  between names; the one thing it buys is a pointer test where two spellings of one identifier meet. That is
  rare. A class is spelled one way almost everywhere, equal spellings are one allocation, so the pointer test
  already decides the common case, and the byte comparison runs only on a mismatch.
- **`ConstName`** is `of(&Name)`: one pass over the text (the folded hash of the namespace prefix mixed with the
  exact hash of the last segment). `Eq`: hash equal, then the prefix equal ignoring case and the last segment
  byte-equal. PHP folds a constant's namespace and not the constant (REG-18.4: `\APP\A` finds `App\A`, not
  `App\a`); a `define('NS\FOO')` name is split the same way. Three more rules decide identity (php 8.5.4 was
  run): `true`, `false` and `null` are case-insensitive in every spelling, even in a namespace that defines a
  constant of that name (`TRUE` is `bool(true)` in `namespace App` with `App\TRUE` defined, `defined('True')` is
  true, and `define('True', 1)` fails while `define('php_eol', 1)` succeeds), so a bare `true`, `false` or
  `null` folds; a `define()` literal is not trimmed of a leading backslash (`define('\\BAR\\X', 2)` succeeds and
  defines a constant no name reaches: `defined('\\BAR\\X')` is false), unlike a written `\BAR\X`, which
  resolution strips; and for a project whose range includes PHP 7, `define($n, $v, true)` registers a
  case-insensitive constant (the code models it: `PhpFeature::CaseInsensitiveConstantsRemoved`), a flag on the
  declaration gated by the version range.
- **`ArrayKey`** is `ShapeKey` under its proper name (PHP's array key, not a shape's). NAME-2.3 interns its string bytes in a weak table so `Str` equality is a pointer
  test, and **`ArrayKey::Str` becomes an opaque type built only by the interning constructor** (`ShapeKey::Str(PhpString)`
  is a public variant with 22 sites today, and `PhpString::from(&str)` builds un-interned bytes);
  `ArrayKey::from_name(&Name)` canonicalizes as PHP stores array keys (`'1'` and `1` are one key, `'01'` is
  another). The checker's `LocalKey::Index` and container-field facts hold it (25 G C4 step 5).
- **Serde:** each writes its spelling or bytes and re-interns. A persisted database therefore holds spellings,
  and equality after a restore is by the rules above.
- **Tests:** each map against a `HashMap<String, _>` keyed by the text or its `to_ascii_lowercase()`, on
  random spellings (bytes at or above 0x80 unchanged, as PHP folds ASCII only); one text interned from 16
  threads is one allocation; `FoldedName` equal implies hash equal and `Ord` agrees with `Eq` (property
  tests); a case-only edit leaves a `FoldedName`-valued product equal and changes no text a reader can print;
  `ConstName` finds `\APP\A` as `App\A` and not as `App\a`, folds a bare `true`, `false` and `null`, and keeps a
  `define()` literal's leading backslash; `ArrayKey::from_name` against a table generated from php 8.5 runs
  and checked in (PHP turns the string `-9223372036854775808` into the int `PHP_INT_MIN`, which
  `canonical_int_key` misses today because its digits overflow `i64::parse`; the fix is in the same commit, and
  the table has the overflow cases).

### H1.3 The interner: one, counted, swept. REG-8.6 re-specified

- **One interner, process-wide, as built.** Names cross every boundary (the stub layer, the engine's memos, a
  restored database, the LSP's tables), and `==` on them is a pointer test; two interners would make that
  false. The per-project interner REG-8.6 asked for is not built, and the ledger row is rewritten to what it
  was for: names do not accumulate in a long session.
- **Liveness by count.** `triomphe::ThinArc` has no weak counts, so the table holds its names strongly and a
  sweep drops those whose count is one (only the table holds them). Sweeps run when a shard has doubled since
  its last (as `WeakTable` does now) and at a revision boundary of a long-lived session, after the writes
  have cancelled other handles. A sweep runs under the shard lock, and every clone out of the table takes the
  same lock, so a name with count one cannot be cloned while it is dropped. A name a salsa memo or a persisted
  database holds is held, so it survives. **The ordering rule:** the clone returned to the caller is taken
  before the shard's sweep runs (a new entry has count one, and today's `WeakTable::get_or_insert` sweeps right
  after `insert_unique` while its local `Arc` is held; a counted table must not drop what it has just made).
  **The sweep's schedule and cost** are stated with S-1's numbers: the doubling trigger as now, and at a
  revision boundary of a long-lived session a sweep of at most one shard per write (a scan of 16 shards per
  keystroke would not do), its time per pass and the bytes of count-one names retained at the peak recorded.
  **Which tables are counted:** the name table; `Interner::str` (the weak `Arc<str>` of inline HTML, doc
  comments and index keys), NAME-2.3's byte-string table and the type interner stay on `std` `Arc` with
  `Weak`, as now. A strong table keeps dead names allocated until a sweep, where today the text is freed at
  the last drop, so S-1 has a retention criterion. **Tests:** a concurrent intern, drop and sweep stress with
  the pointer-equality debug assertion on.
- **The builtin knowledge's names** are held by the embedded data and are never swept.
- **Fallback:** if S-1 finds `ThinArc` unfit (its read path costs, or no crate gives a thin header-plus-`str`
  type), `Name` is one `Arc<NameData>` whose single `Box<str>` holds the text and then the lowercase form
  (sliced at `len`: safe, two allocations against today's three, and `Weak` counts stay, so the weak table and
  its sweep stay as they are), with the header's fields in `NameData` (`last_segment`). Nothing above
  `syntax::name` changes either way.

### H1.4 Declaration identity

```rust
// plain values: 'static, Eq + Hash + serde, named anywhere
pub enum DeclFile { Project(FileId), Builtin(u32) }   // Builtin: an index into the embedded file table (J1.2)
pub struct DeclRef { pub file: DeclFile, pub kind: DeclKind, pub name: FoldedName, pub occurrence: u32 }   // kind: Class (any class-like) or Function
pub struct ConstDeclRef { pub file: DeclFile, pub name: ConstName, pub occurrence: u32 }   // `const` and `define()`
pub enum MemberName { Method(FoldedName), Property(Name), Constant(Name) }    // PHP folds a method's name only; an enum case is a class constant, one namespace
pub struct MemberRef { pub decl: DeclRef, pub member: MemberName }
// in the engine only: salsa interned structs built from the values above, one key per PHP namespace
#[salsa::interned(persist, revisions = usize::MAX)] struct DeclKey<'db> { decl: DeclRef }   // immortal (Part E 38): never reused, no creation edge
// the five probes below are collectable: made for names that do not exist too, held by nothing (Part E 38)
#[salsa::interned(persist)] struct ClassKey<'db>    { name: FoldedName }   // class-likes: PHP keeps classes, interfaces, traits and enums in one namespace
#[salsa::interned(persist)] struct FunctionKey<'db> { name: FoldedName }   // functions: another namespace (`class A {}` and `function a() {}` coexist)
#[salsa::interned(persist)] struct ConstKey<'db>    { name: ConstName }
#[salsa::interned(persist)] struct GlobalKey<'db>   { name: Name }         // a global variable, exact: `$Foo` and `$foo` are two
#[salsa::interned(persist)] struct MemberKey<'db>   { decl: DeclRef, member: MemberName }
```

- `DeclKind` is the existing `registry::DeclKind` (`index.rs`), restricted to its `Class` and `Function`
  variants here (a class-like's own sort, class, interface, trait or enum, is data in `ClassDecl`, not part of
  its identity: two declarations of one folded name in different sorts are two candidates for a duplicate
  report); constants have `ConstDeclRef`. `declarations_named` is generic over the key and returns the matching
  ref type (`DeclRef` or `ConstDeclRef`), and `locate` takes `ClassKey` only: functions and constants cannot be
  autoloaded. `occurrence` is the ordinal among the file's declarations of the same kind and folded name, in
  source order, nested ones included (PROD-5.1's vectors give the order). It holds no position, so inserting a
  line moves nothing; inserting a duplicate above shifts the later duplicates, which are different declarations
  then.
- **Member identity refines decision 3** ("a member is (class id, kind, folded name)"; ruled 2026-10-18): PHP
  folds a method's name and not a property's or a class constant's (php 8.5.4: `$a->DOSTUFF()` runs `doStuff`,
  `isset($a->P)` is false while `isset($a->p)` is true, `defined("A::c")` is false while `defined("A::C")` is
  true), so a member is (class's `DeclRef`, `MemberName`), folded for methods and exact for the rest; a
  `Provider` method read takes the `FoldedName` it was written as, so one method has one memo and one edge
  whatever spelling is asked. Putting a single `Name` in the key would make the set of keys depend on which
  spelling was interned first. **The kinds are PHP's namespaces:** an enum case is a class constant
  (`const A` beside `case A` is "Cannot redefine class constant E::A", and `ReflectionClassConstant::isEnumCase()`
  is true for a case), so constants and cases are one kind, and `Foo::BAR` is one read and not two; static and
  instance properties are one kind too ("Cannot redeclare C::$x"); a property and a method may share a name, so
  they stay apart. Only the member sentence of decision 3 changes.
- **Anonymous classes are file-local, and their identity is the ordinal (ruled 2026-10-18).** They keep the name the ledger and the review give them, from their
  position: `class@anonymous/{shown path}:{line}[:{i}]` (decision 5, PROD-5.1, SYN-3.4; F33 makes the path the shown
  one, `File.shown`, so no spelling of the project argument reaches a name), and types and messages show it as they
  do today. That name carries a line, so it is a spelling and not an identity: a line inserted above one renames it, and
  an immortal `DeclKey` built on it would mint a key, and keep its memos, for every inserted newline. So the
  `DeclRef` of an anonymous class is (file, `Class`, the constant name `class@anonymous`, its ordinal among the
  file's anonymous classes), with no line in it, and the positional name is derived from the file's site table.
  They are therefore left out of `decl_names` and `decl_index` (nothing else can name one, since no
  source spells it, so no candidate is lost), and the provider reaches one through its name: a name with the
  `class@anonymous/` prefix has its trailing `:line[:i]` read, **every reading is tried against `FileSet`'s
  `shown` index, and the one whose file's `decls` hold exactly that name is taken** (a path can itself end in
  `:digits`, legal on Linux and macOS, so `…/a.php:12:2` is (`a.php`, 12, 2) or (`a.php:12`, 2); when both
  exist the name is an unknown class), as a recorded read of those `decls`, which is what a type that
  flowed out of the file (an inferred return) needs; a path no file has is an unknown class. The displayed-type
  parser (`anonymous_class_end`, which `try_parse_displayed` and the type-drift gate use) ends a name at a `:`
  not followed by a digit and at a space or a bracket, so it fails on `C:/x/a.php` and on a path with a space;
  it peels `:digits[:digits]` from the end of the token instead, with tests for `C:/x/a.php`, a path with a
  space and a file named `a.php:12`. A moved one changes its own file's products and nothing global.
  **The consumers that see an anonymous class are each decided from an enumeration of the code** (the first list
  came from a reviewer, and the code has more): `find_dead_classes` skips it already (`!is_anonymous()`);
  `find_dead_members` iterates `registry.classes` unfiltered and reports the unused members of an anonymous class
  today (read from the code, not run), so it reads the file's `decls` to keep doing so, with a fixture for an unused
  member of an anonymous class; dead abstract classes and dead enum cases cannot meet one. **Namespace cycles keep
  it** (`circular_namespace_deps.rs`), attributed to its enclosing namespace, which the registry records where it
  registers the class (`anonymous_class_decls` has the `NameContext`): `namespace_of` gives `""` for its name today,
  so it is a global-namespace node, and `new class implements \App\Contracts\Logger {}` plus any `App\Contracts`
  class extending `\Exception` draws a false cycle between the two; its edges are real structural dependencies.
  **Workspace symbols** (`lsp/workspace_symbol.rs`) drop the class entry and keep its methods and properties, which
  have names and places. **Import completion** (`lsp/completion.rs`) drops it, since no source can spell it.
  **`main/demo.rs` keeps it**, because it dumps what the registry holds. **`registry/scope.rs` is not an iteration
  to decide:** it is the finishing-pass scope the engine deletes, and an anonymous class goes through the levels
  like any class, reached through its file's declarations. **The class-shape parent-call rules**
  (`project/class_shape.rs`) walk only named class statements (`collect_classes`) and include it: an anonymous
  subclass that overrides a method a configured rule says must call `parent::m()` is checked by nothing today (the
  closed-class rule does not apply, an anonymous class being closed by construction; the architecture rules count a
  class's references by span, so an anonymous class's references already go to its enclosing class). Each change is
  a VAL-2.x delta with a fixture. REG-11.13 (diagnostics name it `class@anonymous`) is a display change of its own.
- **`DeclKey` is immortal and the probes are collectable (ruled 2026-10-18; Part E 38).** `DeclKey` takes
  `revisions = usize::MAX`, which is decision 3's "is never reused" kept as ruled: a stale id cannot name another
  declaration by construction (salsa's own check on a read is a `debug_assert`, H1.0 7, so a collected slot's stale
  id would read the new occupant silently in a release build), and an immortal key records no creation edge and no
  persisted interned edge. `ClassKey`, `FunctionKey`, `ConstKey`, `GlobalKey` and `MemberKey` are probes by an asked
  name, made for names that do not exist too (every half-typed identifier asks) and held by nothing, so immortal
  would only grow the table; they stay collectable, and H1.6's rule keeps them honest. Only values interned under a
  LOW-durability query are collected, and creating such a value records the edge. **The cost of the immortal key is
  what it retains:** a dead declaration's key keeps its memos until the database is rebuilt (an `lru` on the heavy
  queries drops values, not keys), so S-3 measures the retained bytes over an edit replay and H4.6 bounds them with
  an in-process compaction. If the bytes are too large, the fallback is a collectable `DeclKey` and decision 3's
  sentence amended to "a stale id never names another declaration".
- **No id leaves the engine.** What reflections, summaries and products hold are names (`Name`,
  `FoldedName`, `ConstName`) and `DeclRef`s. A reflection that holds an id could not be compared for cutoff,
  persisted by the stub cache's format, or shared with the stub layer, which has none. A parent is the
  `FoldedName` it was written as, resolved to its declarations when a level reads it (H2.4).
- **No result stores a member id.** A member is looked up as (class's `DeclRef`, `MemberName`) in the class's
  member table, and its slot there is a position in one level's result; the interned `MemberKey` of H2.4
  lives in the engine only.

### H1.5 Two rules for values the engine compares

**(a) Positionless.** A value that a query of another file reads holds no span, line or column of the file it
comes from (a file's own diagnostics and per-node results do: nothing but that file's check and the finalizer
read them, and their position is their meaning). "Where" is a `DeclRef` or `MemberRef`, and the position is read
from `file_sites(file)`, a separate query over the tree's declaration vectors (PROD-5.1). Why: when a new value
is equal the engine keeps the old one, and when it is not equal every reader of it runs again. With a position
in the value, an edit above a class invalidates the class and all its readers; with the position left out of
the equality instead, the memo keeps a stale position. Consequences:
- `Provenance` becomes `{ origin, site, declaring_class }`, where `site` is the declaration's ordinal data
  (the `DeclRef`'s occurrence, kind and name, and a member's ordinal in its class), and `declaring_class` stays
  (the declaring class, a trait for an imported member; the LSP reads it, `workspace_symbol.rs`);
  `declaration_span()` becomes a lookup, `Registry::span_of(&Provenance)`, through the file's site table.
  `class_declaration`, `method_declaration` and the other `Registry::*_declaration` accessors
  (`provenance.rs:263-312`; 25 uses elsewhere, in three files) become the same lookup.
- **Every span-bearing type reachable from a result type is listed and given an ordinal or body-relative form:**
  `Span`, `QualifiedName` (`{ written, resolved, span }`, with no `PartialEq` of its own, held by
  `BodySummary`'s `PurityCall::Function` and `Callee::{Function, Static}`) and `Ident` in the registry,
  `summaries`, `init_flow` (`Event::Read(Name, Span)` holds the span of a premature read), `purity` and
  `bodies`. An `xtask arch` rule says a result type reaches no `Span`, `QualifiedName` or `Ident`, as the rule
  of (b) does for `Type`.
- **REG-4.15's "a member's span is a field of its entry" is withdrawn:** the entry holds its ordinal. REG-4.11
  ("provenance as the only location record") is then over (`Origin`, `site`).
- A rule that quotes another declaration's line in a message (duplicate sites, `previously declared at`) reads
  that file's sites, and so depends on that file's positions: correct and minimal.
- `Origin::File(FileId)` stays in meaning, but its codec changes: `OriginData::File` stores no file number
  today and `Origin::File(_)` and `PendingFile` come back pending, so a persisted database could not restore
  its file table with it. The codec writes the `FileId` and `PendingFile` is deleted.

**(b) Total, exact equality.** The equality a result type implements compares every field a reader can
observe, exactly: `Name` by pointer, `Type` by node, floats by bits; never a semantic `PartialEq`, never "up to
case or order". Why: backdating keeps the old value when the new one compares equal, so anything observable that
the equality ignores goes stale (a union printed in its old order, a class in its old case), and an edit replay
would then differ from a fresh load (G5's tool 7). The two paths of H1.0 6 both apply: a tracked struct's
field keeps its old value, and a tracked function's result is stored new with the old `changed_at`, so the
exactness rule and its test cover both. Mechanism: salsa takes the result's `PartialEq` (the options
are that or `no_eq`; `options.rs`), so a result type implements `PartialEq` over fields whose own `PartialEq` is
exact. `types` gains `Type::same_node` and a newtype `Exact(Type)` (`PartialEq` and `Hash` by node) that
results hold in place of a bare `Type`. A test per result type changes each field in turn and asserts `!=`
(S-8 runs it once for a field and once for a function result, comparing a reader's output across a case-only
edit); an `xtask arch` rule rejects a result type that derives `PartialEq` over a bare `Type` (it parses one
file at a time with `syn`, so it sees a field written `Type` and not one reached through another struct, an
`Arc` or a container of another crate's struct such as `ClassReflection`'s `method_tags`: each result type's
own test covers those). Other leaf types are inexact on purpose too (`Shape::eq` ignores `next_index`,
`ObjectTypeCell::eq` is always true, and `FoldedName` is case-blind by design, with no spelling to show). `Type`'s
semantic `PartialEq` stays what the checker uses. **The stronger form S-8 evaluates:** if few `==` and
derived-`PartialEq` sites on `Type` rely on its semantic equality (43 `== Type::` sites and 268
`derive(PartialEq)` types today, which S-8 classifies), the semantic one is renamed `Type::equals` and `==`
means the node, compile-driven as `Borrow<str>` and `FoldKey for str` were, after which a derived `PartialEq`
over result types is exact by construction and `Exact` and the rule are belt and braces.

### H1.6 Where an engine id may be named

`salsa` is named only in the engine module (a crate from G1); so are `File` (the input), `DeclKey`,
`ClassKey`, `FunctionKey`, `ConstKey`, `GlobalKey` and `MemberKey`. The engine's public surface is `Provider`
(the accessors, H2.4), `Session` and plain values (`DeclRef`, `ConstDeclRef`, `MemberRef`, `FileId`, names).
An `xtask arch` rule enforces both, as G1's rule does for tree ids, and a release-build test says no engine id
appears in a result type. A reflection, summary, product, the checker and the LSP see no id. The
checker-facing reader is a trait in `reflection` (the accessors of H2.4) that `engine::Provider` implements,
so `analyser` depends on a trait it owns and not on its own consumer (H2.4).

### H1.7 What batch 4 builds, and what it must not

Builds: (1) the thin `Name` with its count-based sweep, which is NAME-2.1's and lands in the same commit
(a thin name has no `Weak`, so nothing frees a name until the sweep exists; gated by S-1, else the fallback
above); (2) `FoldedName`, `ConstName`, the `ArrayKey` rename, and `Type::same_node` and `Exact` (H1.5b); (3)
NAME-1.1, 1.5, 1.6 and SYN-4.5 (`ConstName`'s record kind); (4) NAME-1.2 over **values, not only maps**: the
table is of fields, parameters, returns and maps, with one census command and one count (the mentions of a
`HashMap<String` or `HashSet<String`, std or Fx, outside tests were 181 at `8a1ca1e0`; 187 with tests; the plan
had carried 209 and 25 G 211; 18 `BTreeMap<String`, 9 `BTreeSet<String` and one `HashMap<Box<str>` sit outside
that pattern, and `Vec<String>` (290) and `Option<String>` (220) hold identifiers among other text). It **leads
with the registry's `&str` entry points** (the ledger's cost order), and the compiler lists only those once
NAME-1.3 deletes `TableKey` and `FoldKey for str`; the value half has no compile-time forcing function, so its
completeness check is a reviewed table with a grep gate on identifier-named `String` fields. The `String` keys
of the machinery W2 and W3 delete (`ChangedNames`, `Scope`, `reset_scoped`, the guarded sets in `Locals`) are
converted because NAME-1.3 cannot delete `FoldKey for str` while a caller holds text, which is a type change an
end-state invariant needs (Part E 35); (5) NAME-1.3, NAME-1.4, NAME-2.2, NAME-2.3. Must not: introduce an id, a
second interner, a value that holds a position, or a result type whose equality is semantic. NAME-1.2's
registry entry points are the ones REG-4.12 makes into the `Provider`'s accessors, so REG-4.2 and 4.12 are
written on `FoldedName`.

**Checks.** The conversions follow the ledger's order of cost. Oracle for them: `registry-dump`, `--dump-expr-types` and
O2 before and after (G5), with VAL-2.7's container-field delta read. The engine does not exist yet, so the
rules of H1.5 are tested where they can be in batch 4: `Exact`, the same-node tests and the case-only-edit test
of H1.2. The test that two `Provenance` values for one class at different lines are equal once the span is out
moves to the commit that removes the span (REG-4.15, W2 step 5): from batch 5 (PROD-5.1's site table) until then
declarations hold their spans and the site table at once.

## H2. The engine (Part F 13)

### H2.1 The contract, and what it is built on

What the code above the engine may rely on, whichever engine is below:

1. **Inputs change only by a driver write.** A query is a pure function of inputs and other queries: no clock,
   no environment, no filesystem except through the file source, no state that depends on the thread or the
   order.
2. **Reads are recorded:** every input and query a query called, an absent name included.
3. **Early cutoff.** A new result that is equal (H1.5b) to the old one stops the change there. salsa backdates
   only when the new memo has no cycle heads, is not provisional and did not drop in durability, and only when
   the old value is still there (`function/backdate.rs`): so every member of a recursion SCC (purity, property
   writes, inference, constants on a cycle) re-runs its readers whenever anything in the SCC re-executes
   (cheap for constants and purity, relevant for inference), and an evicted `parse_analysed` cannot be
   compared; eviction costs less than it looks, since a re-executed memo's `changed_at` is the maximum of its
   inputs', so an evicted tree re-parsed from unchanged inputs invalidates no reader, and one whose inputs
   changed could not have been backdated anyway.
4. **A write cancels** the queries running on other handles; a cancelled unit is retried or dropped, and no
   partial result is ever visible.
5. **A cycle** is resolved by its rule (H3.3) or is a bug, reported as one.
6. **Results are `'static` values.** A borrowed view lives as long as a database borrow.
7. **Determinism:** the same inputs give the same results at any thread count and in any order (B8's
   equalities, tested through G5's dumps).

Built on salsa 0.28.5 in the engine module (Part E 33; a crate from G1). It uses `#[salsa::db]`, `input`,
`interned`, `tracked` (with `returns(ref)`, `lru`, `cycle_result`, `cycle_fn` and `cycle_initial`, `heap_size`),
`Durability`, `Cancelled`, `cancellation_token` and `persist`, and one tracked struct, `FileProducts` (H2.3),
whose fields are tracked one by one (`tracked_struct.rs`: `update_fields` moves a field's revision only if its
value changed; `tests/expect_reuse_field_x_of_a_tracked_struct_changes_but_fn_depends_on_field_y.rs` is the
pattern). It does not use accumulators: a diagnostic gathered in an accumulator is outside the result, so
outside cutoff and outside the persisted value. **The feature set is pinned:** `salsa` with
`default-features = false` and `macros`, `inventory`, `persistence` and `salsa_unstable` (`heap_size` needs the
last; `rayon` is used by salsa only for an `Either` impl and `accumulator` is unused), and `triomphe` without its
default features; `persistence` adds `serde`, `erased-serde` and `thin-vec/serde`. The crates new to `Cargo.lock`
are `salsa`, `salsa-macros`, `salsa-macro-rules`, `boxcar`, `intrusive-collections`, `inventory`,
`portable-atomic`, `thin-vec`, `tracing`, `tracing-core`, `typeid`, `erased-serde` and `triomphe`. The repository
runs `cargo deny check` (`deny.toml`, `rust-ci.yml`), so S-1's and S-2's commits pass it (advisories, yanks,
licences), and S-3 adds the clean and incremental build times and the release binary size to its measured
list, since the project has a build-time gate.

### H2.2 Inputs

| input | grain | holds | durability |
|---|---|---|---|
| `ReflectionInputs` | singleton | reflection settings, the configured PHP version range (decision 6; when set it overrides every owner's), assertion tables, `stub_paths` | high |
| `CheckInputs` | singleton | the rule settings and the level | high |
| `OutputInputs` | singleton | `ignore_errors`, the baseline, the formatters' settings; read only by `analysis` | high |
| `File` | one per path ever seen | `path` (set once), `shown` (the path as reports spell it, R6's `/` form relative to the report root, set with the path and by the driver when the root changes), `number: FileId` (an opaque handle: nothing is ordered by it, E1-10), `role`, `analysed`, `stamp` (analysed and eager files; a lazily reached context file's is the query `file_stamp`, below), `overlay` | analysed: low; context: medium; configured stubs and the installed PHPStan's files: high |
| `FileSet` | singleton | the sorted `Vec<File>` that exist now, with the path index that `locate` and the include resolution read, and the index by `shown` that an anonymous class's name is resolved through (H1.4) | low |
| `Packages` | singleton | Composer roots, autoload maps with their keys (classmap entries, PSR-4 and PSR-0 prefixes with their directories, files), owners, vendor directories, and per Composer root its declared PHP range (`config.platform.php`, else `require.php`) and its required packages (Part E 42) | medium |
| (`Builtin`) | **not an input** | the stub corpus (`registry::stubs::Corpus`, an `Arc` shared by every database in the process) is a constant of the database: the build identity in a persisted file's key covers any change of it, and a never-changing singleton could not be persisted without a custom codec (salsa asserts that every input a persisted memo reads is `persist`, and a custom `deserialize` gets no database argument) or set again after a restore | – |
| `SqlSchema` | singleton | the schema file's digest, or a live reflector's fingerprint | medium |
| `ProbeEpoch` | singleton | a counter the driver bumps when the filesystem, or a live SQL schema, may have changed (H4.5) | low |
| `Extensions` | singleton | the registered extension implementors and a fingerprint of the set and its configuration (J2) | high |

- Splitting the settings in three is the grain at which a change should reach: a formatter or baseline change
  must not re-resolve a class, and a rule change must not either. Within an input the grain is the field: a salsa
  input field has its own revision and durability, and a read records that field alone (`input.rs:230`). So each
  setting is a field of its input, and the checker reads settings through a view that fetches a field from the
  database the first time a check uses it and keeps it for the rest of that check (one `OnceCell` per field).
  The recorded reads are then the settings that file's check consulted, and toggling a rule reruns the files in
  which that rule's setting was consulted, not every file. Copying every field into `Checker` when it is built
  would make each file depend on all of them, so the checker's `self.settings.x` reads become calls on the view
  (mechanical: 114 of them in 34 files of `checker/` today, reaching the leaf flags of the extensions'
  `…Settings<bool>` structs, which are the fields).
- **The range of a file is its owner's (Part E 42, ruled 2026-10-18).** The owner of a file is the nearest
  ancestor `composer.json`, a fact of each directory's listing: A8's scan finds a degrade root's nested manifests before
  the walk (COMP-8) and `Packages` holds those directories (A1 builds the vendor fact beside it and not this one); a file
  with no owner uses the project root's manifest. `range_of(File)` is a
  tracked projection of `Packages`: the configured range if one is set, else the owner's declared range (Composer's
  grammar by interval arithmetic, COMP-2), else decision 18's default (open above, ruled 2026-10-18: Part E 40, M5d). It compares equal when the owner's
  range does, so a manifest edit reruns only the files it owns. A declaration's validity is a `VersionSet` that
  does not depend on the range, and the reader intersects it with its own file's range (J1.2); a builtin reading is
  selected with that range (`Readings::over`). A level's memo is keyed by the range only where something it reads
  differs across versions (J1.2), so a class with no version-sensitive ancestor has one memo whatever the owner's
  range, and a project with one range pays nothing. Activation is any owner's: it only turns checks on. A hull of the owners' ranges would judge Symfony's 202
  manifests at `>=8.4.1` against versions they cannot run on, and the root's alone would judge its 8 manifests at
  `>=8.1` as if they needed 8.4.
- `stamp` says whether the file's content may differ, and what it holds follows decision 13 (CACHE-13.1). On a
  file system known to keep a change time that no program sets (ext4, xfs, btrfs, APFS and the like) it is the
  `stat` tuple, trusted under the racily-clean rule, whose reference time is the clock reading taken at the file's
  own stat (Part E 15; decision 13's "the index's own write time" does not fit a database written after the whole
  check); on any other, and on Windows, it is the content hash, so
  the file is read and hashed. Either way the driver bumps it when the content may differ (an overlay changes, or
  the rule distrusts the tuple). The walk takes no `stat` today, so the stamp is new work: for the analysed and
  the eager files, which are all read anyway, the loader takes it from the open descriptor before the read. A
  **lazily reached context file** (H4.1) has no input stamp, because a query may not write an input: its stamp is
  the tracked function `file_stamp(File)` over `ProbeEpoch`, statted by the query that first needs it before it
  reads the bytes (the design of `path_is_file`, H4.5), so a first read is tracked, a later edit is found when
  the epoch moves, and a restore does not turn "none" into "some". The file *source* is not an input; it is
  a handle in the database (the overlay map and the disk), read only by `file_digest(File)` and
  `parse_analysed`. Because the driver bumps `stamp` whenever the content may differ, a read is always
  consistent with it.
- `shown` is a field because the checker takes it (`set_current_file`, level 9's R18) for the path
  rules (`in_test_code`, the restricted symbols' path rules) and the finalizer matches `ignoreErrors` paths and
  baselines against it (R6); an anonymous class's name carries it. It is a function of the path and the report root,
  so it is a field of the input the driver sets and not a query; a change of root rewrites it, and only the files
  that read it rerun.
- **Honesty rule:** an input is changed by the driver and by nothing else, and a query reads nothing but inputs
  and queries. The checker has two live reads, and both are made queries over the epoch input to keep it:
  `include_target_exists` (`path_is_file`, H4.5), and a live SQL reflector, whose `PREPARE` per distinct SQL text
  (`resolve_sql_row_type` falls through to `context.reflect(sql)`, cached by text in `SqlContext`'s own map) is
  the tracked function `sql_columns(SqlText)` over `SqlSchema` and `ProbeEpoch`: the driver bumps the epoch at
  each run start and when drift detection fires, so a persisted answer is asked again once per run and an
  unchanged answer cuts off, and `SqlContext`'s own cache goes (the memo is the cache). A closure-gate case
  changes the test database's schema between two runs: the warm run's diagnostics equal a cold run's.

### H2.3 Queries

`returns(ref)` throughout: a reader gets `&'db T` from the memo, no clone. Equality is H1.5b's.

| layer | query | key | result | notes |
|---|---|---|---|---|
| file | `file_digest` | `File` | the content digest, nothing else | **analysed files only.** Depends on the stamp (`File.stamp`, or `file_stamp` for a lazily reached file) and is persisted (digest only). The text is not a result: `parse_analysed` reads it from the `FileSource` handle under the stamp's protection, so a restored `file_digest` memo that is valid by stamp never hands out a missing text (H4.6). A context file keeps no text (LOAD-3.4): its `file_products` reads the stamp and the bytes once, in the worker's reusable buffer, and the products are the cutoff, so a touched but identical file stops there; its skimmed bodies' text is a field of its products, below |
| file | `parse_analysed` | `File` | `Arc<Parsed>`: the tree, errors, marks | a tracked function for **analysed files only**, which need their tree twice (their products, then their check). Kept for the whole run in the batch (no `lru`) and under `lru` in the server (H2.9). A context file has no tree query: its tree is built inside `file_products` (H2.9). Infallible (G3) |
| file | `file_products` | `File` | a tracked struct with one tracked field per product: `names` (PROD-5.8's record), `decls` (`FileDecls`, positionless, H1.5a), `decl_names` (the file's `DeclRef`s alone, anonymous classes left out, which `decl_index` reads), `decl_edges` (each class's `extends`, `implements` and `use` names as written, positionless, which `hierarchy_cycles` reads), `sites` (the positions), `facts` (mentions flags, `has_declaration_in_body` and the rest, and the file's validity `VersionSet` from its marks, G3.5), `globals` (H3.7), `bodies` (a context file's skimmed function bodies, each copied out by ordinal as PROD-4.2 does today, so that `decls` holds no body text and a body edit leaves it equal; an analysed file's bodies are in its tree), `doc_blocks` (DOC-2.9: each doc comment parsed once and found by its offset, read by the checker and by no level), `ignores`, `lines` (`LineStarts` or `LineIndex`, SYN-3.1), `use_sites` | made in one pass over the tree, so no intermediate is a memo: an analysed file reads its `parse_analysed` memo, and a context file's tree is built in the worker's scratch builder and cleared once the products exist (LOAD-3.5); **a reader depends on the field it reads:** a body edit leaves `decls` equal, a member edit changes `decls` and leaves `decl_names` equal, a line inserted above a class changes `sites` and nothing else |
| index | `decl_index` | none | names to `[DeclRef]`, over every eager file's `decl_names` | O(all declarations); see below |
| index | `declarations_named` | `ClassKey` / `FunctionKey` / `ConstKey` (generic over the key) | `Arc<[Candidate]>`, sorted by **the files' path order** (the `FileSet` order), never by `FileId`: a `DeclRef` (or `ConstDeclRef`) and its validity `VersionSet` (the `Availability` over a range is the reader's, H2.2) | **the recorded read of a name.** An absent name is an empty slice. A file's declarations exist only where its syntax holds (decision 15, Q-10): the availability of a project or context declaration is its file's validity `VersionSet` (G3.5's marks, a field of `facts`), which the reader meets with its own file's range (`range_of`, H2.2), and a file valid nowhere in that range gives that reader no candidate, as PHP refuses it there. A file number is first-seen order (`file_table.rs`: a new path takes the next free number), so a database edited into its state and a fresh load number the same paths differently; sorting by path order (which `FileTable::rank_of` already is) is what keeps candidate order, the `global_site_index` lists and every accumulation, and with them a union's member order and "previously declared at", equal between the two (a test builds one file set by two histories and compares `registry-dump`, candidate order included) |
| index | `hierarchy_cycles` | none | the edges dropped | one pass over the declared edges (`decl_edges`) of the project's and context's classes (H3.3); a member or signature edit reruns nothing in it |
| class | `class_ancestors`, `class_own`, `class_inherited`, `class_members`, `class_final` | `DeclKey` | REG-7's five levels | H3 |
| class | `class_member` | `MemberKey` (`DeclKey`, `MemberName`) | `Option<Arc<Member>>`: one member of `class_final`, absent included | a projection, so its readers rerun only when that member changed (H2.4) |
| function | `function_final` | `DeclKey` | REG-7.12 | |
| constant | `global_constant` | `ConstDeclRef`, interned | type and throws | keyed by a declaration like the levels; the name-level combination of candidates is the `Provider`'s (H3.4) |
| constant | `class_constant_value` | (`DeclKey`, name) | value type and throws (decision 1) | `cycle_result` |
| constant | `class_constant_value_only` | (`DeclKey`, name) | the type without the constant's PHPDoc, and whether the PHPDoc was dropped | the fallback of the PHPDoc case in H3.3(2); no PHPDoc edge enters it |
| body | `body_summary` | (`DeclKey`, member ordinal) | `BodySummary`, `InitFlow` | on demand, from the declaration's body text |
| body | `purity`, `property_writes` | `DeclKey` / member | verdicts (REG-1) | `cycle_result` |
| body | `receiver_types` | (`DeclKey`, member ordinal) | the inference in a no-effects mode, for the receivers that stage (H3.8) | may not read `purity` or `property_writes`; read by both |
| body | `infer_body`, `inferred_return`, `inferred_throws` | `DeclKey` / member | REG-17 | fixpoint with widening (H3.3); `infer_body` reads `purity`, `property_writes` and `receiver_types` |
| body | `effective_return`, `effective_property` | `DeclKey` / member | the declared type, else the inferred one (H3.8) | the inference family; the `Provider` reads signatures from `class_member` and these |
| check | `check_file` | `File` | `FileCheck`, a tracked struct with one tracked field per fact (diagnostics before finishing, member refs, escaped enums and classes, named-argument facts, property-assign facts, property-type sites), so each pass depends on the fact it reads | the type dump is a request, not a memo |
| check | `property_type_verdicts` | the class group (the declaring `DeclKey` of the writer scope) | `judge_property_types`'s verdicts for that group | `judge_property_types` merges the facts of every writer in path order, expands them through used traits and re-runs the checker on the other files declaring the class "for their writes alone" (`project.rs`), so the verdicts are keyed by group over an index query of writers built from the check facts; a keyless query would make the first worker to arrive compute every `check_file` while the others block |
| check | `file_diagnostics` | `File` | `check_file`'s diagnostics with the per-file project rules added (duplicates, ignore parse errors) and the verdicts of this file's own property-type sites (`check_too_wide_property_types`), sorted | the batch phases stay explicit: all `check_file`s first, then the verdict-reading diagnostics and the passes |
| project | `global_sites` | `File` | the file's `FileSites`: its global-variable facts resolved through the provider | H3.7 |
| project | `global_scope_files`, `global_site_index` | none | the files whose top level runs in global scope; each name to the files that assign it | H3.7 |
| project | `global_variable` | `GlobalKey` (a `Name`, exact) | one global's accumulated type | H3.7; the provider's `global_variable(&Name)` |
| project | the whole-program passes | none | dead code, class shapes, architecture, banned constructs, Composer and mock hygiene, unmatched ignores | each reads per-file facts, not trees (LOAD-5.1) |
| project | `analysis` | none | `AnalysisResult`: the finalizer over `file_diagnostics`, the passes and `OutputInputs` | |

- **Notation.** `file_decls(file)`, `file_sites(file)`, `file_names(file)` and `line_index(file)` in the text below
  are the `decls`, `sites`, `names` and `lines` fields of `file_products(file)`, each read with its own
  dependency.
- **The declaration index.** One tracked query over every eager file's `decl_names` (the `DeclRef`s alone),
  O(all declarations), run again when a declaration is added, removed or renamed and not when a member or a body
  changes: those change `decls` and leave `decl_names` equal, which is why the two are separate fields. It
  beats the alternative designs on what an edit costs: per-name input slots maintained by the driver (the first
  design considered) put derived data back into inputs and need a second revision per edit; a whole-index query
  is plain, and the re-running readers (`declarations_named` of the names asked so far, a hash probe each,
  about 374 Ir by REG-16.1's figure) back out at once when their slice is equal. For Piwigo's size that is of
  the order of 10 to 50 ms per edit that adds, removes or renames a declaration, and nothing for any other
  edit. S-3 measures it; the alternative is the slot design, and the switch is local to these two queries.
- **Why `declarations_named` returns declarations and not one:** decision 11. A name binds to the
  declarations PHP may bind it to; most names have one, a guarded polyfill has two, and each is its own
  entry. `Candidates` is `One(DeclRef)` or `Many`, and the level queries are per declaration; the provider
  combines candidates the way decision 6 combines a builtin's readings (H3.4).
- **`Builtin` declarations** are not in `decl_index`: `declarations_named` adds them from the corpus where they
  are available in the version range (J1.2), and their levels are queries keyed by `DeclKey` too, built from the
  corpus records. The corpus never changes, so queries that read nothing else are never re-verified.

### H2.4 The `Provider`: the accessors, and how a name is read

`Provider<'db>` is the only way the checker, the rules and the LSP read reflection. It holds `&'db dyn Db` and a
per-provider table (`FoldedName` to the answer), is made per check or per request, and is not `Sync` (a handle per
thread, as the database is). Its methods are REG-4.12's accessors (about 280 sites outside `registry/` become
calls on it): `class(&Name)`, `function(&Name)`, `constant(&ConstName)`, `method(&ClassView, &FoldedName)`,
`is_subclass_of`, `ancestors`, `class_constant`, `purity`, and the whole-registry iterations REG-7.10 names.
**The checker, the rules and the LSP do not name `Provider`: the checker-facing reader is a trait in
`reflection`** (these accessors; generic or `&dyn` in `Checker<'a>`), with `engine::Provider` as its implementor.
`analyser` sits below the engine, and a `Checker` that named `engine::Provider` would make `analyser` depend on
its own consumer, which `engine` already is (it calls `check_program`), a cycle the module graph forbids; the
trait keeps `File`, `DeclKey` and the other keys out, as H1.6 intends. H3.1's `LevelReader` is the smaller trait
on the level side.

- A read of a name: probe the provider's table by the name's stored hash (67 Ir, REG-16.1's figure for "one query
  per name per file, then a table of the file's own"); on a miss, `declarations_named(ClassKey::new(db, name))`
  (`FunctionKey` for a function, `ConstKey` for a constant) and the candidates' `class_final`. The provider keeps
  the answer, an absent one too. Inside a tracked function (`check_file`) the reads are recorded; outside one (an
  LSP request, a test) they are plain.
- **Granularity: a read goes to the query that answers it.** Whether a name exists reads `declarations_named`
  and nothing else; a subclass test reads `class_ancestors` (level 1), which an edit to a method or a property leaves equal; a
  member (a method, property, class constant or enum case, present or absent) reads `class_member`, a
  projection of `class_final` that returns that one member and backdates when the member is equal; only a
  whole-class question (abstract-method and interface-completeness checks, member listings) reads `class_final`.
  So editing one method's signature reruns the files that read that method, where today's `dependents` and
  `signature_dependents` stop at the class and rerun every file that reads it at all. `MemberKey` is the engine's
  interned (`DeclKey`, `MemberName`: H1.4); a method is asked by the `FoldedName` it was written as, so one
  method has one memo and one edge whatever spelling is used, and a property, class constant or case by its
  exact `Name`; an absent member is a recorded read (`None`), so a member added later
  reaches the readers that looked for it, and the `__call` and `__get` fallbacks are member reads of their own.
  When `class_final(C)` changes, the projections of C that were read rerun (a lookup and an equality each).
  **Cost, unmeasured:** REG-16.1 counted names (107,068 (file, name) pairs), not members, so the edge count at
  this grain is S-3's: it counts the distinct (file, class, member) reads and what their edges cost (a
  `QueryEdge` is 12 bytes and a packed one 8 in salsa's source, `zalsa_local.rs`, besides the memo's own
  overhead, H2.9), against the criteria H5 gives. The finer grain is the design; reading `class_final` for
  members is the fallback if S-3 kills it, a `Provider` setting that changes no caller.
- **Signature against body.** `class_final` holds no body, summary or position. What a body shows (purity,
  property writes, an inferred return) is its own query, asked when a rule asks, so a file that only calls a
  method depends on the method's signature and not on its body.
- **REG-4.12 first, on the old storage.** The accessors land before the engine, over the existing maps; the
  engine then replaces what is behind them. That is W2's order (H3.6).

### H2.5 The check

`check_file(db, File)` builds a `Provider` and a `Checker` (G1), runs `check_program` over
`parse_analysed(file)`'s root, and returns `FileCheck`. It reads: the file's `parse_analysed`, `file_decls`, `file_sites`, `file_names`, `line_index`; the
`CheckInputs` fields it consults (H2.2), `ReflectionInputs`, `Packages` (the owner); everything its provider reads;
and an included file's `file_digest` (an analysed file) or stamp (a context file's `file_stamp`), whose tree `includes.rs`
takes from `parse_analysed` or, for a context file, parses unmemoized, so editing an included file rechecks its includers, which today is the dependents index's job. **The statement-level poll `db.unwind_if_revision_cancelled()` is new work:** today `should_abort()` is
polled once per affected file, between `check_file` calls (`project.rs`), and no poll exists inside `checker.rs`
or `checker/*.rs`, so a cancelled write waits for the file being checked. The polls inside a file are added per
statement and, where one statement can be huge, per item of an array or call-argument list, per member when a
union is built, per declaration in the products pass and per size step in a parse over a stated size (S-5's accept
includes a 50 MB array-literal statement). Requests that need more than the memo (inlay hints, semantic tokens, hover,
completion probes, find-references' member collection) run the checker again, unmemoized, on a `Provider` outside any
query, as `checker_for` does today; E6's observers (Stage D) later replace the re-run.

### H2.6 Revisions

- **The batch run** writes every input before any query runs, and no write follows. salsa starts a revision
  at each input setter (`ingredient_mut` calls `new_revision`), so N writes are N + 1 revisions; that is
  harmless here (no handle exists to cancel, and the results do not depend on the count), and the cost of a
  `cancel_others` per write is paid only in the server. Nothing in the batch run is evicted by `lru`, since
  trees are not memos (H2.9).
- **The server** makes a revision per write, and **applies a notification's events as one batch** (today
  `handle_watched_files_changed` takes the whole `Vec<FileEvent>` and coalesces it to the last event per path;
  one `FileSet` write per event would cancel and wait once for each): `didOpen`, `didChange` (the overlay and the
  `stamp`), `didClose`, watched-file events, configuration. After the writes the server asks for `file_diagnostics` of each analysed
  file in priority order (the edited file, the open files, the rest), and publishes each as it completes and
  only if it differs from what was published. A file whose reads did not change verifies in a few
  microseconds (REG-16.1: all 2,792 in 5 to 10 ms); a file that read something that changed runs its check.
  This replaces `walk_affected`, `closure_scope`, `dependents` and `signature_dependents` wholesale.
- **TYPE-4's memo** carries the revision it was filled in and is cleared by the first access in a later one. A
  write cancels running queries, so no thread reads the memo at an older revision after a newer one began.

### H2.7 Concurrency and determinism

One database handle per worker, on the existing pool (`worker_pool`, 64 MiB stacks). **How the handles are
made:** the database is `Send` and not `Sync` (its `ZalsaLocal` holds a `RefCell`), so a closure that captures
`&db` cannot be shared; and rayon's `for_each_init` calls `init` once per rayon job, not once per thread, so it
would make a handle per job split. The driver therefore makes N handles (`db.clone()` on the driver thread, or a
`StorageHandle` and `into_storage` on each worker) and puts one in a slot per pool thread (indexed by
`rayon::current_thread_index`), or hands one to each of N long-lived worker tasks that pull files from a queue.
A write waits until every other handle is dropped (`Storage::cancel_others`, `storage.rs`, with no timeout, which
warns that a worker holding two handles deadlocks it), so a thread holds at most one, none survives into a write,
the driver stops dispatching once a write is pending (a worker that starts a unit after the flag is set only
makes a handle to drop it), and the server's request handlers take a handle per request and drop it before they
return. **The server applies writes on a thread other than the one that answers `$/cancelRequest` and
`shutdown`,** since the writer waits for every handle and an unbounded wait must not hold up those.
**Queries do not use `rayon` inside, and no closure inside a query touches a database handle:** a tracked query
body reads everything it needs on its own thread first, then fans plain data out to rayon (a handle cloned on
a worker would record its reads on its own query stack, not the pass's, and a same-thread cycle could be
reported when rayon runs a job inside a wait); the whole-program passes read the per-file facts sequentially
(about 2,800 reads) and parallelise only the pure merge, or are plain driver functions over query results. A
query that is already running on another thread is waited for by the asker (salsa blocks, with deadlock
detection for cross-thread cycles). **A stack guard:** a chain of tracked wrappers recurses on the stack (a
`class_ancestors(C)` calls `class_ancestors(parent)`, a hundred thousand deep for a hundred-thousand-class
chain, and constants that name each other likewise), and salsa has no stack growth, so the engine's wrappers call
`syntax::stack::guard`, and S-5 and S-6 add a 100,000-deep hierarchy and constant chain on a 256 KiB thread (F20's
depth test, extended to the engine). Determinism: no query reads the order of a hash map's
iteration (REG-4.14); a union's member order does not depend on scheduling (the rule `judge_property_types`
already follows: facts merge in path order); and nothing the engine sorts is ordered by `FileId` (H2.3). S-9
measures contention on shared levels, which REG-16.1 did not.

### H2.8 Cancellation and panics

- **Cancellation** is salsa's: a write unwinds other handles' queries with `Cancelled::PendingWrite`, and
  `Database::cancellation_token()` cancels one handle's (`Cancelled::Local`); both unwind by `resume_unwind`,
  without the panic hook. `$/cancelRequest` calls the request's token; F1's `CancellationToken` is that token.
  A cancelled request answers `ContentModified` or `RequestCancelled` (Part E 36 says which methods the client
  retries); a cancelled analysis unit stops and the next revision starts it again. **A local cancel is ignored
  while the thread executes a query with `cycle_result` or `cycle_fn`, and everything nested under it** (salsa
  disables local cancellation there, `function/execute.rs`, because a local cancel would poison the cycle; the
  token is also reset when the outermost attach scope ends), so a hover or completion that reaches a constant,
  purity or inference query cannot be interrupted by `$/cancelRequest` until that query returns, and only the
  pending-write flag still works. The inference passes get statement-level polls of the global flag, and S-5 has
  a case: `$/cancelRequest` during a long inference completes the request, and the server drops the answer
  instead of interrupting it.
- **Isolation, in two modes.** Each per-file unit and each request runs under `crash_context::isolated` (F1, the
  crash reporting plan). *At the driver, outside any query,* it first separates `Cancelled` (not an error:
  `Cancelled::catch`'s downcast) from any other payload (an internal error for that unit) and returns
  `Err(Cancelled)` for the unit. *Inside a tracked query* (the crash plan runs each whole-program pass as
  `isolated(Unit::Pass(name), ..)`, and `carry` catches a worker's panic and resumes with a `CarriedPanic`
  payload) it re-raises a `Cancelled` payload unchanged with `resume_unwind` and records only real panics, and
  `carry` passes `Cancelled` payloads through untouched: a `catch_unwind` that swallowed a `Cancelled` would let
  the query return, be memoized and survive into the next revision with a partial result. A unit that got
  `Cancelled::PropagatedPanic` (another thread's query panicked while this one waited on it) is run once more;
  if the same query panics again it is this unit's internal error, as it is for every file that reads a query
  that cannot complete. A test: a write arriving while a pass runs inside `analysis` leaves no `analysis` memo,
  and a real panic in one pass leaves the others' diagnostics.
- **Internal errors are not a query value.** A panic leaves its query unexecuted, so `analysis` and every
  whole-program query that reads a panicking `check_file` would panic too, and no `AnalysisResult` would exist
  to carry the report. So `check_file` catches the panic at its own boundary into a value, `FileCheck` with an
  internal-error record (the panic's payload text and the unit), which compares and persists like any result;
  `analysis` carries those records into `AnalysisResult` (exit status 2 and "a file with an internal error isn't
  cached" are F2's and follow from it: a `FileCheck` holding a record is not persisted). The "N more file(s)
  loaded for context" count of `check_project.rs` is a count of what this run read, which a lazy and persisted
  load changes between warm and cold, so it is reported from the files the load listed, not the files it read.
- **Cancellation by unwinding depends on the stack guard restoring its limit on unwind.** `syntax::stack::check`
  stores the outer limit, calls `stacker::grow` and restores `LIMIT` only when `grow` returns normally; stacker's
  `_grow` catches a panic inside the new segment, drops its own guard and resumes the unwind on the old stack,
  which skips the restore, so a later guarded call on that thread could compare against a freed segment's address
  (the case exists before this plan: `session.rs` wraps the parse in `catch_unwind`). F20's new `stack` crate
  restores `LIMIT` from a drop guard so that it also runs on unwind, with the test F20 lacks: a panic from inside
  a grown segment, caught, then a 100,000-level recursion on the same thread; S-5 repeats it with a `Cancelled`
  unwind.
- **A panic in a query** leaves it unexecuted; the next ask runs it again (the property REG-7.9 asks of
  `OnceLock`) for an ordinary query. **A query with a cycle strategy (`cycle_result`, `cycle_fn`) is poisoned
  instead:** salsa installs a poison guard for each execution (`execute.rs`), a panic leaves a valueless
  provisional memo, and a retry in the same revision throws `PropagatedPanic` again, so a constant, purity or
  inference query that panicked cannot be re-run until the next revision; the unit's internal error is built
  from the first panic's record, kept in the engine by query key. A panic-injection test over a shared level, a
  constant, a purity and an inference query, and a parallel-resolution test over cyclic fixtures, stay REG-7.9's
  tests.

### H2.9 Memory

- **Which trees are memos.** A tracked `parse` with `lru` for every file, evicted by `trigger_lru_eviction`, was the
  first cut. It frees every evicted tree on the one thread that evicts, which is the cost LOAD-3.3 measured and
  removed (−138.6 M Ir and −214 MiB peak when a context tree's drop moved to the worker that built it), and most
  trees are context trees. So a context file's tree is built in the worker's scratch builder inside `file_products`
  and cleared once the products are made (LOAD-3.5): it is freed, if at all, where it was allocated, and it is
  never a memo. A context file's tree wanted later (navigation into a vendor file) is parsed unmemoized. An
  analysed file's tree is a memo, `parse_analysed`, because its products and its check both read it. In the
  batch run it is kept for the run and dropped at exit, as the analysed trees are today; LOAD-5.2 (free each
  tree when its check ends) is memory only, and item 9 shrinks what it would free from 151 MB (MEAS-2; the
  analysed trees at exit) to the flat tree's size, so whether it is still wanted is S-7's to say, with the cost
  of freeing on another thread; **until S-7 says, W3 builds it as the ledger has it** (LOAD-5.2 needs LOAD-5.1,
  whose facts W3 builds, and LOAD-3.1; Part E 39), and S-7 chooses between freeing on the checking worker and a
  bounded eviction, or drops it explicitly in Part D. In the server
  `parse_analysed` has an `lru` (`set_lru_capacity`), so the batch and the server hold trees alike (Part E 28) and
  differ only in the capacity; a tree evicted is parsed again from the overlay or the disk, whose digest
  `file_digest` holds (LOAD-5.3). Eviction never loses cutoff for unchanged inputs (H2.1 3).
- **Memos.** A class's levels share structure (member maps behind `Arc`; a level stores what it changes), so a
  class costs about its final reflection plus the ancestors' set. `heap_size` is set on the heavy queries so the
  memo bytes are measured, not guessed. The dependency graph is one edge per distinct read of each query: about
  107,000 for Piwigo's checks at the grain of names (REG-16.1's count), 12 bytes each (`QueryEdge`; 8 packed),
  plus the member reads of H2.4 and the interned-key edges of H1.4, which S-3 counts. **Retained memos (Part E 38):**
  an immortal `DeclKey` keeps the memos of a dead declaration until the database is rebuilt, so `heap_size` also
  measures them over an edit replay, and H4.6's in-process compaction bounds them.
- **The kill criterion** is REG-12's: peak RSS at or below the phase start.

## H3. REG-7 and REG-8 on the engine (Part F 14)

### H3.1 The five levels as queries

Each level is a tracked function keyed by `DeclKey`, per declaration (decision 11: no merge). A level reads the
declarations, a lower level of any class, and its own level only along an ancestor edge; that rule is what
keeps the queries acyclic.

| level | query | result | reads |
|---|---|---|---|
| (declaration) | `class_decl` | `Arc<ClassDecl>`, the entry of `file_decls(file)`: parents as written, `use`d traits, `@mixin` and `@method` tags, members as declared, doc types as written names and `TypeNode`s, templates, the file's `ImportScope` | `file_decls(file)` (a change anywhere in the file re-runs it; it is a projection and backdates) |
| 1 | `class_ancestors` | the `extends`/`implements` closure in resolution order, the `use` closure in composition order, `@mixin` targets, unresolved edges (a name no declaration answers to, with the class declaring it), the folded ancestor set that `is_subclass_of` reads, the constants composed with the traits' | `class_decl`; `declarations_named` of each parent; `class_ancestors` of each candidate parent; `cycle_edges_of` |
| 2 | `class_own` | own members composed with the `use` closure's declared members, `@method` tags, every type resolved in the class's context (imported aliases, `value-of`, class-constant types, constant-named types), the interim assertion step | `class_decl`, `class_ancestors`, the traits' `class_decl`, and the exporters' `class_decl` and the target's `class_ancestors` for an imported alias; `class_constant_value` and, for a constant-named type, `global_constant` and `declarations_named` (`constant_named_value` reads a constant and a class's existence) |
| 3 | `class_inherited` | PHPDoc inheritance and property hooks | `class_own`, the ancestors' `class_inherited` |
| 4 | `class_members` | each `@mixin` target's public members, added where the class has none of that name; then the member owners | `class_inherited`, the mixin targets' `class_inherited` |
| 5 | `class_final` | decision 8's combination of native and PHPDoc types, the relation reading other classes' hierarchy and members | `class_members`, other classes' `class_ancestors` and `class_members` through the relation |

A level stores what it changes and shares the rest with the level below (member maps behind `Arc`), so the
five memos cost about the final reflection plus deltas (H2.9 measures). `function_final` is REG-7.12's resolver
(`finalize_standalone_sig` today); `global_constant` is the constant case.

**Where the logic lives.** The crate split's L3 `reflection` holds the resolution logic and L4 `analyser` the
checker, and both sit below the database. So the levels are written as **pure functions over a reader trait**
(`reflection::levels::own(reader: &dyn LevelReader, decl: &ClassDecl) -> OwnLevel`, with
`LevelReader::{declarations_named, class_decl, ancestors, cycle_edges_of, own, inherited, members,
constant_value, global_constant}`; its keys are reflection-level declaration references that the engine
interns, so no engine id crosses the trait), and the engine module wraps each as a tracked function whose
reader calls the other tracked functions, so every call records its read. Tests of `reflection` use a reader
over plain maps, with no database; the checker reads through the checker-facing trait of H2.4, which `Provider`
implements. This is what keeps salsa in one module, `engine`, which sits between `analyser` and `project` in the
crate split's layers (L6, the first half of `project`'s role: the result cache and the dependency graph are what
it is), and what keeps the contract of H2.1 implementable by another engine.

### H3.2 The finishing passes, by level

| today (`run_finishing_passes`, `project.rs:1533`) | becomes |
|---|---|
| `canonicalize_class_name_case` | deleted (REG-4.2: folded keys) |
| `flatten_traits`, `resolve_method_tags`, `apply_assertion_table` (interim until C6) | level 2 |
| `resolve_deferred_type_imports`, `resolve_deferred_value_of`, `resolve_constant_named_types` | level 2, on demand, so `class_has_deferred_type`, `function_has_deferred_type` and the eager pass go (REG-7.2) |
| `flatten_phpdoc_inheritance`, `inherit_property_hooks` | level 3 |
| `resolve_deferred_mixins`, `precompute_member_owners` (and `_after_mixins`) | level 4 |
| `precompute_ancestor_names` | level 1 |
| `drop_doc_members_excluded_by_native` | level 5 |
| `reset_demand`, `changed_since`, `walk_affected`, `closure_scope`, `finishing_scopes`, `Scope` | deleted: recorded reads and cutoff |
| `infer_global_variables_of` | `global_sites(File)`, `global_scope_files`, `global_site_index` and `global_variable(GlobalKey)` (H3.7) |
| `rebuild_pest_config` | an ordinary query over the Pest files |

### H3.3 Cycles: three, each with its own rule

1. **Hierarchy (REG-7.1).** One query, `hierarchy_cycles`, runs Tarjan's algorithm over the declared
   `extends`, `implements` and `use` edges of the project's and context's declarations (candidates expanded; the
   builtin corpus is acyclic and left out), over a canonical node order, and returns the edges between two members
   of one component, a self-edge included. `cycle_edges_of(DeclKey)` slices it per declaration, so a cycle
   appearing or going away re-runs only the classes whose own slice changed; `class_ancestors` drops those edges.
   "The same whichever class resolves first" holds by construction, since no class resolves before the pass. No
   salsa cycle is ever entered, so the default strategy (panic) stays for these queries and a hierarchy cycle that
   reached it would be the bug REG-7.9 says it is. Cost: one pass over the declared edges of the project's and
   context's classes (12,546 classes in the 20 G profile's load), run again when declarations change; S-6
   measures it.
2. **Constants (REG-7.6, decision 1).** `class_constant_value` and `global_constant` use salsa's
   `cycle_result` (`FallbackImmediate`): every query on the cycle gets the fallback, which is the rule, "every read
   is `never`, and the throw is its throw point". Its fallback function receives the query's own id
   (`tests/cycle_fallback_immediate.rs`), so the PHPDoc case, a constant whose PHPDoc reaches a cycle, can fall
   back per constant to *its type without that PHPDoc*, and the PHPDoc is reported unresolvable. **The
   value-only query is `class_constant_value_only`** (H2.3: key (`DeclKey`, name), `cycle_result` = `never`
   plus the definite throw, and a flag in its result for "PHPDoc dropped"; no PHPDoc edge enters it). The
   fallback is: ask it; if it is itself on a value cycle the answer is `never` plus the definite throw, else its
   type and the flag. salsa's tests show a query can be called from a cycle initial function
   (`tests/cycle_initial_call_query.rs`) and that re-entering the cycle panics
   (`cycle_initial_call_back_into_cycle.rs`), and `cycle_initial` and `cycle_result` receive the query's own id
   and its arguments (the macro passes them; only `cycle_fn` gets the `Cycle`), which is what S-6 confirms. "Reported once"
   is produced by a report query over the value-only SCC, at the earliest member in declaration order. A
   constant that merely reads a cycle's member computes from the fallback, as PHP's read of it would throw.
3. **Bodies.** `purity` and `property_writes` use `cycle_result` with the impure / `Anything` answer: REG-1's
   rule is that a declaration in or reaching a cycle never leaves it, which is what an immediate fallback
   on every member gives. `inferred_return` and `inferred_throws` (REG-17) use `Fixpoint`: `cycle_initial` is
   `never`. **Termination is shown, not asserted (ruled 2026-10-18).** A recursive body that wraps its own result
   (`return $n ? [f($n - 1)] : 1;`, a generic or object argument, a closure type) grows one level of nesting per
   iteration while no literal changes; the `types` crate caps union width (64) and shape fields (256) and has no
   depth cap; and salsa does not stop at 200 iterations (`cycle.rs:58`): it panics, in release too, and re-runs
   every member of the cycle each time. Decision 17's "a literal that an iteration changed" is also a function of
   the iteration history, so a cycle entered at another member could end at another type, against H2.1 7. So
   `cycle_fn` is the capped operator, a function of the value alone: the iterate is the least fixpoint of
   `cap ∘ f` from `never`, which does not depend on the member the iteration is entered from. `cap` is the
   `types` crate's own generalization (a union past `MAX_UNION_MEMBERS` generalizes by the chain in `union.rs`,
   and the result is a fixed point), the shape-field cap, and two caps that do not exist yet, a nesting depth and
   a generic-argument depth. A literal that stays the same across iterations is kept (`'done'` stays `'done'`),
   which the first draft's "widen every literal in a recursive position after the first iteration" lost; one that
   keeps changing (`$n + 1`) is bounded by the union cap and generalizes to its kind there. A legitimate chain
   therefore takes up to about 66 iterations (64 members, then the generalization), so the safety net, when
   `cycle.iteration()` reaches K, returns the position's top type and is an internal error in a debug build, sits
   just under salsa's cap of 200: it is never expected to fire, and salsa's own cap is never relied on. Decision
   17's sentence becomes "the iterate is the least fixpoint of the capped operator", with the same intent (an
   inferred return keeps the literals its body returns). S-6 adds the nested-wrapper fixture, a mutual-recursion
   fixture entered from each member, and a `$n + 1` fixture whose iteration count it records: if the cost of
   about 66 iterations per member is too high, a lower per-kind literal threshold inside the cycle operator is the
   fallback, which is a function of the value as well.

Any other cycle is a bug. In a debug build it panics with the query stack; in a release build the unit
reports an internal error, as a panic does.

**A cycle is homogeneous.** salsa lets the query that is entered first decide: the cycle head's strategy
(`cycle.rs`, "the cycle head is responsible for managing iteration") either iterates the whole cycle to a fixed
point or gives every member with a fallback its fallback. Entered from another query, a cycle of mixed strategies
could resolve differently, which H2.1's determinism rule forbids. So every cycle has one strategy: the hierarchy has
none (the pass above), constants and purity are `cycle_result`, inference is `Fixpoint`, and an edge from one family
into another may not close a cycle. REG-17.4 is the row that threatens this (H3.8).

### H3.4 Candidates

`declarations_named` returns every declaration the name may bind to. The `Provider` turns that into a
`ClassView`: `One(&ClassFinal)` for the usual name, `Many` for a guarded polyfill or a duplicate. Every accessor on
`ClassView` has the `Many` semantic defined once, in the combinators decision 6 gives a builtin's readings:
an argument is checked against each candidate, a return or a property's type is their union, and a member that
only some candidates have is available in part (reported unless a guard covers it); the relation reads such a
class as each candidate, an ancestor every candidate has being certain and one that only some have possible.
`class_ancestors` carries that certainty. No level query handles `Many`; the levels are per declaration.

### H3.5 What the rows become

| row | becomes |
|---|---|
| REG-7.1 to 7.5, 7.12 | the queries above |
| REG-7.6 | `class_constant_value`, `global_constant` with `cycle_result` |
| REG-7.7 | nothing is built as scratch: `ClassDecl` carries `method_tags`, `trait_aliases`, `resolved_trait_conflicts` and `type_imports` as declared; the rules that read them read `class_decl` |
| REG-7.8 | `Session::load` demands every file's `file_decls` and `decl_index`; the whole-program passes are queries; `index_and_flatten` is deleted |
| REG-7.9 | H2.8 and H3.3 |
| REG-7.10 | `Provider`'s whole-registry iterations over `decl_index`, provenance and level 1 |
| REG-7.11 | by construction: a trait's members are read from its current declaration every revision |
| REG-8.1 | input writes: `FileSet`, `File.role`, and in W2 `file_decls` as an input (H3.6); collision policy is `declarations_named` plus candidates |
| REG-8.2 | superseded by the recorded reads (already so in the ledger) |
| REG-8.3 | `file_decls`; `DuplicateClassSites` and `DuplicateFunctionSites` become a query over `decl_index` grouping by exact spelling |
| REG-8.4 | `Provider`, which a database with no corpus cannot make; no `ReflectionBuilder` exists to be split off |
| REG-8.5 | the checker reads its file's declaration from `class_decl` and `file_decls`; `redeclared_methods` goes |
| REG-8.6 | H1.3 |
| REG-8.7 | the engine's closure gate: a random edit sequence applied through `apply` equals a database built fresh on the final file set, compared by `registry-dump` and by every `Provider` answer for every name; plus, during W2, the engine against the old passes (H3.6) |
| REG-9 | each staleness case is a test of the closure gate; the fix is the construction |
| REG-11 | behaviour fixes, each its own delta commit in the level or the declaration builder (`class_decl`) whose logic it changes, after step 3 of H3.6: 11.1 on PROD-5.1's vectors, 11.15 on the `Provider`'s folded keys, 11.16 on the entity-index queries (H3.7); 11.14 is superseded |
| REG-17.1 to 17.4 | the queries above the levels, H3.8 |
| REG-10 | a `TestProject` builder (inline files into a database with the corpus) replaces `Registry::build`'s 188 tests' setup; `Registry::build` and `Origin::Unknown` are deleted |
| REG-16.5 | salsa's event callback (`WillExecute`, `DidValidateMemoizedValue`, `WillBlockOn`; `event.rs`) counts executions and validations by query; the *asker* of a query is not in the event, so the provider records the first reader of each name itself (S-3) |

### H3.6 The order inside W2, and what `file_decls` is until W3

W2 puts the registry on the engine before the pipeline is on it, so in W2 `file_decls(File)` is an **input**: the
existing loader parses a file and builds its declarations as today, and `apply(FileDelta)` sets them (REG-8.1's
`FileDelta` is exactly that). W3 turns it into a query over `parse`. Everything above it, `decl_index`, the levels,
the provider, does not change between the two. During W2 the old checker driver still decides what to recheck
(`walk_affected` and the dependents indexes stay until REG-16.4), reading the registry through the `Provider`
outside any query. In W3 `check_file` becomes a tracked query and the recording starts.

Within W2, each step has the same shape (a "closure gate" per step), **run with the cadence of your rule: the tests,
the fixtures and the in-binary comparison per commit, and the `registry-dump` and O2 comparisons once, at W2's stage
checkpoint** (the dumps are produced where they are cheap and compared there):
1. **REG-4.2 and REG-4.12** on the old storage: exact-spelling accessors, folded keys on `FoldedName`, the
   `Provider` facade over the old maps. `registry-dump` before and after is equal except REG-4.2's approved delta.
2. **The engine module, shadowing.** Inputs, `decl_index`, `declarations_named`; the `Provider` still answers from
   the old maps and, in a debug build, compares each answer with the engine's (a listed exception to Part E 13's
   policy, deleted with step 3's last level).
3. **One level at a time** in the order 1, 2 (with REG-7.6 and its needs, REG-18.2 and 18.3), 3, 4, 5, then
   REG-7.12 and REG-7.10: the `Provider` flips that level's answers to the engine, the old pass stays as the debug
   oracle for one stage, the in-binary comparison runs over Piwigo, the vendor projects and the fixtures in the
   tests, and the old pass is deleted in the next commit. Deltas (VAL-2.9, 2.10) are read as they land.
4. **REG-8's close:** `apply` for every edit path, the closure gate, REG-9's tests, REG-10's builder.
5. **The data-model rows that wait for the levels.** REG-4.15 (a declaration built straight into its final form,
   with an ordinal where it had a span: H1.5a) is level 2's construction, so it lands with step 3's level 2; REG-4.11
   (provenance as the only location record) lands after REG-8.5 and REG-10 have removed `is_stub`'s and
   `Origin::Unknown`'s readers.
6. **REG-11's behaviour fixes,** one commit each after the structural ones, each a delta approved in the plan and
   fixtured first.
7. **REG-12's gate:** the gated steps' medians, and the test that checking one file resolves no class the file does
   not reach (a count from the event callback, not a guess).

### H3.7 What is not a level

**Global variables.** `infer_global_variables_of` (`registry/global_vars.rs:323`) is not a fixed point, though it
is run again per stale file. A file's sites are a function of its `GlobalFacts` and the registry: a `StaticValue`
names a call, a static call, a class constant or a method call, never another global, and
`GlobalFacts::resolve` turns it into a type through the registry (run per file, in parallel). The merge then applies
the sites of the files that `resolve_global_scope_files` accepts (an include graph matched by path suffix) to one
accumulator per name. Nothing in a site reads another global, so **there is no cycle before REG-17.1**; once
REG-17.1 makes an undeclared return the body's inference, a site `$x = f();` resolves through
`effective_return(f)`, whose body can read a global (`function get() { global $cache; return $cache; }` with
`$cache = get();`), and `global_sites(F)` → `effective_return(f)` → `inferred_return(f)` →
`global_variable(cache)` → `global_sites(F)` closes. So the global queries and `effective_return` and
`effective_property` belong to the inference family (H3.8): `Fixpoint`, `cycle_initial` = `never`, the same
widening. Before REG-17.1 they keep the default strategy, since there is no cycle to enter. On the engine:

| query | key | result |
|---|---|---|
| `file_products.globals` | `File` | the file's `GlobalFacts` (already serializable), made by the products pass; the include sites are a field of their own, since they depend on no type |
| `global_sites` | `File` | `FileSites`: the facts resolved through the `Provider`, so a changed return type or class constant reruns exactly the files whose sites call it |
| `global_scope_files` | none | the files whose top level runs in global scope, from the include sites and the `File` paths |
| `global_site_index` | none | each name to the files that have a site for it |
| `global_variable` | `GlobalKey` | one name's accumulator `finish`ed: the sites of the files the index lists that the scope set accepts |

The `Provider` exposes `global_variable(&Name)`, so a file that reads one global depends on that global's entry
alone. The `stale` sets, `global_sites` as registry state and the staleness bookkeeping go. The crate split's C9
moves the code out of reflection and J2's SUM-2 changes what counts as a site; neither changes these queries.
The shape above is a fixture of S-6, entered from each of its queries, on one thread and eight.

**Queries with no state of their own.** The doctrine entity index, `names_with_assert_tags`, the PHPStan ancestry
fact and `phpstan_class_files` are queries over `decl_index` and provenance (REG-18.6 makes the last a provenance
query). The Pest configuration and the SQL context are inputs to the check, as `pest_config` and `sql` are
fields of `Project` today.

### H3.8 What REG-17 adds above the levels

REG-17.1 to 17.4 put body inference under the types a caller sees, and a reflection that read inference would make
the levels cyclic (`class_final(C)` reads the inference of `C::m`, which reads `class_final(C)`). So:

- **The levels never read inference.** `class_final` is a function of declarations, as H3.1 has it. Above it sit the
  *effective* queries: `effective_return(DeclKey, member)` (the declared return, else REG-17.1's inferred one, and
  for a context declaration REG-17.3's widening of a refinement its own code refutes) and `effective_property(DeclKey,
  name)`, and `inferred_throws` (REG-17.2). The `Provider`'s method and property views take the signature from
  `class_member` and the return and property type from these. A file that calls `C::m()` therefore depends on
  `m`'s body only when `m`'s return is undeclared or refinable, and a declared one backdates at once.
- **Families.** Inference (`infer_body`, `inferred_return`, `inferred_throws`, `effective_return`,
  `effective_property`, the global queries of H3.7, and the receiver types of REG-17.4) is one family with
  `Fixpoint`; `infer_body`, `effective_return` and `effective_property` are rows of H2.3's table. Purity and
  property writes are another with `cycle_result`. REG-17.4 has a summary read a receiver's type from the body's
  inference, and the inference of a call read the callee's purity (it forgets what the callee may change).
  `purity(f)` → `infer_body(f)` → `purity(f)` would be a mixed cycle, and so would `purity(f)` →
  `infer_body(f)` → `purity(g)` → `infer_body(g)` → `purity(f)` for mutual recursion: salsa records
  dependencies at query grain, so reading `infer_body(f)` "for receiver types alone" depends on its whole
  execution, including its reads of callees' purity. **So the staging is a query, not a hope:**
  `receiver_types(DeclKey, member)` runs the inference in a no-effects mode and may not read `purity` or
  `property_writes`; `purity` and `property_writes` read it; `infer_body` reads `purity`, `property_writes`
  and `receiver_types`: a DAG by construction. Which receivers stage is stated, because soundness needs it: for
  `$this->a->b->m()` the receiver's type depends on what earlier calls may have forgotten
  (`forget_members_a_call_may_change`), so an effect-free typing could follow a call to the wrong body and claim
  purity; only receivers no call can change (locals not taken by reference, parameters, `$this` itself) stage,
  and every other construct keeps the conservative answer it has today (impure, writes anything). S-6 tests it:
  the same results whichever query of a cyclic fixture is entered first, and on one thread and eight.
- **What it costs.** `effective_*` are per member and asked per call site; the effective return of a member with a
  fully declared return is one lookup of `class_member`'s return and no inference.

## H4. The pipeline, persistence and the Session (Part F 15)

REG-16.4, CACHE-13.2 and the crate split's F1 and F2 are one design: F2's own design was superseded by
decision 3, and building F1 and then REG-16.4 would build the Session twice. What follows is that one design.

### H4.1 The batch run

1. `Session::resolve(config)`: the database (the corpus `Arc` as its constant, the pool), the inputs (cold: all
   created; warm: restored first, H4.6), the settings inputs, `Packages` from the Composer load (vendor discovery
   still runs beside the manifest load, LOAD-9). Until STUB-11 embeds it the corpus is built beside the walk and
   the products (STUB-3), before the first query.
2. The walk (LOAD-2, as built) numbers each path and makes its `File`; `FileSet` is set once. **The stamp is new
   work** (CACHE-13.1's, W3 step 5): the walk takes no `stat` today (`Listed` holds path, role, stub flag and
   owner), so the loader takes the stamp from the open descriptor, before the read, for the files it reads anyway;
   a lazily reached file has none (H2.2's `file_stamp`). Steps 1 and 2 overlap as they do now (LOAD-8.10, 65 ms warm): the analysed and
   stub walks start at once, vendor discovery runs beside them, the context listing starts when discovery ends, and
   `Packages` and `FileSet` are written when all of them are done; no query runs before that. LOAD-2.4 hands each
   listed directory's files to the pool while the walk goes on, and a write per directory would cancel the queries
   running, so that overlap is kept outside the queries:
   the pool runs the pure function `products_of(bytes, role)`, and its results wait in the load's cache keyed by
   (path, stamp): a query that had to know the digest to look an entry up would read and hash the bytes the pool
   already read. `file_products(file)` finds its entry there, or calls the function, so it stays a function of the
   stamp it read. Whether the overlap matters is S-3's to measure (LOAD-2.4's wall effect was never measured), by
   S-3's stated rule; otherwise the plain order, walk and then products, stands. Input creation is confined to the driver by a private constructor and an `xtask arch`
   rule (salsa does not enforce that a query may not create an input).
3. Load: `file_products(file)` for every file of the **eager set** (the analysed files, scan and stub paths, and
   Composer's `files` entries; functions and constants cannot be autoloaded), in parallel on the pool, each file
   read once and parsed once, its products made: an analysed file's tree is its `parse_analysed` memo, kept for the
   check, and a context file's is built in the worker's scratch builder and cleared (H2.9). Then `decl_index`.
   Context *classes* are not in the eager set:
   - Vendor discovery lists the autoload directories' files as it does today and each gets a `File` (a query may
     not create an input, H2.2, and a located path needs one), but none is read or parsed until a name asks for it.
     When no unguarded eager declaration answers a name, `declarations_named` asks `locate(ClassKey)` and reads the
     located file's products. A guarded eager declaration is a candidate beside the located file (decision 11).
   - `locate` is Composer's `ClassLoader::findFile` over `Packages` and `FileSet`: the classmap entry; else the
     longest PSR-4 prefix and the first of its directories that holds the file; else PSR-0 (STUB-7.5; decision 11's
     "an autoloaded class in the file Composer's loader maps it to"). The directory test is a lookup in `FileSet`,
     so no filesystem read. The index is keyed through the root's case rule (level 9's R7, probed once per root and
     exact on Linux), so a candidate spelled in another case than the disk's finds its file where the volume folds
     case; where the rule's fold is not the volume's the lookup asks the system as discovery does (R9) through
     the tracked `resolve(PathKey) -> Option<resolved spelling>` of H4.5, whose answer is then looked up in
     `FileSet`'s index (a boolean probe yields no `File`). A candidate spelled through a link below an
     autoload directory, which Composer's `file_exists` finds, is not an index entry (LOAD-1.1 resolves each link
     once and `FileSet` holds resolved paths, where eager loading lists those files): the remaining misses are
     decision 14's delta and S-11 tests them with a link below a PSR-4 directory. **Unicode normalisation:** APFS
     and HFS+ look names up insensitively to normalisation while listing the stored spelling, so R7's probe is
     extended with a normalisation test (an entry with a non-ASCII letter looked up in its other normalisation),
     `FileSet` and `shown` are keyed through the rule, and NFC and NFD names are tested on the macOS leg.
   - This is the crate split's "context classes are read on demand through the locators" and the larger win of the
     engine for a big `vendor/`: a vendor file nothing reaches is never read or parsed. **Decided: W3 builds it,**
     so the loader is not written eagerly first and rewritten at W4, and STUB-7.5 moves from W4 to W3.
   - Its prerequisite is not built: the generated-file reader keeps paths only (`GeneratedEntries` drops the
     classmap's class names and the PSR prefixes) and `Autoload` has no `psr-0`, so the maps first keep their keys
     and gain PSR-0. That addition to `composer` is A7, ahead of W3: it changes no output, and is tested against the
     generated files' own arrays on `php` (the lookup that reads them is `locate`, W3's, tested against
     `ClassLoader::findFile`).
   - The delta (a vendor class that no autoload mapping reaches becomes unknown) is decision 14's, and **if S-11
     fails for a package the case returns to you as a proposed change to decision 14; the eager fallback for
     that package ships only after that ruling.** A lazily reached context file is stat-ed by `file_stamp`
     (H2.2). S-11 measures the listing and the read set.
4. Check: the analysed files, largest first, in parallel on the pool: `file_diagnostics(file)` runs `check_file`;
   levels resolve on demand on whichever thread asks first.
5. The whole-program queries, then `analysis(db)`: the finalizer.
6. With persistence on, the database is serialized with no handle alive, and only if something changed (H4.6).

What this removes from `Session::load`: the serial `insert_numbered_file` merge (the declaration index replaces
it), `finish_loading`'s passes (levels on demand), `complete_skipped_references`, and the file table's per-file
bookkeeping that is now `File` fields. The serial stretches the fixes plan measured (the finishing passes,
1.16 to 1.52 s of the off run at 2 to 3 of 8 cores) become the `decl_index` pass plus on-demand work spread over
the pool: REG-7's wall-time prize.

### H4.2 The server

The initial load is the batch load. After it: `didOpen` and `didChange` set the overlay in the `FileSource` and
bump the file's `stamp` (a nonce, which is never a disk stamp); `didClose` removes the overlay and stats the file;
watched-file events stamp, create or drop `File`s and set `FileSet`, **one notification being one batch** (the
stamps, creations and removals are collected, `FileSet` and its indexes are rebuilt once, each input is written
once, and queries restart once; a test creates 30,000 files in one notification). A configuration change writes
the settings inputs. **`reconfigure` runs on the full trigger set** of the crate split's F1, each with the
inputs it writes: a changed `scan_paths`, `exclude_paths`, `stub_paths` or `no_composer` (roles and the eager set
change: `FileSet`, `Packages`); a change to `composer.json`, `composer.lock` or `vendor/composer/installed.json`
and a regenerated `vendor/composer/autoload_*.php` (discovery reads them: `Packages`; with `locate` lazy a stale
`Packages` gives wrong lookups); a change to the baseline or a file it includes (`OutputInputs`); and the
watcher registrations are extended to those files (the watcher registers `**/*.php` only today, so they are
unwatched), so "reload the window" is gone. Then H2.6: `file_diagnostics` in priority order, published as each
completes and only when it differs.

### H4.3 F1, reconciled

| F1 item | on the engine |
|---|---|
| `AnalysisResult` owns its data | `analysis(db)`'s value: file and not-file errors, internal errors (the records `check_file` catches at its own boundary: H2.8; a file holding one is not persisted), stats, warnings, the inline-ignore records (from `file_products.ignores`, so a persisted result finalizes without parsing its file); "N more file(s) loaded for context" counts the files the load listed, a function of the inputs |
| the finalizer | a function of `file_diagnostics`, the whole-program results and `OutputInputs`; stale baseline and unmatched-ignore entries are its output |
| entry points | `Session::run`, `update(FileDelta, &CancellationToken)`, `reconfigure(Config)`, `generate_baseline`, `apply_fixes`, `snapshot()` (a database handle for one request); `Project`'s ~15 entry points collapse into them |
| `Project` folds in | the file table is `File` and `FileSet` plus the VFS; the registry is `Provider` over the engine; `dependents` is gone; `use_sites` and the escape sets are `file_products.use_sites` and a project query; `pest_config` is a query; `sql` is an input |
| the VFS | contents from `FileSource` (overlay first); membership decided once, by `classify` (LOAD-1.2, built), for the walk, a file the editor opens and a watched-file event alike; identity is the canonical path (LOAD-1.1, built; one canonical form on every platform, level 9's R3); URIs are level 9's R5 (RFC 8089 on each platform, an unopened file named in the spelling its client gave the folder), lines and file events F1's text unchanged |
| panic isolation on the pool | H2.8 |
| cancellation and the event loop | **changed.** F1 has requests run "against the last published snapshot while an update builds the next". salsa has one current revision: a write cancels the other handles' queries. A request is answered from its handle, and a request cancelled by a write answers `ContentModified` (or is run again by the loop) — rust-analyzer's model. F1's "publish the edited file first, then the open documents, then the rest" is H2.6's order of queries |
| last good AST | **gone with recovery.** The engine needs `parse` to be infallible, so REG-16.4 needs SYN-1.9 (batch 3), which the sequence already has first |
| `reconfigure` | settings, `FileSet` and `Packages` writes (H4.2) |
| one Session per root | one `ProjectDb` per root, sharing the pool, the interner and the corpus `Arc` (each database's corpus constant is the same `Arc`) |
| files outside every folder, untitled buffers | a database rooted at the nearest config or `composer.json`, else for the file alone; an overlay-only file has a `File` with `overlay` and no stat |
| the workspace load runs as the CLI's | `Session::load` (H4.1) |
| LSP parity | the project-level rules are queries, so they run on every update by construction, their per-file data kept as products |

### H4.4 F2, reconciled

| F2 item | on the engine |
|---|---|
| resolved dependencies, absent names included, includes, trait hosts | the recorded reads: an absent name is a `declarations_named` read; an included file is a `file_products` read of it; a trait's host reads the trait's levels. `deps.rs`, `Project::dependents` and `signature_dependents`, `register_references`, `transitive_affected_files*` and `bare_name` are deleted, and with them the whole of PROD-5.4, PROD-5.7 and LOAD-4.7's file-references half (Part E 35) |
| fingerprints per exported name | early cutoff on `class_final`, `function_final` and the constants, and per member on `class_member` (H2.4): a "fingerprint" is the equality of the value a reader read |
| incremental collectors | per-file facts as `file_products` fields and queries, and the whole-program queries over them |
| the persistent cache per analysed file | the database's memos: `FileCheck` (raw errors before the finalizer), the products, the levels, the verdicts, with the dependency graph |
| the key | the file is named by (build identity, project root, config key). The config key hashes `ReflectionInputs` and `CheckInputs`; these are *also* inputs, so a change inside a session invalidates exactly its readers; `OutputInputs` are in neither. Alternating configurations keep their own files (F2's entry sets), the four most recently used kept |
| location, mode `0700`, `CACHEDIR.TAG`, atomic write under a lock, `--no-result-cache`, `clear-result-cache` | unchanged in what they ask, and built for every platform (level 9's R11): the directory is `dirs::cache_dir()` joined with `phpstan-rs` (`PHPSTAN_RS_CACHE_DIR` overrides), so `~/Library/Caches` and `%LOCALAPPDATA%` as well as `$XDG_CACHE_HOME`; mode `0700` is Unix's, and a Windows directory under `%LOCALAPPDATA%` is the user's by inheritance, with ownership and not-a-link checks before an existing directory is read or written; the write is a temporary file renamed over the target, as the two caches do today, which replaces an existing file on Windows too unless another process holds it open, so a reader takes no lock and sees the old file or the new; **the lock is a dedicated `.lock` file** (`File::lock`, portable; the data file is never the lock target, since Windows' lock is mandatory against reads by other handles), taken by the writer around the temporary write and the rename, and a lock failure runs the batch uncached with one warning; retention is the four most recently used config keys per project root (F2's entry sets) under a byte cap (`PHPSTAN_RS_RESULT_CACHE_MB`, default 2048 as the file cache's), last use refreshed on a read at most every 12 hours (`REFRESH_AFTER`); the per-file cache (`file_cache.rs`) and its switches keep serving until CACHE-13.2 replaces them and then go, with `--no-result-cache` taking their place; a result file made by one platform's build is never read by another's, since the build is part of its key, and whether `debug_assertions` belongs in the key is decided in the commit that writes it |
| the LSP writes back only results whose overlay matches the disk | a result whose reads include an overlay's stamp is dropped from the write (H4.6), since an overlay's stamp is never a disk stamp; the server serializes at shutdown or after an idle period, not "at any quiet moment" |

### H4.5 Probes outside the file set

`include_target_exists` (`checker/includes.rs:283`) calls `is_file()` on the live filesystem. A query may not,
and a query that reported an untracked read would run again every revision, which for the many files that
`include` something would cancel the point of a warm run (a minority: 242 of Piwigo's 2,798 non-vendor files name
`include` or `require` by a loose count). The probe is a query: `path_is_file(db, IncluderKey, PathKey)`, a
tracked function that stats the path, **keyed by the includer** and reading its stamp, over a singleton input
`ProbeEpoch` that the driver bumps when the filesystem may have changed: once when a persisted database is
restored, on every watched-file event of any extension (the watcher registers `**/*.php` only, so a created
include target of another extension, or outside the watched folders, would move no epoch: watchers are
registered for the directories probed), and on `didSave`. Keying by the includer means an edit to the includer
re-probes its own paths, as every recheck of an includer calls `is_file` afresh today (creating the target and
then editing the includer clears `include.fileNotFound`). Each probe re-stats when the epoch moves, and an
unchanged answer stops there. The probes' paths are interned keys, so a restored database knows which to ask
again. `resolve(PathKey) -> Option<resolved spelling>` is the same design for `locate`'s fallback (H4.1). S-10
counts the distinct probes on the corpora, requires zero stat calls without a bump and a stated ms per bump.

### H4.6 Persistence

- **Format:** salsa's `persistence` feature, with the heavy values `Lazy`, in one whole in-memory buffer (salsa
  reads each memo key as a borrowed `&str`, which needs it; the project's `postcard` is such a format) inside an
  envelope of magic, format version, length and checksum, as `file_cache.rs` has. A tracked function's memo is
  written by its result type's own serde impls (`persist` on a tracked function takes no functions of its own; on
  an input, an interned struct and a tracked struct it may, `salsa-macros/src/options.rs:318-333` with
  `tracked_fn.rs:66` against `input.rs:72`, `interned.rs:72`, `tracked_struct.rs:68`). So the levels' and the
  check's results are types whose `Serialize` writes the bytes `Lazy` already holds and whose `Deserialize` reads
  them back as bytes, decoded on first `get` (CACHE-12's `Lazy`, whose equality by bytes and file number is
  already what cutoff needs). Restore is then a copy plus the graph, not a decode of every value. S-4 decides
  whether this beats the cache's own container (CACHE-9 and CACHE-10): if restore is slower than today's
  per-file decode in wall time, the container carries the values and salsa carries only the graph.
- **Persisted: the whole chain below every persisted memo, down to the base inputs.** salsa does not flatten
  onto the persisted queries: it replaces a non-persisted dependency by the base-input leaves beneath it, and
  its nested walk descends through every edge and inserts only input fields and persisted collectable interned
  structs (H1.0 10). If `check_file` were persisted and `declarations_named`, `decl_index`, the levels,
  `class_member` and `path_is_file` were not, every restored memo would hold edges to every eager file's stamp,
  `FileSet` and `Packages` instead of to the values it read: cutoff would exist only in the session that built the
  memos, one changed stamp or one created file would rerun every `check_file` that read a name after a restore,
  and each memo's edge list would be the whole leaf set. So persisted are: **every input a persisted memo reads**
  (`File`, `FileSet`, the settings, `Packages`, `ProbeEpoch`, `Extensions`, `OutputInputs`, `SqlSchema`: salsa
  asserts at serialization that each such input is `persist` and panics otherwise, so an `xtask arch` rule says
  every salsa input is `persist`, and an engine test serializes a database after one `check_file` and one
  `analysis` with every singleton present); the interned keys; `file_digest`; `file_products`' fields;
  `decl_index`, `declarations_named` and `hierarchy_cycles`; the five levels and `class_member`;
  `function_final`; the constants; `path_is_file`, `file_stamp` and `sql_columns`; `check_file`, the verdicts,
  `file_diagnostics`; and the whole-program results, with `Lazy` values that do not duplicate shared `Arc`
  structure. **Not persisted:** the trees (`parse_analysed`; a context tree is no memo), the accessors' tables,
  and `Builtin`, which is a constant of the database (the process-wide corpus, H2.2) and so a persisted memo
  needs no input for it. The alternative, if the chain is too large (S-4 measures its bytes per memo and per
  edge), is to keep the dependency graph in our own file. A path is persisted as a Unix name's bytes
  (`OsStrExt::as_bytes` and `from_bytes`) or a Windows name's UTF-16 code units (`OsStrExt::encode_wide` and
  `OsStringExt::from_wide`), as one pair of functions added to `path_bytes` (which cuts components and has no
  such API today), not through serde's `PathBuf`, which refuses a name that is not UTF-8.
- **Restore: two paths, never mixed.** salsa deserializes into a database that has no input (a singleton that
  exists already panics, and input slots are re-allocated in the saved order under an assertion; a
  never-changing input cannot be set again), and `deserialize` changes the live database in place and is not
  atomic. *Cold:* `Session::resolve` creates every input. *Warm:* the file is read whole and its envelope
  verified before any byte reaches `deserialize`; `deserialize` runs under `catch_unwind` into a database made
  for the restore; then each singleton is fetched (`try_get`), those the file lacked are created, and the
  others are set only when they differ (an input's setter is a change whatever it is given). **On any error or
  panic that database is dropped, one warning is printed, the run is cold, and the file is replaced at the end;
  a database that saw a failed restore is never reused** (a truncated stream leaves salsa's ingredient `Vec`
  one element short, and an out-of-range index panics in `Vec::remove`; without this a corrupt file, one from a
  skewed build, or one written by another local user into a shared cache directory would fail every run the
  same way until someone cleared the cache). The driver then reconciles: each walked path finds its persisted
  `File` through the persisted path index, **keyed by the exact canonical spelling** (a case-only rename is a
  delete and a create; the index covers every `File`, dead ones included), and its stamp is set if the `stat`
  differs or the racily-clean rule (CACHE-13.1) distrusts it, and on Windows or a file system the rule does not
  know the file is read and hashed instead (decision 13); new paths get new `File`s; paths that are gone leave
  `FileSet`; the settings and `Packages` are compared; `ProbeEpoch` is bumped. A lazily reached context file's
  stamp is `file_stamp` (H2.2), so nothing is "the files a persisted memo names". Queries then verify, and run
  only where a read changed. Nothing the engine sorts is ordered by `FileId` (H2.3), so a restored database's
  numbering (old numbers kept, new paths given the next free one) cannot change a result.
- **Write only when something changed.** The database is serialized at the end of a batch run only if a query
  executed or an input changed since the restore (an event-callback counter), the report is printed before the
  write, and a no-change warm run rewrites nothing (the project's rule for its caches). `as_serialize` cancels
  other workers and waits for their handles, so the server serializes at shutdown or after an idle period,
  never while a request runs, and S-4 bounds serialize time. Memos derived from a dirty buffer are not written:
  an overlay's stamp is never a disk stamp, and a result whose reads include one is dropped from the write, not
  trusted (F2 writes back "the results of files whose overlay matches the disk; an overlay's results are never
  cached").
- **Compaction (Part E 38, ruled 2026-10-18):** interned keys and `File`s of things that no longer exist stay in
  a persisted database, and an immortal `DeclKey` also keeps the memos of a dead declaration. The rule is a size
  ratio: when the keys, dead `File`s included, outnumber the live declarations several times over, the database is
  rebuilt. *At the start of a run* that means the run starts cold and writes a fresh file. *In a server* it means an
  in-process rebuild, because a server that runs for days has no next start: a new database is loaded from the live
  file set at idle (cold, or restored from the last file written while the ratio was fine) and replaces the old one
  once it has caught up, and the old one is dropped with its last handle. S-4 picks the ratio and measures the
  rebuild from a series of edit-replay runs, and S-3 measures what an immortal key retains before it.
- **Sequence:** CACHE-13.1's stat stamps first (they feed `File.stamp`), then the persistence, with the cache
  closure gate: a sequence of edits applied through cached runs gives, at every step, a cold run's result; a
  warm run after no change equals the cold run and executes no `check_file` (counted from the event callback),
  **even when `ProbeEpoch` is bumped**; after a restore, one body edit and one file creation rerun exactly the
  `check_file` set the live session reruns; a panic-injected file is in the warm-run gate; and the restore is
  tested with a truncated file, a flipped byte behind a recomputed checksum, an out-of-range ingredient index
  and a file from another build key.

### H4.7 W3's order

1. The file source and `File` inputs are built (LOAD-1.4). `file_digest`, `parse_analysed` and `file_products` become
   queries, and `file_decls` flips from W2's input to a field (H3.6).
2. `check_file`, `file_diagnostics`, `property_type_verdicts`, the whole-program queries and `analysis`.
3. The server's update path moves to H2.6; `update_file*`, `walk_affected`, `closure_scope`, `dependents` go.
4. The batch driver (H4.1); `finish_loading` (94 calls, 116 mentions at `ffa7cccb`) and its callers become `Session`.
5. CACHE-13.1, then CACHE-13.2 and the cache closure gate.
6. TYPE-4 on the revision, REG-16.5, LOAD-3.1, LOAD-5.2, LOAD-5.1's use-site index, REG-17 and CHECK-7, CHECK-8.
7. At every step the edit replay (G5's tool 7) equals a fresh load.

### H4.8 Interim work the engine deletes

`deps::Collector`, `recorded_references`, `RefKey` and the dependents indexes exist only to decide which files to
recheck (`deps.rs` says so: "used purely to decide which files might need re-checking"). PROD-5.7 (references as
one vector per file), LOAD-4.7's file-references half and PROD-5.4's rewrite are work on them; REG-16.4 deletes the
machinery whole. PROD-5.8 (the record of written names) stays: its readers are rules, not the dependents index.
Part E 35.

## H5. What no reading settles: spikes S-1 to S-12

Each is a short build with a measurement and a stated accept and kill. None changes this plan's decisions;
each can send one design to its stated alternative. S-1 runs before batch 4, since it decides the name's layout.
S-2 and S-3 run early too, since the choice of engine is cited by everything from batch 4's rules (H1.5) to the
wall, and a skeleton is cheap against finding out at W2 that salsa is unfit. **Each spike has a gate point:** S-1,
S-2 and S-3 before batch 4; S-12 before item 9's step 2; S-4 right after S-3 and before W2's result types (H4.6 already states them); S-5 and
S-9 on the S-3 skeleton, before W2 step 2; S-6 before W2 step 3; S-8 with batch 4; S-7 before W3 step 1; S-10
before W3 step 2; S-11 before W3 step 4. **The measurement rules:** a wall-time accept is G5's wall-time rule
(`abab` pairs, stated pair count and p-value); "the phase start" is REG-12's gate baseline (`xtask gatetime
--since 58cfc5d2`, medians, peak RSS at or below it), and for a skeleton that replaces only part of the
registry it is the same binary built at the skeleton's parent commit; every wall or Ir comparison states its
commit and mode (off or warm), and every skeleton also measures a cold run (below). **One exchange rate between memory
and time serves every spike that trades them (S-7, S-12):** 50 MiB of peak RSS for 1% of wall time, or of the check phase's
Ir where Ir decides (proposed, stated once, so that no spike sets a rate of its own).

| # | question | method | accept | otherwise |
|---|---|---|---|---|
| S-1 | is `triomphe::ThinArc<Header, u8>` fit for `Name`? | a scratch crate: `from_header_and_slice`, pointer identity (`as_ptr()`, there is no `ptr_eq`), the count and uniqueness (`strong_count(..) == 1`), `Send + Sync`, no `Weak`; **how a `&str` is read back without `unsafe` of ours** (validation on every `as_str`, `lower` and `Deref`, or a crate's thin header-plus-`str` type such as `slice-dst`'s `StrWithHeader` behind `erasable::Thin`, judged on merit) and the corpus total of those reads; intern and lookup against `Interner::name` (260.8 M Ir over 1.01 M calls) and `name_hashed` (354.8 M over 1.44 M); allocation count; **retained bytes of count-one names at the peak, the sweep's time per pass, and the share of `FoldedName` comparisons that take the byte-compare path** on Piwigo, the vendor projects and the fixtures; the hash function: one defined over the text's bytes alone, so NAME-1.5 can hash a name's parts as one text and compare a candidate part by part (a hash whose result depends on how the bytes are chunked cannot); where none is fast enough, NAME-1.5 joins the parts in a reused scratch buffer, which allocates as little | the API gives identity and a count; Ir per intern not above today's; **the corpus total of `as_str`, `lower` and `Deref` Ir not above today's loads**; allocations per name 1 | H1.3's fallback: one `Arc<NameData>` with a single `Box<str>` holding the text and the lowercase form (two allocations, the weak table and its sweep kept) |
| S-2 | do salsa's macros build under `unsafe_code = "deny"`, and does the engine build and run on every leg? | **first, build the skeleton under the workspace's `deny` with no `allow`** (a probe on the pinned toolchain suggests an external crate's macro expansions are not reported; only a failure grants the engine module `#![allow(unsafe_code)]`, Part E 33); one of each salsa item; clippy with the workspace lints; an `xtask arch` check that the module's own text has no `unsafe` token; the skeleton's tests run on the macOS, Windows and aarch64 Linux legs of `rust-ci.yml` **and as a `cargo test --release` of the skeleton (an integration test binary built with the release profile) on each leg that constructs a database and runs one query**, because salsa's default `inventory` feature registers its ingredients at link time (with `thin` LTO and `codegen-units = 1` in the release profile, which no step of `rust-ci.yml` links), which is where the platforms differ; `cargo deny check` | it builds under the unchanged lint, or with the one exception and the `xtask` check, and its tests pass on every leg including the release one | the pinned feature set (H2.1) without `inventory`, where one leg fails: every ingredient is then registered by hand in `Storage::builder()` as salsa's `manual_registration` test does (a forgotten one fails at run time); else an engine of our own with H2.1's contract |
| S-3 | cost of the shape | a skeleton on Piwigo plus two vendor projects (the load with and without LOAD-2.4's overlap of walk and products): `file_products`, `decl_index`, `declarations_named`, the level queries over the old declarations, a `Provider` with the member grain of H2.4; it counts the distinct (file, class, member) reads, which REG-16.1 did not, and the interned-key edges; **the memos an immortal `DeclKey` retains** (the bytes of a dead declaration's levels after an edit-replay series of renames and deletions, and the key table's growth); **the distinct ranges and the extra level memos of range-keyed levels** on `vendor/symfony`, `vendor/laravel` and `vendor/rector-src`, whose manifests differ (J1.2); **it measures a cold run** (load plus check, no persistence: cold Ir and wall, the median of repeated runs on Piwigo and the vendor projects), since a batch run pays recording, interning and memo creation for every edge with no incremental benefit and W3 cannot undo a slower cold run; clean and incremental build time and the release binary size | a name read ≤ 80 Ir per lookup (today's 73 to 80); warm verification of all checks ≤ 20 ms (REG-16.1: 5 to 10 ms for 107,068 edges, so 47 to 93 ns per edge, which bounds the member grain at about 220 thousand edges); the member first reads' Ir ≤ 3% of the check phase's (at REG-16.1's ≈1,000 Ir per query that is the same bound; 1% would fail on names alone); the edges' bytes (count × 12 B, salsa's `QueryEdge`, plus memo overhead measured) ≤ 1% of the phase-start peak RSS; `decl_index` and its readers ≤ 50 ms per edit that adds, removes or renames a declaration; a signature edit to one method reruns only the files that read it (a count from the event callback); peak RSS ≤ the phase start; `heap_size` per class level recorded; the retained bytes of dead declarations after the replay series are at most 1% of the phase-start peak RSS (proposed), or H4.6's compaction brings them back under it; **cold-run Ir and wall time not above today's by a stated margin (proposed: 3% wall at p < 0.05 over 20 `abab` pairs)**; the walk/products overlap is kept only if it saves at least 20 ms of the warm load at p < 0.05 over 20 pairs | per-name slots for the index; `class_final` for members (H2.4's fallback); a collectable `DeclKey` with decision 3's sentence amended (Part E 38); an engine of our own |
| S-4 | persistence | on the S-3 database, with the format named (`postcard`, one whole buffer, behind the envelope of H4.6): serialize and restore with `Lazy` values and with the cache's container, both with the chain below every persisted memo (H4.6) and with the graph in our own file; growth over an edit-replay series; the baseline re-measured at S-4's commit | restore **wall time** (and Ir) against the per-file decode's wall share, which runs in parallel on the pool where salsa's `deserialize` is one sequential walk under `&mut` (Ir parity alone could still be a serial wall regression at the head of every warm run); verify-after-restore ms for no change, for one body edit and for one added file; serialize ms; bytes per memo and per edge; the `check_file` executions after a restore plus an edit equal the live session's; the replay-growth series bounded (the stated ratio of H4.6's compaction) | the container for values, salsa for the graph only; or the graph in our own file |
| S-5 | cancellation | a write while a long check runs; a panic inside a shared level with N readers; `$/cancelRequest` during a long inference; a 50 MB array-literal statement and a stalled read; the stack-guard case of H2.8 repeated with a `Cancelled` unwind; a 100,000-deep hierarchy and constant chain on a 256 KiB thread | unwinding within 50 ms of the write at the polls of H2.5; one internal error per distinct cause per reader, none lost, no hang; a cancelled request completes and its answer is dropped by the server; the deep chains do not overflow | more polls; the retry policy changed |
| S-6 | cycles | `hierarchy_cycles` on Piwigo and vendor; a shuffled-order property test; `cycle_result` constants and purity, with a fallback that calls the value-only query (H3.3); the capped fixpoint of H3.3 with the nested-wrapper fixture (`return $n ? [f($n - 1)] : 1;`), a mutual-recursion fixture entered from each member and a `$n + 1` fixture whose iteration count it records; the cyclic fixture of inference and purity (H3.8) and the global-variable shape of H3.7, each entered from every one of its queries, on one thread and eight | the pass ≤ 30 ms; the same edges in any declaration order; the three rules' results equal the old passes'; the same result from every entry point; no run reaches the safety net just under salsa's 200-iteration panic, and the `$n + 1` count is at most about 66 | the explicit per-constant DFS; a lower per-kind literal threshold in the cycle operator (H3.3) |
| S-7 | tree retention and frees | the batch run's load and check with analysed trees kept for the run, against the same trees bounded by `lru` and evicted by one thread between phases (`trigger_lru_eviction`), against freed by the checking worker; context trees in the scratch builder | peak RSS ≤ the phase start; Ir of the frees (mimalloc's cross-thread paths included) and wall time not above today's; a bounded variant is kept only if it lowers peak RSS by at least 50 MiB for each 1% of added wall time (the exchange rate stated in this section's measurement rules) | LOAD-5.2 is built as the ledger has it (W3) and S-7 chooses between freeing on the checking worker and a bounded eviction; or LOAD-5.2 is dropped explicitly in Part D (the trees kept for the run, as memory only); a check scheduled on the worker that parsed its file where the queue allows |
| S-8 | exactness | `Exact`, a result type per class of value, the arch rule; the count of `==` and derived-`PartialEq` sites on `Type` that rely on its semantic equality (H1.5b's stronger form) | a field change makes every result type unequal, once for a tracked field and once for a function result across a case-only edit; a semantic-equal union in another order is unequal | a manual `PartialEq` per result |
| S-9 | threads | the S-3 skeleton's check phase at 1, 2, 4 and 8 threads against today's | the speedup curve not below today's; memo contention under 5% of CPU; cold-run wall not above today's (S-3) | `lru`/sharding of hot queries; a read-through table per worker |
| S-10 | probes | `path_is_file` over `ProbeEpoch` on a file with 200 includes, warm, and the distinct probes counted on the corpora | warm `check_file` of an unchanged file verifies with zero stat calls (`strace -f -c`) without an epoch bump, and at most 5 ms for the 200 per bump | the probe's keys batched per directory |
| S-11 | lazy context loading | the S-3 skeleton with `locate` (H4.1) over `vendor/symfony`, `vendor/monolog` and a package with PSR-0 and one with a classmap, and a link below a PSR-4 directory: list, then read on demand; the files read against today's (all of them); `locate` against Composer's `ClassLoader::findFile` on `php` for every class the old loader found | the files read are those the analysed files reach through `locate`, transitively; the listing costs no more than today's discovery; `locate` agrees with `findFile`; each class the old loader found and no mapping reaches (a link included) is listed as the delta | the case returns to you as a proposed change to decision 14, and the listed files of that package are read eagerly only after that ruling, or the listing is made lazy by directory |
| S-12 | spans or token indices (decision 4's last sentence; Part E 3) | a scratch build over the corpus (Piwigo, two vendor projects, the stubs): the nodes of I1 once with `Span` fields and once with token indices, the token arrays a kept tree must then keep (kind plus start offset, about 5 B a token), the exact bytes of a kept tree under each, using the node and token counts of step 0's census, and the readers' hot paths under each (`span()`, an identifier's name, a string's bytes, the infer cache's key) as callgrind Ir over the checker's reads of a tree; the layout of I1 is otherwise held fixed | token indices are chosen only if the bytes they save in the kept trees (net of the token arrays they must keep) reach the exchange rate of this section against the readers' added Ir in the check phase (50 MiB for each 1%, proposed): on trees of the plan's estimated 35 to 60 MB that needs a reader cost near zero, which is why byte spans are the expectation; otherwise byte spans, which B3 to B5 and I1 assume | spans, the measurement recorded as the answer to decision 4's last sentence; tokens, a change confined to B3's `span` fields, the retained token arrays of I2 and `Rooted` |

If S-2 or S-3 fails, the plan's contract (H2.1) is implemented by an engine of our own: memo tables with revision
stamps and dependency lists, early cutoff, a cancellation flag and cycle handling. Nothing above the engine
module changes, which is why only that module names salsa.

## H6. What the engine cluster changes elsewhere

- **Part B and G.** G1's rule on ids gains a home: engine ids are named in the engine module only (H1.6), beside
  `syntax`, the infer cache and the `ProbeScope` module. G4's rooted `ProbeScope` takes its `Arc<Ast>` from
  the `Arc<Parsed>` of `parse_analysed` (or an unmemoized parse for a context file), which the probe holds for its own life. B7 step 2's scratch builder is H4.1's per-worker
  scratch.
- **Part C.** Batch 4 is H1.7, with S-1 and S-2 before it. Batch 5 loses PROD-5.7 and LOAD-4.7's
  file-references half (Part E 35); its PROD-5.1 keeps anonymous classes out of the declaration names and lays out the site table
  that the declarations no longer hold (Part E 34). CACHE-13.1 is the source of `File.stamp` and is built at W3 (Part E 15); CACHE-8
  waits for S-4 and CHECK-4.2 goes with REG-18.4 (Part E 41), and REG-14.5 is folded into REG-7.5 (A4).
- **Part D.** W2 is H3.6; W3 is H4.7, with SYN-1.9 added to REG-16.4's needs; W3's open question ("which parts of
  REG-8 it builds on") is answered by H3.5; STUB-7.5, the lazy Composer locator, moves from W4 to W3 (H4.1).
- **Part E.** Rows 33 to 42.
- **Part F.** Rows 10, 13, 14 and 15 are designed here. Row 16 (C5's data model) now has its fixed points:
  the corpus as a constant of the database, `locate`, the version range per owning manifest (`range_of`, H2.2).
- **The ledger.** REG-8.6 is re-specified (H1.3). REG-4.15 loses its span field and REG-4.11 becomes a record over
  (`Origin`, site) (H1.5a). REG-16.2's and decision 3's "never reused" stand as ruled for `DeclKey` (Part E 38,
  ruled 2026-10-18), and decision 3's member sentence is refined to PHP's namespaces (H1.4). REG-16.4 needs SYN-1.9. REG-16.5 is the
  event callback plus the provider's trace of first readers (H3.5). PROD-5.4, PROD-5.7 and LOAD-4.7's
  file-references half are marked "deleted by REG-16.4" (H4.8). CACHE-13.2 is H4.6, keyed without the version
  range (Part E 37), and CACHE-9 to CACHE-11's form follows S-4. **CACHE-13.1 and decision 13** still describe a
  per-project index file and "the index's own write time": they become `File.stamp` and the stat-time rule of
  Part E 15. STUB-7.5 moves to W3 and needs the Composer maps to keep their keys and gain PSR-0 (H4.1, A7).
  PROD-5.1 and SYN-1.10 keep anonymous classes out of the declaration names and the global index (H1.4).
- **Other documents.** `CROSS_PLATFORM_PLAN.md` R11 (the caches; built at `a94e27ec`: each platform's cache
  directory through `dirs`, and an entry's last use refreshed through a write handle) is the pattern the database file
  follows: its directory, lock, last-use refresh and cross-platform behaviour are H4.4's; `CRASH_REPORTING_PLAN.md` and F2's move of
  `crash_report::cache_dir()` into `project`; `UNSAFE_PLAN.md` (Part E 33: its end state changes only if S-2 finds
  the exception necessary); and `file_cache.rs` with its switches (H4.4).
- **`CRATE_SPLIT_PLAN.md`,** edited at approval, in the docs commit series of Part E 20: C4's "five levels, each a
  per-class `OnceLock`", "constant reference cycles type as `mixed`" and "a change to any reflection input resets
  every memo" become H3.1, decision 1 and the recorded reads; F1's published snapshots become H4.3's cancellation;
  F2's resolved dependencies, fingerprints and entry sets become H4.4; the crate count and Appendix A's module
  assignments gain the engine and `php-version` (J3).
- **Not decided here, and why.** Only the container against salsa's serialization for values (S-4), which a
  restore's speed settles and a reading cannot. The member grain (H2.4), the settings fields (H2.2) and the lazy
  loading of context classes (H4.1) are decided; their spikes (S-3, S-11) can send each to a stated fallback.

---


# Part J. The wall's later designs (Part F 16, 17, 18)

W4 to W7 are far from batch 4, and the code they build on will have changed by then, so this part settles what can
be settled now: the data model that must be fixed first (J1), the rows that make C6 to C9 tracked work (J2), and how
Stages D to G meet the ledger and the engine (J3). It does not re-design Stage D's semantics, which the crate split
plan states at length and which stay as written.

## J1. C5's data model on the engine (Part F 16)

Fixed already: decision 6 (gates as data, one parse per file, queries take a range), 7 (embed the builtin knowledge,
read PHPStan's classes from the project), 10 (what php-src verifies bounds a type; other sources are claims inside
the bound), 11 (a name binds to the declarations PHP may bind it to), 16 (failure values), 18 (7.0 onward). Missing,
and set here: the representation, where it sits on the engine, the generator's format and the order.

### J1.1 Representation

```rust
pub struct VersionSet(u32);      // bit i is KNOWN_PHP_VERSIONS[i], append-only; a PhpVersions range is its contiguous case (batch 3, the php-version crate: Part E 32)
pub enum Availability { Always, Never, InPart }                  // of a VersionSet within a range (batch 3 as well)
pub struct Readings<T>(SmallVec<[(VersionSet, T); 1]>);          // a value by version set; disjoint; one entry nearly always (STUB-7.1)
```

**A range keeps its exact bounds, and its upper end is open when the constraint's is** (Part E 40, COMP-2; this reads
decision 18's "to the newest version" as "and later" for the default range, ruled on 2026-10-18, M5d).
`PhpVersions` carries the range as version ids beside its minor hull: `lo` with its patch (`>=8.4.1` is 80401) and `hi`,
absent when the constraint has no upper bound (`>=8.4.1`, which 282 manifests in the corpora say) or none is declared, and
else the constraint's own (`^8.1` ends at 89999, `>=8.0.2, <8.0.15` is 80002 to 80014), which COMP-2's interval
arithmetic produces directly. The hull, the known minors the interval touches, stays what builtin readings and feature
checks read. Declaration guards read the ids: `GuardExpr::on` already works on `[min.id(), max.id() + 99]` and takes `lo`
and `hi` instead, so `PHP_VERSION_ID >= 80600` is in part for `>=8.4.1` and never only where `hi` excludes it, `< 90000` is
always for `^8.1` and in part for `>=8.1`, and `>= 80401` is always for `>=8.4.1`. A flag alone would lose what today's
hull gives, by accident, for `^8.1` (`< 90000` is always true there now, and for `>=8.1`, where it is wrong). The corpora
already hold such guards: Symfony's `polyfill-php86` (`bootstrap.php`, `SortDirection.php`) and Guzzle's `StreamHandler`
test `>= 80600`, and var-dumper's `SplCaster` and Composer's `PlatformRepository` test `< 90000`. The type of
`PHP_VERSION_ID` can be `int<lo, hi>`, where a range that spans several minors gets no range type for it now (the
consensus of its two ends differs). A builtin added in a later version is still unknown. `VersionSet` itself stays closed.

`Readings::over(range)` yields the entries whose set meets the range. A return type is the union of the readings over
the range; an argument is checked against each (decision 6). **The form is canonical:** all entries with equal
values are merged by union of their sets and the entries are ordered by lowest bit (a bit set has no "adjacent"),
so two `Readings` of equal meaning compare equal, and a `Readings` of types compares exactly (H1.5b). A builtin
declaration is
`{ avail: VersionSet, extension, signature: Readings<Signature>, claims, throws, purity, failure: Readings<Option<Failure>> }`;
a class adds `parents: Readings<..>` and members that carry their own `avail` and readings. Nothing in a record holds
a position (H1.5a); its provenance is `Origin::Builtin { file, extension }` and a member ordinal, which a stub file's
site table resolves to a place for go-to-definition into a stub.

### J1.2 On the engine

- The embedded records are **a constant of the database, not an input** (H2.2): an `Arc<BuiltinKnowledge>` with
  per-file `Lazy` records, probed by folded hash as `NameTable`'s `Base` is today, whose identity is covered by the
  build identity in a persisted file's key. `DeclFile::Builtin(index)` names the file in H1.4's `DeclRef`. Until
  STUB-11 embeds them the same constant is the corpus the run builds from the stub sources (STUB-7.6's layer with
  its per-file parse memo): lazily filled, and a pure function of the sources, so it is a cache and not state.
- **The range a file reads is its owner's** (`range_of`, H2.2; Part E 42, ruled 2026-10-18): `effective_range` is a
  query per owning manifest, the configured range, else the owner's `config.platform.php` or `require.php`, else
  7.0 to the newest version narrowed to the versions all the analysed files of that owner compile on (STUB-7.1),
  which reads their `facts.validity`. Only in that last case does an analysed file's mark reach the range; a
  configured range is `ReflectionInputs.range` as it stands and overrides every owner's. **Both exceptions of
  decisions 15 and 18 are specified.** A file loaded only through guarded includes (every load of it is an include
  that a guard covers, and no autoloader maps it) does not narrow the range and is not reported; that is not a
  property of the file, since it depends on the include graph and on guards evaluated over the very range being
  computed (`bootstrap.php`'s version check covering `bootstrap80.php`). So `effective_range` reads each analysed
  file's validity, its include sites and the guards covering them, computes the range with every guarded-include-only
  file left out, and then checks the guards against the result; a library whose analysed tree holds a PHP 8-only
  polyfill file therefore keeps its range, and G3.5 reports no construct in that file. When the files left share
  no version, the whole range is read and the conflict is reported once, as the empty-intersection case.
- STUB-13.1's phar locator is a locator in `declarations_named` beside Composer's (STUB-7.4: no locator's place
  makes a declaration win). A phar entry is a `File` whose bytes are a range of the phar's, read by
  `file_products` in its byte-reading path (a phar entry is a context file; `file_digest` is for analysed files),
  so the rest of the pipeline sees a file.
- `declarations_named` applies decision 11's binding: a builtin name is a candidate where its `Availability` over the
  reader's range is not `Never`; an unguarded project redeclaration of it is a candidate only where the builtin is not
  available (`InPart` keeps both); guarded polyfills, locator results and eager files are candidates by their own
  rules; no order picks a winner.
- A builtin declaration's levels are the same tracked functions as a project class's, keyed by its `DeclKey`; they
  read the declaration's `Availability` and the readings that meet the reader's range from the (never-changing)
  record. A level whose inputs differ across versions is also keyed by the range, normalized to the versions at
  which anything it reads changes (an interned `RangeKey`), so a class with no version-sensitive ancestor has one
  memo whatever the owner's range, which is the common case, and a project with one range pays nothing. A manifest
  edit that changes an owner's range reruns only what that owner's files read under the old range.
- The version range is an input (`Packages` per owning manifest, `ReflectionInputs` for the configured one) and
  not a copy of the stub layer per range (decision 6), and STUB-7.6's "one layer serves every range" is the
  database itself. S-3 counts the distinct ranges and the extra level memos on the multi-manifest corpora
  (Symfony, Laravel, rector-src), and the cost of a project with several ranges is stated there.

### J1.3 How the data is built (STUB-11 and STUB-12)

The build script of the data crate runs the code the runtime runs (`reflection`'s record builder) over the sources,
in decision 10's order: php-src's stubs from 8.0 (STUB-12.1: `phpstan/php-8-stubs` is a new vendored corpus, since
nothing in `vendor/` carries it today, and the native signature base from 8.0; `registry/stubs.rs`'s module doc,
which says php-src's stubs "turned out to be the wrong target", is corrected with it); real-binary dumps for 7.0 to
7.4 (STUB-12.2); phpstorm-stubs for the PECL extensions, its attributes read as data (STUB-12.3). Native readings
and the PHPDoc return type at func-info precision form the bound. PHPStan's function map and its deltas, the
`.stub` files and phpstorm's PHPDoc are claims intersected inside it; `@throws` and purity are united; templates
come from the `.stub` file; the function map's two `bleedingEdge` files have their "new" sections applied
unconditionally (never kept looser behind a flag: ROADMAP M4). **What the record holds:** the bound, the
build-time claims, and the combination of them as a cache. A project-dependent overlay (`stub_paths`, a
`StubFilesExtension`, the configured assertion tables, an extension's defaults) is combined at run time by the same
function over the bound and all the claims, with an extension's own inputs (ExtDs's version, which
`ExtDsStubFilesExtension` reads, is known at run time only) an input of that combination; so a run combines
nothing of the built-in sources, and REG-7.12 and STUB-11 agree on where each combination happens: at build time
for the built-in sources, at run time for the project-dependent ones. STUB-12.5's disagreement report is a
build-time check (`xtask stubs-check`), not run-time work. The test that the embedded records equal a runtime
build runs on each target; `**` is folded by a pure-Rust `pow`.

### J1.4 The generator (STUB-12.2)

`xtask stubs-dump` runs a PHP image of each version from 7.0 to the newest (the 8.x runs cross-check the php-src
stubs against a binary) by digest, with a script kept under `tools/php-stub-dump/`. **The image is built per version
with every in-tree extension enabled** (a Dockerfile under `tools/php-stub-dump/`, base image still by digest): a
stock official image builds only its default extensions, and `mysqli`, `intl`, `gd`, `bcmath`, `sockets`, `soap`,
`pcntl`, `gmp`, `zip` and the like need `docker-php-ext-install`; the dump's header lists which extensions it
contains. The script lists the internal
functions, classes, interfaces, traits and constants through Reflection: for each parameter its name, type (named or
union), by-reference, variadic, optional and default (the constant's name when the default is one); the return type;
the flags; for a class its parents, members with visibility, `static`, `final` and `abstract`, constants and
their types; and the extension. **What Reflection reports before 8.0 is unverified here** (no older PHP is installed,
and internal arginfo before 8.0 is believed to carry few parameter or return types and no default values), so a
spike precedes STUB-12.2: run the script on the 7.4 image and count the positions that have a reported type, a
return type and a default. Where Reflection has none, the bound for 7.0 to 7.4 is the probe's accepted and refused
argument kinds, or a claims-only declaration that STUB-12.5 reports as unverified, and `type` and `default` leave
the pre-8.0 format if they are not there. It then probes the failure value (decision 16) by the survey's procedure:
every function whose required parameters are by value is called with an object for each, outside `strict_types`,
and what it returns or throws is recorded (503 `null`, 186 `false`, 4 continuing, 100 throwing, at 7.4). **A
function with a required by-reference parameter is probed with a variable passed for it** (php 8.5.4 has 74 of
them among 1,934 internal functions: `openssl_x509_export`, `openssl_pkcs12_read` and the like, which "by value
only" would skip). The output is JSON text per version, sorted, under `crates/stub-sources/data/` and checked in
(text is source; "checked-in binary data is out", STUB-10.2), with the image digest, `PHP_VERSION` and the extension
list in its header. CI does not run it (it needs docker); `xtask stubs-dump --check` compares a fresh dump with the
committed one when docker is there (the comparison matrix has no such check, only regeneration commands per
image tag, `php:8.0-cli` and the like). The cross-check against the php-src stubs runs on every version from 8.0 to
the newest, not only 8.0.

### J1.5 CHECK-10, the engine read over the range

The checker already follows one precedent: `NextArrayIndex` and `PhpFeature::supports` return a `TrinaryLogic` when
the range straddles a version that changes the behaviour. CHECK-10 generalizes it into a table of version-sensitive
behaviours (a loose comparison of a number and a non-numeric string, a division by zero, arithmetic on a
non-numeric string), each with its thresholds and its semantics. **Each behaviour is a `Readings<Semantics>`,** the
same type as the stub data (J1.1), not a fixed pair: one family has more than two across a range (php 8.5.4:
`"abc" * 1` throws `TypeError` while `"5 apples" * 2` gives 10 with a warning, and 7.x behaves differently
again). `behaviour.at(range)` yields the semantics that meet the range; with more than one, the operation is
computed under each and the types are united, with "possible in part" for what a report says. The table is
data, tested against committed per-version output of a matrix regenerated by hand (`tools/php-comparison-matrix`),
and by the digest-pinned `xtask stubs-dump --check` when docker is there.

### J1.6 Order (W4, with the engine in place)

STUB-7.1 (the data model and one parse per file, with `Readings`; `VersionSet` and `Availability` are batch 3's, Part
E 32), 7.3 (the attributes as data: the stub-attribute pass and `resolve_names` go), 7.2 and 7.4 (the locators;
7.5, `locate`, is W3's: H4.1), 7.6 (the stub layer as the database's corpus constant), 7.7 and 7.8 (provenance and
enumeration queries), 7.9 and 7.10 (range queries), 7.12 (the gate), STUB-8 (guards), STUB-12.1 to 12.5 (the sources
and the check, behind the pre-8.0 spike of J1.4), CHECK-10's version table (its operator sites are D3's), STUB-13.1
(13.2 is W6's). STUB-11 (W6) embeds what STUB-12 builds.

## J2. C6 to C9 as tracked work (Part F 17)

REG-15.1 "needs C6 to C9 (not tracked here)". These are the rows to write into the ledger, with their needs, cut
against decisions 1 to 18 and the engine. Nothing here changes the plan's behaviour lists; they become each row's
body.

| row | what (the plan's C-phase) | needs |
|---|---|---|
| OVL-1 (C6) | the `.stub` PHPDoc overlays (30 files of `conf/config.neon`, 11 more by the six gated `StubFilesExtension`s: SocketSelect 2, ReflectionClass 4, ReflectionEnum 2, BcMath, JsonValidate and ExtDs 1 each, so 41 files of `stubs/`) as claims inside decision 10's bound; all the functions (143 by a tokenizer count at `ffa7cccb`; the crate split's 146 is stale, recount when the row is written) and all method PHPDoc apply, and reach an implementation's untyped parameters by PHPDoc inheritance (level 3) | STUB-12.1, STUB-7.1, REG-7.3 |
| OVL-2 | declarative patch entries replace the Rust patch functions of `registry/stubs.rs`; data files in `stub-sources`, read at run time by the stub layer (STUB-7.6), and by STUB-11's build script later (the same files) | STUB-7.6 |
| OVL-3 | the function map and its seven deltas by version: the `SignatureMapParser` port is STUB-12.4's (one row, with the parser replacing `patch_functionmap_array_shapes`); OVL-3 keeps the fixtures and the settle-by-decision-10 list: its signatures against phpstorm's PHPDoc settle by decision 10 (the plan's list of oracle misses, the curl handles, `printf`'s `Stringable`, `round()`'s `RoundingMode`, and the about 60 narrower parameters the function map and the overlay stubs give (`array_chunk()`'s `positive-int` length, `explode()` with `''`, `chr(256)`, `flock()`'s `int-mask`, the `Reflection*` constructors' `class-string`…), becomes fixtures). The native signature base is `phpstan/php-8-stubs` (8.0 and later) with the dumps below (J1.3), the two `bleedingEdge` files have their "new" sections applied unconditionally (ROADMAP M4), and `registry/stubs.rs`'s module doc is corrected | STUB-12.4 |
| OVL-4 | a function with several variants (a phpstorm overload, a function-map `'1` form) keeps them all; a call takes the variant its arguments fit, named ones included (`selectFromArgs`); a call that fits none is `arguments.count`. A new dimension beside version readings | REG-7.12, OVL-3 |
| OVL-5 | `resources/functionMetadata.php` as the builtin purity source | STUB-12 |
| OVL-6 | user `stub_paths` with PHPStan `stubFiles` semantics: a stub's PHPDoc refines the declaration's and never relocates it (decision 11's overlay); omitted members are kept; a stub declares a symbol only when nothing else does. Combined at run time with the bound and the built-in claims by the same function the build uses (J1.3) | STUB-7.4 |
| OVL-7 | `StubFilesExtension` (its files, and ExtDs's version, are a run-time input of the combination) | OVL-6 |
| OVL-8 | the assertion tables compiled to overlays; `apply_assertion_table` and level 2's interim assertion step deleted; the Mockery and Nette Tester defaults to their extension modules | OVL-6 |
| RES-1 (C7) | the `TypeNodeResolver`: one name scope (namespace and imports, active templates, aliases, `self`) for the registry and the checker, as an implementation of the scope `phpdoc::lower` is generic over (I4.1); an enclosing function's templates inside an anonymous class. ("PHPDoc keyword types resolve before namespace qualification" is done in I4.1's extraction; only `number`, `scalar` and `numeric` yielding to a class or alias of that name is here) | I4, PROD-5.2, DOC-2.2 |
| RES-2 | native `TypeHint` nodes resolved directly, not printed and re-parsed (about 85 sites) | RES-1 |
| RES-3 | PHPDoc array types united with iterable types, a template name read as a template wherever a type is expected (shape keys included), a generic with fewer arguments than templates filled as `TypeNodeResolver` does, defaults taken | RES-1 |
| RES-4 | a type alias defined through itself is `typeAlias.circular`: the engine's `cycle_result` on the alias query, per alias | RES-1 |
| RES-5 | a constant name in a PHPDoc type resolves through `use const` and its initializer | RES-1, REG-18.3 |
| RES-6 | the resolver's tests, one for each F40, F45, F46, F47, F51, F61 and F63 item of `STAGE_B_REVIEW.md` that the crate split's C7 lists; `types::parse` is already gone (I4) | RES-1 to RES-5 |
| RES-7 | `TypeNodeResolverExtension` (the extension point, registered through the `Extensions` input) and the three behaviours the crate split's C7 last bullet lists: assert targets `$param->prop`, `$param->method()` and `$this->method()` are kept, `@throws` and assert types get alias substitution, and a written `Traversable` resolves like any name, with the probes the crate split cites | RES-1 |
| EXT-1 (C8) | `Provider` implements `ClassHierarchy` as the tri-state relation (the trait takes `Name`s, lends memoized views instead of allocating, exposes `TemplateParam`s and gains the enum-cases query); the boolean `is_subclass_of` is deleted; an ancestor through an unresolved edge and a missing class answer `Maybe`; the 13 package sites and 26 in `types` (outside tests, at `ffa7cccb`) that call `Relation::structural()` switch (approved delta); the two project tests that pin a removed ancestor answer `Maybe` and are renamed through `gates/test-renames` | REG-7.1, REG-7.4 |
| EXT-2 | the `@property`, `@method` and `@mixin` implementors, and the built-in ones (universal object crates among them), replace level 2's tags and level 4's mixins; member lookup asks native members (own, traits, ancestors) first and these after, and a tag at the same or a closer hierarchy distance than the native member wins | REG-7.3, REG-7.4 |
| EXT-3 | C8's behaviour fixes, one commit each, fixtured: `@mixin` on a trait, a mixin never shadowing a native member, a mixin's inherited members, its own mixins and a used trait's (guarded per type and member against recursion), `@mixin Box<int>` binding `Box`'s templates and `@mixin T` mixing in the bound argument, a static mixin method under `__callStatic`, `mixin_exclude_classes`, a trait's tags reaching its hosts, a `@method` tag not a prototype, `@property-read`/`-write` as one property, enums reading `@method`/`@mixin`, `@phpstan-require-extends` on an interface | EXT-2 |
| EXT-4 | the class-reflection data extensions (`access_attributes`, `doctrine_entity`) through the trait; attribute arguments evaluate to types | REG-18.7, EXT-2 |
| SUM-1 (C9) | the summaries API: purity and global-variable inference leave reflection; `AnalysisContext` (the `Provider` and the summary accessors) is created here and completed by D6; `BodySource`, `FileBodies` and `NoBodies` move to the analyser | REG-7, H2.3 |
| SUM-2 | the global-variable fixes: sites include `foreach` bindings, destructuring, by-reference out arguments and appends, and expression typing keeps unkeyed array items, resolves call returns (templates, `static`, dynamic return types) and binds `self::`; a global's summary holds `null` and every value it can have, and `mixed` once a write the fold cannot see may reach it | SUM-1 |
| SUM-3 | the purity fixes: shell-exec and `throw` impure, a class-constant read pure, calls and `new` through parse-time names, method dependencies along inheritance, a static call's purity its method's, a user function judged by its body, a `pure-callable` parameter | SUM-1, OVL-5, REG-17.1 |

**On the engine.** Extensions are **an input**: an `Extensions` singleton holds the registered implementors and a
fingerprint (the set, each one's configuration and each one's `ResultCacheMeta`, the crate split's hook for an
extension's declared hidden inputs: a project-level file-set hook such as ext-pest's), which every query that calls
one reads first, so a changed extension or configuration invalidates exactly what called it and a persisted
database made under other extensions is not trusted. **An extension reads the world only through engine-provided
handles that record the read:** the file source, `FileSet`, a declared hidden-input fingerprint (its
`ResultCacheMeta`), and, for a live database, the `SqlSchema` and epoch input of H2.2 (the SQL extension's live
reflector `PREPARE`s per distinct text, which is the world and not a function of its arguments, and becomes the
epoch-keyed query `sql_columns` as `include_target_exists` becomes `path_is_file`, H4.5). It is given no other API:
no `std::fs`, environment or clock in an extension's crates, which `xtask arch` can check, because the harness's
double run (each registered extension run twice over the fixtures, failing on a difference) cannot see a stable
hidden input. With these rows REG-15.1's need reads "OVL, RES, EXT and SUM done".

## J3. Stages D to G, reconciled (Part F 18)

Read against the crate split plan's stage headers and D1 to D6, E1, E5 to E7, F and G, and the ledger's rows. The
plan's text stays the authority for what each phase does; this fixes how it meets the ledger, the engine and the
designs above.

| plan | ledger | resolution |
|---|---|---|
| D1 holder storage: every `Locals` map into holder storage, ExprKey interned, persistent maps, byte-identical | CHECK-6.3 (slots, persistent radix arrays), 6.13, 6.14; G1, G2 | **One change.** D1 is built with CHECK-6.3's design (a variable's `LocalKey` is its slot; 32-way `Arc`-shared radix arrays, with the diff that skips shared subtrees) and the stamps of G2, and deletes the two alias maps of G1 (the only tree references in `Locals`) with their `Rooted` values. D1 states the contract ("interned, persistent, byte-identical") and not the structure; the ledger's measured structure fills it. CHECK-6.13 and 6.14 are D1's follow-ups. **The level-3 findings whose designs add per-variable state to `Locals`** (F5's `maybe_undefined` set, F6's closure-literal alias, F45's reference groups, F61's unset-property keys) are this change's by the rule of K2: their fixtures now, their fixes inside D1 and D5 on holder storage (the fix review's own first step is exactly this `ExpressionHolder`) |
| D2 statement results, contexts, StmtHandlers; termination, last conditions, constant conditions | CHECK-6.2 (`has_yield` and `collect_throws` go) | CHECK-6.2 is D2's `StatementResult`; **F69** (implicit calls as throw points) is D2's, and CHECK-9 follows it. D2's `syntax::annotations` (goto labels, last conditions, try/catch types, parent statement types, the closure-argument context, the pipe and magic-constant rewrites; and the kinds the crate split names besides: `DeclarePositionVisitor`, `ImmediatelyInvokedClosureVisitor`, `NewAssignedToPropertyVisitor`, `StandaloneThrowExprVisitor`) are side tables of the flat tree that the builder fills in a pass at `finish`, beside `goto_labels` (B3); B3 and I1 reserve the place and the tables are added to the layout at D2. An immutable tree is not rewritten, so `RemoveUnusedCodeByPhpVersionIdVisitor`, which in PHPStan rewrites a lone `if`/`else` on `PHP_VERSION_ID` to true or false by the configured version, is a mark that records the comparison (operator and id) and is evaluated per range by its reader, as `registry/guard.rs` keeps declaration guards symbolic (decision 6: one parse, no version in any key); and the pipe's `f($x)` form is a view over the pipe node |
| D3 one inference path: ExprHandlers, per-`NodeId` results, `infer_expr` deleted, `TypeSpecifier` | CHECK-6.1 (observers, one inference per node), 6.7 (argument binding once), 6.15 (the walks' vectors) | **One change.** The "per-`NodeId` results" are the infer cache of G1.5 generalized: a table indexed by `ExprId` under the tree's serial, the only place a bare id is held in the checker |
| D4 `NodeCallback` sink, observers, result storage | CHECK-6.1's observer families | D4's interface is the observer API; the PHPStan-API walker and the statement rules are its first families (CHECK-6.1's text) |
| D5 semantic convergence, one approved delta each | none | as written; the nsrt and rules oracles of gate 12 are the measure |
| D6 engine APIs: Scope, public `TypeSpecifier`, returned-error `Rule`, derived nodes, Collector, Observer, extension registry, concurrency model, `AnalysisContext`, `InternalRule` deletion | CHECK-6.10, 4.1 | the returned-error `Rule` is I5's `Message`, `RuleError` and `RuleErrorBuilder`; `AnalysisContext` is J2's SUM-1; the extension registry is J2's `Extensions` input; the concurrency model is H2.7 |
| E1 collectors and project-level rules (reference sites, dead code, architecture, class shapes, circular namespaces, composer hygiene, mock hygiene, unmatched ignores) | LOAD-5.1 | **The facts are W3's, the collection E1's** (Part E 39): LOAD-5.1's per-file facts are `file_products` fields built in W3, and E1 moves their collection onto D6's Collector API and adds its semantic fixes over those fields. The passes do not all land in E1: architecture, class shapes and Composer hygiene are E1 collectors; banned constructs are per-node rules in the rules walk (the crate split calls them "per-file checks, not project-level"); mock hygiene is ext-mockstan's (E4); unmatched ignores are the finalizer's (F1) |
| E2 builtins; E3 every diagnostic to `rules` and extension modules; E4 extension modules | none | as written, on D6's API |
| E5 levels: no rule above the level runs | — | `CheckInputs` holds the level; `Pending.level` and the finalizer's level stage go (I5, F1) |
| E6 observers for the LSP's presentation | G4 | hover, inlay hints, semantic tokens and completion read D4's per-node results instead of re-running the checker; G4b's node-at-position query is its first user |
| E7 checker decomposition (the checker's fields' exact sets: 88 field lines at `ffa7cccb`, where the crate split's Appendix C counts 74) | CHECK-6.8, 6.18 | the per-file, per-function and per-pass state sets are D1/D2's; "one checker per worker, reset per file" (CHECK-6.18) is buildable under G1 (a `Checker` takes no tree lifetime), and is E7's because its reset clears exactly the sets E7 enumerates |
| F1 Session, F2 result cache | REG-16.4, CACHE-13.2 | H4; F1's seven fixed-first commits are batch 0's (Part E 22) |
| F3 formatters, F4 binary, F5 language features | none | as written; F5 reads D4's results |
| G1 extraction | — | the list gains `engine` (a module of the package until here, with its `crate engine` line between `analyser` and `project`, H3.1, Part E 33); `php-version` is a crate already (batch 3, Part E 32), so it belongs among G's "crates already", and the crate split's "26 new packages" count is corrected; Appendix A's module assignments and Appendix E's third-party rows gain `salsa`, `triomphe` and `wide` |

**Conventions every Stage D and E commit obeys:** tree-derived values are handles in walks and `Rooted` where one
must be stored, and no bare id is held outside `syntax`, the infer cache and the probe module (G1); **no value that
another file's query reads holds a position of a file other than its own** (a file's own diagnostics and per-node
results may), and an engine-compared value's equality is exact (H1.5); a message is a `Message` (I5); reflection is
read through the checker-facing trait that `Provider` implements (H2.4); an extension reads the world only through
engine handles that record the read (J2).

**Order.** Part D's stands: W5 (C6 to C9), W6 (STUB-11, STUB-13), then W7 (Stages D to G). Stage D is written on the
final reflection API, and C8 changes what the handlers call (the tri-state relation, the extension points) and C9
what they read for purity and globals, so starting D before them would write those calls twice. LOAD-5.1's facts
are W3's and their collection moves into E1 (Part E 39).

**Ledger rows to write** at approval, in the docs commit series of Part E 20: OVL, RES, EXT and SUM (J2), with the
needs above; a D-row and an E-row per phase of Stage D and E that the plan lists, each citing its plan section and
the oracle counts it carries; CHECK-6.1 to 6.3, 6.7, 6.13 to 6.15 re-pointed to "built by D1 to D4"; LOAD-5.1
re-pointed to W3 (facts) and E1 (collection); and the corrections of H6.

---

