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

## Tasks this part owns

None. H1 is a design, and its batch-4 build (H1.7) lists rows that Part C's C1 table already schedules: NAME-1.1 to 1.6, NAME-2.1 to 2.3 and SYN-4.5, all in batch 4. CHECK-13 is decided in H1.4 (anonymous classes reach the class-shape parent-call rules), but no batch table lists it yet, so its build place is open. The row is OPEN and is read against the code when it is worked, as Part N says.

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
