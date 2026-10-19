## A2. LOAD-9: installed packages resolved once

<!-- status:begin -->

## Status of this part's tasks

From the ledger at `13ff6c1e1` on 2026-10-19. Regenerate with `scripts/plan_status.py`; change the ledger, not this table. `also in` lists the other parts that name the task.

| task | status | ledger | also in |
|---|---|---|---|
| LOAD-2.2 | DONE | Walk DONE in `85a4ecad`. PART: the name floor's replacement is built by Part A's A1 (`097c164b`, `40d889bb`, … | 02 03 10 11 12 |
| LOAD-2.4 | DONE | DONE `df804239`. The walk hands each directory's files to the sink as it reads the listing, when one walk alone reaches … | 02 03 21 22 |
| LOAD-2.8 | DONE | Parts 1 and 2 DONE: `27c3b093` (the manifests and `installed.json` read member by member; a repeated key keeps its last … | 02 07 |
| LOAD-3.1 | OPEN | OPEN | 02 05 06 10 12 19 21 |
| LOAD-5.1 | OPEN | OPEN | 05 06 07 10 12 19 21 23 31 |
| LOAD-5.4 | DONE | DONE `a5ab84c4` (Part A's A5a): the passes read through `ast_of`, and `FileState::ast` is gone | 01 02 05 07 10 11 12 |
| LOAD-7.6 | READY | READY; a delta in the config's keys | 06 07 08 10 12 31 |
| LOAD-7.8 | OPEN | OPEN | 03 08 12 31 |
| LOAD-8.10 | DONE | DONE `9513479b`. **The rule:** a walk hands a directory's files over when it is the one walk of their strongest role … | 03 21 |
| LOAD-9 | DONE | DONE `3dcb1192` (Part A's A2): discovery is spawned first, and `read_installed` reads each package's directory from the … | 01 02 10 12 21 |
| SYN-1.1 | DONE | DONE `2f9b1281`: the loop, with a test at a depth the recursion needed a guard for; A6 (2026-10-19) found no standalone … | 02 07 10 |
| SYN-1.2 | READY | READY | 02 05 06 07 10 12 13 31 |
| REG-7.2 | OPEN | OPEN | 05 06 10 11 20 |
| REG-7.5 | OPEN | OPEN | 01 02 05 06 07 10 12 20 22 |
| REG-14 | PART | PART | — |
| REG-14.4 | DONE | DONE `8739437e`; A6 (2026-10-19), off callgrind over the Piwigo run, the sum of `from_decl`'s inclusive Ir over all … | 01 02 10 12 |
| REG-14.5 | FOLDED | FOLDED into REG-7.5 (Part E 41): REG-7.5's combination is written without the `Vec`, and the function it would replace … | 01 02 07 10 12 22 31 |
| REG-16.4 | OPEN | OPEN | 02 05 06 07 08 10 11 12 13 20 21 22 23 31 |
| STUB-7.5 | OPEN | OPEN; a delta, approved (decision 14) | 02 06 07 08 10 12 21 22 31 |
| COMP-5 | PART | PART: A7's maps keep the root's entries (`2c6f1eec`); the flat lists stay filtered, and the class is found through the … | 02 07 12 31 |
| COMP-6 | DONE | DONE `088089ed`: each installed package whose install directory holds a vendor directory by the same test as the root's … | 02 07 12 31 |
| COMP-7 | DONE | DONE: the discovery admission `acf48fb8`, the walk's cut `16418a2c`, the tests `c14aa865`. The batch delta for a … | 01 02 03 07 10 12 31 |
| COMP-8 | DONE | DONE `5b76d855`: the nested roots load before the walk, so owners, roles and contexts are fixed before any task runs. … | 01 02 03 07 12 19 31 |

<!-- status:end -->

**Order.** A2 needs A1's `vendor_location` (its composer commit comes first); A3, A5 and A5a are independent of both.

**Now (`1f351379`).** `PackageContext::load` (`composer/src/lib.rs`, line 107) builds
`ComposerManifest::load` in the struct literal before `PendingVendor::spawn`, so discovery waits
for it. `read_installed` (`manifest.rs`, line 284) calls `resolve` (a `canonicalize`: one `readlink`
per component) on `vendor/<name>` and again on the install path, which Composer writes as
`../<name>` from `vendor/composer` for a package installed in place, so the same directory:
409 resolutions for 205 packages, 2,654 of the main thread's 3,055 `readlink`s, 8 to 14 ms before
discovery starts (4 to 7 perf samples in each of four recordings, about half system calls).

**Change.**
1. `PackageContext::load` computes `vendor_location(root)` once (A1's function: it reads the manifest's bytes for
   `vendor-dir` before it parses anything, and takes the environment as a value), spawns discovery with that answer, then
   loads the manifest. The two run beside each other; the manifest is still needed on the main thread (promoted path
   repositories feed `dirs`), so only the head start and the manifest's own shorter run are gained. Discovery takes
   nothing from the manifest today (it hard-codes `vendor`), so it asks no second time, and the answer is handed to
   `read_installed` as well: one read of `composer.json`, one canonicalize for each candidate, and the candidates'
   listings, which discovery's own reads share. COMP-7's admission (A1) makes discovery read the package list too, and
   the list is made once: a `OnceLock` shared by the manifest load and the discovery thread, whichever asks first reading
   `installed.json`, so the two still run beside each other.
2. `read_installed` takes the load's `DirListings` (`DirListings::listing` is safe to ask from the
   main thread while discovery asks: the first ask reads a directory and a later one gets that
   listing) and that answer's `installed` directory, in place of `<root>/vendor`. For a package `v/p` it
   reads the dirent kinds from the listings of that directory and of `v` (`Listing::entry`): both
   real directories give `R/v/p` with no `readlink`; a link, an unlisted entry (a case-folding
   volume whose spelling differs from `installed.json`'s, too) or an unknown kind falls back to
   `resolve`, as today. Listings are keyed by resolved directory, which `R/v` is once `v` is known
   not to be a link.
3. The install path: join it to `<vendor>/composer` and normalize lexically; when that equals the
   lexical `<vendor>/v/p` the package's `install_dir` is its `vendor_dir` field, with no second
   resolution. The shortcut holds only for this shape: the path has at most one leading `.` or `..`, then plain
   names (an interior `..`, as in `../v/q/../p`, resolves elsewhere when `q` is a link, so it is resolved); the
   package name splits into exactly two plain components, and on Windows the joined child is spelled by
   `HashedPath::child` (the cross-platform plan's R3), since a name such as `aux` or `nul` makes `canonicalize` return a verbatim
   path that a plain join does not; an absent or null `install-path` (a metapackage) means `install_dir` is the
   `vendor_dir` field with no second resolve; and `composer` must be a real directory (its kind from the vendor
   directory's listing), because `..` is lexical only across real directories. Piwigo's `installed.json` has 201
   `../name`, four `./name` (composer/pcre, semver, spdx-licenses, xdebug-handler) and one absent path among 206
   entries; the `./name` ones work because the rule is lexical equality, and a test names them. Otherwise (a path
   repository whose `install-path` is `../../libs/x`) resolve it, as today.

**Tests.** `cargo test -p phpstan-rs-composer`: for a temp tree with an in-place package, a
symlinked package (path repository), a symlinked `vendor/v`, a symlinked `vendor`, a symlinked
`vendor/composer`, a missing package directory, an unlisted one, an interior `..`, a `./name` package, a null
`install-path`, a non-plain Windows name (`aux`, on the Windows leg) and an incomplete listing, the new
`read_installed` gives, for every package, `std::fs::canonicalize` of its directory: explicit expectations, with
no copy of the old function kept as an oracle (Part E 13). The same expectation runs over the real
`installed.json` files of the 14 vendor trees that carry one (amphp, composer, doctrine-collections, doctrine-orm,
drupal, guzzle, laravel, league-commonmark, league-flysystem, monolog, psalm, reactphp-http, rector-src, symfony)
and Piwigo's.

**Measure.** `strace -ff -c` `readlink` count (3,055 before), a perf recording of the main thread
before the walk (off and warm), off and warm callgrind on `read_installed`, and
`scripts/abab.py` (20 pairs, warm and off, the Piwigo output checked on every run) against the batch's base commit.

## A3. REG-14.4: the parameters, not a `Vec<&str>`

`from_decl` (`function_sig.rs:828`) collects `param_names: Vec<&str>` for every function, whether or not
its doc has an assert tag, and hands it to `resolve_assert_tags` three times; `classify_assert_target`
(`asserts.rs:76`) only asks `contains`. Both take `has_param: &dyn Fn(&str) -> bool`, one closure over the
declaration's parameters (`|name| f.params.iter().any(|p| p.name == name)`) shared by the three calls. No list is
built, and the assert code names neither a parameter nor a list type, which item 9's step 1 turns into a view, so
that step does not touch it again. No behaviour change. Tests: `cargo test -p phpstan-rs --lib registry::`.
Measure: the heap profile's block count at `from_decl` (the 20 G report gives the cost of this row as allocations,
M10, which an instruction count does not show) and off callgrind, `from_decl` inclusive. The ledger's REG-14.4 text
("passes the parameters on") is reworded to the predicate (Part N).

## A4. REG-14.5: folded into REG-7.5 (Part E 41)

`reconcile_with_native` (`decide.rs:13`) is the build-time combination of a PHPDoc type with its native type.
Decision 8's rule replaces it at the final level: REG-7.5's combination reads the class hierarchy, where `decide.rs:211`
trusts a class member unjudged because the registry is still being built. Its callers are the passes that REG-7.2
and 7.5 turn into levels (`function_sig/type_resolution.rs:157`, and `resolve.rs:1274`, `:1305`, `:1340`). Making it
allocation-free now would be work on code W2 replaces, for 41,085 allocations, so REG-14.5 is not built on its own.
What it found is how the replacement is written, and REG-7.5 starts from it: (1) the members are a borrowed slice,
the union's own (`self.repr()`) or `std::slice::from_ref(&self)`, with no `vec![self]` and no `into_repr`, which
copies the member list while the type is shared (`lib.rs:337`); (2) the branches that answer without building are
the doc type inside the native type (the doc type), the native type inside the doc type (the native type) and the
disjoint pair (the native type), each with no `kept`; (3) every other case builds its result once. Decision 8's rule
also *intersects* a PHPDoc member that shares values with a native kind (`callable(int): string` on `Closure` is
`Closure(int): string`, `T` on `Foo` is `T&Foo`) and keeps a native class that no member covers, which today's
function never adds back; those answers are built, not chosen, and they differ from today's, so the existing
`reconcile_with_native` cases in `registry/tests/function_signatures.rs` and `phpdoc_inheritance.rs` that decision 8
keeps carry over as the new function's first tests, one for each branch, and the ones it changes are rewritten as its
approved deltas.

## A5. SYN-1.1: `peeled` as a loop

```rust
pub fn peeled(&self) -> &Expr {
    let mut expr = self;
    while let ExprKind::Grouping(inner) | ExprKind::ErrorSuppress(inner) = &expr.kind {
        expr = inner;
    }
    expr
}
```
(61.2 M Ir over 2.54 M calls; the 20 G report counted 160 M with the thread-local reads.) A test
with a deep `((((…1…))))` (the parser's guard grows the stack; the loop needs none). The guard in
`Drop` stays until Part B. `cargo test -p phpstan-rs-syntax`.

## A5a. LOAD-5.4: no reader gets an empty file

**Now.** `FileState::ast()` (`project.rs`, line 223) is private to the `project` module, whose child modules see it, and
returns the kept tree, or `&[]` in a release build when the file kept none (a debug assertion says no reader asks).
Twelve non-test sites call it: the unmatched-ignore pass's `trait_line_ranges` (`project.rs:828`), the use-site
indexing of an edit and of a bulk load (`:1828`, `:1876`), the phpstan-API check (`:2621`), and eight in LOAD-5.1's own
passes, `project/architecture.rs` (two), `banned.rs`, `class_shape.rs`, `composer_hygiene.rs` (three) and
`mock_hygiene.rs`. Each reads a file whose tree is kept (an analysed file, or any file of a project that retains
source), so the fallback never runs today. LOAD-3.1 (W3) changes which files keep a tree, and item 9's step 1 changes
the type of `kept_ast`; an accessor that can return an empty tree would make the new API carry an empty-tree
constructor for the one case that must not happen.

**Change.** `FileState::ast` is deleted, and every reader asks the project's `ast_of(id)`, which gives the kept tree
or parses a context file again; the kept tree and the re-parsed one are already the two arms of `AstRef`. No reader
skips a file for lacking a tree, because a skip would silently drop its diagnostics the day trees are evicted
(REG-16.4 makes them "parse results the engine evicts and parses again on demand") and would leave the twelve sites to
be rewritten again. The analysed-only passes already iterate `iter_with_ids`, so they get the kept arm at the cost of
a lookup. Nothing else holds an empty-tree fallback. No behaviour changes: the files those sites read keep their
trees. The test that can fail is a build-once project whose context file keeps no tree: the passes run through
`ast_of` without reading an empty slice, and the accessor's removal is itself checked by the compiler.
`cargo test -p phpstan-rs --lib -- project::`.

The change is a type change that an end-state invariant needs (a reader cannot be handed a tree that is not
there), not a structure the engine deletes: `ast_of` is the shape of `parse_analysed`'s accessor.

## A7. The autoload maps keep their keys and gain PSR-0 (Part E 40; STUB-7.5's prerequisite)

**Now.** `Autoload` (`composer/src/manifest.rs`, line 29) has `psr4`, `classmap`, `files` and `excluded` and no `psr-0`;
`GeneratedEntries { files, dirs }` (`vendor.rs`, line 237) reads `autoload_namespaces.php` (PSR-0) as `Reading::Dirs`
and keeps no prefix or class key: the classmap's class names and both PSR prefixes are dropped, so the lazy locator
`locate` (H4.1, STUB-7.5, W3) would have nothing to look a class up by. The ledger has no row for it (Part E 40).

**Change.** `Autoload` gains `psr0` (prefix to directories, with Composer's PSR-0 rule for `_` in the class part)
beside `psr4`. `GeneratedEntries` keeps the classmap's class-to-file entries and the PSR-4 and PSR-0 prefix-to-
directories maps beside the flat `files` and `dirs` lists that the walk reads, and those maps are what `Packages`
(H2.2) carries. Nothing that reads the lists changes: the keys are new fields, so no output moves and the row has no
delta.

**The maps keep every entry the generated files list, the root package's own included** (COMP-5). The root's `autoload` and
`autoload-dev` targets are in the generated maps, below `$baseDir` and outside `vendor/`, and the flat lists drop them
today, so `check-project <named file>` does not know a class of the project that no analysed file names (the crate split
counts 1,253 of the Piwigo oracle's 1,855 phpstan-rs-only `class.notFound` naming a `Piwigo\` class). Only the flat lists
stay filtered to the vendor directory, the path repositories and, after A1's COMP-7 commit, the install directories of packages
Composer installed elsewhere, so nothing the walk reads moves in A7; W3's lazy locator finds the class. No interim fix is built for the batch path before W3 (the rule of Part E 35 and 41, which replaces such a fix by the
locator), and a by-name oracle run carries that noise until then.

**Tests.** `cargo test -p phpstan-rs-composer`: for each of the 14 vendor trees that carry `vendor/composer/` (A2's
list) and Piwigo, the keys of `GeneratedEntries` equal the arrays that `php` loads from `autoload_classmap.php`,
`autoload_psr4.php` and `autoload_namespaces.php` (a script run by `php`, its output compared in the test as data);
a hand-written PSR-0 manifest (`Vendor_Pkg_Class` with the underscore rule), a nested-prefix case, and Piwigo's own
`Piwigo\` prefix (below `$baseDir`, outside `vendor/`) in the PSR-4 map. The lookup that reads the maps is `locate`, W3's,
and is tested against `ClassLoader::findFile` there (S-11).

**Where it sits.** After A1 and A2, which edit the same crate; independent of A3 to A5a; A8 follows it. It has no validation
of its own beyond the crate's tests and A6's runs, where the output is identical.

## A8. The other Composer roots' vendor contexts (COMP-6 and COMP-8; A1's test)

**Now.** Discovery runs for the Composer roots the session knows: the nearest manifest at or above each configured
directory (`composer_roots`, `session.rs`), which for a degrade root (no `paths`) is the one nearest the workspace root.
Two kinds of project beyond them autoload from a vendor directory the session never reads, and their classes are unknown
(`class.notFound`, and the `mixed` that comes of it). **A package's bundled `vendor/` (COMP-6):** `rector/rector` and
`symplify/easy-coding-standard` autoload a `bootstrap.php` that registers their own `vendor/autoload.php` at runtime, so
their classes live in a `vendor/` inside the package (`vendor/laravel/vendor/rector/rector/vendor` and
`vendor/rector-src/vendor/symplify/easy-coding-standard/vendor` in the corpora): the crate split counts 495 phpstan-rs-only
`class.notFound` in `vendor/rector/rector` alone. **A nested project (COMP-8, added on 2026-10-18 at the user's word):** a
repository that keeps more than one Composer project (a monorepo of packages, or `tools/cs/` with a `composer.json` and a
`vendor/` of its own) opened as a degrade root has one known root, so its nested projects are analysed with no context
from their own vendor trees; A1's rule already leaves their vendor directories out of the analysed walk, and nothing loads
them as context. A configured `paths` entry already has its project, because the nearest manifest above it is its root.
Counted on the corpora (2026-10-18), with A1's cuts and the root's own manifest left out: Symfony has 211 nested
manifests, Drupal 66, phpstan-src 68, Laravel 39, Composer 32, rector-src 27, league-flysystem 15 and Psalm 7; the nested
projects that hold an installed vendor directory are all test fixtures (four in rector-src, two in Composer, one in
Drupal), so the corpora exercise the finding of the roots and their ownership at scale, and the loading of nested vendor
trees only on fixtures.

**Change, bundled vendors (COMP-6).** After `read_installed`, each installed package whose install directory holds a vendor
directory by A1's test gets the same discovery as the root: `vendor_location(install_dir)` (a package has its own
`composer.json`), then `discover_vendor_context` on the installed directory it names, and its entries join the project's
vendor context. The cost is a look for `vendor` in the listing of each package's directory (205 packages for Piwigo; the
listings are shared with the walk and with discovery, and the commit counts how many are new). A class that the project's
install and a bundle both declare is two candidates (decision 11), and a context declaration is never reported (decision 9),
so no duplicate is reported.

**Change, nested projects (COMP-8).** *The roots are found before the walk.* The walk tags each file with its owner as it
lists it (`Listed.owner`, from the immutable `PackageIndex` that `Walk::new` takes), decides roles from the seeds, and the
context walk is built from the vendor contexts, so every root must be known before the walk starts: a root found by the
walk's own tasks would make owners, roles and contexts depend on task timing. In a degrade root `Session::resolve` therefore
runs a **scan**, `Walk::manifest_dirs`, one more mode of the walk: it runs `walk_dir`'s traversal over the same listings,
with the degrade walk's own child decision (the excluded names, A1's rule, the known roots' seeds, links followed and
claimed once), hands no file to the pool (`take` is `None`, as for a walk that hands nothing over) and returns, in path
order, the directories below the root whose listing holds a `composer.json` file entry. The child decision is the walk's
one function, so the scan and the walk cannot disagree about which directories exist. `DirListings` caches the listings, so
the walk that follows reads each from the cache and A1's per-listing facts are computed once; what the scan costs is that the
reading of files starts after the listing of the tree and not beside it (Measure). `--no-composer` runs no scan.
*What is a nested root.* A directory with a `composer.json` that parses, below the root, outside the directories the walk
leaves out, and not the install directory of a package of a known root: the install directories of path-repository
packages (promoted, so their files stay owned by the installed package) and of packages installed elsewhere (COMP-7: a
package's own manifest is not a project) are not roots, and the ones inside a vendor directory are never reached. A manifest
that does not parse is no root (`ComposerManifest::load` gives `None`). *Loading and merging.* Each nested root gets
`PackageContext::load` as a known root does (manifest, `vendor_location`, `read_installed`, discovery), the loads running
in parallel on the pool; a root whose `vendor_location` finds no installed vendor directory takes
`PendingVendor::from(VendorContext::default())` and not a discovery thread (Symfony has 211 nested manifests and no vendor
among them). The results are merged after the known roots, in path order of the canonical path, into `vendors`,
`composer_manifests` (so the indices of `PackageOwner` are the same on every run), `promoted`, the seeds and R8's `trees` and
`unbuilt` (a nested project that has a manifest and no built vendor directory puts its `vendor` in `unbuilt`, as a known
root does, so that its `require 'vendor/autoload.php'` is not judged). Nothing depends on thread count or timing: the roots
are a function of the tree, the order is by path, and the merge happens before the walk.
*What the roots change elsewhere.* (i) Ownership: a file under a nested project is owned by that project (`Root(i)`; the
deepest manifest wins in `PackageIndex`), which gives Part E 42's "owner = nearest ancestor `composer.json`" its data, and
`check_composer_hygiene`, which runs once for each manifest of the index (`session.rs`: `for root in 0..manifests().len()`),
now checks a nested project's files against its own manifest where the outer one's was read; the rule is opt-in, the change
is a delta for a project that enables it, and the per-call setup that does not depend on the root (the declaring files in
path order, the class and function maps) is built once. (ii) `phpstan_api_check_enabled` is the OR over the roots, as the
`|=` of the loop already makes it for several configured roots; the PHP range (`composer_php_versions`) and the extension
activation (`root_manifest`) stay the session root's until LOAD-7.6 reads the owners (Part E 42). (iii) A name that two
roots' vendor trees declare is two context declarations of one name, which the registry resolves as it resolves any
duplicate today (decision 11's candidates at W2; at W3 `locate` over the asking file's own owner's maps binds each file to
its own project's autoload), so until then a file may see the declaration of another project. (iv) A configured `paths`
entry keeps the nearest-ancestor rule: it names what is analysed and so names a project's directory to have its vendor, and
the scan would sit before a walk whose overlap of listing and reading the gate measures (LOAD-2.4, LOAD-8.10); this row
measures the scan on Piwigo's configured tree, and if it costs less than the gate's noise, configured entries take it too (the
same call for each configured directory, with its delta). (v) A project created after the load is a root when the session
is resolved again (H4.2's trigger set on `composer.json`). At W2 `Packages` holds the roots (H2.2), and the manifest
directories the scan found are the owner facts that `range_of` reads.

**Delta.** The output of a project that installs a package with a bundled `vendor/` changes: the `class.notFound` that named
a bundled class, and the `mixed` that came of it, go (batch and editor). A degrade root over a tree with nested Composer
projects (editor) gets their vendor classes as context, so those `class.notFound` go as well, their `vendor/autoload.php`
includes are judged against built trees and not against the outer one's, their files are owned by their own project, and
the opt-in hygiene rule checks them against their own manifest. The ruling of 2026-10-18 on A1's delta commit (M5d: "agree")
covers the bundles; the nested projects are COMP-8's, added at the user's word. **Tests.** A fixture package with a bundled
`vendor/composer/`; a package without one costs no read of a bundle; `check-project` over `vendor/laravel` and
`vendor/rector-src` before and after (A6). For COMP-8, a degrade root with `composer.json` and an installed `vendor/`, and
`tools/cs/composer.json` with an installed `tools/cs/vendor/` whose `autoload_static.php` lists `Cs\Fixer`, and
`tools/cs/run.php` using it: the scan returns `tools/cs`, `run.php` is owned by the nested project and has no
`class.notFound`, `tools/cs/vendor/` is not analysed (A1), the include trees hold it, and a nested project with a manifest and
no vendor puts its `vendor` in `unbuilt`; a manifest under a promoted path-repository package, one under a package installed
elsewhere and one that does not parse are no roots; two nested projects that declare one class give the same registry on one
thread and on eight, whichever directory sorts first; `--no-composer` runs no scan; the scan and the walk list the same
directories over a tree with links, excluded names, a Composer directory and a seed (every directory one enters, the other
lists); a hygiene fixture whose nested project uses a package its own manifest requires and the outer does not.

**Measure.** The scan: the load phase of `incremental-bench` over Piwigo's root, `vendor/symfony`, `vendor/drupal` and
`vendor/laravel` (degrade roots), `abab` pairs of the base and HEAD, warm and off; a counter of listings read in the walk
after the scan shows zero new ones (the listing is not done twice), and the price is the wall of listing before reading,
which the pairs state. The merge: the threads started (`strace -f -c`: none for a root without a vendor directory) and the
211 small manifest parses of Symfony on the pool. Hygiene over `vendor/symfony` with the rule on: 212 calls, the setup built
once. **Where it sits.** After A1 (the rule and the seeds), A2 (the package list) and A7 (the maps).

## A6. Validation, once, after the last row

The batch's base is the commit before the first of these rows that lands; every "before" below is a worktree at it
(copy `vendor/phpstan-src` in, its own target dir, its binaries copied to `.corpus-runs/gate/bin/<commit>/<profile>/`
at once so `abab.py` finds them, the worktree removed afterwards), and every "after" is HEAD. The item-0 baseline
does not exist yet (batch 0 comes after Part A), so this validation compares binaries and the Piwigo digest.

- `cargo test --workspace` and `cargo test -p phpstan-rs --test fixtures` (the workspace run skips fixtures); A1, A2,
  A5, A7 and A8 edit `composer` and `syntax`, whose tests run here with the rest.
- `check-project vendor/monolog` stdout identical (both `LC_ALL=C sort`ed) between the base and HEAD: file cache off,
  then cold and warm.
- The Piwigo identity oracle, off and warm: 171,439 bytes, sha256
  `8ff144c3fee775166369b993a58814e0f247845fbf850f94d0c41b57924488b1` (2,792 files, 581
  diagnostics). Piwigo and `vendor/monolog` set their paths on the command line and have the default vendor
  directory, so they cannot see A1 (a configured root does not apply the walk's rule): no row here changes a diagnostic on
  them (Piwigo's tree holds one `composer.json` outside `vendor/`, its own, and no package installed outside `vendor/`),
  A1's delta commit reaches only a project that sets `vendor-dir`, `COMPOSER_VENDOR_DIR` or `COMPOSER`, its COMP-7 admission
  only a project whose Composer installed a package outside the vendor directory, and A8 only a project that installs a
  package with a bundled `vendor/` (its scan runs in a degrade root only), which their own fixtures show.
- **A1's delta on real projects (local, Linux):** the files the degrade walk lists, with their roles, and the
  `classify()` answers for sample paths (a vendor file, a nested project's vendor file, `lib/vendor/`, `.git`), at
  the base and at HEAD, over Piwigo's root and over each of the 14 vendor trees that carry a
  `vendor/composer/installed.json`; the difference is the delta the user ruled on, and it should be only the
  `vendor/` directories that are not Composer's and the Composer vendor directories that are not named `vendor`. COMP-7
  adds nothing here, since the one package the corpora install outside its vendor directory is a path repository's
  (Drupal core, promoted), so its fixtures carry it.
- **A8's delta:** `check-project` over `vendor/laravel` and `vendor/rector-src`, each of which installs a package with a
  bundled `vendor/`, before and after (both `LC_ALL=C sort`ed); the difference should be the `class.notFound` that named a
  bundled class and what followed from it, and every other tree's output is identical. **COMP-8's delta:** the roots
  and owners that the degrade walk finds over `vendor/symfony`, `vendor/drupal`, `vendor/laravel`, `vendor/rector-src` and
  `vendor/composer`, at the base and at HEAD (the scan's roots equal the counts of A8's "Now"): the analysed files and their
  roles are identical, since the scan changes owners and contexts and not what is analysed, and the differences are the
  owners of the nested projects' files, the context files of the nested vendor trees (fixtures only) and `unbuilt`.
- Off callgrind (`dhat` profile with frame pointers, with `PHPSTAN_RS_NO_FILE_CACHE=1` and `PHPSTAN_RS_NO_STUB_CACHE=1`,
  the ledger's off mode; the 20 G profile was file-cache-off with a warm stub cache, a different mode, so its total is
  not written into §1's off table): `Expr::peeled`, `from_decl` (with the heap block count, A3) and `read_installed`
  inclusive Ir against the base's binary, total beside it. LOAD-9's wall pairs (A2). One heavy job at a time,
  `nice -n 19`.
- **LOAD-2.8's parts 1 and 2** (`27c3b093` and `53e6f766`) are 125 commits back, so these runs cannot isolate them
  (Part E 25): they are two separate callgrind and `abab` comparisons, each commit against its parent (`53e6f766` and
  `6759cc39`), with binaries built in the gate and `dhat` frame-pointer profiles and kept as above.

Bookkeeping when it lands (the full list of ledger and `WORK_STACK.md` edits the plan implies is Part N): ledger rows
to DONE with hashes (LOAD-2.2 stays PART until LOAD-7.8 deletes `path_under_excluded_dir` and the constant, which
`A1` keeps for `.git` and `node_modules`; REG-14 stays PART; LOAD-9; SYN-1.1; LOAD-5.4; the COMP rows A1, A7 and A8
close, N6), §1's Ir table, §2.1;
`WORK_STACK.md` (item 2's bullet goes, LOAD-9 leaves the "Outside the items" line, item 8 keeps 14.1 and 14.3, item 9
leaves SYN-1.2 onward, item 19 loses LOAD-5.4, state line). LOAD-2.8's row says its parts 1 and 2 are "not measured
(needs approval)"; the approval is standing, and the separate runs above measure them.

---
