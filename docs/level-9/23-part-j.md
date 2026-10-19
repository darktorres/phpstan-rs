# Part J. The wall's later designs (Part F 16, 17, 18)

<!-- status:begin -->

## Status of this part's tasks

From the ledger at `b9de6dcf1` on 2026-10-19. Regenerate with `scripts/plan_status.py`; change the ledger, not this table. `also in` lists the other parts that name the task.

| task | status | ledger | also in |
|---|---|---|---|
| LOAD-5.1 | OPEN | OPEN | 04 05 06 07 10 12 19 21 31 |
| PROD-5.2 | READY | READY | 02 05 09 10 11 |
| DOC-2.2 | OPEN | OPEN | 05 07 08 09 10 12 |
| REG-7 | OPEN | OPEN | 05 06 08 12 18 19 20 21 |
| REG-7.1 | OPEN | OPEN | 06 10 20 |
| REG-7.3 | OPEN | OPEN | 06 10 20 |
| REG-7.4 | OPEN | OPEN | 05 06 10 20 |
| REG-7.12 | OPEN | OPEN | 06 10 19 20 |
| REG-15.1 | OPEN | OPEN | 06 24 |
| REG-16.4 | OPEN | OPEN | 02 04 05 06 07 08 10 11 12 13 20 21 22 31 |
| REG-17.1 | OPEN | OPEN | 10 20 |
| REG-18.3 | READY | READY | 06 10 20 |
| REG-18.7 | OPEN | OPEN | — |
| STUB-7.1 | OPEN | OPEN; a delta, approved (decision 14) | 05 06 07 08 10 11 12 13 |
| STUB-7.4 | OPEN | OPEN | 08 10 |
| STUB-7.6 | OPEN | OPEN | 06 08 10 12 |
| STUB-7.11 | OPEN | OPEN | 08 10 |
| STUB-8 | OPEN | OPEN; a delta, approved, both parts (VAL-2.4) | 05 06 10 |
| STUB-10.2 | DONE | DONE (decided 2026-10-15, §2.4 decision 7): yes, for PHP's declarations and PHPStan's function map and `.stub` files; … | — |
| STUB-11 | OPEN | OPEN | 05 06 07 10 12 21 24 |
| STUB-12 | OPEN | OPEN | 06 08 |
| STUB-12.1 | OPEN | OPEN | 06 10 |
| STUB-12.2 | OPEN | OPEN | 06 08 10 12 |
| STUB-12.3 | OPEN | OPEN | 06 10 |
| STUB-12.4 | OPEN | OPEN | 06 10 12 |
| STUB-12.5 | OPEN | OPEN | 06 10 |
| STUB-13 | OPEN | OPEN | 06 07 10 |
| STUB-13.1 | OPEN | OPEN | 06 10 |
| CACHE-13.2 | OPEN | OPEN | 05 06 07 08 10 12 21 22 |
| CHECK-4.1 | OPEN | OPEN | 05 08 09 12 |
| CHECK-6.1 | READY | READY; approved in principle (VAL-2.8) | 05 06 07 12 |
| CHECK-6.2 | READY | READY | 06 07 12 |
| CHECK-6.3 | OPEN | OPEN | 05 06 07 12 13 14 |
| CHECK-6.7 | OPEN | OPEN | 05 06 07 12 |
| CHECK-6.8 | READY | READY | 05 06 10 11 12 |
| CHECK-6.10 | READY | READY | 05 06 07 08 09 12 |
| CHECK-6.13 | OPEN | OPEN | 05 06 07 12 |
| CHECK-6.14 | OPEN | OPEN | 05 06 07 12 |
| CHECK-6.15 | OPEN | OPEN | 05 06 07 12 |
| CHECK-6.18 | READY | READY | 05 06 10 12 14 16 |
| CHECK-9 | READY | READY | 05 06 07 10 11 12 24 31 |
| CHECK-10 | OPEN | OPEN | 05 06 07 08 10 12 |
| COMP-2 | OPEN | OPEN. Not part of Part A; before the baseline (batch 0), since it changes a range and so a dump | 07 12 19 31 |

<!-- status:end -->

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
