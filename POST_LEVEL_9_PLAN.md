# The batch after the cross-platform fixes: Parts A, C to F, G5, I2 to I5, K, L and N

This document is one of four that hold the plan written on 2026-10-18 and 2026-10-19 (the other three are `FLAT_TREE_PLAN.md`,
`ENGINE_PLAN.md` and `PLAN_REVIEW_LEDGER.md`). It keeps the plan's section numbers, so a reference such as "Part E 40" or
"I4.5" reads the same in every document. The plan was approved to start on 2026-10-19 ("commit and start the plan").

Where the parts went:

| part | document |
|---|---|
| A (the batch after level 9), C, D, E, F, G5, I2 to I5, K, L, N | this document |
| B (item 9: the flat tree), G1 to G4, G6, I1 | `FLAT_TREE_PLAN.md` |
| H (the engine cluster), J (the wall's later designs) | `ENGINE_PLAN.md` |
| M (the workflow review's findings, 267) | `PLAN_REVIEW_LEDGER.md` |

The parts are copied as written in the plan; later edits to a design are made in its document, and the ledger rows and the
stack change through the commit series of Part N. Where a design says it "waits for the user's word", the ruling of 2026-10-18
(Part E, M5) is the answer.

---

## Overview (the plan's opening, as written)

Written in plan mode from the code at `0fb1a4de` (Parts A to F; the other session had the fixes plan's R8 in the tree),
`30aa2002` (G) and `8a1ca1e0` (H to J); the review passes re-read the code at `52844b07` and `ffa7cccb`. Line numbers
are hints from those commits and the tree moves, so a citation is found by the symbol it names; "the fixes plan's R7"
is `ITEMS_1_TO_3_FIXES_PLAN.md` (ledger LOAD-8.7) and "level 9's R7" is `CROSS_PLATFORM_PLAN.md`.
Nothing here has been built or run. Part C (what follows item 9) and Part D (what follows that:
levels 1 to 7) are at the end, and Part E gives the most proper answer for every pending decision,
by number. Where Parts B to D lean otherwise (their first pass weighed churn), Part E governs.
Part F lists what needed designing; Part G holds the designs of its rows 1 to 4 and 6, Part H the engine cluster
(rows 10, 13 to 15), Part I the designs the batches wait for (rows 5, 7, 8, 11, 12) and Part J the wall's later
ones (rows 16 to 18). Part K records the review of 2026-10-18 by task group, and Part L the second pass by ledger row;
where either changed an earlier part, the change is made there. Part M tracks the findings of the workflow review of the
whole plan (267, each checked in the main thread: 265 hold, one holds in part, one cannot be checked here); the confirmed
ones are folded into the parts they touch, M3 says where, and the ones that depart from a ruled decision were put to
the user and ruled on 2026-10-18 (M5; the rulings are folded in too). M5d lists the four answers of the audit of the same
day, which the user ruled on ("agree"), and the two extensions that came with them at the user's word (COMP-7 and COMP-8,
"also (a) and (b)"). Part N lists the documents and the ledger and stack edits the plan implies.

- **Part A** designs the batch that follows level 9: five rows (`WORK_STACK.md`: item 2's last part, item 8's
  REG-14.4, item 9's first step, item 19's LOAD-5.4, and LOAD-9), the composer follow-ups A7 and A8 (Part E 40) and
  REG-14.5, which is folded into REG-7.5.
- **Part B** designs item 9 (SYN-1: the flat tree) as a staged migration. It needs the user's
  ruling on nine decisions first (B2, D1-D9); the three that matter most are D1 (handles and
  views, not bare ids), D2 (staged migration over the boxed tree first) and D3 (what a node
  records of position, which the ledger leaves to a measurement: spike S-12 makes it before step 2).
  D4 keeps the ledger's layout, ruled on 2026-10-18: the tree owns its source, a string is a span,
  and a side buffer holds the bytes that no source has.

Both start only after the active level 9 has finished. The level this plan was written under, the items 1 to 3
fixes, finished on 2026-10-17 (`791a57c6`). The level 9 now in the stack is the cross-platform fixes
(`CROSS_PLATFORM_PLAN.md`, rows R0 to R18, started 2026-10-18; at `c5bec58c` R0 to R15 and R18, the module graph, are
built and R16 and R17 are open; the other session moves this). Its rows edit what this plan's first rows edit: `session.rs`, `project.rs`, `walk.rs` and
`lsp.rs` (R5 to R7), `includes.rs` (R8), `composer` (R3, R9, R16), `main.rs` (R10) and the caches (R11). R18 moves
code this plan reads: `file_id` and `shard` into reflection, the completion rules (`Completion`, `Reach`,
`BlockCompletion`) and `is_truthy_literal` into `syntax`, the rebind and include-guard mention bits into
`checker::mentions` (the project hands the checker the file's whole `mentions`), `set_current_file` taking the shown
path, `path_bytes` into a crate of its own (`phpstan-rs-path-bytes`, which R16 works in) and the allocator into one
module of the binary. Heavy jobs run one at a time, and the stack's rule is that level 8 resumes when level 9
finishes. Start condition: `git log` shows R17 and R18, `WORK_STACK.md` has no level 9, `git status` is clean, `ps`
shows no other build or profile. Every design below is read against that level's code (the case rule of R7, the
canonical form of R3, the `/`-spelled report paths of R6, the module homes of R18) and says where it meets one.

Conventions for every row: `cargo fmt` first, then `cargo check`, `cargo clippy -D warnings`,
the targeted tests, a commit with a pathspec. Comments name no row or phase. Output of long
runs goes to a file under `~/.cache`, then is searched. No saving is claimed without a
measurement.

**Validation is at the boundary the user names:** the end of each top-level item (the 19 items of the 20 G plan and
the stages of the crate split), never at a step or a row inside one, and no checkpoint is added that the user did not
ask for. Where a design below says a comparison "at each step" or "at each commit", the dump or oracle run is produced
at that point (a cheap run that stores output) and compared with the stored baseline at the item's end, and the suites,
fixtures and corpus passes run once there; the per-commit checks are `cargo fmt`, `cargo check`, `cargo clippy` and the
touched module's tests.

Two gates hold every row that moves code, and they are cheap to different degrees. `cargo xtask modgraph` parses the
graph and runs `cargo metadata`, so it runs per commit: a new crate gets its `crate` line and a new top-level module of
the package its `module` line in `gates/module-graph.txt` in the commit that makes it, as does a nested module whose
crate differs from its parent's, such as `registry::purity analyser`; a module that only the library root holds is
rejected. `cargo xtask tests` compiles every test target (the test profile for the workspace, the gate profile for the
benches and the fixtures target) and so runs at the validation boundary; a test that is renamed, merged or deleted is
listed as `old -> new` or `old -> REMOVED: reason` in a `gates/test-renames/*.txt` file in the commit that does it
(the gate reads every commit of the range, so "the same commit" is a convention), or the manifest fails; item 9's test
rewrite and the retired oracles are the large cases. A new crate also joins `LIBRARY_CRATES` in `xtask/src/arch.rs`
(else its no-print rule skips it) and the build-time scenario lists in `xtask/src/main.rs` (an unlisted crate is
unconstrained); the new crates of this plan (`php-version`, `engine`, `stack`) do both in the commit that makes them.
A row that adds an `xtask arch` rule adds it with the code that satisfies it. **CI does not run `cargo xtask gates`:**
the `gates` job of `rust-ci.yml` (Linux only, `ubuntu-24.04`) lists its steps one by one and calls `xtask modgraph`,
`arch` and `tests`, and the other systems are the `platforms` job. So a new `xtask arch` rule reaches CI by itself,
and any other cheap gate (F20's recursion lint) is added in two places, `gate_steps` and a step of the `gates` job. A
heavy tool (the dumps, the baseline, the `oracle` profile run) stays local: it needs the frozen Piwigo checkout and a
quiet machine, and a dump spells paths and diagnostics as its platform does.

---


# Part A. The batch after level 9

| # | row | item | what | where |
|---|---|---|---|---|
| 1 | LOAD-2.2, last part | 2 | the name floor is replaced: a Composer vendor directory is recognised by what Composer wrote in it (ruled 2026-10-18, M5d), and so is third-party code that Composer installed outside it, whose classes become context in every mode (COMP-7); `.git` and `node_modules` stay by name; the project's vendor directory is resolved as Composer does, for discovery, `read_installed` and the include lookup | `project/session.rs`, `project.rs`, `project/walk.rs`, `crates/composer` |
| 2 | LOAD-9 | none (R14's finding) | installed packages resolved once, beside vendor discovery | `crates/composer/src/lib.rs`, `manifest.rs` |
| 3 | REG-14.4 | 8 | `from_decl` passes the parameters on, not a `Vec<&str>` of names | `registry/function_sig.rs`, `function_sig/asserts.rs` |
| 4 | REG-14.5 | 8 | not built on its own: `reconcile_with_native` is replaced by REG-7.5's combination, which is written without the `Vec` (A4, Part E 41) | `crates/types/src/decide.rs` |
| 5 | SYN-1.1 | 9, step 1 | `Expr::peeled` becomes a loop | `crates/syntax/src/ast.rs:1417` |
| 6 | LOAD-5.4 | 19 | no reader gets an empty file: `FileState::ast` goes, so item 9's API needs no empty tree (A5a) | `project.rs`, `project/{architecture,banned,class_shape,composer_hygiene,mock_hygiene}.rs` |
| 7 | the composer follow-up of Part E 40 (a COMP row) | none | the autoload maps keep their keys, the root package's own entries included (COMP-5's data), and gain PSR-0, which STUB-7.5's lazy locator needs (A7); no output moves | `crates/composer/src/manifest.rs`, `vendor.rs` |
| 8 | COMP-6 and COMP-8 | none | the vendor context of every other Composer root: a package's bundled `vendor/` (COMP-6) and, in a degrade root, a nested project's (COMP-8), found by A1's test (A8); a delta, with A1's | `crates/composer/src/lib.rs`, `vendor.rs`, `project/session.rs`, `project/walk.rs` |

Why these (the READY audit): item 2 has only this part open (LOAD-2.1 and 2.3 to 2.8 are in).
Items 3 and 5 have no READY row (LOAD-3.1 needs REG-16.4; 3.5 needs SYN-1.2; LOAD-4.7 needs
PROD-5.7, which needs PROD-5.8 and PROD-5.2 of item 13). Items 4, 6 and 7 hold only crate-split
rows (level 2, deferred). Item 8: REG-14.1 needs PROD-5.8 (item 13), 14.3 needs REG-8.3 (level 2),
14.2 is done, so 14.4 and 14.5 are all that is READY. Item 9: SYN-1.1 is READY and "comes
first, on its own". LOAD-9 is READY and sits in `WORK_STACK.md`'s "Outside the items" line (added by `791a57c6`), in
none of level 8's item lists. Items 11 to 16, 18 and 19 have READY rows but come after 9 in the plan's build order.

A2 needs A1's `vendor_location` (A1's composer commit comes first); A7 edits the same crate and follows A1 and A2; A8
needs A1's test, A2's package list and A7's maps; COMP-7's two commits follow A2, whose shared package list the
admission reads, and the admission comes first, so that no commit leaves a class unknown that the walk still analysed
(a role of analysed wins over the context one); A3, A5 and A5a are independent of them and of each other. Validation,
once, after the last of them (A6). Four answers of the audit were ruled on 2026-10-18 ("agree", M5d) and are built in:
A1's mechanism for the skip, which changes the wording of the fixes plan's decision 7; A1's commit that makes discovery,
`read_installed` and the include lookup follow `config.vendor-dir`, `COMPOSER_VENDOR_DIR` and `COMPOSER` (Part E 40), a
delta; A8, a delta of the same kind; and the PHP range's open upper end (J1.1), which is not Part A's; Part E 35 leaves
three rows unbuilt. The two extensions added with them are A1's third clause and its discovery filter (COMP-7) and A8's
scan (COMP-8); each has a delta commit of its own. A6 runs with all of them.

## A1. LOAD-2.2, last part: the name floor

**Now.** `DEFAULT_SCAN_EXCLUDED_DIR_NAMES = ["vendor", ".git", "node_modules"]`
(`project.rs:3415` at `ffa7cccb`) and `path_under_excluded_dir` (`:3421`, which now takes the walk's `Excluded`, since
level 9's R7) serve three editor paths and the walk:

| use | code | what the names do |
|---|---|---|
| degrade root (no `paths`) | `Session::resolve`: `excluded_dir_names = DEFAULT_…`, carried to `walk_listing` and to each `Root` | the analysed walk of the root skips a directory with one of the names, at any depth |
| a file under no configured root | `unlisted_role` (`session.rs`), via `Session::classify` and `classify_unlisted` (`session.rs:389`, called from `lsp.rs`) | context when any component of the shown path has one of the names |
| a never-seen path in the test default and `incremental-bench` | `Project::update_file_cancellable` (`project.rs`) | the same test decides `analysed`; the editor does not call it, since `lsp.rs` passes the session's role to `update_digested_as` |
| classifying a path the walk did not list | `Root::contains` (`session.rs`) | no directory below the root has an excluded name |

The Piwigo gate sets `paths`, so none of this runs there; the editor is the only user. Three more places name `vendor`
and are not part of the floor, and the first version of this section listed none of them: vendor discovery
(`discover_vendor_context`) and `read_installed`, which hard-code it; R8's include lookup, where `Session::resolve` builds
`trees` and `unbuilt` for each Composer root from `composer_root.join("vendor")`; and the PHPStan-API check
(`phpstan_api.rs`), which counts a PHPStan class as installed when any component of its file's path is `vendor`.

**Decision 7 (the fixes plan's, ruled 2026-10-17; the ledger's decision 7 is another one).** The three editor uses of the
floor "skip Composer's vendor directory by its resolved path, so a `vendor/` outside Composer is walked", and the `.git` and
`node_modules` names stay until `exclude_paths` (LOAD-7.8) exists, when the function and the constant go. **The mechanism
below keeps the decision's rule and adds to it, which changes the decision's wording (ruled on 2026-10-18, M5d:
"agree").** Both behaviours the decision states hold (Composer's vendor directory is left out; a `vendor/` of the project's
own code is walked). The rule is kept for the target that a manifest names beside itself, and a directory that Composer
wrote into is recognised wherever it is, because the path rule alone has a hole that the first version of this section
accepted as "a limit": a nested project whose vendor directory lies outside its own subtree (a shared `../../vendor`, with
a root that has no manifest) is walked as analysed code, thousands of files with diagnostics, where the name rule skipped
it. The path rule alone was the alternative put to the user (M5d).

**Two questions, two answers.** Which directories are Composer's, so that the analysed walk leaves them out and a path
under one is context? And which one is the project's, so that discovery, `read_installed` and the include lookup read it? The
first version answered both with one resolved path. Today nothing answers either well: `discover_vendor_context`
(`composer/src/vendor.rs`) needs `<root>/composer.json` to be a file and `<root>/vendor/composer` a directory,
`config.vendor-dir` is read nowhere (the crate split's B8 follow-up, `CRATE_SPLIT_PLAN.md`; Part E 40), and a project with
`"config": {"vendor-dir": "libs"}` has Composer's vendor directory at `libs`, and one with `"libs/vendor"` at a directory
two levels down, so the name test is wrong for them in both directions.

*Composer's directories are the ones Composer targets or has written into.* A directory is Composer's (`is_composer_dir`,
in the composer crate) in either of two ways, each a function of listings alone. **The target:** its parent holds a
manifest, and its name is the one component that the manifest's vendor directory resolves to (`vendor` by default, or
`libs`; `vendor_location` below), whether or not Composer has built it yet. That is decision 7's rule, evaluated where the
manifest is. It is needed in its own right, because it keeps a fresh `composer install` classified as it is now: the server
registers `**/*.php` watchers, `handle_watched_files_changed` gives each created file the role that `classify` returns, and
the project keeps that role ("`analysed` is its role when the project has not seen it before"), so a rule that waited for
Composer's marks would index the thousands of files an install creates as analysed code, and publish their diagnostics,
until `composer/` appears. **The marks:** it holds a `composer/` directory that holds `installed.json` or
`autoload_real.php`, which finds it at any depth and under any name (`libs/vendor`, a shared `../../vendor`, a build copy
such as `build/vendor`, a stale install left by a changed `vendor-dir`, a nested project's wherever it points). Composer 2
writes `autoload_real.php`, `autoload_static.php` and `ClassLoader.php` at every dump (`AutoloadGenerator::dump`) and
`installed.json` at every install; the 14 vendor trees that are installed (A2's list) and Piwigo have all three, and the six
trees that are not installed have no `installed.json`. The test reads the directory's own listing and `composer/`'s, and
the target reads the parent's (`DirListings::listing`, shared with the walk and with discovery), for the exact names
Composer writes. A `vendor/` of the project's own code has neither a manifest naming it nor marks, and is walked, as
decision 7 asks. The answers are kept beside the listing (a `OnceLock` per listing), so every task, the reaching fold and
the oracle read one fact, and a directory changed mid-load changes nothing in this load. What neither clause covers is a
target that is not a direct child (`libs/vendor`, `../../vendor`) while Composer is still building it: it is walked until
its marks appear. The price of the marks: a test fixture that copies an installed `vendor/composer/` into the project's own
tree is left out too. **On the corpora** (checked 2026-10-18), 28
directories hold `composer/installed.json`: 24 are named `vendor` and 4 `fake_vendor*`. The name rule, the path rule and this
rule agree on 23 of them, each with a manifest beside it. They differ on five test fixtures that copy an installed tree and
have no manifest beside them: the four `fake_vendor*` of Symfony's PhpUnit bridge tests, which only this rule leaves out,
and Laravel's `tests/Foundation/Fixtures/vendor`, which the name rule and this rule leave out and the path rule walks. Only
the Symfony four hold PHP files (16 in all, each with `autoload.php`, `composer/autoload_real.php` and `installed.json`, the
marks that the PhpUnit bridge itself looks for); the other fixtures hold `installed.json` and little else. The path rule's
hole is in none of them, since it comes from a layout the corpora do not have.

*Third-party code that Composer installed outside the vendor directory (COMP-7, added on 2026-10-18 at the user's word).*
Composer does not put every package under `vendor/`. A custom installer (the `composer/installers` plugin, configured by
`extra.installer-paths`, which Drupal, WordPress and others use) installs a package where the project asks (`web/core`,
`web/modules/contrib/token`, `wp-content/plugins/akismet`), and Composer records the directory in `installed.json` as
`install-path`, the shortest path from `vendor/composer` (`FilesystemRepository::dump`: `findShortestPath($repoDir,
$path, true)`; `null` for a metapackage, whose installer returns no path; run through the vendored Composer on
2026-10-18, `/proj/web/core` from `/proj/vendor/composer` gives `../../web/core`, and a directory sharing only `/` gives an
absolute path, which `composer_dir.join` keeps). Neither clause above sees these directories: no
manifest names them as a vendor directory and Composer wrote nothing into them. Today a degrade root walks them as analysed
code (the name rule never matched `web/core`: thousands of files with diagnostics), and every mode misses them as
context, because discovery keeps only what lies under the vendor directory and drops the rest as first-party
(`Resolver::locate`). The third clause is the evidence Composer did leave, the package list of a known Composer root:
**a directory is Composer's when it is the install directory of a package that a known root's `installed.json` lists,
outside that root's vendor directory, and that is not a path repository's.** A path repository's packages
(`ComposerManifest::path_repository_packages`: the install directory lies under a `type: path` repository's directory)
are the project's own code, promoted to analysed paths, and stay so, so that a directory is never both promoted and left
out; a package that a custom installer copied out of a path repository lies outside the repository's directory, and is a
copy of code that is analysed at its source. A metapackage has no install directory and adds none. The **seeds** are those
install directories, canonical (`read_installed` resolves them), kept in one sorted, deduplicated slice that holds only the
ones strictly below the session's root, and they are a function of files read before the walk starts, so no task's
timing reaches them. A directory that matches an `installer-paths` pattern but is not listed is not a seed: the pattern
`web/modules/{$name}` also matches the project's own `web/modules/custom`, so a match proves no package, and the target
clause of the vendor directory has no analogue here. What follows is stated as a limit and tested: the files that a first
`composer require` creates are analysed until the session is resolved again (the server keeps the first role it gave a
created file, and `**/*.php` is all it watches), which H4.2's trigger set ends at W3 (a change to `installed.json`
re-resolves) and a restart does before. **On the corpora** (checked 2026-10-18), the 29 `installed.json` files (the
corpora's 28, fixtures included, and Piwigo's) list 1,329 packages, and one lies outside its vendor directory: Drupal core's own
`drupal/core` at `../../core` (type `drupal-core`, a path repository, so promoted and no seed). No corpus holds an
installer-paths package, so the fixtures carry this clause, written in the shape Composer writes.

*Their classes must be context.* `Resolver::locate` keeps a path that lies under the vendor directory (or is written
inside it, a path repository's link) and drops one that resolves outside as first-party, and a generated file lists a
custom-installed package's autoload directories below `$baseDir` and outside `vendor/`
(`$baseDir . '/web/core/lib/Drupal/Core'`). So they are dropped, and leaving `web/core` out of the analysed walk would
leave its classes unknown, a regression for a degrade root, which has them as analysed code now; a project with
configured `paths` never had them (`class.notFound` for every `Drupal\Core\…` class). The filter therefore admits what
lies under a seed as well as what lies under the vendor directory, in every mode. Discovery needs the package list for it,
so `PackageContext::load` makes the list once, a `OnceLock` shared by the manifest load and the discovery thread (whichever
asks first reads `installed.json`: A2's one read), and the two still run beside each other. The generated cache holds the
raw entries keyed by the file's bytes, so the filter does not touch it. The admitted directories and files join
`VendorContext`'s flat lists, so the context walk lists them with the vendor ones; A7's maps hold them with their keys
either way, and at W3 `locate` finds a class through those keys and the same directories are `FileSet` entries (H4.1), so
this is not work that the engine replaces.

*The project's directory is Composer's resolution,* found once per Composer root by `vendor_location(project_dir)`: the
manifest is a file (`composer.json`, or the file that the `COMPOSER` environment variable names, which Composer's
`Factory::getComposerFile` and PHPStan's `ComposerHelper::getComposerJsonPath` both read; the code joins `composer.json`
in four places today, `composer_roots`, the include trees, `ComposerManifest::load` and discovery, and one `manifest_path`
function replaces them); the candidates, in Composer's order, are the `COMPOSER_VENDOR_DIR` environment variable when it is
set, the manifest's `config.vendor-dir` when it has one, and `vendor`; each is resolved as Composer's
`Config::get`, `Platform::expandPath` and `Config::realpath` do (`{$name}` references to other config values expanded,
trailing `/` and `\` trimmed, a leading `~/`, `$VAR` or `%VAR%` expanded, a value absolute when it starts with a slash, a
drive letter, a scheme and `://`, or two backslashes, which is Composer's test and not `Path::is_absolute`, else joined to
`project_dir`) and made canonical, and equal candidates count once. The answer holds `resolved`, the first candidate, which
is the directory Composer builds and may not exist yet, and `installed`, the first candidate that is a Composer vendor
directory by the test above. `installed` is a later candidate when the first is not built, so that a stray variable, or a
`vendor-dir` changed and not yet reinstalled, loses no context that a project has today. The session reads the environment
once and passes it as a value (it is an input of `Packages`, H2.2), so no query reads the process. The rules were run on
2026-10-18 through the vendored Composer's `Config`, in memory, on eight inputs: `libs` gives `<project>/libs`, `./libs/`
gives `<project>/./libs` (Composer does not normalise; the canonical form does), `../vendor`, an absolute value kept, `~/v`
expanded, the variable beating the manifest, and a relative variable joined to the project directory. PHPStan reads the same
manifest key and no variable and joins `$root . '/' . trim($dir, '/')`
(`ComposerHelper::getVendorDirFromComposerConfig`), which turns an absolute `/abs/v` into `<root>/abs/v`; this follows
Composer, not PHPStan. The `{$name}`, `$VAR` and `%VAR%` forms were not run, and the commit runs them.

**Who calls what.** The walk, `Root::contains`, `unlisted_role`, `classify_unlisted` and `update_file_cancellable` ask only
whether a directory is Composer's (`is_composer_dir`; the target clause reads the parent's manifest, and
`vendor_location`'s first candidate gives its name) or lies under a seed (COMP-7; the session's slice, which
`classify_unlisted` has not). Discovery, `read_installed` (A2) and the include lookup ask
`vendor_location`: discovery and `read_installed` read `installed` (none installed is no vendor context, as now), and
`Session::resolve` puts each Composer root's `installed` among the include lookup's `trees` and, when the root has a
`composer.json` and none is installed, its `resolved` among `unbuilt`, so that a project whose vendor directory is `libs`
and is not installed yet gets no `include.fileNotFound` for `libs/autoload.php`, as `vendor` gets none now. The PHPStan-API
check (`phpstan_api.rs`) asks neither: it is REG-18.6's, and answers by the owning package, not by a path component.

**Every Composer project's, not only the session's.** The session knows the Composer roots that lie *above* the
configured directories (`composer_roots`, `session.rs`), so a rule built on known roots would miss a nested project's
`packages/foo/vendor/`, which the name test skipped. The test above needs no root: the target clause reads the manifest
beside the directory, whichever project it belongs to, and the marks are the directory's own, so both are as exact for a
nested project as for the session's. A promoted root (a path repository's directory) keeps its empty exclusion
as today, so its own `vendor/` is not left out. The seeds (COMP-7) do need roots, since a package list is a file: the known
roots give theirs, and A8's scan finds a degrade root's nested projects before the walk so that their lists feed the
slice too, and the whole slice is final before any task runs.

**Discovery's base directory.** `discover_vendor_context` takes `$baseDir` to be the vendor directory's parent, which is the
project directory only for `vendor/` directly under it. Composer writes `$vendorDir` and `$baseDir` at the head of each
separate generated file as the shortest path code from the generated directory (`Filesystem::findShortestPathCode`:
`dirname(__DIR__)`, `dirname(dirname($vendorDir))` for `libs/vendor`, `$vendorDir . '/x'`, or an absolute literal when the
directories share only `/`, as for a vendor directory elsewhere). `autoload_static.php`, which Composer 2 always writes,
spells every path from `__DIR__` and is unaffected; the older separate files, which discovery reads when the static file is
missing or does not read, need those two assignments evaluated (string literals, `__DIR__`, `$vendorDir`, `dirname(...)`
and `.`), with the vendor directory's parent kept only for a file without such a head. The generated cache keeps the head
with the entries, so its key (the file's bytes) still holds.

**The owning manifest is not A1's.** A file's owner for H2.2's range and the activation (Part E 42) is the nearest ancestor
that holds a `composer.json`, a second fact of the same listing: A8's scan finds a degrade root's nested manifests before
the walk, `PackageIndex` holds them and the walk tags each file with its owner from it (COMP-8), and W2 reads the same
directories into `Packages`; A1 builds the vendor fact only.

**The walk's reach follows from the listing.** The walk counts, for every directory, the walks that reach it (`Reaching`,
made by `Starts::below` from the directory above's, and by `Starts::reaching` from the top for a root or a link
target), because a walk hands a directory's files to the pool while it goes only when it is the one walk of their
strongest role that lists them (LOAD-2.4, LOAD-8.10), and `walk_dir` asserts in every unit test that the carried
reach equals the ancestor-by-ancestor oracle (`reach_by_ancestors`). Both filter walks by `Excluded::excludes(name)`
(`walk.rs`, the calls in `Starts::below` and `Reaching`), a fact of the parent's listing. A Composer directory is known from
its own listing and its parent's, so its reach is made as for any child, and its own task, which has read them, drops the walks
whose `Excluded` has the rule before it decides what to hand over or what reach to give its children; a walk without the
rule (a configured `paths` root, a promoted root) keeps it. Were the parent's count left as it is, it would count a walk
that lists nothing there: safe (it only keeps a file from being handed over), but it loses the overlap for a degrade root's
vendor files and fails the oracle's equality. `reaching(dir)`, the fold from the top for a root or a link target, and
`reach_by_ancestors` make the same cut at an ancestor that is a vendor directory, which they read from the same cached
fact (a configured `paths` root does not apply the rule, so the Piwigo gate never asks), so the oracle compares two
computations of one fact and its equality catches a bug in the carrying and not a disagreement of the filesystem. A link
target gets no reach (`walk_root` computes it only for scopes that are not outside) and `reaching` also serves `Job::Files`;
a link is tested at its target before the target is queued, and a link to a vendor directory is not followed. The covering
roots of `Starts::of` are those that exclude nothing: `Excluded::is_empty()` is no names, no rule and no seeds (COMP-7). This is the part that
the build has to prove: it is argued from `walk_dir` and `Starts::below`, and not built.

**Change.**
1. The composer crate gains `is_composer_dir` (over the directory's and its parent's listings), `vendor_location` and
   `manifest_path`, with the resolution above; `discover_vendor_context` and `read_installed` take `installed` and no longer
   join `vendor` (A2 passes one answer to both), and discovery evaluates the generated files' own `$vendorDir` and
   `$baseDir`. COMP-7 adds `ComposerManifest::installed_elsewhere(vendor)` (the installed packages whose install
   directory lies outside `vendor` and which `path_repository_packages` does not return), and discovery's filter admits what
   lies under their directories (the shared package list is `PackageContext::load`'s, above).
2. `Excluded<'a>` (it exists since level 9's R7: `names` and the root's `case` rule, `walk.rs:97`) gains
   `composer_vendor: bool`, a borrowed handle on the session's environment and a borrowed sorted slice of seeds
   (COMP-7), and stays `Copy`. A walk whose `Excluded` has the flag leaves out a directory that is Composer's, decided by
   the directory's own task (the root of a walk is never tested, as no name is applied to it either). The names are
   applied before a child's path is built, as now, and go through the root's case rule (R7), which is why A1 follows R7;
   the names the test looks for are exact, since Composer wrote them. A seed needs no listing, so the parent's task decides
   it from the child's path alone (a binary search, nothing while the slice is empty, compared through the root's case
   rule as the names are) before the child's `below` is made: the reach counts need no correction for it, `reaching` and
   `reach_by_ancestors` make the same cut at a seed ancestor, and a link whose target is at or below a seed is not queued.
   `Excluded::is_empty()` is no names, no flag and no seeds.
3. `Session::resolve` sets `composer_vendor` and the seeds in the degrade branch only, where the names apply (a
   configured `paths` entry names what is analysed, and a `paths` entry that includes `web/core` analyses it until
   `exclude_paths`, LOAD-7.8; discovery's admission above applies in every mode, since context is not a role that only a
   degrade root grants);
   `DEFAULT_SCAN_EXCLUDED_DIR_NAMES` becomes `[".git", "node_modules"]`; it builds R8's `trees` and `unbuilt` from
   `vendor_location`; and it builds the seeds, before the walk, from the known roots' manifests (A8's nested roots add
   theirs): each `installed_elsewhere` install directory strictly below the session's root, sorted and deduplicated.
4. `Root::contains` applies the three below the root (no directory between the root and the path has an excluded name, is
   Composer's or is a seed). `unlisted_role(path, root)` is the names plus the same test on the directories below `root`,
   and a prefix test against the seeds (each ancestor below `root` by binary search); with no session
   (`classify_unlisted`) every ancestor counts for the names and the Composer test, which also ends today's reading of any
   `vendor` anywhere in an absolute path (`/home/me/vendor/proj/a.php` is context now), and there are no seeds, since no
   package list is known: a file outside every folder that sits in a package installed elsewhere is analysed there, as
   before. The test reads a listing where the load has one and `symlink_metadata` where it has none.
5. `update_file_cancellable` (the test default's and `incremental-bench`'s path, not the editor's) tests the same two
   things for a never-seen path.

No `ProjectSettings` field is needed: the filesystem is asked, and only for a path the walk did not list.

**Behaviour delta.** A `vendor/` below the root that Composer does not target and has not written into (no manifest beside
it that names it, and no `composer/` holding `installed.json` or `autoload_real.php`) is now analysed in the editor; before
it was left out. A Composer vendor directory is left out wherever it is and whatever it is called (`libs/vendor`, a shared
`../../vendor`, a nested project's, a build copy, a stale install, a link to one); before, only a directory named `vendor`
was. A fresh `composer install` with the editor open keeps its files context, as the name rule does. A promoted root keeps
its empty exclusion as today, so its own `vendor/` is not left out. **The delta commit (ruled on 2026-10-18, Part E 40 and
M5d):** a project whose `config.vendor-dir`, `COMPOSER_VENDOR_DIR` or `COMPOSER` names another directory or file gets its
installed packages, its vendor context and its include trees from that directory in the batch run too (it got none, since
discovery looked for `vendor/composer`), which changes its output; that part is a commit of its own, with its fixture.
**COMP-7's two commits:** third-party code installed outside the vendor directory is left out of a degrade root's analysed
walk (before, it was analysed: a Drupal project's `web/core` and contrib modules gave diagnostics in the editor), and its
classes are context in every mode (before, a degrade root had them as analysed code and a configured-`paths` project had
them not at all, so every `Drupal\Core\…` class was a `class.notFound` there). The admission in discovery is the first
commit: it changes the batch output of a project that uses a custom installer, and has its fixture. The walk's cut is the
second and is editor-only (it follows A2 and the admission, as the ordering paragraph says). A path repository's package, the project's own `web/modules/custom` and a contrib
directory that `installed.json` does not list are analysed as before.

**Tests.** Existing tests build a `vendor/` with no Composer files and assert the name rule, so under decision 7 they
lose their premise and are rewritten with an installed `vendor/composer/` (holding `installed.json`):
`lsp/tests/workspace_scan.rs` `vendor_directory_is_excluded_from_the_analyzed_walk`, `lsp/tests/protocol_and_session.rs`
`initialize_does_not_publish_diagnostics_for_files_under_vendor_directory`, the two vendor cases of
`project/tests/file_discovery_and_misc.rs` (`vendor/lib.php` and a never-seen `vendor/acme/Foo.php`) and the two
`classify` cases of `project/tests/session_identity.rs` (`proj/vendor/new.php` and `outside/vendor/y.php`). New: a
`lib/vendor/` outside Composer is analysed, and so is a `lib/vendor/composer/` of the project's own code (no
`installed.json`), and so is a `vendor/` of the project's own code beside a manifest whose `vendor-dir` is `libs`; files
created under `<project>/vendor/` and under a configured `libs/` before any `composer/` exists (an install in progress) are
context, as are those under a nested project's `vendor/`; a nested project's `packages/a/vendor/` is left out, with a
manifest beside it and without one; a shared
vendor directory outside every nested subtree (`packages/*/composer.json` naming `../../vendor`, the root having no
manifest) is left out; a build copy `build/vendor/` and a stale install beside a `libs/` are left out; a link to a Composer
vendor directory is left out (R13's `link()` helper); `.git` and `node_modules` still are; the same directory under another
letter case on a folding volume (the macOS and Windows legs). Resolution, in `phpstan-rs-composer`: the cases are run through
`php` on the vendored Composer's `Config` and checked in as data, namely `libs`, `libs/vendor`, `./libs/`, `../vendor`, an
absolute path, `~/v`, `COMPOSER_VENDOR_DIR` absolute over the manifest's and relative, a `{$config.x}` reference, `$VAR` and
`%VAR%`; a project with each finds its installed directory and discovery reads its installed packages; a project whose
variable names an uninstalled directory keeps the `vendor/` it has; separate generated files for `vendor/`, `libs/vendor`, a
vendor directory elsewhere and an absolute path, written once by `composer dump-autoload` on a throwaway project and checked
in as data, give the same entries as their `autoload_static.php` does. Include lookup: a `libs` project that is not
installed has no `include.fileNotFound` for `libs/autoload.php`, and an installed one has `libs` among the trees.
**COMP-7**, in `phpstan-rs-composer`: `installed_elsewhere` over an `installed.json` written in the shape Composer writes
(`../../web/core`, `../../web/modules/contrib/token`, a metapackage with `"install-path": null`, a path repository's
package under `packages/`, one whose install path is the root's own directory and one outside the root), and discovery's
admission, which gives a Drupal-style project's classmap and PSR-4 entries below `$baseDir . '/web/core'` as context and
still drops the project's own `src/`. Walk and session, on one tree (`composer.json` with `extra.installer-paths` and a
path repository, an installed `vendor/composer/`, `web/core`, `web/modules/contrib/token`, an unlisted
`web/modules/contrib/stale`, `web/modules/custom/mine` and `packages/shared`): the degrade walk lists `mine`, `stale` and
`shared` as analysed and none of `web/core` or `token`, with the carried reach equal to the oracle's at every directory and
the same list on one thread and on eight; `Root::contains` and `unlisted_role` agree with the walk for a created file under
each, and `classify_unlisted` calls them analysed; a file created under a contrib directory that `installed.json` does not
list yet is analysed (the stated limit); a link to `web/core` is not followed; the seed spelled in another letter
case on a folding volume (the macOS and Windows legs); and a project with configured `paths = ["web/modules/custom"]`
knows the `Drupal\Core\…` classes through discovery.
Session/project tests: `unlisted_role`, `classify_unlisted` and `update_file` of a never-seen path agree for Composer's
vendor, a nested project's, `lib/vendor/` and `.git`. `walk_identity.rs`: a degrade root over a Composer project, and over
a monorepo with a nested project, hands over the vendor files from the context walk only and none twice, with the carried
reach equal to the oracle's at every directory (the existing assertion), for a vendor directory named `vendor`, named
`libs` and named `libs/vendor`, and for two overlapping walks (a degrade root and a configured root over the same tree) so
that the per-walk application of the rule is exercised. `cargo test -p phpstan-rs --lib -- lsp::tests::workspace_scan
project::` and `cargo test -p phpstan-rs-composer`.

**Measure.** None for time (editor-only, behavioural; the test adds one binary search per listed directory and a read of
`composer/` for the few directories that hold one, and the seeds one more search per child directory while the slice is not
empty). **Real data:** the files the degrade walk lists, with their roles, over
Piwigo's root and over each of the 14 vendor trees that carry a `vendor/composer/installed.json`, are compared before and
after on a worktree at the commit before this row, and differ only where a `vendor/` that is not Composer's, or a Composer
vendor directory that is not named `vendor`, exists; A6 runs it.

## A2. LOAD-9: installed packages resolved once

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
   `HashedPath::child` (level 9's R3), since a name such as `aux` or `nul` makes `canonicalize` return a verbatim
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


# Part C. What follows item 9

Read from the 20 G plan's items 10 to 19 (its §10), the ledger's rows, needs and notes
(`PROFILE_DRILL_PIWIGO_LEDGER.md` §2 to §13) and the code at `0fb1a4de`. The savings are the 20 G
report's estimates for the off run (the ledger's "now" is 20.37 G at `8a1ca1e0`; 20.57 G is the 20 G profile's
`cl-20g`, file cache off with a warm stub cache); PROD-1 to PROD-3 and DOC-1 overtook parts of
items 13 and 14, so none is a claim. Validation is at the end of each item, as before.

## C1. The path, in order (the ruled order of Part E's "The sequence this gives"; the second pass made this table agree)

1. The active level 9 (the cross-platform fixes; R0 to R15 and R18 are built at `c5bec58c`, R16 and R17 are open); then Part A's rows
   (A1 to A3, A5, A5a, A7 and A8; A4 is folded into REG-7.5).
2. Batch 0 (Part E 13 and 22: the oracle tools and the baseline, F10's tests, F20, F21, F77, the level-3 backlog that
   changes a dump, and the seven fixes the crate split's F1 puts first), then item 9: steps 0, 1, 2, then step 3
   (LOAD-3.5, the scratch builder, which is item 3's row). Steps 4 and 5 (SYN-1.9, SYN-1.10) come later (batch 3, and
   C4).
3. The batches below.

| # | batch | rows | estimate | why here |
|---|---|---|---|---|
| 1 | tokens | item 10: SYN-2 (I2), with the three rows its token table already uses: SYN-4.1 (the watch table), SYN-4.8 (`QualifiedName`) and SYN-4.9 (digit separators), which have no needs | ≈1.7 G | the largest single row; needs item 9; designed with the tree (the parser's `take` calls the builder's `name` and `text_span`); item 9's baseline serves it; the three rows go here so the lexer and the skim are not written over `Identifier Backslash Identifier` and again over one token |
| 2 | the lexer, second pass | items 11 and 12: SYN-3.1-3.4 (I3 for 3.2), SYN-4.2-4.10 less 4.5, 4.8 and 4.9 | ≈0.2 G + ≈0.35 G | same files as batch 1; SYN-4.1 shrank (the lexer interns nothing: I2) and is in batch 1; 4.3 needs SYN-3.2, 4.7 needs 4.2 |
| 3 | parse recovery (in: Part E 11; changes output) | SYN-1.9, Q-10's parser half, SYN-6 (G3); the `php-version` crate with `VersionSet` and `Availability` (Part E 32); F77's diagnostics now come from the tree's `errors()` | none (VAL-2.10 deltas) | after batch 1, so the parse functions are rewritten once; Q-10's version half needs STUB-7.1 (level 2) |
| 4 | names | S-1 first; item 16 (NAME-2.1-2.3), **then** item 15 (NAME-1.1-1.6), and SYN-4.5 (`ConstName`) | ≈0.1 G + ≈0.6 G; ≈15 MB | names first (Part E 12): products and PHPDoc work are born on `FoldedName` and `ConstName`; NAME-1.4 needs NAME-2.1, so the plan's number order has the needing item first |
| 4b | the constant lookup | REG-18.4 with CHECK-4.2 (needs `ConstName`; a delta, approved 2026-10-18: M5b) | CHECK-4.2 is 5.1 M Ir | one place for the two rows: after batch 4, before batch 5 (Part E 41 folded CHECK-4.2 into REG-18.4) |
| 5 | a file's products | item 13: PROD-5.8, 5.2, 5.1, 5.3, 5.5, 5.6 (PROD-5.7 is not built: Part E 35), and REG-14.1 | ≈0.45 G | three returns to earlier items open here (C3); PROD-5.4 is REG-16.4's to delete; PROD-5.8's record is keyed by `FoldedName` and `ConstName` (batch 4's types), and `RefKey` goes with the dependents machinery in W3 |
| 6 | PHPDoc | item 14: DOC-2.1 first (F41: Part E 14), then 2.2 to 2.9, with REG-4.10 folded into 2.9; TYPE-5 | ≈0.35 G; the memos' memory | 2.2 needs PROD-5.2 (batch 5), 2.5 needs NAME-1 (batch 4), 2.6 needs 2.1 and 2.3, 2.9 needs the comment's offset (SYN-3.4, batch 2); item 14 closes |
| 7 | the check's temporaries | item 18's temporaries (the `Message` of I5, CHECK-6.4 to 6.6, 6.9 to 6.12, 6.16, 6.17) and the scratch reuse of CHECK-6.8 and 6.18 (allocations that hold no tree handle) | the temporaries' share of ≈1.6-2.0 G | the inference core and the scopes group are Stage D's (Part E 16): their ≈1.0-1.3 G and ≈0.15-0.25 G arrive with it; F69 and CHECK-9 are D2's (Part E 17), CHECK-4.2 is batch 4b's, and the one-checker-per-worker form of CHECK-6.18 is E7's |
| 8 | nothing is left that is not the wall's | LOAD-5.4 moved to Part A (A5a); CACHE-13.1 is W3's, built once as `File.stamp` (Part E 15); CACHE-8 waits for S-4 (Part E 41); LOAD-5.1's six passes' per-file facts are W3's `file_products` fields, and E1 moves their collection onto D6's API (Part E 39); LOAD-5.2 and LOAD-3.1 are W3's (LOAD-5.2 needs both LOAD-5.1 and LOAD-3.1) | – | the decision point of C4 comes after batch 7 |
| 9 | decision point | – | – | level 8's READY list is empty; what is left is levels 1 to 7 (C4, and Part D) |

The table's per-batch estimates add to about 3.75 G before batch 7's share (1.7 + 0.2 + 0.35 + 0.1 + 0.6 + 0.45 +
0.35); the 20 G plan's own sum for items 10 to 18, about 5.5 G (tokens and the check 3.5 G of it), counted the
inference core and PROD-5.7, which Part E 16 and 35 moved or removed. Item 9's adds the rest of its 0.8-1.2 G,
itself in doubt (B8).

## C2. Notes by batch

**1. Tokens (item 10).** `TokenStream { kinds, spans, extras, docs, parts }` (I2: a token is a kind and a span, and
its text is read at the span), three cursors, per-worker buffers, interpolation fragments from a stack of scratch
streams; `Token`, `Tok`, `Supply`, `next_raw`/`next_tok`, the `VecDeque` and `fill`/`pop` go. Readers outside
`syntax`: `stub_attributes.rs` (`parse_native_type`), `output/codeframe.rs`, `lsp/completion.rs`, and the
`fuzz` targets `lex_parse` and `token_stream_parse`. Oracle: item 9's dump, which includes the
token list. Decision 4 has the token arrays designed with the tree; the only thing batch 1 needs
from item 9 is `b.name(&src[span])` and `b.text_span(span)`, which the parser calls at take time (the lexer never touches the
builder: I2) and B5 provides. The batch includes SYN-4.1's watch table, SYN-4.8 (`QualifiedName`) and SYN-4.9 (digit
separators), which I2's table already uses and no need holds back; the watch bits are set per segment of a qualified
name (`FileFacts::mentions` is set for "an identifier, a whole name or one segment of a qualified one"), and the body
skim has a stated reading for a `QualifiedName`, a leading backslash and a `namespace\` prefix, each with a test.

**2. Lexer, second pass (items 11 and 12).**
- Item 11's SYN-3.4 changes the *meaning* of two fields item 9 lays out (`doc_line` becomes the
  doc's offset, `removal_span` becomes the member's bounds); the sizes are equal, so item 9 needs
  no change and either order works.
- SYN-3.3 (the server indexes context files on demand while the loaded hash still matches) is
  READY and is LSP-only. The on-demand index is needed regardless of the engine: SYN-3.1 gives a context file no
  column table, so the server's navigation into one needs it. Its *hash check* is a stopgap: on the engine a context
  file keeps no text (H2.3), and the check becomes a comparison of `File.stamp` (the `stat` tuple on the file systems
  where a change time is trusted, the content hash elsewhere), replaced at W3; keep the check small.
- Item 12's rows are independent micro-rows except 4.3 and 4.7 (their needs are in the table); 4.1, 4.8 and 4.9 are in
  batch 1 (note 1). SYN-4.8 is NAME-1's round trip for qualified names. SYN-4.1 shrinks: its first half, the
  identifier lookup inlined into the lexer's arm with the scratch table, is gone, because the lexer interns nothing
  (I2: the parser's `take` interns); its perfect-hash watch list stays, built by a `const fn` in the caller from the
  lists it composes (level 9's R18 makes `WATCHED` the join of three layers' lists, `stub_attributes`,
  `checker::mentions` and `file_declarations`, so `syntax` cannot see the entries when it compiles, and a build script
  there could not either). SYN-4.5 (`RefKey::Constant` keeps namespace and name apart) is `ConstName`, which NAME-1.2
  introduces, and it is the `Constant` kind of PROD-5.8's record (`RefKey` itself goes with the dependents machinery
  in W3), so it is built in batch 4 and not as a stand-in in batch 2.
- Oracle: item 9's dump (positions, fix spans, ignore marks, anonymous-class names), the line-index
  tests over multi-byte and astral lines, the LSP tests.

**3. Parse recovery (in: Part E 11).** SYN-1.9 turns `parse` into "a tree and its errors" with error
nodes (their variants arrive here, not in item 9's step 1: Part E 9, G3.3); Q-10 is half built (a context file that
fails to parse keeps what it declares) and its version half needs the range from STUB-7.1; SYN-6
then fixes the parser gaps Q-2 probed (`$s{0}`, `(real)`, `(unset)`, `(void)` as an expression, a
stray control byte, `function exit`). Its output changes are VAL-2.10's.

**4. Names (items 16, 15).** NAME-2.1 changes `Name`'s representation to one allocation (a thin
dynamically sized value, which a crate such as `triomphe` provides, judged on merit; our own code
stays without `unsafe`). NAME-1.2 is the largest churn of the batch: every `String`-keyed map and every
identifier held as text, by cost order, ending with `TableKey for str` and `FoldKey for str`
deleted (NAME-1.3) so the compiler lists the rest; its registry entry points meet REG-4.2's
accessors and REG-8.6 (level 2). Its container-field keys are an approved delta (VAL-2.7). The machinery that
W2 and W3 delete (`ChangedNames`, `reset_scoped`, `closure_scope`'s frontier sets, the dependents indexes) holds
`String` keys too. Its keys are converted here, because they are a type change and NAME-1.3 cannot delete
`FoldKey for str` while a caller holds text; what is not built is a new structure for it (Part E 35). Oracle:
O2, `--dump-types`, `--dump-expr-types` and `registry-dump` (the last two are built in batch 0: Part E 13).

**5. Products (item 13).** Order inside: PROD-5.8 (the record of written names, also what
REG-14.1 needs; it extends the parser's `Recorder` but is keyed by `FoldedName` (class-likes, functions) and
`ConstName` (constants), batch 4's types, and not by `RefKey`, which the engine deletes with the dependents machinery)
and PROD-5.2 (`ImportScope`s) first; PROD-5.1 (the declaration vectors; item 9
already reserves declaration ids on entry, B3); PROD-5.3, 5.5, 5.6 (one walk, bodies as a sorted
slice, scratch builders); PROD-5.7 is not built (Part E 35). PROD-5.5's slice is keyed by the
body's ordinal (H1.4's `occurrence` and the member's ordinal), not by its span (H1.5a): an edit above a body then
leaves every other entry equal. PROD-1 to PROD-3 delivered most of this item's premise, so its Ir is
smaller than the report's; its memory and what it unlocks are the case for it. Oracle: item 9's
dump, `registry-dump` (built in batch 0), O2.

**6. PHPDoc (item 14).** DOC-2.1 comes first, as F41 (Part E 14, I4): the grammar's lowering replaces the text
grammar, and `DocType` is eager. Then the memo work (DOC-2.2, 2.4, 2.8), tag effects (2.3), borrowed tokens (2.5),
one vector of nodes per docblock (2.7), one product per doc text (2.6) and doc blocks as a per-file product found
by the comment's offset (2.9). DOC-2.9 needs REG-4.10 (the builder parses each doc comment once), and the two are one
change: the builder and the checker read a doc comment's block from the file's doc blocks, which is how the builder
comes to parse it once. So REG-4.10 is folded into DOC-2.9 and does not wait for level 2, and item 14 closes whole.
The statement `@var` memo overlaps DOC-2.2, so it is taken after 2.2. TYPE-5 (F14) follows F41 in this batch, as its
own commit after I4.5 step 5: it is the ledger's rule (the array member takes `K` as `array<K, V>` does), built once
templates and aliases are marked, and F14's design text, which restricts `K` and drops the array member, is
superseded (I4.5 step 6).
Before 2.9 the blocks live where `FileState` keeps a file's products; the offsets, the sharing and the checker's
reads are what survive into `file_products.doc_blocks` (H2.3).

**7. The check (item 18).** The *inference core* (CHECK-6.1 observers and one inference per node, 6.2, 6.7, 6.15) and
the *scopes group* (CHECK-6.3 variable slots and persistent radix arrays, then 6.13 and 6.14) are Stage D's
(Part E 16, J3): they are the structure of the crate split's D1 to D4, and building them now would write the
traversal twice. What this batch builds is exact, local and survives either way: the *temporaries* 6.4 (which bumps
a format), 6.5, 6.6, 6.9, 6.10 with CHECK-4.1 (the `Message` of I5), 6.11, 6.12, 6.16 and 6.17, and the scratch reuse
of CHECK-6.8 and 6.18. CHECK-4.2 goes with REG-18.4 in batch 4b (Part E 41). F69 and CHECK-9 are D2's: F69 records
implicit calls' throw points on the recorders (`record_call_throw_point`, `current_function_throw_points`) that D2's
`StatementResult` replaces, so by the crate split's Appendix F ("a bug in code a later phase rewrites gets a fixture
now and its fix as that phase's behaviour commit") and this plan's own rule (K2) they get their fixtures in batch 0
and are built in D2, and CHECK-9 follows F69 there; CHECK-7's checker half goes to D2 and CHECK-10's operator
sites to D3 for the same reason (the answer that departs from the first pass, M5). TYPE-5 is batch 6's.
CHECK-6.8 and 6.18 reuse allocations that hold no tree reference (the infer cache's slots, scratch vectors of types
and keys, message buffers) across functions and files; a table that holds a rooted expression (G1's alias maps) is
cleared at each file and its allocation not kept, so that it does not keep the previous file's tree alive. That is
all of 6.8 and 6.18 that batch 7 builds and it is final: the ledger's "one checker per worker, reset for each file"
is buildable under G1 (a `Checker` takes no tree lifetime), but its reset clears the per-file, per-function and
per-pass field sets that E7 enumerates, so it is E7's (J3).

**8. Nothing is left that is not the wall's.** The batch kept item 19 and the first half of item 17, and the
pass over them found each already belongs elsewhere. LOAD-5.4 (no reader gets an empty file: `FileState::ast` goes)
is READY with no need and removes the empty tree that item 9's API would otherwise carry, so it is Part A's A5a.
LOAD-5.1 turns each project-wide pass into per-file facts (architecture, banned constructs, class shapes, Composer
hygiene, mock hygiene, unmatched ignores): each pass needs CHECK-6.1 or PROD-5.3, class shapes and unmatched ignores
need PROD-5.1, and the use-site index as a per-file product needs REG-16.4; W3 builds the six passes' per-file facts
as `file_products` fields (H2.3 and H4.4 already read them, and the persisted whole-program queries need them), and E1
moves their collection onto D6's Collector API and adds its semantic fixes over those fields, so the facts' data
is built once (Part E 39). LOAD-5.2 (free each analysed tree after its check, the memory win) needs both LOAD-5.1
(the facts, so no pass reads a tree after its check) and LOAD-3.1, which needs REG-16.4 (Part E 28), so it is W3's and
follows S-7's verdict (H2.9). CACHE-13.1 (entries checked by `stat`) is the source of `File.stamp`, and MEAS-1 puts what it
replaces at ≈60-70 M Ir of reads plus their system calls, with the per-file decode (≈1,260 M) untouched: a
standalone index file would be deleted the day the persisted database holds the stamps, so it is built once, as the
stamp, at W3 (H4.7 step 5, Part E 15). CACHE-8 is READY and independent: `Writer::finish`
(`registry/container.rs`) builds the cache file in a second `Vec` beside the body, so the cold run holds both at
its peak (−15.3 MB there and −31.4 MB allocated, per the heap report's H1). The writer serves the stub caches
(`stub-base`, `stub-base-phpstan`, `stub-data`, `stub-index`), which go with STUB-11. A memory row: no instruction
saving is claimed, and nothing changes on disk. It is built once the persistence's format is chosen (S-4, Part E
41): the writer that survives is the one worth making right.

## C3. Returns to earlier items (each is unblocked inside a later batch)

| row | item | opens when |
|---|---|---|
| LOAD-3.5 | 3 | SYN-1.2 (item 9, step 2) |
| LOAD-4.7, file-references half | 5 | PROD-5.7, which is not built (Part E 35); its reverse-index half goes with REG-16.4 |
| REG-14.1 | 8 | PROD-5.8 (batch 5); 14.3 still needs REG-8.3 |
| DOC-2.2 | 14 | PROD-5.2 (batch 5) |
| DOC-2.5 | 14 | NAME-1 (batch 4) |
| NAME-1.4 | 15 | NAME-2.1 (batch 4, first half) |
| CHECK-4.1 | – | CHECK-6.10 (batch 7) |
| CHECK-6.7, 6.15 | 18 | CHECK-6.1 (Stage D) |
| CHECK-6.13, 6.14 | 18 | CHECK-6.3 (Stage D) |
| LOAD-5.1 | 19 | PROD-5.3 and PROD-5.1 per pass; W3 builds the facts, E1 the collection (Part E 39) |
| SYN-6 | – | SYN-1.9 and Q-10's parser half (batch 3; its version half waits for STUB-7.1) |
| SYN-1.10 | 9 | PROD-5.1 and STUB-7.3 (level 2) |

## C4. The wall: what needs levels 1 to 7

After batches 1 to 8, the 20 G plan's remaining rows all sit behind the crate split's C4/C5 and the
engine. They are: LOAD-3.1 and 3.3's `update_file` half; LOAD-4.7's index half; LOAD-5.2 and the
use-site index; item 4 (REG-4.15, 7.4, 8.3); item 6 (STUB-8, CACHE-9/10, REG-7.2); item 7
(TYPE-4); item 17's CACHE-13.2; NAME-2.4; PROD-5.4; and SYN-1.10 (DOC-2.6 and 2.9 are batch 6's; item 14
closes whole). The final
validation (VAL-1) follows item 19 and is blocked by nothing but its own tooling (C5, 4). The
roots of that chain are READY today: **REG-4.2** (exact-spelling accessors; folded
method keys) and **REG-16.2** (interned identity), then REG-16.3 (the engine), which gates REG-7,
TYPE-4, REG-16.4 and so LOAD-3.1, CACHE-13.2, LOAD-5.2 and REG-17. Levels 1 to 7 were deferred on
2026-10-16 "to be reevaluated together"; level 8's READY list runs out at batch 8, so that is when.
A second session could design REG-16.2/16.3 while batches 5 to 7 are built, as this one is designing
now. Part D lays out what the wall holds and the order I would propose.

## C5. Rulings and risks (the questions; Part E has the current answers)

1. **SYN-3.2's SIMD** (batch 2). It specifies SSE2 16-byte blocks and AVX2 32-byte blocks behind
   `is_x86_feature_detected!`. The workspace denies `unsafe_code`, nothing in it uses `core::arch`,
   and `UNSAFE_PLAN.md` removes `unsafe` from everything but the PCRE FFI. Options: SSE2 through a
   safe crate such as `wide` (baseline x86-64, no dispatch) and a `u64` word loop elsewhere, no
   AVX2; or a small audited exemption like the PCRE one. The saving falls without AVX2 and has to
   be measured (`LineIndex::new` was 270.4 M over 19,211 lexers in the 20 G profile: 63.5 M of per-line `memchr`
   calls, 83.4 M of column tables, 116.3 M of its own, and 7.2 M unattributed; the ledger's later 363.4 M is over
   25,753 builds, stub files included).
2. **Item 14 closes whole** (Part E 14 undefers DOC-2.1 as F41, and REG-4.10 is folded into DOC-2.9: note 6).
3. **Order inside batches 4 to 6** (13, 16, 15, 14 against 13, 14, 15, 16): the first leaves no item
   half done and needs no return to a finished item, at the price of leaving the plan's numbering.
4. **Oracles for items 13 to 16.** Their lines name `registry-dump`, `--dump-expr-types` and the
   PHPDoc resolution record, which are VAL-1.4, 1.6, 1.7 and do not exist (`--dump-types` and O2 do).
   VAL-1 "is not a precondition for any row", so the batch can go on Piwigo's output, O2 and
   `--dump-types`; building the three tools first (4 to 6 commits) makes "exact" checkable beyond
   what Piwigo exercises. (Answered in Part E 13: they are built in batch 0.)
5. **Item 18's groups**: confirm the split and that the inference core is validated by reading its
   diff by family. The inference core and the scopes group also meet the crate split's Stage D
   (Part D, "What bears on the order", 2), which decides whether they are built in batch 7.
6. **CACHE-13.1 for context files early** (batch 8, or sooner): a warm-run wall lever, independent of
   the engine. (Answered in Part E 15: it is built once, as `File.stamp`, at W3.)
7. **When to reevaluate levels 1 to 7** (C4), and whether the engine's roots (REG-4.2, REG-16.2)
   start as design work before then. Part D has the proposed order and its rulings.

---


# Part D. After batch 8: the wall (levels 1 to 7)

Read from `WORK_STACK.md` (levels 1 to 7), the ledger's REG, STUB, CACHE, CHECK, TYPE, NAME, VAL and Q
sections, `PROFILE_DRILL_PIWIGO_20G.md` §12, and `CRATE_SPLIT_PLAN.md`'s stages C to G (the plan was
last edited on 2026-10-18, `c1a7e224`, for the module graph; its stage text dates from 2026-09-28). Nothing here is
built or measured. Figures are the ledger's and the
reports', and "none" means the ledger gives no instruction figure. Steps are W1 to W8 (W for wall),
so they do not collide with Part B's D1-D9 or the crate split's phases D1-D6.

## What is left when batch 8 is done

Level 8's READY list is empty, so the stack's rule ("work the lowest item that has a READY row")
picks nothing. Level 2 still has READY rows (REG-2, REG-4.2, 4.5, 4.7, 4.9, 4.10, 4.12 to 4.14,
REG-5, REG-6, REG-16.2 and the Q-11 follow-ups), but levels 1 to 7 were deferred on 2026-10-16 "to
be reevaluated together". The 20 G plan's unbuilt rows, and what they are worth:

| remaining | needs | worth |
|---|---|---|
| REG-7, REG-8: the finishing passes become queries | REG-4.2, REG-16.3 | **wall time**: the passes run 1.16-1.52 s of the 2.02 s off run and 0.75-1.19 s of the 1.70 s warm run, at 2 to 3 of 8 cores (MEAS-4, the runs under `perf record`; the ledger's medians are 1.796 s off at `a5a4d85f` and 1.322 s warm at `6705eedd`). Ir: none; the passes cost 0.97 G, plus ≈0.7 G that other threads steal |
| CACHE-13.2: the engine's results persisted | REG-16.4 | the warm run's parse of the analysed files (1,967.9 M), their check (6,414.6 M) and the per-file decode (≈1,260 M), of 13.18 G, where a file's recorded reads did not change (MEAS-1): the ceiling, for a re-run that changed nothing |
| STUB-11: the builtin knowledge built at compile time | STUB-10.1, REG-15 | the stub build is 3.18 G of the 20.93 G off run at `791d29cb` (20.37 G at `8a1ca1e0`); the report's estimate after C4 and C5 is ≈0.2-1.5 G per cold run |
| item 6: REG-7.2, STUB-8, CACHE-9/10 | REG-7.1, STUB-7.1, 7.6, the persistence's format | ≈0.35 G and ≈40 MB; the warm run decodes 87,146 stub function records (682.5 M) |
| item 7: TYPE-4, then NAME-2.4 | REG-16.3 | up to ≈0.3 G (`is_super_type_of` 422.5 M) |
| LOAD-3.1, LOAD-5.1's use-site index, LOAD-5.2 | REG-16.4 | memory: the analysed trees (151 MB in MEAS-2; 173 MB at `5fb858c9`) freed as each check ends |
| REG-17, CHECK-7, CHECK-8, CHECK-10 | REG-16.4; REG-7.6; REG-7.5; STUB-7.1 | semantic, not savings; REG-17 adds work |
| tidying: PROD-5.4, SYN-1.10, REG-14.3, LOAD-4.7's index half | STUB-7.3; REG-8.3; REG-16.4 | deletions behind the rows above |

## What the wall contains

| part | rows | state |
|---|---|---|
| C4, the registry | REG-2 to REG-12, REG-16, REG-18 | tracked. Done: REG-1, 3, 4.1, 4.3, 4.4, 4.6, 4.8, 14.2, 16.1 |
| C5, the stub layer | STUB-7, 8, 10 to 13; CACHE-9 to 11 | tracked; nothing built |
| C1 to C3 follow-ups (Q-11) | LOAD-7.x, REG-18.x, CHECK-11 | tracked; READY: LOAD-7.1, 7.3 to 7.7, REG-18.1 to 18.6, CHECK-11 |
| C6 to C9: overlays, `TypeNodeResolver`, extension points, summaries | none | **not tracked** (REG-15.1 says so); about 300 lines of the plan |
| C10 | REG-15 | tracked; needs C6 to C9 |
| Stages D to G: the engine, everything out of it, the Session and the LSP, the extraction wave | none | **not tracked**; about 5,000 lines of the plan (D 1,626, E 2,074, F 913, G 425) |
| level 3 | 136 open review tasks (`STAGE_B_REVIEW.md` 16, `STAGE_B_FIX_REVIEW.md` 105, `STAGE_C_REVIEW.md` 15) | runnable now: its pause reason, level 4's slow suites, is over |
| levels 1, 4, 5, 6 | the roadmap lanes (paused for the split), the gate-speed leftovers, one rename carry-over, the dropped master ledger | parked |

## Proposed order

First pass. Part E and the review change it in five places: W1 is a correctness backlog and partly runs before
item 9 (Part E 22); W2 lands REG-4.15 with level 2 (H3.6); W3 includes Stage F1 and F2 (19), the six passes' per-file
facts (39) and STUB-7.5, the lazy Composer locator, which leaves W4 (H4.1); W5 builds C7 on F41's lowering (14);
W7 includes item 18's reconciled inference core and scopes (16), F69 with CHECK-9 and CHECK-7's checker half (D2) and
CHECK-10's operator sites (D3).

| step | what | rows, in order | notes |
|---|---|---|---|
| W1 | the correctness backlog and the follow-ups that need no engine | the Q-11 follow-ups LOAD-7.1, 7.3-7.7, REG-18.1, 18.5, CHECK-11 (REG-18.4 is batch 4b's: it needs `ConstName`); REG-2 (the gates' inventories); the data-model rows the ledger lists READY with no need: REG-4.5, 4.7, 4.13, 4.14, REG-5, REG-6 (REG-4.9 is REG-4.12's change and is built with it, W2 step 1; REG-4.10 is folded into DOC-2.9, batch 6); level 3's findings | placed in the sequence, not left "between heavy runs": every finding or follow-up that changes a dump goes before the baseline in batch 0, the rest between heavy runs, and a later behaviour-changing commit runs `dumps-compare`, lists its differences and rolls the baseline forward (Part E 22); none depends on the engine; REG-18.2, 18.3 and 18.6 are W2's needs; LOAD-7.6, REG-18.4 and CHECK-11 are deltas the ledger marks READY with no approval in VAL-2.9 or decision 14, approved by the user on 2026-10-18 with the conditions of Part E 42 (LOAD-7.6 is W1's except that its range per owning manifest is designed with W2), and LOAD-7.2's PHPUnit half is ruled: ungated; LOAD-7.5 and the Q-11 table cite `lsp/workspace_config.rs:66` and `:214`, a file LOAD-1.3 deleted: the LSP's lookup is `root.join("phpstan-rs.toml")` in `lsp.rs` (found by that symbol), to correct in the row when it is worked |
| W2 | C4's spine, the engine's roots first | REG-4.2 and REG-4.12, then REG-16.2, REG-16.3, REG-7.1 to 7.12 (7.6 needs REG-18.2 and 18.3; REG-4.15 lands with level 2: H3.6 step 5), REG-8.1 to 8.7 (8.4 needs REG-18.6), REG-9, REG-10, REG-4.11, REG-11, REG-12 | each commit runs `cargo check`, clippy and the touched modules' tests, and the full gate tier, `registry-dump` and O2 run once at W2's stage checkpoint (the crate split's rule: validation at the stage checkpoints you named, not per level or per commit); most rows are approved deltas (VAL-2.9); REG-4.2 and 4.12 meet NAME-1.2's registry entry points (batch 4), so they are written on the `Name`-keyed API |
| W3 | what the engine opens: the prizes | REG-16.4, then CACHE-13.1 (the stamp, for every file) and CACHE-13.2, STUB-7.5 (the lazy Composer locator, H4.1), LOAD-3.1 (and 3.3's `update_file` half), LOAD-5.1 (the six passes' per-file facts as `file_products` fields, then the use-site index), LOAD-5.2; LOAD-4.7's index half; TYPE-4.1 to 4.4, NAME-2.4; REG-16.5; REG-14.3; CHECK-8 (CHECK-7's checker half is D2's, W7); REG-17 | REG-16.4's listed needs (REG-16.3, SYN-1.2, LOAD-1.4) stop short of REG-7 and REG-8, but LOAD-4.6's text has REG-7 and REG-16.4 together replace `closure_scope` (REG-7 the passes that use it). H3.5 and H4 settle it: REG-8's rows are the inputs and queries it builds on, and REG-9 to REG-12 are W2's |
| W4 | C5, the stub layer | STUB-7.1, then 7.3 (it opens SYN-1.10; PROD-5.4 is gone with REG-16.4) and 7.6 (CACHE-9/10/11 follow, once CACHE-13.2 has fixed the persistence's format); 7.2, 7.4, 7.7 to 7.11 (7.5 is W3's); the gate 7.12; STUB-8; STUB-12.1 to 12.5; CHECK-10's version table (its operator sites are D3's, W7); STUB-13.1 | STUB-7 needs REG-12 as a parent row, and 7.1, 7.2, 7.4, 7.7, 7.8 and 7.11 list no need of their own; deltas are VAL-2.4, 2.9 and 2.10 |
| W5 | C6 to C9, then C10 | the rows are written with the plan's approval (Part E 20, J3); then the overlay layer, `TypeNodeResolver` (built on F41's lowering, batch 6, which has already deleted `types::parse`: Part E 14), the extension points, the summaries; REG-15.1 and 15.2 | the plan lists many approved deltas for each |
| W6 | the embedding | STUB-10.1, then STUB-11; STUB-13.2 (what remains of the stub loader once W3's Session has replaced `Project`; the row is reworded when W3 lands) | the cold-run prize |
| W7 | Stages D to G | rows to be written after a re-read (rulings 2 to 4) | the architecture program; no tracked row needs it |
| W8 | VAL-1, then level 1 | the end validation of what was built (VAL-1.13 against VAL-1.12's baseline, VAL-2.9 and 2.10 read as they landed); the roadmap lanes resume | |

## What bears on the order

1. **The prizes sit at the end of one chain.** Items 10 to 19 cut instructions inside the parse and
   the check. They leave the finishing passes' structure alone (NAME-1 shortens their hashing leaves,
   `is_folded_eq` and `fold_hash`), and none stops a warm run from parsing and checking a file that
   did not change, which CACHE-13.2 does. The chain is REG-4.2 and REG-16.2 (both READY), REG-16.3,
   REG-7 and REG-8, REG-16.4, CACHE-13.2.
2. **Stages D to G were written without the 20 G plan.** The crate split plan cites the 25 G report once
   (`CRATE_SPLIT_PLAN.md:336`, §10.1) and no ledger or stack, and it still says "five levels, each a per-class
   `OnceLock`" and "constant reference cycles type as `mixed`" where decisions 3 and 1 say otherwise. For C4 and C5 the ledger's
   rows are the authority. For C6 to G there are no rows, so the stale text is all there is. Where it
   meets what the ledger builds:

   | crate split | ledger | meeting |
   |---|---|---|
   | D1: `Locals` maps into holder storage, `ExprKey` interned, persistent maps | CHECK-6.3, 6.13, 6.14: slots, persistent radix arrays | both replace the scope's storage |
   | D3: one inference path, `ExprHandler`s, per-node before-scopes, `infer_expr` deleted | CHECK-6.1, 6.7: one inference per node | both make each node inferred once |
   | D4: a `NodeCallback` sink; the `RefCell` recording channels become observer events | CHECK-6.1, 6.15: observer families | both introduce observers, shaped differently |
   | D2: `syntax::annotations` (goto labels, last condition, try/catch types, closure-argument context) | item 9's tree | the annotations need a place in the stored tree; no such module exists |
   | E7: the checker's field sets | CHECK-6.8, 6.18 | the same per-function and per-file state; batch 7 builds only the scratch reuse (C2 note 7), and the one-checker-per-worker form is E7's file-state set |
   | F1: the Session | LOAD-1 (done), LOAD-8.8, REG-16.4 | F1 lists the LSP's own settings assembly and walk as outside the Session; LOAD-1.3 and 1.4 deleted both |
   | F2: result cache, resolved dependencies, fingerprints; `deps.rs` and `dependents` deleted | REG-16.4, CACHE-13.2 | decision 3 replaced the design |

   The plan orders Stage D "structure before semantics" because a transition built on
   `check_stmt`/`check_expr` "would be written twice" (Appendix F). The same reasoning applies to item
   18's inference core and its scopes group, which are structure, and to F69 with CHECK-9 and CHECK-7's checker half,
   which sit on the recorders D2 replaces; its temporaries group (CHECK-6.4 to
   6.12, 6.16, 6.17 and the scratch reuse of 6.18) is local and survives either way. (First-pass reading. Part E 16 and J3 compare §12.10 with
   D1 to D4 and find one architecture.)
3. **Stage C is the price of the prizes in three tiers.** W2 and W3 (C4's engine slice) give the
   wall-time and warm-run prizes and TYPE-4. W4 (C5) gives item 6. W5 and W6 (C6 to C10) are what
   STUB-11's cold-run prize costs: it needs REG-15, which needs C6 to C9. No tracked row needs Stages
   D to G; REG-7.8 and STUB-7.8 name F1 and E4 only as later clean-ups.
4. **DOC-2.1 and C7 delete the same grammar.** DOC-2.1's "text grammar" is `types::parse_type_string`
   (the crate split's §1 calls it the second grammar: 219 mentions in 30 files, besides the 58 sites
   of `parse_written_type`), and C7 deletes `types::parse` with it. (First pass: DOC-2.1 deferred, taken inside C7.
   Superseded by Part E 14 and I4: F41 builds the lowering in batch 6 and deletes `types::parse` there; C7
   then builds the name scope on it.)
5. **The 20 G plan's end validation will find items 4, 6, 7 and 17's second half unbuilt** if it runs
   after item 19 as written, and VAL-1's tooling (the canonical type printer, `registry-dump`, the
   expression-types dump) does not exist (Part E 13 builds it in batch 0). `registry-dump` taken before and after REG-7 would show what
   the change does to the registry itself, beyond what the crate split's gates (corpus, type
   snapshot, baseline, nsrt) see.

## Rulings (Part D; the questions, with Part E's answers)

1. **The order.** W2 and W3 (the engine slice and its prizes) before C5 to C10, with Stages D to G as
   a separate decision (recommended); or the crate split's own order, C4 to G, which puts CACHE-13.2
   behind Stage F, where its text is stale.
2. **Item 18's inference core and scopes** (CHECK-6.1, 6.2, 6.3, 6.7, 6.13 to 6.15): build them in
   batch 7 as planned and re-read D1, D3 and D4 against what they leave, or leave them to Stage D and
   build only the temporaries now. The first takes the largest check-phase saving (§12.10,
   ≈1.0-1.3 G) early and makes Stage D's text partly redundant; the second keeps the structure in one
   place and waits for the engine port. (Answered: Part E 16 takes the second.)
3. **Rows for C6 to C9, and for D to G.** A docs task that can run beside the engineering: re-read the
   phases against decisions 1 to 18, write ledger rows with needs, so that REG-15 and STUB-11 depend
   on tracked work. (Answered: Part E 20, at approval.)
4. **Whether Stages D to G are built as designed**, or re-planned after W6 on what the engine and the
   flat tree leave. No tracked row needs them. (Answered: Part E 21.)
5. **DOC-2.1 folds into C7** and stays deferred until then. (Answered otherwise: Part E 14.)
6. **VAL-1's tooling before W2** (it is a precondition for no row): the printers and `registry-dump`
   would make REG-7's change to the registry visible. Say if it goes first. (Answered: Part E 13, batch 0.)
7. **The fillers (W1)**: used only between heavy runs, or left for after the prizes. Their value is
   not speed: level 3 is review findings, and the Q-11 follow-ups finish C1 to C3. (Answered otherwise: Part E 22,
   a correctness backlog placed in the sequence.)
8. **Design next**: the spine's roots (REG-4.2, REG-4.12, REG-16.2, REG-16.3) in a session of their
   own while batches 5 to 7 are built, as Part B was designed; and REG-16.4 once item 9's step 1
   has fixed the tree API it reads. (Done in Part H; Part E 18 asks for H1 before batch 4.)

---


# Part E. Pending decisions: the most proper course for each

Churn and effort are not weighed, as you ruled; each answer is what is correct and complete. Part E
supersedes the leans in Parts B to D wherever they differ: those parts keep the facts and the
designs, and are brought in line as you rule. "Changed" marks an answer that differs from the
first pass. On 2026-10-18 you ruled on the nine of M5a and on the deltas of Part E 42 and M5b, and on the four answers of
the audit of the same day (M5d: A1's mechanism, #40's delta commits and open-above range, #35: "agree") with the two
extensions that came with them (COMP-7 and COMP-8: "also (a) and (b)"); the answers below carry those rulings. Nothing else
is ruled until you say so.

Ruled so far: the fixes plan's decisions 1 to 7 (2026-10-17; A1 cites the seventh) and the ledger's
decisions 1 to 18 (2026-10-15), which the plan cites by number. Gates, not decisions: the go, and
the start condition (the intro): `git log` shows level 9's R17 and R18 and `WORK_STACK.md` has no level 9. Batch
numbers below are the new order (see "The sequence this gives"). An answer that departs from a ruled decision's
wording says so, and M5 lists those (ruled on 2026-10-18) and, apart, the answers the workflow review changed.

## The answers

| # | decision | most proper | needed before |
|---|---|---|---|
| 1 | D1: what consumers hold | **`Copy` handles and views for every walk; a stored expression is a rooted `(Arc<Ast>, id)` (changed by the review).** The first pass kept bare ids in checker state, the second put the tree's lifetime on `Locals` (415 mentions in 66 files, `Rc<Expr>` in 11 places). Stage D removes the tree from `Locals` altogether (conditional holders replace alias replay), so that lifetime would be built in item 9 and unbuilt then, which the no-interim rule (35, 41) refuses. So `Locals` and `Checker` take no tree lifetime: the two alias maps and three checker fields hold `Rooted<Expr>`, which is decision 4's "an id lives only in products owned with its tree" in the type, and a bare id is only ever a cache key (G1) | item 9, step 0 |
| 2 | D2: how to migrate | **Staged,** with the baseline dump taken at the end of step 1, so step 2 is checked byte for byte against the boxed tree's dump; the scaffolding it costs (1,000 to 1,500 lines, D2's row in B2) is argued against the no-interim rule there | step 0 |
| 3 | D3: position | **Decision 4's last sentence stands: a measurement settles it (ruled 2026-10-18).** Spike S-12, before step 2, builds both layouts for the corpus: the exact bytes of a kept tree with byte spans against token indices, and the readers' hot paths. The expectation is **byte spans**, which B3 to B5 and I1 assume: a token-indexed tree must keep its tokens (about 5 B a token), which breaks even only at about 20 B saved per node, more than an expression node can shed | step 0 (the census counts tokens); S-12 before step 2 |
| 4 | D4: payload bytes | **The ledger's layout, ruled 2026-10-18 with no amendment: the tree owns its source, a string is a span of it, and a side buffer holds the bytes no source has** (decoded strings, folded and synthesized text, the lossy decode of a source that is not valid UTF-8). A `Text` is a range of the source or of a side buffer, two bits of the length choosing. It copies no payload, a span always resolves because the tree owns what it indexes, and a context tree copies no source (the builder takes the worker's read buffer by swap). The arena design (a copy of every payload; kept trees 5.6 MB smaller at Piwigo scale) needed the side buffer too, for the bytes no source has, so it saved one thing and cost a copy of every payload and a second kind of text | step 0 |
| 5 | D5: names in nodes | **`NameId`** into a per-tree table: one refcount per distinct name, and nodes that are plain data | step 2 |
| 6 | D6: the mutating passes | **On the builder** until SYN-1.10 deletes the passes and `into_builder` with them; `Ast` never gets a `&mut` | step 2 |
| 7 | D7: fabricated nodes | **Remove them first:** a tree that cannot be built by hand makes the sites call functions on parts | step 0 |
| 8 | D8: rows | **Regroup** (SYN-1.3 is step 1; SYN-1.2, 1.4, 1.5, 1.7, 1.8 are step 2; new SYN-1.11 and 1.12) | step 0 |
| 9 | D9: error nodes | **Not in step 1 (changed).** Design the error-node model (what an error node keeps, such as salvaged children, and that it reads as unknown) with SYN-1.9, before step 2 fixes the layout, and add the variants there, so each has a producer, a meaning and tests. A `_ =>` arm for a variant nothing produces decides its meaning by accident | step 2 (design), batch 3 (variants) |
| 10 | SYN-3.2's SIMD | **A fused one-pass kernel (newline mask and non-ASCII mask) on a safe SIMD crate (refined), judged on merit and not on `Cargo.lock`.** The candidates are `wide`, `core::simd` (no `#![feature]` exists in the workspace today, so it would be the first, and its API moves with each bump of the pinned nightly), `pulp`, and a `u64` word kernel (one path on every target, no dependency, about one instruction per byte). `wide` is chosen: stable API, and already in the build through `imbl`, with SSE2 or NEON as the baseline and no AVX2 dispatch; our own code holds no `unsafe`, though `wide` holds some of its own (the `transmute` in `u8x16::new`, the NEON intrinsics, `unsafe impl Pod`). **The accept rule:** the kernel beats the `memchr` plus `is_ascii` version by at least 20% (proposed) in Ir and in a criterion bench on x86-64 and is not slower on the aarch64 runners (macOS, Linux) or on Windows, where `move_mask` is four NEON instructions and a horizontal add, which `memchr` itself avoids there; otherwise `memchr` stays and `wide` is dropped, or the `u64` kernel replaces it. That version stays as the test oracle, and SYN-4.3 shares the kernel. The PHP line-terminator rule (a lone CR ends a line, as in the lexer's comments, close tag and heredocs) ships with it in one change (I3). If a measurement says AVX2 matters, a safe dispatching crate such as `pulp`, judged on merit | batch 2 |
| 11 | the parse-recovery batch | **In (changed from optional).** Decision 4 says every parse yields a tree, and an analyzer that stops at the first error is wrong for the editor. SYN-1.9, Q-10's parser half and SYN-6 are one design, built after SYN-2 because recovery synchronizes on tokens; the version half waits for STUB-7.1. F77's reporting (a parse failure is `phpstanRs.parseError`, non-ignorable, and fails the run) does not need recovery and is built in batch 0, so that item 9's parser rewrite cannot refuse a file unseen; batch 3 only changes where its errors come from | batch 3 (F77: batch 0) |
| 12 | order of items 13, 16, 15, 14 | **Names first: 16, 15, 13, 14 (changed).** `FoldedName` and `ConstName` do not exist yet (NAME-1.2 makes them), so products and PHPDoc work should be born on them, and DOC-2.5 and F41's lowering produce `Name`s. The first pass put 13 first only to avoid converting code (`DocNames`, `add_uses`) that PROD-5.8 deletes | batch 4 |
| 13 | oracle tools | **Item 0 first (changed).** Between Part A and item 9, in G5's order: read HEAD against the last validated state (VAL-1.1), build the canonical type printer (1.4, after F21), the full-field O2 (1.8), `registry-dump` (1.6), the expression-types dump and the PHPDoc record (1.7), the server session (1.9) and the edit replay (1.10) and the memory profile (1.11); run the `oracle` profile once over all of them (1.2, which includes the replay, so it comes after it); retire the debug oracles (1.3, which also shrinks item 9's reader set); take the baseline (1.12). VAL-1.5 is item 9's dump. The 20 G plan designed it so (oracles are dumps), Piwigo's 581 diagnostics are a weak oracle for exact refactors, and your rule is not to gate on Piwigo. This reverses VAL-1's deferral to the end. **The policy on old implementations:** no row keeps one alive as a standing test; a comparison against the implementation being replaced is allowed for one stage where a dump cannot see the difference (every `Provider` answer for every name, H3.6 steps 2 and 3; I4.5 step 3 only if the PHPDoc record cannot see what a per-`TypeNode` comparison finds; I2's comparison of the old and new lexer's (kind, span) pairs), each is listed in Part N with the commit that deletes it, and A2 keeps none | batch 0 |
| 14 | DOC-2.1 | **Undefer it, as F41 (changed).** The review's F41, the ledger's DOC-2.1 and C7's grammar item are one piece of work: one grammar, a `TypeNode` to `Type` lowering, the string parser deleted, internal strings through `parse_phpdoc_type`. It fixes real bugs (a class named `Infinity`, `Nan` or `inf` read as a float; `1_000` and `0x1F` rejected; conditionals split at the first `:`), so "the saving is gone" does not apply. First in batch 6, before DOC-2.2; C7 later builds the resolver on it | batch 6 |
| 15 | CACHE-13.1 for context files | **W3's, built once as `File.stamp` (changed from batch 8).** The stamp (the `stat` tuple, taken before the read, the racily-clean rule, the file-system test, the content hash where a change time cannot be trusted: decision 13) is permanent, and a per-project index file that stores it is not: the persisted database holds `File.stamp` and the memos it validates, and deletes the index. **The racily-clean rule is applied at the stat:** decision 13's reference time, the index's own write time, is git's because git writes its index at once, and here the database is written after the whole check; so each stamp records the clock reading of its stat and is trusted only when its mtime and ctime are older than that reading by a margin of at least the file system's timestamp granularity (ruled 2026-10-18; decision 13's wording changes), and the file-system test is an explicit list of accepted types with the content hash as the default (the commit that names the system call and crate judges a new dependency on merit, and tests the three platform legs). MEAS-1 puts what the index replaces at about 60-70 M Ir of read wrappers and two hashes (`fs::read` 15.2 M, `File::open` 4.3 M, `crc32` 30.4 M, `xxh3` 17.1 M; the system calls and I/O are not in Ir and have not been timed), with the ≈1,260 M of per-file decode untouched until CACHE-13.2. The ledger gives analysed files to CACHE-13.2 already. Built at W3 step 5 before CACHE-13.2, the stamp has its own tests and the cache closure gate; the no-interim rule (25 G §10.7) of Part E 35 and 41 | W3 |
| 16 | item 18 | **Reconcile, build once, as Stage D (changed).** §12.10 ("PHPStan's `NodeScopeResolver`... record and replay") and D3 and D4 (`RecordingNodeCallback`, per-node results) are one architecture; D1 and CHECK-6.3 both specify the scope's storage; CHECK-6.2 is D2's `StatementResult`. The crate split builds the layers bottom-up, so the engine goes on the final reflection API, and the 25 G report's own no-interim rule (its §10.7) says not to build what the traversal replaces. So CHECK-6.1, 6.2, 6.3, 6.7, 6.13 to 6.15 (and the LOAD-5.1 passes that need CHECK-6.1) go to Stage D, reconciled first; batch 7 is the temporaries, with 6.10 and 4.1's message type designed as D6's `RuleErrorBuilder`. The cost: §12.10's ≈1.0-1.3 G and the slots' ≈0.15-0.25 G arrive with Stage D | batch 7 |
| 17 | CHECK-9 and TYPE-5 | **Neither in batch 7 (changed).** CHECK-9 only after F69 gives implicit calls (`__get`, `offsetGet`, `__toString`, `include`, `Traversable` iteration) throw points, or it reports live catches as dead. F69 records them on `record_call_throw_point` and `current_function_throw_points`, the recorders that D2's `StatementResult` replaces, so by the crate split's Appendix F ("a bug in code a later phase rewrites gets a fixture now and its fix as that phase's behaviour commit") F69's fixtures go in batch 0 and F69 and CHECK-9 are built in D2, CHECK-9 after F69. The same reasoning puts CHECK-7's checker half in D2 and CHECK-10's operator sites in D3 (J3). TYPE-5 is F14, which the review sequences after F41, so it comes right after #14's work | batch 6 (TYPE-5); D2 (F69, CHECK-9; fixtures in batch 0) |
| 18 | when to reevaluate | **Now, for the engine's identity model (changed).** Design REG-16.2's ids, REG-8.6's interner ownership and NAME-2's weak tables as one lifecycle before batch 4: names, products and memo keys are built on it. The rest at batch 8 | before batch 4 |
| 19 | order of the wall | **The spine, then the pipeline as queries (REG-16.4, CACHE-13.2) designed as one thing with Stage F1 and F2** (F2's design is superseded by decision 3, and building both is building the Session twice), then C5, C6 to C9, C10, STUB-11 and 13, Stage D, E, F3 to F5, G | the reevaluation |
| 20 | rows for C6 to C9 and D to G | **A docs pass at approval, and edit `CRATE_SPLIT_PLAN.md` itself** so there is one source of truth: the rows for C6 to C9 (J2), the D and E rows (J3) and the plan's edits (Appendix F's "each level is a per-class `OnceLock`" and "constant reference cycles type as `mixed`", §3.9's F1 text about the deleted `lsp/workspace_config.rs`, C4's levels, F1's snapshots, F2's dependencies) are one docs commit series made when you approve this plan, with each superseded paragraph marked in place and pointing at the section that replaces it (Part N lists them). Effort is no reason to leave stale text that misleads until the phase is reached | approval |
| 21 | Stages D to G | **Build them, re-baselined:** they are the target architecture and the home of D5's correctness fixes. The re-read is #20's, not an alternative to building | the reevaluation |
| 22 | level 3 and the Q-11 follow-ups | **A correctness backlog, not fillers (changed).** Triage the findings first (Part K2: about 115 are distinct; F57, Stage C's O1 and F5 are built), by running each probe on the gate binary. F10's symptom is fixed (`can_begin_expression`'s list, `3b7e4d95`), so its parser tests and fixture go before item 9's baseline, which must not encode a parser bug, and its exhaustive predicate waits for SYN-2's `TokenKind`; F20 before item 9 (after level 9's R10), so its lint checks the recursion that item 9 writes; F21 before the canonical printers; F41 in batch 6; F77 in batch 0, before item 9's baseline (its design needs no recovery, an analysed file's diagnostic and exit status are the final ones, and a parser rewrite that newly refuses a file then shows in O2; batch 3 feeds it from `errors()`). **F77's context-file half** (`phpstanRs.contextParseError`, run-level, failing the run) is new user-visible behaviour that decisions 9 and 15 ("nothing in a context file is reported") do not give, so, as ruled on 2026-10-18, decisions 9 and 15 keep their text and gain one sentence: a context file that fails to parse is reported as a run-level notice (its path, line and column, and the first error; `phpstanRs.contextParseError`), printed where today's "N file(s) skipped: parse error" count is, and it leaves the exit status as it is; an opt-in switch makes it fail the run. Batch 0 builds the notice and records the failure (the count and the first error per file) in the dump; F69's fixtures now, F69 and CHECK-9 in D2 (17); **the seven fixes the crate split's F1 puts first** (the debounce thread's stack size and the scan's parses on the pool, `queue_change` writing `latest_text`, `didClose` restoring the disk's content, `--generate-baseline` keeping the entries of files not analysed, `--fix` skipping a file changed since analysis, a closed output pipe ending the run quietly, rename refusing qualified names, imports and aliases) end the server, lose data or corrupt files on today's code, are independent of everything here and are all still open at `ffa7cccb` (read in the code), so they are commits of batch 0, each with its test, and rows of the ledger and `WORK_STACK.md` (Part N), not part of W3. **Where each finding sits is a place in the sequence, not "between heavy runs":** every finding or follow-up that changes a dump goes before the baseline; the rest in the review's own order (F4 first), fixture first where a later phase rewrites the code (K2 names which); a later commit that changes behaviour runs `dumps-compare`, lists its differences as its delta and rolls the baseline forward | batch 0 |
| 23 | Part A's validation | **Once, after the last row,** against binaries built at the batch's base commit and the Piwigo digest (A6): the item-0 baseline does not exist yet, since batch 0 comes after Part A. The rows are independent; none changes Piwigo's or `vendor/monolog`'s output, and A1's editor delta, its `config.vendor-dir` commit and A8 are read separately (A6) | Part A |
| 24 | item 9's validation | **Once, at the end of the item (step 3), by your rule of validating at the end of each top-level item.** The syntax dump baseline is produced at the end of step 1 (a cheap run that stores output, not a suite) and is compared with step 2's dump at the item's end; the item-0 baseline (expression types, registry, O2), taken from validated code before the item, covers step 1's readers and is compared then too. The suites, the fixtures, the Piwigo oracle and the measurements run once, then (B7). The first answer validated after step 1 and step 2 as well, which was a checkpoint you did not ask for | step 1 |
| 25 | LOAD-2.8's parts 1 and 2 | **Measure (changed).** Builds are pre-approved and the ledger claims nothing unmeasured: callgrind and `abab` at `27c3b093` and `53e6f766` against their parents | Part A bookkeeping |
| 26 | the LSP completion probe | **Neither import nor text (changed):** a checker entry point that takes a statement prefix of the last-good function and the receiver as an expression handle from another tree. Today the receiver comes from the live buffer (`parse_standalone_expr`) and the function from the last-good AST, spliced with sentinel spans and `NodeId::SYNTHETIC`. Handles evaluate across trees, so no synthetic nodes, no import API and no text splice. **It is throwaway on purpose:** step 2 makes the tree immutable, so the probe must change then, and recovery cannot come first because it is built on the layout step 2 fixes (G3.3); a window with no completion is worse than a small tail (about a hundred and fifty lines), and batch 3's node-at-position query on the request's own parse (G4b) deletes the tail and the last-good tree with no fallback kept. Observers (E6, F5) replace the re-run later | step 2 |
| 27 | the step 1 codemod | **Use it, by span-based text edits;** never reprint a file (a `syn` printer drops comments), with `cargo check` as the oracle, one module at a time | step 1 |
| 28 | LOAD-5.2 batch-only | **No: keep its needs (changed).** A batch-only version makes the batch run and the server hold trees differently, which LOAD-1 and decision 3 removed | W3 |
| 29 | NAME-2.1's thin name | **`triomphe::ThinArc<Header, u8>`, after a spike that settles how the text is read back as a `&str`.** A header plus one slice fits (the text, then the folded form), and the interner frees names without `Weak` (triomphe 0.1.14 has no weak counts) by a unique-count sweep, which REG-8.6 already plans. Its thin pointer exists only for slices (a header plus a `str` is a fat pointer), so without `unsafe` of ours a `&str` costs a UTF-8 validation on every `as_str`, `lower` and `Deref`, unless a crate supplies a thin header-plus-`str` type (`slice-dst`'s `StrWithHeader` behind `erasable::Thin` is one to evaluate, on merit). S-1 therefore measures the read path and accepts the thin name only if the corpus total of those reads is not above today's; the fallback is one `Arc<NameData>` whose single `Box<str>` holds the text and the lowercase form (sliced at `len`): two allocations against today's three. No hand-rolled `unsafe` | batch 4 |
| 30 | where Part B lives | **Its own repo document** (precedent: `ITEMS_1_TO_3_FIXES_PLAN.md`; call it `FLAT_TREE_PLAN.md`), with the ledger rows and `WORK_STACK.md` updated in the same series | approval |
| 31 | (new, from G3) an analysed file with parse errors: the parseable parts are checked and reported as usual, or only its errors are reported (PHPStan's behaviour) | **Checked (G3.6), ruled 2026-10-18.** Q-10 asks for one rule for the batch run and the server, and decisions 4 and 15 keep what parses and make the rest unknown (decision 6 says nothing of parse failures); the conservative reads (error nodes infer the error type of F21, a statement error sets `any_variable_may_exist` and `variable.undefined` honours it, a missing name is a zero-width name every reader treats as unknown, nothing reported inside an error node, whole-file opt-in rules off) are what keep it quiet. The editor is where files are broken most, and recovery exists for it (Part E 11). No ruled decision says an analysed file's own findings are reported, and this departs from `CRATE_SPLIT_PLAN.md:6883-6891` ("a file with parse errors is still not analysed (its findings would be noise)"), which Part E 20 supersedes. **The bar is stated before the check is built (G3.7.3):** on the mutation corpus, for damage to a single token, the diagnostics that are new, meaning absent from the undamaged file's own and not parse errors, are none at the 95th percentile of damaged files. A damage class that misses the bar falls back, for the files it damages, to parse errors only, with the registry and navigation keeping the recovered declarations; an unclosed brace that reshapes everything after it is the likely class to miss | batch 3 |
| 32 | (new, from G3) `PhpVersion`, `PhpVersions` and `PhpFeature` move out of `types` so the parser can record marks: a leaf crate (`php-version`), or a module of `syntax`, which `types` already depends on | **A leaf crate**, as the crate split makes every shared base (F20 proposes the same for the stack guard). It moves the code as it is, with `KNOWN_PHP_VERSIONS` starting at 5.3, and adds what the marks and `declarations_named` need before W4: `VersionSet` (bit i is `KNOWN_PHP_VERSIONS[i]`, append-only, so a new version is a new bit at the end) and `Availability` (J1.1), a file's validity being a `VersionSet`; `PhpVersions` gains exact version-id bounds (J1.1; ruled 2026-10-18, M5d); STUB-7.1 only adds `Readings` to them. Decision 18's 7.0 floor, with the notice for a range that reaches below it, is a mask over the same bits and not a rebase of the list, and is STUB-7.1's change and a delta of its own, so batch 3 changes no project's range; until STUB-7.1, `declarations_named` reads a builtin as `Always` available. A narrowing by a patch-level `PHP_VERSION_ID` is conservative (a minor stays in the range if any patch takes the branch; `narrow_at_least(80103, false)` would drop 8.1 although 8.1.0 to 8.1.2 take it). **`SyntaxFeature` is the grammar-only subset** of today's 45 `PhpFeature` variants, in the leaf crate with the one threshold table, and `PhpFeature` in `types` delegates to it for those and keeps the rest (the library gates such as `PrintfHhSpecifier`, `FilterThrowOnFailureConstant` and `TentativeReturnTypes` are not the parser's); array unpacking with string keys depends on the unpacked array's keys, not the syntax, and stays out. A new crate is not one row: the last one (`27091848`) touched 19 files: its `crate` line in `gates/module-graph.txt`, `LIBRARY_CRATES` and the build-time scenario lists in `xtask`, `fuzz/Cargo.lock`, the dependents' manifests and `gates/test-renames/module-graph.txt` for the tests that move | batch 3 |
| 33 | (new, from H2 and S-2) the engine crate and `unsafe_code`: salsa's macros emit `unsafe impl` into the crate that uses them, which the workspace's `deny` is thought to reject; allow it in `crates/engine` alone, gated, or build an engine of our own with no macro output | **Conditional on S-2, which builds the skeleton first under the workspace's `deny` with no allow.** The macros do emit `unsafe impl` (salsa's `HasStorage`, the `inventory` constructor), but rustc does not normally report most lints, `unsafe_code` among them, for tokens that come from another crate's macro expansion: a probe on the pinned toolchain (an external `macro_rules!` and an external attribute macro emitting `unsafe impl` and `unsafe` blocks, under `#![deny(unsafe_code)]`) compiled without a diagnostic, while the same code written in the crate was rejected. So the exception is probably unnecessary, and the engine keeps the workspace's two (PCRE, the allocator). Only if rustc reports the expansions of salsa's own macros does the engine's module get `allow(unsafe_code)`, with an `xtask arch` check holding its own text at zero `unsafe` tokens and `UNSAFE_PLAN.md`'s end state amended to "everywhere but PCRE, the allocator and the engine's generated code". **Until G1 the engine is a module of the `phpstan-rs` package, not a crate:** `reflection`, `analyser` and `project` are modules of it until C10 and G1, and a crate built at W2 could name none of the types it wraps; it takes a `crate engine` line (between `analyser` and `project`) and a `module engine engine` line in `gates/module-graph.txt`, and G1 extracts it | W2 |
| 34 | (new, from H1.5a) positionless declarations: `Provenance` holds a declaration's ordinal data and the positions live in the file's site table; REG-4.15's "span a field of its entry" is withdrawn; an anonymous class, whose name carries its line, is a file-local entity outside the global index | **Yes.** On the engine an inserted line would otherwise invalidate every class below it and all their readers (today it already marks them changed and rechecks them), and ignoring the span in the equality instead would keep stale spans in a backdated memo. An anonymous class keeps the name the ledger and the review give it (`class@anonymous/{shown path}:{line}[:{i}]`: decision 5, PROD-5.1, F33), so types and messages display it as they do today, but **its identity is the ordinal (ruled 2026-10-18)**: its `DeclRef` is (file, `Class`, the constant name `class@anonymous`, its ordinal among the file's anonymous classes), with no line in it, so an inserted line mints no key and moves nothing, and the positional name is derived from the file's site table. Because no other file can spell it, it stays out of `decl_names` and `decl_index` and is found through the file its name carries (H1.4), so a moved one changes its own file's products and nothing global. The name's split is not parsed by guessing: every reading of the trailing `:digits[:digits]` is tried against the `shown` index and the one whose file's `decls` hold exactly that name is taken (both existing is an unknown class), so a file called `a.php:12` is no trap, and the displayed-type parser peels the same suffix. The consumers that see an anonymous class are each decided in H1.4 from an enumeration of the code (the first list was the plan's, not the code's): dead members keep it, read from the file's `decls`; workspace symbols drop the class and keep its methods and properties; import completion drops it, since no source can spell it; namespace cycles keep it, attributed to its enclosing namespace (`namespace_of` makes it a global-namespace node today, which draws false cycles); `demo` keeps it; `registry/scope.rs` is not an iteration (the engine deletes the scope, and an anonymous class goes through the levels like any class); and the class-shape parent-call rules, which walk only named class statements, include it. Each has a fixture, and each change a VAL-2.x delta | before batch 5 (PROD-5.1 lays out the vectors) |
| 35 | (new, from H4.8) PROD-5.7, LOAD-4.7's file-references half and PROD-5.4's rewrite work on `deps::Collector`, `recorded_references` and the dependents indexes, which REG-16.4 deletes whole: build them in batches 5 and 8, or leave them | **Leave them; keep PROD-5.8 (ruled 2026-10-18, M5d).** The savings (targets, upper bounds in the ledger's sense: ≈0.34 G of the off run, `recorded_references` 265.6 M and `register_references` 77.2 M) end when the wall's W3 deletes the machinery, and the 25 G audits' no-interim rule (§10.7) says not to build what a later traversal replaces. The line is between a new structure for a gain (PROD-5.7's one vector per file, a format bump: not built) and a type change that an end-state invariant needs (NAME-1.2 converts the same machinery's `String` keys, because NAME-1.3's deletion of `FoldKey for str` is a compile-time guarantee that a caller left holding text would break). It stands or falls with building the wall (#19, #21): if the wall is dropped, the three rows are READY again. Checked in the code on 2026-10-18: the readers of the reference sets besides the dependents index (`mentions_phpstan`, which loads PHPStan's classes while a file names a `PHPStan` name; the stub index at `registry/stubs.rs`; the body index at `registry/bodies.rs`) move to PROD-5.8's record, which is built, so only the dependents' feeders go; about 77 M of the 343 M is on the serial insert. **The rule is the 25 G report's own (§10.7), which the user's audits produced; it removes three rows of the user's plan, and the user ruled on it on 2026-10-18 ("agree", M5d)** | batch 5 |
| 36 | (new, from H4.3) the server's snapshot model: salsa cancels a request when a write arrives (the request answers `ContentModified`, or the loop runs it again), where F1 wanted requests to run against the last published snapshot while an update builds the next | **salsa's model**, rust-analyzer's. F1's model needs persistent structures through the whole engine, which is an engine of our own. The visible difference is a request cancelled by a keystroke, and a client retries a `ContentModified` answer only for the methods it lists in `general.staleRequestSupport.retryOnContentModified` (LSP 3.17; semantic tokens and inlay hints in common clients, not hover, completion or definition), so the server reads that capability at `initialize`, runs a request the client will not retry again on the new revision, and answers `ContentModified` only for the retryable ones (H2.8) | W3 |
| 37 | (new, from H4.4) the persisted database's key: one file per (build, root, config key) with the settings also inputs, or one per (build, root) with the settings only inputs, as CACHE-13.2's row words it | **Per config key, with the settings also inputs, and the config key leaves out the PHP version range.** One file would thrash when two configurations alternate (the editor beside a `--set level=9` run), which F2's entry sets exist to prevent; the inputs keep a change inside a session exact. Decision 6 says no cache key holds a version, so the range stays an input and a changed range reruns only what read it; what changes is CACHE-13.2's wording ("the configuration ... are inputs, not keys"), which H6 lists. **The build identity** is `PHPSTAN_RS_SOURCE_HASH` plus the rustc version and a persistence-format constant (salsa orders its ingredients by type name, so the bytes follow the compiler's layout); F2's GNU build ID, executable size and modification time cannot work, since the project's builds write no build ID and size and mtime change on every relink | W3 |
| 38 | (new, from H1.4) REG-16.2's "never reused": keep salsa's collection of interned slots (a stale id panics and invalidates what read it; memory stays bounded), or turn it off with `revisions = usize::MAX` (the literal "never reused", and a table that grows for the life of a server and of the persisted file) | **Ruled 2026-10-18: `DeclKey` is immortal, and the name-probe keys are collectable.** `DeclKey` takes `revisions = usize::MAX`, the literal "never reused" of decision 3, which stays as ruled for declaration ids: a stale id cannot name another declaration, by construction and not by discipline (salsa's own check on a read is a `debug_assert`, so with collection a stale id of a slot re-interned this revision would read the new occupant silently in a release build), and an immortal key records no creation edge. `ClassKey`, `FunctionKey`, `ConstKey`, `GlobalKey` and `MemberKey` are probes by an asked name, made for names that do not exist too (every half-typed identifier asks), and nothing holds them, so immortal would only grow the table; they stay collectable, and H1.6's rule (no id outlives its query or leaves `crates/engine`: an `xtask arch` rule and a release-build test that no engine id appears in a result type) keeps them honest. Only values interned under a LOW-durability query are collected, and creating such a value, not reading it, records the edge. **Two costs of an immortal `DeclKey` are stated and measured in S-3:** a dead declaration's key keeps its memos (the five levels, a full `ClassReflection` among them) until the database is rebuilt, and an anonymous class must not carry its line in the key (E34). **The bound is an in-process compaction (H4.6):** the persisted-file rule fires at the next start, which does nothing for a server open for days, so the server rebuilds its database at idle once the keys outnumber the live declarations several times over. If S-3 finds the retained bytes too large, the fallback is a collectable `DeclKey` and decision 3's sentence amended to "a stale id never names another declaration" | W2 |
| 39 | (new, from J3) LOAD-5.1's passes (architecture, banned constructs, class shapes, Composer and mock hygiene, unmatched ignores) are the crate split's E1 collector rules: build them in batch 8 as per-file products, or as E1's collectors on D6's API | **Split by form: W3 builds the facts, E1 moves their collection.** The six passes' per-file facts are `file_products` fields built in W3 (H2.3 and H4.4 already read them, the persisted whole-program queries need them, and no analysed tree can be freed after its check while a pass still reads it), so LOAD-5.1 moves to W3 and LOAD-5.2 follows it and S-7's verdict. E1 adds the D6 Collector API and the semantic fixes over those fields, so the data is built once; the passes do not all land in E1: architecture, class shapes and Composer hygiene are E1 collectors, banned constructs are per-node rules in the rules walk (the crate split calls them per-file checks, not project-level), mock hygiene is ext-mockstan's (E4) and unmatched ignores are the finalizer's (F1). Built in batch 8 they would have been written twice; with them batch 8 empties (LOAD-5.4 is Part A's, CACHE-13.1 is W3's, CACHE-8 follows S-4) | W3 (facts); W7 (collection) |
| 40 | (new, from the audit of 2026-10-18) the composer crate's B8 follow-ups sit in `CRATE_SPLIT_PLAN.md:1840-1880` and in no ledger row (Q-11 audited C1 to C3 only): `config.vendor-dir`, Composer's constraint grammar (whitespace after an operator; an open upper bound running to PHPStan's `MAX_PHP_VERSION`), the process-wide panic hook that `discover_vendor_context` swaps while it parses (`vendor.rs:134`), `psr-0` in `Autoload` and the generated maps' keys, the root package's own autoload as context, a package's bundled `vendor/` as context. Also whether `COMPOSER_VENDOR_DIR` is honoured beside `config.vendor-dir`; and two that the re-audit of the same day added: third-party code that Composer installs outside the vendor directory, and the vendor contexts of a degrade root's nested Composer projects | **Each audited against the code on 2026-10-18 (read-only, as Q-11 did, with the vendored Composer's `Semver` and `Config` run in memory and a scan of the 1,594 manifests under `vendor/`); the rows are written (Part N lists them, one COMP row each).** **COMP-1, `config.vendor-dir`, `COMPOSER_VENDOR_DIR` and `COMPOSER`:** read nowhere today; discovery, `read_installed` and R8's `vendor_trees` hard-code `vendor`, and four places join `composer.json` where Composer's `Factory::getComposerFile` (and PHPStan) take the file from `COMPOSER`. A1 resolves them as Composer's `Config` does (PHPStan joins the key to the root, `$root . '/' . trim($dir, '/')` (`ComposerHelper.php:77`), so it misreads an absolute value) over candidates in Composer's order, the first installed one winning, so no project loses context it has today; the variable is honoured wherever it names an installed directory and is an input of `Packages`, so a persisted database made under another environment is not trusted. **The batch-output commit and A1's mechanism for the skip were ruled on 2026-10-18 ("agree", M5d)** (a directory Composer wrote into, not a path a manifest resolves: the wording of the fixes plan's decision 7). **COMP-2, Composer's constraint grammar** (confirmed on the corpora): 5 of 1,594 manifests write a space after an operator (`>= 7`, `>= 8.1`), which `satisfies` rejects, so they get no range; `declared_php_versions` probes patches 0 and 99 of each minor, so a window inside one (`>=8.0.2, <8.0.15`) gets none where Composer accepts 8.0.5; and a bare `8.1` is `==8.1.0.0` in Composer, not any 8.1.x. The fix ports Composer's grammar with interval arithmetic and intersects the intervals with each minor's, instead of probing; it changes a range and so a dump, and goes before the baseline (batch 0). **The upper end:** the crate split's text clamps an open upper bound to PHPStan's `MAX_PHP_VERSION` (80699), and 282 manifests say `>=8.4.1`; runtime truth is open above, so code gated on a later version is possible and not dead (keep all possibilities when unknown), and the corpora already hold such guards (`>= 80600` in Symfony's `polyfill-php86` and Guzzle, `< 90000` in var-dumper and Composer). A range keeps its exact version-id bounds (J1.1), the upper end absent when the constraint has none or no range is declared, and declaration guards read them. This reads decision 18's "to the newest version" as "and later", which **the user ruled on 2026-10-18 ("agree", M5d)**; the grammar half needed no ruling. **COMP-3, the panic hook:** discovery's swap is deleted, since the parse is infallible after G3 and needs no catch there (A2 adds no second user of it), and the other two swap sites (`check_project.rs` and `check_paths_isolating_panics`) get one hook installed at startup that consults a thread-local "silenced" flag, because a swap of the global hook is not thread-safe and its windows overlap once discovery runs beside the manifest load (A2) or per root (H4.3); that is the crash plan's step 1, done before W2 so that H2.8's isolation can assume it. **COMP-4, `psr-0` and the maps' keys:** `Autoload` has no `psr-0` and `GeneratedEntries` drops the classmap keys and the PSR prefixes (STUB-7.5's prerequisite, H4.1); A7 adds them ahead of W3. **COMP-5, the root package's own autoload as context:** confirmed (the crate split counts 1,253 of the by-name Piwigo oracle's 1,855 phpstan-rs-only `class.notFound` naming a `Piwigo\` class); discovery drops the root's entries because they lie below `$baseDir` and outside `vendor/`, A7's maps keep them and W3's lazy locator finds the class, so no interim fix is built (the rule of #35 and #41). **COMP-6, a package's bundled `vendor/`:** confirmed (`rector/rector` and `symplify/easy-coding-standard`); A1's test finds them and the same discovery reads them (A8, in Part A: W3's locator needs the nested maps too and does not replace the work). **COMP-7, third-party code installed outside the vendor directory** (added 2026-10-18, "also (a)"): a custom installer (`composer/installers`, `extra.installer-paths`: Drupal's `web/core` and contrib modules, WordPress plugins) installs packages where the project asks, and `installed.json` records each (`install-path`; `null` for a metapackage: the vendored Composer's `FilesystemRepository::dump` was read), yet nothing recognised them: a degrade root walked them as analysed code, and discovery dropped their autoload entries as first-party, so no mode had them as context. A1's third clause leaves the install directories of a known root's non-path-repository packages out of a degrade root's analysed walk (the seeds), and discovery's filter admits their entries in every mode (a delta commit of its own). One of the 1,329 packages in the corpora's 29 `installed.json` files lies outside its vendor directory and it is a path repository's (Drupal core), so the fixtures carry the clause; a match of an `installer-paths` pattern proves no package, so a fresh install's files are analysed until the session is resolved again (stated and tested). **COMP-8, the vendor contexts of a degrade root's nested Composer projects** (added 2026-10-18, "also (b)"): a degrade root has one known root, so a monorepo's packages and a `tools/cs/` project with a vendor of its own are analysed with no context from it (the corpora hold 7 to 211 nested manifests each, and fixtures with installed vendors). A8's scan finds them before the walk, because the walk tags owners and decides roles from the immutable `PackageIndex` and the seeds, loads them in parallel and merges them in path order; their manifest directories are the owner facts that H2.2's `range_of` reads; configured `paths` keep the nearest-ancestor rule unless the scan measures below the gate's noise. PHPStan does not read the variable, which is not a reason (the project is judged on runtime truth) | A1 (the skip rule, the resolution and COMP-7; the delta commits are ruled); A7 (the maps); A8 (COMP-6 and COMP-8); COMP-2's grammar (batch 0, before the baseline); the rest before W3 |
| 41 | (new, from the review of 2026-10-18) rows whose code a later task replaces: REG-14.5 (`reconcile_with_native`, which REG-7.5's combination replaces), CACHE-8 (the cache `Writer::finish`, which serves the stub caches STUB-11 deletes and a persistence format that S-4 has not chosen), and CHECK-4.2 (the class-constant key, which REG-18.4 re-keys). Build each now for its small interim saving, or fold it into the task that replaces it | **Fold them.** REG-14.5 into REG-7.5 (A4 says how the replacement is written), CACHE-8 after S-4 (if the cache's container survives as the persistence's value format its writer is built once, right), CHECK-4.2 into REG-18.4 (batch 4b: it needs `ConstName`, which batch 4 makes, so it is not a filler of W1; its delta is one of Part E 42's). Each saves little (REG-14.5 is 41,085 allocations; CACHE-8 is memory in a cold run only; CHECK-4.2 is 5.1 M Ir), and each would be written twice. REG-14.5 and CACHE-8 wait on the wall as #35 does (REG-7.5 is W2's, and S-4 settles the persistence), so they too come back as READY rows if the wall is dropped. The same rule as #35, and the same line: a type change that an end-state invariant needs is built (NAME-1.2); a new structure or a micro-optimisation of code that goes is not | Part A; batch 8 |
| 42 | (new, from the workflow review) deltas the ledger marks READY, or OPEN with half a ruling, that neither VAL-2.9 nor decision 14 approves: LOAD-7.6 (the config's keys: grouped keys named for the PHPStan parameters they cover, one casing, no key repeating its section's name, PHP versions and extension activation read from every discovered Composer root's manifest), REG-18.4 (a constant's namespace folds case at every lookup: `\APP\A` finds `App\A`, which is `constant.notFound` today), CHECK-11 (`property.unused`: ORM-mapped properties count as used while ext-doctrine is active, and view objects passed to a template renderer are not reported) and LOAD-7.2's PHPUnit half (whether PHPUnit's assertion narrowing and mock types are gated on ext-phpunit's activation, which nothing reads today, or stay a fact of the program, since a PHPUnit assertion throws whenever PHPUnit runs it) | **Ruled on 2026-10-18, each with a condition.** **LOAD-7.6** is approved as a rule: the commit opens with the full old-to-new key table, shown to you before any key is renamed; the rename is hard, with no aliases (an alias is a second spelling kept forever), and the unknown-key error (LOAD-7.1) names the new key; the one casing is snake_case (only the `sql` section is camelCase today); and the PHP range and the extension activation of a file come from its owning manifest, the nearest ancestor `composer.json`, a fact of each directory's listing that A8's scan finds for a degrade root (COMP-8) and `Packages` holds from W2 (a hull would judge 202 Symfony components, whose manifests say `>=8.4.1`, against versions they cannot run on; J1.2 and H2.2 change; a file with no owner uses the project root; activation is any owner's). **REG-18.4** is approved (php 8.5.4: `\APP\A` and `\app\A` find `App\A`, and `\App\a` does not). **CHECK-11** is approved with a definition its row lacks: the commit says which calls are template renderers (a configured list of methods, with defaults for Latte, Twig and Smarty when installed), the ORM half reads the resolved mapping attributes and annotations, and ROADMAP 182g later replaces the blanket with the properties templates read. **LOAD-7.2's PHPUnit half** stays ungated: the narrowing resolves the callee to `PHPUnit\Framework\Assert` (`checker/narrowing/phpunit.rs`), the mock types come from PHPUnit's own PHPDoc, and the code after an assertion only runs when it held. They are not fillers: W1's other rows do not wait for them. The first three change output on a project that uses them and each carries its fixture and a VAL-2.10 line | W1; batch 4b (REG-18.4) |

## Audit of 2026-10-18: where a design was left short

The question was whether every design is the most proper or some still avoid churn. Some were left short for
effort, some for a measurement, some by omission. Read for phrases that defer
(a measurement, "as touched", "W3's call", an alias, a first version), they showed the following. Each was
checked against the code or salsa's source before it was changed, except the price of the member grain, which
only S-3 can measure.

| was | why it was short | now |
|---|---|---|
| a read of a class is `class_final` as a whole; finer "if a measurement asks" (H2.4, H4.4) | the class grain is today's, and an edge per member costs memory | the member grain is the design (`class_member` projections, existence from `declarations_named`, subclass tests from `class_ancestors`); `class_final` for members is the fallback S-3 can send it to |
| settings: "a finer split is the next step if a measurement says" (H2.2) | not seen that salsa tracks an input field by itself (`input.rs:230`) | a field per setting and a lazy per-check view, so a toggle reruns the files that consulted it |
| `decl_index` read each file's whole `decls` (H2.3) | the first cut | it reads `decl_names`, so a member or body edit never reruns it |
| `infer_global_variables_of` "stays what it is until C9", described as a fixed point (H3.7) | not designed; and wrongly described: it has no cycle | per-file facts, `global_sites`, `global_scope_files`, `global_site_index`, `global_variable(GlobalKey)` |
| lazy loading of context classes: "W3's call" (H4.1) | the locator was filed under W4 | decided: W3, with eager listing and lazy read; STUB-7.5 moves; its prerequisite (maps with keys, PSR-0) is named; S-11 |
| `ArrayKey`: "renamed, or aliased" (H1.2) | an alias saves 242 edits | renamed, no alias |
| the other report sites "convert as touched; nothing forces them" (I5) | the compiler was not made to force them | the checker's own `report` takes `Message` only, so every site converts |
| A1 gave the composer crate `vendor_dir` and left `config.vendor-dir` to "when it lands" | the setting was filed as someone else's follow-up, and it is in no ledger row | A1 reads it; B8's other composer follow-ups are Part E 40 |
| Part B6 said the stored-alias identity was "not designed" | stale after G2 | points at G2 |
| spans or token indices settled by an argument (D3, Part E 3) | the ruled text says a measurement settles it, and the argument rested on estimates | spike S-12 builds both layouts for the corpus before step 2 |
| payloads in the tree's own arenas (D4, Part E 4) | the plan was defending its own first design; its arguments held for the ruled layout too, and the arena still needed a side buffer for the bytes no source has | the ledger's layout: the tree owns its source, a string is a span, a side buffer holds the rest |
| every interned key made collectable, or every key immortal (H1.4, Part E 38) | the cheapest reading of "never reused", priced for the key and not for what it retains | `DeclKey` immortal, the name probes collectable, the retained memos measured by S-3 and bounded by an in-process compaction (H4.6) |
| `Constant` and `Case` as two member kinds (H1.4) | an enum case is a class constant in PHP (`const A` beside `case A` is "Cannot redefine class constant") | one exact-name kind for both |
| an analysed file with parse errors reported as parse errors only, for the wrong reason (Part E 31) | the benefit listed was what goes away, and "PHP runs nothing" does not make a finding in an intact region untrue | checked, behind a bar stated before the check is built |
| the hull of the roots' PHP ranges, with the exact form left to W4 (Part E 42, LOAD-7.6) | W2 would put one database-wide range into the memo keys and W4 would re-key them; the corpora differ within one repository (Symfony 202 manifests at `>=8.4.1`, 8 at `>=8.1`) | a range per owning manifest, designed with J1.2 and H2.2 |
| the plan's list of anonymous-class consumers (Part E 34, H1.4) | taken from a reviewer, not from the code | enumerated from the code: namespace cycles and `demo` keep them, workspace symbols keep their members, `registry/scope.rs` is not an iteration, the class-shape parent-call rules include them |
| `callable(int)` lowered silently as a bare `callable` (I4.2) | a parity argument about Psalm, and it discarded the parameters | a PHPDoc syntax error for the missing return type, and `callable(int): mixed` |
| the checker's `extract()` and `eval` truncating every diagnostic of the body, and `include` left to a flag (G3.6) | the flag was the minimum, and exact include modelling was called optional | `extract` makes every variable `mixed` and drops nothing else; exact include modelling is CHECK-12, on the scope model of Stage D |
| A1's skip rule: a vendor directory known by the path a manifest resolves, targets carried down the walk, a nested project's out-of-subtree target walked and a test saying so (A1) | the cheapest reading of decision 7's "resolved path"; the hole was written down as a limit, and a monorepo with a shared vendor and a manifest-less root regresses against today's name rule | a Composer vendor directory is recognised by what Composer wrote in it, a function of the directory's own listing: exact for any layout, no carried targets, no seeds, `Excluded` stays `Copy`; it changes the wording of decision 7 (ruled 2026-10-18, M5d) |
| A1's `vendor_dir` returning an `Option` with no fallback, and "every use calls it" (A1) | the list of uses came from the plan and not from the code (R8's `trees` and `unbuilt` in `Session::resolve` hard-code `vendor`), and no fallback loses the context that a stray variable or an unreinstalled config has today | `vendor_location` returns the resolved path and the installed one, over candidates in Composer's order; the include lookup is a caller; discovery takes `$baseDir` from the generated file's head |
| the composer follow-ups COMP-2, COMP-5 and COMP-6 "to be audited when worked" (Part E 40) | an audit deferred is a scope decision | audited on the corpora: the grammar's defects and an open upper bound (COMP-2), the root's own entries kept in A7's maps (COMP-5), A8 (COMP-6) |
| S-12's accept rule: a tree smaller by 10% and readers within 1% of the check phase (H5) | not on S-7's exchange rate: 10% of a 35 to 60 MB tree is 4 to 6 MB, which S-7's rate prices at about 0.1% of time, against up to +1% of the check phase | one exchange rate for both spikes (H5), stated once |
| A1 recognising a Composer directory by its marks alone (the first recommendation of the audit) | a fresh `composer install` with the editor open creates thousands of files before `composer/` exists, and each takes the role `classify` gives it and keeps it: the name rule gave them context, the marks alone would give them analysed code and diagnostics | decision 7's own rule for the target a manifest names beside itself, installed or not, together with the marks |
| COMP-1 reading `config.vendor-dir` and `COMPOSER_VENDOR_DIR` only | Composer's `COMPOSER` variable names the manifest file (`Factory::getComposerFile`; PHPStan's `getComposerJsonPath` reads it too), and four places join `composer.json` | one `manifest_path` function, with the variable read once by the session |
| A1's recognition of Composer's directories stopping at the vendor directory, and discovery's filter keeping only what lies under it (`Resolver::locate`) | "first-party or vendor" was read as "outside or inside `vendor/`"; Composer installs packages elsewhere by `installer-paths` (Drupal, WordPress), `installed.json` says where, a degrade root walked them as analysed code, and no mode had their classes as context | the install directories of a known root's non-path-repository packages are Composer's too (the seeds), and discovery admits their entries (COMP-7, ruled with "also (a)") |
| a degrade root having one Composer root (A8, `composer_roots`) | the roots were the nearest manifest above each configured directory, which for a degrade root is one: a monorepo's packages and a `tools/cs/` project were analysed with no context from their vendor trees, and every file was owned by the outer root | a scan before the walk finds the nested roots (211 below Symfony's, in the corpora), loads them in parallel and merges them in path order (COMP-8, ruled with "also (b)") |

Left as they are, each for a reason that is not effort: #35 (the interim dependents-index work is not built, since
REG-16.4 deletes what it feeds; ruled on 2026-10-18), #36 (what salsa's cancel model costs, which decision 3 already
took), #33 (probably no exception is needed at all; S-2 decides), the spikes' stated fallbacks, and the items of
I4.3 that wait for C7's name scope, which the lowering does not have.

## The sequence this gives

0. Level 9 finishes (the other session): R16 and R17 (R0 to R15 and R18 are built at `c5bec58c`); level 8 then resumes
   with this plan.
1. Part A's rows: A1 (its mechanism for the skip, its `config.vendor-dir`, `COMPOSER_VENDOR_DIR` and `COMPOSER` commit and
   its COMP-7 commits, all ruled on 2026-10-18, M5d), A2, A3, A5 (SYN-1.1), A5a (LOAD-5.4), A7 (the autoload maps keep
   their keys, the root's own entries included, and gain PSR-0) and A8 (the vendor context of every other Composer root: a
   package's bundled `vendor/` and a degrade root's nested projects, with A1's delta commit), then
   A6's validation against the batch's base binaries (REG-14.5 is folded into REG-7.5).
2. Batch 0, in this order: VAL-1.1; the seven fixes the crate split's F1 puts first, each with its test (#22); F10's
   tests and fixture, F77 (analysed files; its context half is a run-level notice that leaves the exit status, ruled
   2026-10-18), F20, F21 and F69's fixtures; every level-3 finding or Q-11 follow-up that changes a dump (W1), with
   the Part E 42 deltas apart (approved 2026-10-18 with their conditions, #42), and COMP-2's grammar (#40: it changes a
   range, so a dump; its open-above half is ruled, M5d);
   then item 0's tooling in G5's order and the baseline (#13, #22). F10's symptom and F57 are built.
3. Item 9: the error-node model designed (#9), then steps 0 to 3, with spike S-12 between step 0's census and step 2.
4. Batch 1 tokens (I2) with SYN-4.1, 4.8 and 4.9; 2 the lexer's second pass (I3 for SYN-3.2); 3 parse recovery
   (SYN-1.9, Q-10's parser half, SYN-6) with the `php-version` crate (#32); 4 names (S-1 first; item 16, then 15: H1.7)
   with SYN-4.5; 4b the constant lookup (REG-18.4 with CHECK-4.2, #41; a delta approved on 2026-10-18, #42); 5 products (item 13,
   without PROD-5.7: #35; PROD-5.1 with file-local anonymous classes and the site table: #34); 6 PHPDoc (F41 first,
   I4, then DOC-2.2 to 2.9 with REG-4.10 folded into 2.9, then TYPE-5); 7 the check's temporaries (the `Message` of
   I5, CHECK-6.4 to 6.6, 6.9 to 6.12, 6.16, 6.17 and the scratch reuse of 6.8 and 6.18; F69 and CHECK-9 are D2's: #17);
   8 is empty (LOAD-5.4 is Part A's; CACHE-13.1 and the LOAD-5.1 facts are W3's, #15 and #39; CACHE-8 after S-4, #41).
5. The spikes at their gate points (H5): S-12 before item 9's step 2 (it settles whether B3's nodes hold byte spans
   or token indices; step 0's census feeds it); S-1, S-2 and S-3 before batch 4, with H1 (the engine's identity
   model); S-4 right after S-3 and before W2's result types; S-6 before W2 step 3; S-8 with batch 4; S-10 before W3
   step 2; S-11 before W3 step 4.
6. The wall: Part D's W2 to W8, with W2 as H3.6, W3 as H4.7 (REG-16.4 also needs SYN-1.9), W3 including Stage F1
   and F2 (the seven fixed-first commits of F1 are batch 0's) and the six passes' facts, W4 as J1, W5 as J2 with C7 on
   I4's lowering, and W7 as J3 (item 18's reconciled core, F69 with CHECK-9 and CHECK-7's checker half in D2, exact
   include modelling, CHECK-12, on the scope model of D1 and D2, CHECK-10's operator sites in D3).

## What the answers rest on (each checked in the code or the docs)

- DOC-2.1, F41 and C7's grammar item are one design. TYPE-5 is F14, which the review itself orders
  after F41. The iterable arm is in `types/src/parse.rs`.
- F69: implicit calls record no throw point, and CHECK-9 reads the throw points.
- §12.10 of the 25 G report and the crate split's D3 and D4 describe the same port of PHPStan's
  `NodeScopeResolver`, down to record and replay. The 25 G audits adopted a no-interim rule (its
  §10.7).
- Item 0 was skipped: the 20 G plan's §9 makes oracles dumps, and the stack moved VAL-1 to the end.
  `FileBodies` still keeps statements in debug builds for the old oracles.
- Open parser findings: F10 (the bare `yield` shapes parse since `3b7e4d95`, through a list of tokens that cannot begin an expression; the review's exhaustive `starts_expression` and its tests are not there), F77 (the "file(s) skipped: parse error" line is
  still printed), F20 (no `xtask recursion`, no `stack` crate). F57's arrow body and its ternary
  error are already in the parser.
- The probe splices a receiver parsed from the live buffer into the last-good function
  (`completion.rs`, `build_probe_scope`).
- `FoldedName` and `ConstName` are not in the code. `Locals` has 415 mentions in 66 files and
  `Rc<Expr>` sits in 11 places. `wide` comes in through `imbl`.
- The seven fixes the crate split's F1 puts first are all open at `ffa7cccb` (the debounce thread has no stack size,
  `queue_change` does not write `latest_text`, `didClose` is a no-op, `--generate-baseline` writes the run's results
  alone, `--fix` patches at the earlier analysis's offsets, rename returns an edit for every span, and a closed
  output pipe is not handled).
- The workspace's `deny(unsafe_code)` very likely does not report the expansions of an external crate's macros (a
  probe on the pinned toolchain compiled an external `macro_rules!` and an external attribute macro that emit
  `unsafe` without a diagnostic); salsa's own expansion was not built, which is what S-2 does first.

---


# Part F. What still needs designing

What exists: Parts A and B are designed (B has the open points below). The 20 G plan's items 0 to
19 have designs written at `5fb858c9`. The ledger's rows carry specifications. Level 3's tasks carry
"Design" paragraphs written at `3eaddbd1`. The crate split plan has designs for C6 to G dated
2026-09-28. "Needs a design" below means the design is missing or no longer fits; a design that
exists but needs a read against HEAD is a check, and is listed after the table.

## Needs a design, in the order the work needs it

| # | design | what exists | what is missing | needed before |
|---|---|---|---|---|
| 1 | How the checker holds a tree (Part E 1). **Designed: G1** | B4's rule for ids in checker state; `Checker<'a>` borrows registry and settings, `Locals` has no lifetime, `Checker::new` has 30 call sites, `Rc<Expr>` sits in 11 places | whether `Locals` and the checker take a tree lifetime or root what they store (rooted: Stage D deletes the lifetime again); the rooted form for an expression kept past its walk; skimmed bodies' short-lived trees; the infer cache key | item 9, step 1 (it fixes the API's signatures) |
| 2 | Stored-alias identity. **Designed: G2** | `Rc::ptr_eq` merge tests on the condition and query-builder aliases (`control_flow.rs:446`, `:478`), meaning "both branches inherited this very store"; the `@var` memo keyed by a doc text's address (`var_tag_types.rs:290`) | whether the stored value keeps instance identity (a store stamp, or an `Rc` around the handle) or takes the node's identity, which would be a semantic change; the memo's new key | step 1 |
| 3 | The error-node model. **Designed: G3** | decisions 4 and 15; Q-10, SYN-1.9 and SYN-6 as needs; F77's reporting design | what an error node keeps (salvaged children) and how readers treat it; recovery points on the token stream; marks for versions; how the registry gets "an entry of unknown shape" | step 2 (layout); the rest before batch 3 |
| 4 | The probe as a checker entry point (Part E 26). **Designed: G4** | the current probe (`completion.rs`: synthetic statements, sentinel spans) shared with hover through `probe_receiver_type` | the entry point's inputs and outputs (a statement prefix, an expression handle from another tree), the hover path, static and script scopes | step 2 |
| 5 | Node layout per variant. **Designed: I1** (the census confirms it) | B3's shape and size limits | which of the 47 expression and 26 statement kinds are inline or in side tables, and the nested lists' layouts; it needs step 0's census | step 2, after step 0 |
| 6 | Item 0's tooling plan (Part E 13). **Designed: G5** | 20 G item 0: the printer, `registry-dump`, the expression-types dump, O2, the session, the edit replay | a refresh against HEAD (the registry's shape moved: templates, flags), the baseline procedure at a current commit, what is kept and for how long | batch 0 |
| 7 | Item 10's token payloads under decision 4. **Designed: I2** | 20 G item 10: `TokenStream` with payloads as `&'a Name` and `&'a [u8]` into an arena | the arena was dropped (it is not `Sync`): payloads become ids into the builder's tables, which grow while the lexer runs, so references into them cannot be held | batch 1 |
| 8 | SYN-3.2's kernel (Part E 10). **Designed: I3** | SYN-3.1's two types; the old SSE2 and AVX2 plan | the fused newline and non-ASCII kernel on `wide`: block size, tails, line-terminator rules, the `memchr` oracle | batch 2 |
| 9 | Parse recovery as a whole. **Designed: G3.4, G3.8** | (3) | the recovery algorithm; the load's failure reporting (F77); the editor's rule for a failed parse (`Project::update_from` parses before it unindexes, so a failed parse leaves the file's registry entries as they were) | batch 3 |
| 10 | The engine's identity model (Part E 18). **Designed: H1** | decision 3; REG-16.1's measurement; REG-16.2 and REG-8.6 as rows; NAME-2's weak tables | ids, interners and their lifecycle as one design; the API of `FoldedName`, `ConstName` and `ArrayKey`; NAME-2.1's thin name (a spike) | batch 4 |
| 11 | The single grammar's lowering (Part E 14). **Designed: I4** | F41's design (a `TypeNode` to `Type` lowering, the string parser deleted), DOC-2.1's (`phpdoc::lower`), C7's list | where the lowering lives (F41 moves the grammar to its own crate if `types` cannot depend on `phpdoc`), `parse_phpdoc_type`, how DOC-2.2's memo keys over it | batch 6 |
| 12 | The rule message type. **Designed: I5** | CHECK-6.10 and 4.1; the crate split's D6 `RuleErrorBuilder` | one lazy message type that is also D6's | batch 7 |
| 13 | The engine. **Designed: H2** | decision 3 (principles); REG-16.1 (cost of a query: 374 Ir, 67 per name per file) | the queries and their keys; read recording through REG-4.12's accessors (they are one design); early cutoff; threads and the pool (REG-16.1 did not measure queries on several threads); cancellation; panics (REG-7.9); cycles; revisions in a batch run | W2 |
| 14 | REG-7 and REG-8 as engine queries. **Designed: H3** | rows 7.1 to 7.12, 8.1 to 8.7 | each level's query, key and cutoff, on (13) | W2, after (13) |
| 15 | The pipeline as queries, persistence and the Session. **Designed: H4** | REG-16.4, CACHE-13.2, the crate split's F1 and F2 (F2's design superseded by decision 3) | one design: inputs, the result cache and its key, dependency fingerprints, `AnalysisResult`, `update(FileDelta)`, salsa's `persistence` or the cache's container | W3 |
| 16 | C5's data model. **Designed: J1** | decisions 6, 7, 10, 11, 16, 18; rows STUB-7.1 to 7.12, STUB-12 | version ranges as data, the stub layer on the engine, STUB-12.2's generator and its format, CHECK-10 | W4 |
| 17 | C6 to C9. **Designed: J2** (as rows) | the crate split's text (stale: per-class `OnceLock`, constant cycles typed `mixed`) | ledger rows with needs, and the text re-cut against decisions 1 to 18 | W5 |
| 18 | Stages D to G reconciled. **Designed: J3** | the crate split's D to G; §12.10 of the 25 G report | one inference design from §12.10 and D1 to D6, the rule API, E1 to E7, F3 to F5, G | W7 |

## Designed, but needs a read against HEAD (a check, not a design)

- 20 G items 11 to 19, written at `5fb858c9`. The premise has moved for 13 (PROD-1 to 3), 14
  (DOC-1), 15 and 18; the ledger's targets say where.
- Level 3's "Design" paragraphs, written at `3eaddbd1`. F57 looks built; the 136 count has stale
  items.
- Part B's measurements, re-taken at step 0.

## Specified, or waiting on a ruling

- LOAD-7.2's PHPUnit half and the deltas of LOAD-7.6, REG-18.4 and CHECK-11 were ruled on 2026-10-18 (Part E 42):
  the PHPUnit half stays ungated, and the three deltas are approved with their conditions. LOAD-7.8's settings are
  each "its phase".
- REG-17, CHECK-7 and CHECK-8: the semantics are fixed by decisions 9, 17, 1 and 8; their designs
  belong to their phases.
- Level 1: the roadmap's phases, not audited here.

## Order

Every row of the table is designed: 1 to 4 and 6 in Part G, 10 and 13 to 15 in Part H, 5, 7, 8, 11 and 12 in Part
I, 16 to 18 in Part J. What no reading settles is H5's eleven spikes (S-1 and S-2 before batch 4) and step 0's
census, which confirms Part I1's layout. Row 9 is G3.4 and G3.8.

---


## G5. Item 0's tooling plan (Part F 6)

The 20 G plan's item 0 (written at `5fb858c9`) is the design; this refreshes it against HEAD, builds
on what the gates already have, and says in what order and with what acceptance.

**What exists (verified).** `cargo xtask gates | corpus | compare | type-drift | baseline | gatetime |
modgraph | arch | front-end-dump`, and `tools/gate-runner`, which runs one analysis through the same
library calls `check-project` makes and writes, per configuration, the report lines
(`path, line, col, level, identifier, message, tip`), the emission-order view, the baseline run and
`--dump-types` (every tracked variable's type and native type after each statement, as display text).
Missing against item 0: the diagnostic lines drop the end position, `non_ignorable`, the fix (span,
replacement, description, `within`) and all but a joined tip; `Display` for types drops `mixed`'s
explicit flag and a template's bound, variance, scope and default; `front-end-dump` hashes `Debug`
text; there is no registry dump, no per-expression type dump, no PHPDoc record, no server session, no
edit replay, and no `oracle` profile.

**Tools, in build order.** Each is a commit or two; each ends with its own acceptance. The oracle run comes after the
session and the replay because VAL-1.2 includes the replay, and the oracles go (VAL-1.3) only after that run.

| # | tool | content | acceptance |
|---|---|---|---|
| 1 | VAL-1.1: read the starting state | HEAD against `1892b477` (the last validated state) over the vendor trees, all-on and unscoped reports; the six listed behaviour changes (Pratt rejections, `__halt_compiler`, kind-exact keys, hyphenated words, declarations ruled out by version, the server's UTF-16 columns) read; anything wrong fixed in its own commit | each difference named and either approved or fixed |
| 2 | canonical printers (VAL-1.4), after F21 (the error type as a kind of `mixed`, which changes `Repr::Mixed`'s fields) | `types::canon` over every field of `Repr` (explicit-`mixed` flag and subtraction; template name, scope, bound, variance, default; benevolent unions, call-site variance, accessories; literal bytes and float bits; shape optionality and sealedness; conditional subjects), no interning, addresses or hash order; the same for `Diagnostic` | every printer destructures without `..` and matches without `_`, held by an `xtask arch` rule that the `canon` and `dump` modules contain no `..` in a struct pattern and no `_ =>` arm (a test that must fail to compile cannot be an in-tree test: `Repr` is `pub(crate)`, and a copy of a type with an extra field is not the type the printer destructures), plus a one-time mutation check recorded in the commit message, as tool 4's; two runs and one thread against many give identical bytes |
| 3 | full-field O2 (VAL-1.8) | the gate-runner lines gain end line and column, UTF-16 columns, `non_ignorable`, the fix and every tip; run over Piwigo's three configurations, each vendor project with every rule set, and the fixtures; stub cache cold and warm; per-file cache off, written and read | all states give one output; `xtask compare` reads it |
| 4 | `registry-dump` (VAL-1.6) | taken after the finishing passes, before checking: every project declaration and every record of both stub bases (at the configured range and its ends once ranges exist), with reflection, guard and guard value, purity dependencies and verdict, property-write classification, ancestors, owners, the ancestry fact; the constants, global-variable facts, both reverse indexes, the use-site index, duplicate-site tables, the file table (path, role, load order, absolute path, Composer owner, `mentions_phpstan`). **Defined over an enumeration interface** (all declarations as reflection records, from a registry value), implemented for `Registry` now and for `Provider` at W2 step 1, so that after a level flips to the engine (H3.6 step 3) the dump prints the engine's answers and not the old maps' on both sides; the `Registry` field destructuring below is the pre-W2 completeness check and goes with the struct | sorted by kind, folded name and load order; lazy records decoded, deferred values printed as stored; the dump destructures `Registry` (31 fields at `registry.rs:262`) without `..`, so a field added later is a compile error, and **all 31 are classified**: scratch with its reason (`raw`, `raw_active`, `changed`, `scope`, `scoped_names`, `demand`, and the history-dependent `names_with_assert_tags` ("only ever added, never removed on `remove_declarations`") and `stub_provided_functions` ("not yet overwritten"), which an edited registry and a fresh one hold differently, so printing them would fail the replay's equality by construction) or products that are printed (`doctrine_entity_index`, `trait_users`, `subtype_children`, `recorded_parents`, `as_built`, the two configured assertion tables, and the rest); the printers of `ClassReflection` (about 45 fields), `FunctionReflection` (about 24), the member records and `Provenance` destructure exhaustively too, held by the same `xtask arch` rule as tool 2. The printers never print a version, an id from a process-global counter, an address, an absolute path or a temp-directory name, and a difference between two runs is a finding to triage (engine or dump); mutation test: changing one member's flag changes the dump |
| 5 | `--dump-expr-types` and the PHPDoc record (VAL-1.7) | every expression's inferred and native type in the final pass, keyed by file, span and host class (a trait method is checked once per host); every PHPDoc resolution keyed by file, docblock span, tag ordinal, host class and function | off by default: one `Option` test at the end of inference, nothing when off; the dump is identical across `PHPSTAN_RS_THREADS=1` and the default |
| 6 | the server session (VAL-1.9) | an in-process session over Piwigo and two vendor projects, driven by **a public in-memory session API of the `lsp` crate** (start, send, wait until quiet, run a closure over the `Project`, and a dump request), which `lsp`'s own tests, `tools/gate-runner` and the replay all use. The harness of `lsp/tests/helpers.rs` (`encode`, `decode_all`, `run_rooted`, `run_across_debounce`) is `pub(super)` and test-only today and drives the server's private `run_loop` and `Server`, so it cannot live in `testing` (the module graph gives `testing` no `lsp` edge, `testing` depends on nothing above `project`, and a dev-dependency from `lsp` back on it would be a cycle that links two copies of `lsp`): `testing` keeps the fixture runner only. `run_rooted` pumps all input and exit and returns only the decoded messages, and `run_loop` builds the `Server` inside and drops it, so stepping needs `run_across_debounce`'s thread-and-sentinel pattern (a `didChange` is applied on the debounce worker, so the end of a step is not the end of a message) and a hook into the server's `Project`. The requests: files and roles, `registry-dump`, diagnostics with UTF-16 ranges, code actions, definition, hover and references, and completion (after `->` and `::` at a seeded sample of receivers), signature help, document symbols and semantic tokens, each over a sample chosen by a hash of each path, since item 9's step 1 migrates every `lsp` reader (navigation, completion, signature help, document symbols) and step 2 rewrites the probe. `TempDir` and `TMPDIR` are under `~/.cache` (not the RAM-backed `/tmp`) and removed afterwards | the first dumps equal the CLI's (LOAD-1's equality) |
| 7 | the edit replay (VAL-1.10) | a fixed script over a seeded sample, **on a copy of the corpus** (a hard-link or reflink copy under `~/.cache`, removed afterwards: the Piwigo corpus is frozen and `vendor/*` are submodules whose changes `require_clean_tree` rejects), over Piwigo and two vendor projects: save one unchanged, delete a method, rename a class, change a parameter type, add a file declaring a class named but declared nowhere, delete a file; after each step the project dump, `registry-dump` and every file's diagnostics; at the end a fresh load of the edited tree | the end state equals the fresh load; a failing equality leads to commits of its own, as tool 9's |
| 8 | memory (VAL-1.11) | the dhat profile by holder with untrimmed backtraces, at this commit; the conversion and holder scripts move into `scripts/` first (they live outside the repository today, in `phpstan-rs-kept/prs-profile/dhat-heap-modes/analysis/`, with a hand-curated `OVERRIDES` table) | no allocation left unplaced (the 10,987 of M10), and the holder table at this commit is the new baseline, explained against MEAS-2's (which measured `791d29cb`) by the commits since, LOAD-3.3's -214 MiB among them; no heap noise band exists, so none is claimed |
| 9 | the `oracle` profile and its one run (VAL-1.2) | `gate` plus `debug-assertions` and `overflow-checks`, with `PHPSTAN_RS_NO_FILE_CACHE=1`, over the three Piwigo configurations, each vendor project with every rule set, the server session and the edit replay (the incremental paths hold oracle sites that only they reach); a temporary uncommitted counter shows every oracle site ran | every failure fixed in its own commit |
| 10 | retire the debug oracles (VAL-1.3) | `FileBodies` stops storing statements; `BodySource::body` and the debug-only re-exports go; the test oracles whose second implementation exists only for them go; assertions about one implementation stay (the file cache's round trip) | tests and fixtures pass; item 9's reader set is smaller by these |
| 11 | the baseline (VAL-1.12) | every dump above from the commit that adds the last tool, with the `gate` build at one thread and at the default count (byte-identical) and with the `oracle` build (no assertion); the ordered file lists with roles | stored with the binaries |

VAL-1.5 (the front-end dump) is item 9's: its canonical dump over handles replaces `front-end-dump`
and is baselined at the end of step 1.

**Where things live.** The writers sit with the data: `types::canon`, `registry::dump`,
`checker::dump` (types, PHPDoc), `syntax::dump` (item 9). The gate-runner gains `--dump registry |
expr-types | phpdoc | diagnostics`; `xtask dumps` records the set at HEAD under
`.corpus-runs/dumps/<commit>/`, and `xtask dumps-compare <a> <b>` diffs two sets and classifies every
difference by dump and file. The baseline's binaries are copied to `.corpus-runs/gate/bin/<commit>/<profile>/`
and its profiles to `~/.cache/prs-profile/<run>/` as soon as they exist, and nothing else is kept:
the dumps a comparison reads, and no per-commit hoard.

**What each dump protects.** Item 9: the expression-types dump (the whole checker moves to handles),
the registry dump (unchanged by it), O2, its own dump. Batches 1 to 3: its dump. Batch 4 (names):
registry, expression types, O2 (container-field keys are VAL-2.7's delta). Batch 5: registry, O2.
Batch 6: the PHPDoc record, expression types, registry. Batch 7: expression types, O2 (CHECK-6.4
bumps a format). The wall: `registry-dump` across REG-7, the edit replay across REG-8, REG-16.4 and
CACHE-13.2.

**Wall-time rule.** A wall-time accept in this plan (item 9's, the spikes') is a paired, alternating
comparison by `scripts/abab.py` with the ledger's rule: the pair count stated with the accept, every output
checked against the oracle, a Wilcoxon test at a stated p-value, and an Ir count where Ir decides (the VM has
no performance counters, and plain off runs vary by hundreds of milliseconds: 2,310, 2,296 and 2,878 ms).
`abab.py` has Piwigo's path and digest as constants today and is parametrised by corpus and digest, and item 9's
wall, Ir and heap comparisons measure a second corpus (a vendor project through `check-project` and
`incremental-bench`) beside Piwigo, since the target is general PHP.

**Risks.** The registry dump's field list moves with the registry (REG-4.x already changed it), so
the printers destructure everything: a new field is a compile error in the dump, not a silent gap. A
dump that costs minutes on Piwigo is acceptable; one that differs between runs is a finding to triage
(engine or dump). Nothing here gates on Piwigo alone: each dump runs over the vendor projects and the
fixtures too.


# Part I. The designs the batches wait for (Part F 5, 7, 8, 11, 12)

Each is a section, written from the code at `8a1ca1e0`. I1 is item 9, step 2's layout; I2 to I5 belong to
batches 1, 2, 6 and 7.

## I2. Token payloads under decision 4 (Part F 7)

**The problem.** 20 G item 10 gives `TokenStream` a `payloads: Vec<Payload<'a>>` holding `&'a Name`, `&'a [u8]`
and `&'a str` into an arena. Decision 4 dropped the arena (a `bumpalo` arena is not `Sync`), and the tree's name
table and side buffers (B3) grow while the parse runs, so no reference into them can be held across a push. The
stream cannot hold ids either without the lexer calling the builder for every token.

**The design: a token is a kind and a span, and its text is read at the span when the parser takes it.** The
parser holds the source it parses (it does, the worker's read buffer); the builder interns a name, takes a span of
the source or appends decoded bytes to a side buffer at the moment a node needs it. Nothing is materialized at lex
time that a node may not want.

```rust
pub struct TokenStream {
    kinds: Vec<TokenKind>,     // one byte: the 64 punctuation and structural kinds, the 62 keywords, and the new classes below (Cast, QualifiedName, Number, the string kinds, Error)
    spans: Vec<Span>,
    docs: Vec<DocMark>,        // each doc comment: the index of the token it precedes, and its span
}
```

A template or heredoc carries only its kind and its span: its parts are built when the literal is taken, so a skimmed
body pays for none (the first draft kept an `extras` and a `parts` array, filled at lex time for every template in
the file, with three cursors that had to move in lockstep through a skim that does not take tokens).

| class | the lexer records | the parser's `take` does |
|---|---|---|
| identifier, keyword used as a name | kind (`Identifier` or the keyword's); the `WatchList` bits are set in the file facts from the spelling, by the perfect-hash table of SYN-4.1 | `b.name(&src[span])`: hash once, probe the tree's name table, intern into the global interner on a miss |
| variable | `Variable`, span including the `$` | the name is `&src[span][1..]` |
| qualified name written without spaces | one `QualifiedName` token (SYN-4.8) | `b.name` of the slice, split at the last `\` by the header |
| number | `Number` (the lexer validates the lexeme and records an error for `08`, `1__0`) | parse the digits skipping `_` (SYN-4.9); an integer past `i64` becomes a float, as now |
| single-quoted string | `StringPlain` or `StringEscaped` (does it hold a `\\` or `\'`) | a span of the source for `StringPlain`, with no copy; decoded into `side_bytes` for `StringEscaped` |
| double-quoted string without interpolation | the same two kinds | a span when it holds no escape; otherwise decoded into `side_bytes` (PHP's escapes) |
| double-quoted string, backtick string, heredoc, nowdoc | `Template`, `ShellTemplate`, `Heredoc` or `Nowdoc`, the span of the whole literal, and a flag per token of the string family: has-escape, interpolates, **has-braced-part** (the skim reads this bit) | scan the span with the code that is `scan_interpolated` now and build the parts: a part is `Literal(span, needs_decode)`, `SimpleVar(span)`, `Property { var, prop, nullsafe }`, `Index { var, key }`, `NameIndex { name, key }` or `Complex(span)`, all spans of the source |
| heredoc indentation | nothing | the closing indent is read back from the closing line at the end of the span, and after each LF inside a literal part up to that many space or tab bytes are dropped before the part is decoded by flavour (double-quoted and heredoc decode `\"` differently, backtick decodes `` \` ``, nowdoc decodes nothing); a part whose first literal starts after the first line's indent keeps its span there. Today `strip_heredoc_indent` runs on the whole body before `scan_interpolated`, and a pinned test has `Literal "!\n  "` over `(3,17)-(4,7)`; PHP refuses an under-indented body line ("Invalid body indentation level"), which today is silently cut |
| inline HTML | `InlineHtml` (text only: before `<?=` or the end of the file) or `InlineHtmlOpen` (the text and the `<?php` tag that ends it: the span includes the five bytes of the tag, as `lex_inline_html` advances past them before it returns, which `checker/whitespace.rs` and the statement's span depend on) | a span of the source (less the tag for `InlineHtmlOpen`) |
| `<?=` | the `Echo` keyword with the span of `<?=`, as now | nothing |
| `?>` | `CloseTag`, the span of `?>` and the optional newline it absorbs (today `Token::Semicolon` with that span); the parser treats it as a statement end, and G3.4's stoppers can tell it from `;` | nothing |
| a cast `(int)` | `Cast`, span including the parentheses. **The lexer rule, exactly:** the parentheses hold a case-insensitive cast name between optional spaces or tabs and nothing else (php 8.5.4: `( int )`, `(INT)` and tab-padded forms are casts; a newline or a comment inside is not) | the spelling is `&src[span]` between them, spaces and tabs trimmed (`CastSpelling::from_name`) |
| everything else | the kind | nothing |

Two deltas follow from the cast rule, each with a fixture and approved on 2026-10-18 (M5b) with the others at batch 1: today
the parser recognises a cast by a three-token lookahead that allows comments and newlines between the tokens, which PHP
does not; and `foo(string);` lexes as a cast token and is a parse error, as PHP reports it, where it is a call with a
constant argument today. The oracle excludes `Cast` from its "exact" claims.

- **No payload array.** The 20 G design's `payloads` is replaced by the span every token already has.
  `Payload`, `Token`'s `Arc`, `String` and `Name` fields, `Tok`, `DocGroup`, `Supply`, `next_raw`, `next_tok`,
  `fill`, `pop`, the `VecDeque` and `raw_keyword` go; a keyword's spelling is the source at its span.
- **Fragments.** An interpolation fragment (`{$a['k']}`) is lexed, when the literal is taken, into a scratch stream
  taken from a small stack of them, over the same source with absolute spans, because the file's own stream is
  still in use. The parser's sub-parser reads that stream, as `build_interpolated` does now; the
  `ComplexExpr(String, u32)` text copy goes, being a slice of the source, **except where it was never a slice**:
  `${name[expr]}` is rewritten today to `$name[expr]` (the braces dropped), `${expr}` is re-spelled, and a bare
  `$a[key]` whose key fits none of the three simple shapes is rebuilt with quotes added (a pinned test has
  `"$arr[1a]"` as an index with the key `1a`, which php 8.5.4 refuses, as it refuses `"$arr[foo bar]"` and
  `"$arr[ 0 ]"`). So: `${name[expr]}` is a part of its own (`NameIndex`: the name span and the bracketed expression
  span, lexed as a fragment and built as `Index(Variable, expr)`); `${expr}` is the slice from the dollar to the
  closing brace; and the bare fallback follows PHP: a lexical error (G3.2 item 3), with the pinned test replaced by
  an error test, an approved delta with its `gates/test-renames` entry.
- **Doc comments** are `DocMark { before: u32, span: Span }`. The doc before the current token is the run of
  marks at its index (a cursor beside the token cursor); a single doc is a span of the source, and several are
  folded when a declaration or statement asks, the folded text written once into `side_text` for the group,
  however many statements share it. The rules
  `DocGroup` carries today are kept: a declaration takes the last mark, a statement the folded text (or the last);
  a doc before an attribute list wins and carries no `first_start` (`parse_leading_attributes` re-keys it into a
  fresh group), so the parser holds it as pending state across the attributes.
- **Lookahead at any distance** is `kinds[pos + n]`. **Skimming a body is the three-bit state machine it is today,
  over kinds:** brace depth opens on both `{` and `${`; the bits are *after a member operator* (`::`, `->`, `?->`),
  *after `function`* and *after `PHPStan\`*; and the triggers (the file reads the body at load) are `Eof`, inline
  HTML, a template with the has-braced-part bit, `function` followed by a name, `class`, `interface`, `trait`,
  `enum`, `global`, `include`, `include_once`, `require`, `require_once`, `yield` and `goto` (none of them after
  a member operator), `define`, and the `PHPStan\` prefix, the last two read from the identifier's source only
  for identifiers that begin with `d` or `p`. The oracle is `parse.rs`'s six skim tests, with `${$x}` and
  `QualifiedName` cases added. A skimmed region costs no interning and no string decoding, where today every
  token of it is lexed to a value and dropped.
- **Buffers** are per worker thread, cleared between files and emptied when the load ends; `reserve` starts from
  the source length divided by a constant the census sets (about 4 bytes per token at the project's average).
  **A retained-capacity cap** applies to the token arrays and the builder's vectors, as `KEPT_READ_CAPACITY`
  (1 MiB) does for the read buffer: capacity above it is released after each file, so one 100 MB file does not
  hold nine bytes a token on a worker until the load ends, and a 100 MB file is in the peak-RSS checks of step 3
  and S-7.
- **Errors** (G3): the lexer records a `SyntaxError` with the token's span and kind `Error` where PHP refuses the
  lexeme, and the parser treats that token as a literal operand. A NUL byte is an `Error` token and not `Eof` (the
  stream ends at the real end only), and an unterminated string's span is clamped to the source (G3.2).
- **The stream ends at `__halt_compiler`.** The lexer ends the stream at `__halt_compiler`, `(`, `)`, then `;` or
  `?>` in statement position (not after `->`, `?->`, `::`, `function`, `const` or a backslash), gives the final
  `Eof` the file-end span (the `Block` statement's span reaches the end today), records the data offset (the
  checker already knows `__COMPILER_HALT_OFFSET__`, `checker/global_constants.rs`) and lexes nothing after it:
  today the parser calls `skip_to_eof` (`lexer.skip_to_end()` sets `pos` to the end of the source without
  lexing), and a whole-file lex would otherwise lex binary PHAR data and report errors in it. A test whose payload
  holds `08`, `1__0`, a lone quote, `<<<EOT`, NUL and control bytes gives no warning, no error and the same
  tree.
- **The lexer's result** is `Lexed { stream: TokenStream, ignore_marks, warnings and errors, facts, line_index,
  comments (optional) }`, which replaces `Lexed`, `LexOutput` and `LexEnd`; the entry points that survive are the
  whole file, a fragment and in-PHP (the `tokenize_with_spans` and `with_comments` of `lsp/completion.rs`,
  `stub_attributes.rs` and `output/codeframe.rs`), and the shared name cache `parse_with_names` takes today
  (`NameCache`) is the builder's `b.name` path. **Ignore marks and file facts** are recorded in the lexing loop as
  now.

**What it changes in Part B.** B5's "until SYN-2 the lexer still makes a `PhpString` per literal and the builder
copies it" ends: after SYN-2 the lexer never touches the builder, and the builder's `name` and `text_span` are
called only by the parser. B3's side buffers are filled at take time and a span costs nothing, so a skimmed body
and an unclaimed doc add nothing to them. The token dump of B8 prints (kind, span, the text at the span, and for a string kind the decoded bytes through
the builder's own decode). **It is not "by construction" the same dump as the old tokens':** the new stream
differs from the old in an explicit list, and each difference is a delta with its fixture, or is mapped by the dump's
kind-name function to the old name: `Cast` replaces three tokens (and the two deltas above), `QualifiedName`
several, `Number` merges `Integer` and `Float`, six string kinds replace three, `CloseTag` was `Semicolon`,
`InlineHtml` and `InlineHtmlOpen` keep the old span, a NUL is an `Error` token and not `Eof`, and the stream ends at
`__halt_compiler`. The B8 baseline and this dump also cover a CRLF, a lone-CR and a BOM-prefixed copy of every
corpus file, made in memory (the corpora are LF-only, and the lexer's newline-sensitive sites, a `//` or `#`
comment's end, the newline `?>` absorbs and a heredoc's closing label, take the one terminator function of I3).

**Readers of token lists outside `syntax`:** `stub_attributes.rs` (`parse_native_type`), `output/codeframe.rs`,
`lsp/completion.rs`, `xtask/src/front_end.rs`, the `lex_parse` bench and the test helpers, and the fuzz targets
`lex_parse` and `token_stream_parse`, read `kinds`, `spans` and the source they hold; `Parser::new(&TokenStream,
src)` replaces the test constructors. **`token_stream_parse` is rebuilt,** because it feeds `Parser::new` an
arbitrary `Vec<Token>` with every span default today: it generates arbitrary kinds, prints each as a canonical
lexeme into a source and lexes that, and a `TokenStream` validator (spans inside the source and ordered, a flag
byte on every string-family kind) runs in `Parser::new` in debug builds and is satisfied by the generator.

**Oracle.** O1, O2 and the token dump; the property test that lexing a file and parsing it equals the old
streaming parse on every corpus file (the tree dump); a fuzz target that compares the old and new lexer's
(kind, span) pairs until the old one is deleted (one of the policy's listed exceptions, Part E 13). **Saving:**
the 20 G figure, about 1.7 G, from the plumbing alone (257 Ir per token, 1.77 G), and what a skimmed region no
longer interns or decodes, which no one has measured. **Measure, with a threshold:** callgrind of
`parse_with_ignores` and the existing criterion bench "lex phpstorm-stubs", off and warm, against the base
commit; whole-file lexing adds work the streaming parse did not do (a failed parse lexes the rest, a skim lexes
everything), so the accept is that lex plus parse Ir together fall by at least half of the 1.77 G plumbing
estimate (on the stubs and Piwigo), and if that work eats the saving the design is reconsidered (a bounded-batch
lexer that the parser can stop is the alternative); a measurement that does not show the saving is a finding, not
a claim.

## I3. SYN-3.2's kernel (Part F 8)

Read from `syntax/src/span.rs` (`LineIndex::new`: `memchr_iter` for `\n`, then `Utf8Chunks` for the columns of a
non-ASCII file) and `wide` 0.7.33's source (`~/.cargo/registry/src/*/wide-0.7.33`), which Part E 10 chose on merit
(it is also already in the build through `imbl`).

**What `wide` gives, and what it does not.** `u8x16` has `cmp_eq`, `max`, `min`, saturating add and subtract,
`move_mask` (the high bit of each byte, one bit per lane) and `any`/`all`/`none`. It compiles to SSE2 where the
target has it (baseline x86-64), to NEON on aarch64 and to a scalar `[u8; 16]` elsewhere, chosen at compile time
(`pick!` over `target_feature`). It has **no unsigned greater-than** on `u8x16`, and `u8x32` is real AVX2 only if
the build enables it, which the baseline build does not. So the kernel is built from equality masks and the sign
bit, in 16-byte blocks, with no run-time dispatch (Part E 10's answer).

**The kernel.** One pass over `src` produces the line starts and whether the file is ASCII:

```rust
pub(crate) fn scan_lines(src: &[u8], rule: Terminators) -> (Vec<u32>, bool)   // starts, ascii
```
per 16-byte block: `v = u8x16::new(block)`; `lf = v.cmp_eq(LF).move_mask()`; the vectors are OR-ed (`BitOr` exists
on `u8x16`) and one `move_mask` after the loop decides `ascii` (a `move_mask` per block for the high bits would
cost two or three per block on NEON); and, for the rule that counts `\r`, `cr = v.cmp_eq(CR).move_mask()`. The
block's line ends are `lf` for `Terminators::Lf`
and, for `Terminators::Php`, `lf | (cr & !(lf >> 1))` (a `\r` not followed by `\n` ends a line; `\r\n` ends it at
the `\n`), with one bit of carry across blocks: a `\r` in a block's last lane is a line end only if the next
block's first byte is not `\n`. Each set bit `i` gives the start `base + i + 1`, taken by `trailing_zeros` and
`mask &= mask - 1`. The tail (under 16 bytes) is copied into a zero-padded block and its masks cut to its length.
A start is a `u32` that cannot wrap, since the loader refuses a source of 2^28 bytes or more (B5).

- **One rule, applied everywhere a newline matters.** PHP ends a line at `\n`, `\r\n` or a lone `\r`, and the
  lexer applies it in more places than the line index (php 8.5.4 was run: a `//` or `#` comment ends at a lone CR,
  `?>` absorbs a lone CR, and a heredoc's closing label is found after a lone CR). Today the lexer ends a line
  comment at `\n` or `?>`, absorbs only `\r\n` or `\n` after `?>`, searches heredoc lines by `\n`, and the splitters
  (`codeframe.rs`, the source-line helpers) split on `\n`. So `Terminators` is one function used by the line
  index, the lexer's comment, close-tag and heredoc/nowdoc sites, the shebang skip and the splitters, and SYN-3.2
  ships `Php` in batch 2 with the lone-CR fixtures and their approved delta (a lone-CR file's comment no longer
  swallows the code after it, and its heredocs close): shipping `Lf` and leaving `Php` to F1's text, which
  changes "the lexer's line starts and the line index" only, would leave line numbers following CR while the
  comment swallowed the code after it, and would carry an unused enum arm that `clippy -D warnings` must be told
  about. `CRATE_SPLIT_PLAN.md`'s F1 text is edited to say so. The oracle is a scalar loop; for the old rule also
  the `memchr` version it replaces.
- **Types (SYN-3.1).** `LineStarts { starts: Box<[u32]>, len, ascii }` for every file, from the scan, one
  allocation (`into_boxed_slice` after the exact fill). `LineIndex` is `LineStarts` plus the column tables, built
  for analysed files and the server's open documents only; it is built from the starts and the source, and the
  tables are skipped when `ascii` is true.
- **The column tables** are built from `first_non_ascii(bytes, from)` positions (`move_mask` and
  `trailing_zeros` over the same blocks) with a decoder that looks only at a window of at most four bytes at
  each non-ASCII byte (the maximal ill-formed subpart is one character: `Utf8Chunks`' rule) and, for a source the
  load already knows is valid UTF-8 (`simdutf8` in `source::decode_text`; the lexer holds `text: Option<&str>`),
  takes the width from the lead byte alone. **Keeping `Utf8Chunks` and replacing only its position search would not
  deliver the saving:** `Utf8Chunks::next` is itself a byte-at-a-time scalar decoder over every byte, ASCII
  included (a copy of std's loop is in `bumpalo`'s `lossy.rs`; std's own current source was not readable here),
  so for a valid file one `next()` walks the whole file and the position search then walks it again, and the
  validation walk is most of the 83.4 M the column tables cost. The current `Utf8Chunks` version stays as the
  oracle on random invalid UTF-8, and what the column tables save is restated after that.
- **SYN-4.3's blank runs** use the same blocks: a mask of the bytes in the lexer's blank set (`cmp_eq` for each
  member, or-ed), and the run's length is the number of trailing ones (`(!mask).trailing_zeros()`), crossing to the
  next block while the whole block is blank.
- **Dependency:** `wide` becomes a direct dependency of `syntax`, pinned as the workspace pins its others
  (`=0.7.33`; Zlib or Apache-2.0 or MIT; it uses `safe_arch` and `bytemuck`, and holds `unsafe` of its own too:
  the `transmute` in `u8x16::new`, the NEON intrinsics, `unsafe impl Pod`). No `unsafe` of ours.

**Tests.** The scan against the scalar oracle on: every string of length at most 6 over `{a, LF, CR, 0x80}`
placed at each offset 0 to 33 inside filler of `a`, CR and LF (so the content, not the memory alignment, moves
across the 16-byte block grid: blocks are cut from the slice start); lengths 15, 16, 17, 31, 32 and 33 ending in
CR; a CR in lane 15 followed by a non-LF, by LF and by CR LF; a block of only CR LF pairs; CR at the end of the file
with a length that is a multiple of 16; the ascii flag with 0x80 in lane 0 and in lane 15; 100,000 strings of
length 0 to 200 over `{a, LF, CR, 0x80, 0xff, 0x00}` from a fixed-seed generator; files that end in `\r`, `\r\n` or
`\n`; and the corpus. The suite runs on the aarch64 and Windows legs and says so (`wide`'s scalar branch is never
forced on a leg; only the leg's own target is covered). `LineIndex::line_col` and `offset` on the multi-byte and
astral lines the existing tests use, built through the new path, equal the old. The blank-run and
`first_non_ascii` helpers against their scalar definitions the same way.

**Measure.** Off callgrind of `LineIndex::new` before and after, and the main thread's share: 270.4 M over 19,211
builds in the 20 G profile (63.5 M for 1.68 M `memchr` calls, 83.4 M for the column tables, 116.3 M of its own
and 7.2 M unattributed); the ledger's later 363.4 M is over 25,753 builds, stub files included, with no
breakdown; the baseline is re-measured before the change. The estimate that the scan costs about one
instruction per byte and under ten per line is a model made for SSE2; no saving is claimed before the run.
**Accept and kill:** the kernel beats the `memchr` plus `is_ascii` version by at least 20% (proposed) in Ir and in a
criterion bench of `scan_lines` on x86-64, and is not slower on the aarch64 runners (macOS and Linux) or on
Windows (R16's method: a criterion bench on each leg, since instruction counts are not time), where `move_mask` is four
NEON instructions and a horizontal add and `memchr` itself avoids it. Otherwise `memchr` stays and `wide` is
dropped, or the same API is written over `u64` words (eight bytes at a time, the classic zero-byte test). There is
no scalar-fallback trigger: `wide` takes its scalar branch only where neither `sse2` nor `neon` is a target
feature, which x86-64 and aarch64 never are.

## I4. The single grammar's lowering (Part F 11)

F41 (`STAGE_B_FIX_REVIEW.md`), DOC-2.1 (the ledger) and C7's grammar item are one piece of work. (Finding numbers
are ambiguous across the two review files: F40, F41, F51, F61 and F63 exist in both with different content; this part
cites `STAGE_B_REVIEW` F40, the PHPDoc parsing one, and `STAGE_B_FIX_REVIEW` F41.) Read from `types/src/parse.rs`
(1,429 lines, outline and the keyword table), `phpdoc/src/grammar/ast.rs` (`TypeNode`, 19 variants),
`phpdoc/src/doc_type.rs` and `registry/type_node.rs`, and the call sites (58 mentions of `parse_written_type`
in 3 crates, 219 of `parse_type_string`, 156 of them in `types`' own tests).

**Today.** A PHPDoc type is parsed twice. `phpdoc::grammar` lexes and parses it into a `TypeNode` (and reports
`phpDoc.parseError`); then `DocType::ty` renders the node back to text (`to_source_string`) and
`types::parse_written_type` parses that text again with a second, string-splitting parser, memoized by the text
(2,048 entries in each of 64 shards: 131,072 entries and 64 MiB of text at most). Each of F41's bullets is a place
where one of the two gets the language wrong. `types::parse_display`
is a third reader, of a different language (a type's `Display`, which `assertType()` and the type dumps are written
in); it stays, being `Display`'s inverse.

### I4.1 Where it lives

- **The lowering is `phpdoc::lower`.** `phpdoc` depends on `types` and `syntax`; `types` depends on `syntax` only.
  F41 considered moving the grammar into a crate below `types` so `types` could keep a text entry point. It does
  not need one: outside its own tests `types` calls its parser only from `parse.rs` itself. So `types` loses its
  parser, as the crate split's §3.3 says ("`types::parse_type_string` deleted"), and the grammar stays where
  DOC-2.1 put it. No new crate.
- **The type knowledge stays in `types`:** the keyword table (`parse_type_keyword_or_class`, about 380 lines of
  `int-mask`, `class-string`, `non-empty-lowercase-string`, `key-of`, ...) becomes `types::phpdoc_keyword(word, args:
  &[Type]) -> Option<Type>`, taking lowered arguments where it takes argument text now, and the builders
  the string parser's `try_parse_array_shape`, `try_parse_object_shape` and `try_parse_callable_shape` use become
  public constructors (`Shape::from_fields`, `CallableSig::from_parts` and the like) that `lower` calls. The string
  splitters (`split_top_level`, `top_level_find`, `matching_close`, `try_parse_conditional_type`,
  `parse_type_atom`) are deleted. **The extraction also completes the table** against `TypeNodeResolver`'s
  identifier and generic case lists, with a fixture per keyword: the string parser lacks `number`,
  `empty-scalar`, `non-empty-scalar`, `non-empty-literal-string`, `associative-array`, `static-closure`,
  `static-pure-closure`, `non-empty-mixed`, `callable-object`, `callable-array`, `empty`, the `__string*` pair,
  and the generics `__benevolent`, `new` and `template-type` (checked in the vendored `phpstan-src`, which is
  past 2.2.16; a newer upstream was not fetched), so each reads as a class today, is namespace-qualified and
  reaches `class.notFound`. `number`, like `scalar` and `numeric`, yields to a class or a `use` alias of that
  name (PHPStan's `tryResolvePseudoTypeClassType`), the one scope-dependent part, which C7 does; the rest of
  C7's "PHPDoc keyword types resolve before namespace qualification" is done here, and leaves C7's and RES-1's
  lists.
- **Entry points** in `phpdoc` (one name across Part E 14, F41 and here): `lower(&TypeNode) -> Type` (names as
  written, a leading `\` kept: what `parse_written_type` returns), `parse_phpdoc_type(text) -> (Type,
  Vec<ParseError>)` (lex, parse, lower) and `parse_global_type(text)` (the old `parse_type_string`: leading `\`
  stripped). The text entry points return the errors, because a string a user supplied must not turn into the
  error type, which accepts and is accepted by everything, with no report: `checker/constant_resolver.rs`,
  `restricted_symbol_usage.rs` and `registry/assertion_table.rs` parse user-supplied texts today with no error
  channel, and those callers report. Fixed texts (`predefined_constants.rs` parses nine per call) are statics
  (a `LazyLock` table, as 25 G says), and the text memo goes unless a dynamic caller measures a need for it. An
  error node lowers to the error type; a tag with an error contributes nothing when PHPStan parses that tag
  family (a `phpstan-` prefixed tag, or one in `PLAIN_TAGS_PHPSTAN_PARSES`), and the others apply as they are
  (`docblock.rs`). A *missing-token* error whose recovery is unambiguous (a `callable` with no return type, I4.2)
  is not an error node: the grammar reports it and the lowering keeps what the author wrote, so the tag keeps the
  recovered type.
- **`lower` is a walk generic over a scope** (the identity scope gives `lower`), living in `phpdoc`, so that C7
  adds an implementation (names, templates, aliases and `self` resolved in the walk) and not a second walk. It is
  written against the node accessors that DOC-2.7 keeps (children by iterator), so DOC-2.5 and 2.7 change the
  node's storage and not `lower`; after C7 `DocType` keeps the node, and the resolved type is DOC-2.2's memo value.

### I4.2 The lowering, by `TypeNode` variant

| node | lowers to |
|---|---|
| `Identifier(w)` | `phpdoc_keyword(w, [])`, else the class of that name as written; `$this`, `self`, `static`, `parent` as now |
| `Nullable(t)`, `Union`, `Intersection`, `Paren` | the normalizing constructors (`Type::union_of`, `intersect_types`); `?T` is the normalizing union of `T` and `null` (the string parser builds a raw nested union, a delta: flattened), and `?mixed` is `mixed` (tested by `is_mixed()`); **the written width is bounded:** `union_types_many` flattens every operand with `push_member`, a linear scan per member, and runs its passes before `MAX_UNION_MEMBERS` applies, so a written union of n distinct members costs at least n²/2 comparisons, and `eager` `DocType` runs the lowering for every docblock read; the lowering caps the written width at a multiple of the cap and generalizes as the cap does (or dedupes through a hash set before the passes), with a 50,000-member union, a 100,000-deep nest and colliding-name inputs (`FxHasher` does not resist collisions) in I4.5 step 3's inputs and S-1's |
| `Generic(w, args)` | `phpdoc_keyword(w, lowered args)`, else a generic class reference; `int<a, b>` is a range only for the exact word `int` (the string parser tests `ends_with("int")`, so `Point<1, 2>` becomes a range today, a delta); an argument of a keyword generic is lowered as the type it is, and a wildcard or a variance marker there is accepted as PHPStan does (ruled 2026-10-18): phpdoc-parser turns `*` into `mixed` with bivariant variance and PHPStan's `array`, `list` and the other keyword generics ignore variances, so `array<*, int>` is `array<mixed, int>`; only the class arm used them (`parse_generic_arg`), and today a keyword argument that holds one fails, a delta |
| `Wildcard`, `VarianceArg` | for a class generic as `parse_generic_arg` does: any type; call-site variance |
| `ArrayOf(t)`, `OffsetAccess(b, o)` | `array<t>` (list-ness as the string parser gives it today), the offset-access type with any type as the offset (the string parser read it as a shape key by name, a delta) |
| `ArrayShape { keyword, fields, sealed, unsealed_type }` | the shape constructors with `list`, `non-empty` and `object` refinements kept (`STAGE_B_REVIEW` F40) |
| `Callable { keyword, templates, params, return_type }` | `CallableSig` with its own templates marked; **without a return type** (`callable(int)`) the grammar records a PHPDoc syntax error for the missing return type, as phpdoc-parser and PHPStan do (the colon is mandatory there, and the port's `None` branch carries no note that its acceptance is deliberate), and the node is lowered as written, `callable(int): mixed`, so the tag keeps the parameters the author wrote, where the string parser's fallback, a bare `callable`, discarded them (ruled 2026-10-18; the missing-token recovery of G3.2 item 1, applied to PHPDoc); `Closure(...)` is the intersection of `\Closure` and the callable (`parse.rs`), not the bare class |
| `IntRange`, `IntLiteral`, `FloatLiteral`, `StringLiteral` | the literal types; a float only from a `FloatLiteral` token |
| `ConstFetch { class, const_name }` | the constant-fetch type, a trailing `*` kept; `Foo::class` is `class-string<Foo>` (`Type::class_string`, as the string parser has it) |
| `Conditional { subject, negated, target, if_true, if_false }` | `ConditionalSubject` and the three lowered branches (each branch is the node the grammar built) |
| `Error` | the error type |

The explicit `mixed` of a node is the kind F21 gives (`Mixed(Explicit)`), set at the keyword by `phpdoc_keyword`, as
the string parser already does at `Type::explicit_mixed()`; the text search for explicit `mixed` that C7 mentions is
gone (a70a5b42). Template and alias references are marked by the same later passes as now, until C7's resolver
takes the name scope.

### I4.3 What changes in behaviour (F41, with F19 and F40's grammar items)

`lex_number` follows phpdoc-parser's lexemes (`Lexer.php`, read at 2.3.5; the latest release was not checked): an
optional sign, `0b`, `0o` and `0x` integers and plain decimal digit runs, all with `_` separators and
case-insensitive (`0X1F`, `1E5`, `0B1`), and floats with a fraction or exponent, including a trailing dot (`1.`).
There is no leading-`0` octal in phpdoc-parser (`017` is a plain digit run). The values follow PHP's source
reading and deliberately differ from PHPStan's `(int)` and `(float)` casts of the lexeme (php 8.5.4: `0x1F` reads
31, `017` reads 15 and `0b11` reads 3, where `(int)"0x1F"` is 0 and `(int)"017"` is 17); an integer past `i64` is a
float, as PHP reads such a literal, but `-9223372036854775808` is the int `PHP_INT_MIN` (ruled 2026-10-18): the
sign is part of the PHPDoc lexeme, where in PHP source the literal overflows before the minus applies (php 8.5.4:
the source literal is `float(-9.2233720368547758E+18)` and `(int)"-9223372036854775808"` is
`int(-9223372036854775808)`), and a float would make `@return -9223372036854775808` reject `PHP_INT_MIN`.
Consequences, each with a fixture: a class named `Infinity`, `Nan` or `inf` is a class
(it was `INF` or `NAN`); `@param 1_000 $u` and `0x1F` parse (they gave `phpDoc.parseError`); `1e25` is `1.0E+25`; a
conditional's branches may hold `\Closure(): int`, `self::A` and `?T` (the first-`:` split gave `class.notFound`).
The string parser's other known errors: `value-of` and `key-of` of a list (parsing saw only a bare array or
shape; the lowering lowers the argument as a type, so `value-of<list<int>>` is `int`), and the differences listed
in I4.2 (each a delta with a fixture, approved on 2026-10-18 with M5b's PHPDoc group; the consequences above are
plain bug fixes). `numeric` and `non-decimal-int-string` already have their right types in
`parse.rs` at this commit, and a bare `class-string` stays a plain `string` by design (the comment there says it
has nothing to preserve); what needs the name scope is C7's. `TYPE-5` is not part of this: it is its own commit
after step 5 (below).

### I4.4 The consumers

- **`DocType`** holds the node and the lowered `Type` made when the docblock is read (eager, DOC-2.1): the `OnceLock`
  and the text memo go, and `WrittenType` for a bare `TypeNode` lowers it.
- **DOC-2.2's memo key** needs no node identity. Written types are hash-consed, so the lowered `Type`'s node is the
  identity of the written type (two docblocks that write `array<int, Foo>` share it), and the resolution context is
  interned (PROD-5.2's `ImportScope`, the template names, the aliases, the `self` names). The key is the pair
  (the interned `Type` itself, which the entry holds so its address cannot be reused, and the interned context's
  id): equality is pointer identity on exact forms and costs 8 bytes, and the value is the whole resolved type.
  The first draft keyed a 128-bit fingerprint of the node's `exact` hash; that is 64 bits of `FxHasher` from the
  node, so a collision would hand back another type's resolution with no equality check, and `Node.exact` is
  `pub(crate)`. If DOC-2.8's fingerprint is wanted it is a 128-bit hash over the node's structure behind a public
  `Type` API. A test: `\StdClass` and `\stdClass`, `a|b` and `b|a`, and explicit and implicit `mixed` get
  different keys.
- **The 58 `parse_written_type` and the Rust-side `parse_type_string` sites** (about 20 files of `phpstan-rs`) become
  `phpdoc::parse_phpdoc_type` / `parse_global_type` where the type is written as text for a reason (a builtin's
  return rule, stub synthesis; `predefined_constants.rs` takes statics), and constructors where it is a literal
  `Type::int()` in disguise.
- **`types`' 156 test mentions:** a test that reads a type from text moves to `phpdoc/tests`, where it can use the
  grammar and `Type`'s equality; a test of the relation, a union or a shape that only needs a type uses constructors
  (the crate split's "about 150"). The relation tests keep their text form in the crate that has a grammar.

### I4.5 Order and checks

1. The lexemes: `lex_number` and the quoted-literal lexeme, with their grammar tests, one for each lexeme. The
   string lexeme is a step-1 change and not C7's, since it needs no name scope: a backslash escapes any following
   character inside the token (phpdoc-parser's lexer pairs a backslash with any non-newline character; ours
   unescapes only backslash-plus-the-quote, so a literal ending in a backslash pair swallows its closing quote),
   the unescape rules are phpdoc-parser's (`\\` and `\'` single-quoted; `\\ n r t f v e`, `\xHH`, octal and
   `\u{..}` double-quoted), and `TypeNode::StringLiteral` and the shape key carry bytes (`PhpString`), not `String`.
   Fixtures: the one-backslash literal in a union, an escaped quote, a double-quoted escape, a shape key with an
   escape.
2. `types::phpdoc_keyword` and the public shape and callable constructors, extracted with the string parser still
   calling them (and the keyword table completed: I4.1).
3. `phpdoc::lower`. Its baseline is VAL-1.7's PHPDoc record (batch 0), compared through VAL-1.4's canonical
   printer, so the comparison is exact and the string parser need not stay alive; the per-`TypeNode` comparison
   with `parse_written_type(node.to_source_string())` over the stubs, Piwigo, the vendor projects and the fixtures
   runs for one stage only if that record cannot see what it finds (one of the policy's listed exceptions, Part E
   13). Either way the differences of I4.2 and I4.3 are the list the comparison starts from, and each new
   difference is read and added to it with a fixture.
4. `DocType` eager; the callers converted module by module; DOC-2.2 on the memo key.
5. The string parser, its memo and `to_source_string`'s round-trip use deleted; `types::parse` is gone but for
   `parse_display`. `registry/type_node.rs` (`parse_type_node`) becomes `phpdoc::lower`.
6. TYPE-5, **its own commit after step 5** (batch 6), once template and alias references are marked: it is the
   ledger's rule (the array member takes `K` as `array<K, V>` does, and a float or object key leaves only the empty
   array), with fixtures for a template key, an alias key and a class key, because `K` is a template, an alias or
   a class name when it is lowered and only the resolution step knows. F14's design text, which restricts `K` in
   the iterable arm and drops the array member when the intersection with `int` or `string` is never, is
   superseded by that rule; never inside `phpdoc_keyword`, which stays behaviour-exact.

Oracle: O2 and `--dump-types` over the corpus, with I4.2's and I4.3's differences read. Exact otherwise.

**C7 later** adds the name scope: `TypeNodeResolver` is an implementation of the scope that `lower` is generic over
(I4.1), calling the same `phpdoc_keyword` and shape builders with the resolution of names, templates, aliases and
`self` done in the walk, and `phpdoc::lower` is its context-free case; no second walk is written. C7 also keeps
the pieces of the crate split's C7 text that no row names: `TypeNodeResolverExtension` (the extension point,
registered through the `Extensions` input of J2) and the assert targets (`$param->prop`, `$param->method()` and
`$this->method()` are kept), `@throws` and assert types getting alias substitution, and a written `Traversable`
resolving like any name (J2's RES rows).

## I5. The rule message type (Part F 12)

CHECK-6.10 (messages formatted only when reported, 37 rules), CHECK-4.1 (a message kept unformatted past the
report) and the crate split's D6 (`RuleError`, `RuleErrorBuilder`) need one type. Read from `Checker::report`
(`checker.rs:1507`), `report_builder`, `rewind_to` (`checker.rs:1295`), `reports_are_dropped` and
`diagnostics::DiagnosticBuilder`.

**Today.** `report(message: String, span, level, identifier)` has about 440 call sites in `checker/` (and 8 in
`project/mock_hygiene.rs`) and `report_builder(DiagnosticBuilder)` 113; a message is `format!`ed at the call, so a
type is displayed even when the report is dropped by `rewind_to` (a
loop's second pass, 30 of the 55 `offsetAccess.mightNotExist` messages, about 409 K Ir each), by
`reports_are_dropped()` (after the string is built), or by the level filter (22 more, which E5 removes by not
running a rule above the level). Several rules format a subject for every element they check, reported or not
(`phpdoc_type_compat.rs:359`, 21,919 of them). `report` builds the `Diagnostic` at once, converting the span to a
line and column with the file's `LineIndex`, and pushes it; `rewind_to` truncates the vector. **Not everything
pushed is a `report` call, and not every drop is before the check ends:** `check_trait_methods` checks a trait
once per host, then sorts its own slice of the diagnostics by (span, message), dedups by span and message and
retains by identifier and span (`function_decl.rs`, all before the check ends); the rules' `check_stmt` and
`check_expr` return `Vec<Diagnostic>` that the checker extends (`stmt.rs`, `expr.rs`);
`check_too_wide_property_types` and eleven `self.diagnostics.push(DiagnosticBuilder::new(..).build(self.line_index))`
in `phpstan_api.rs` push ready-made `Diagnostic`s; and `sort_diagnostics` (in `diagnostics`) breaks ties by the
rendered message.

**The type.** One `Message`, with `Own` in `syntax` (the impl for `Name`), the impl for `Type` in `types`, and
`Message`, `message!` and `RuleError` in `diagnostics`, which depends on `syntax` alone and so can name `Own` and
`Name`; a `Message` is a boxed closure and names neither `Type` nor `Name`, only the `Own` impls do. That keeps
`RuleError` where the crate split's 3.7 puts it (`diagnostics`).

```rust
pub struct Message(Repr);
enum Repr { Text(String), Deferred(Box<dyn FnOnce() -> String>) }
impl Message {
    pub fn from_static(s: &'static str) -> Message;  pub fn text(s: String) -> Message;   // the only constructors
    pub fn render(self) -> String;  pub fn rendered(&mut self) -> &str                   // rendered: caches into Text, for the dedup and the sort's tie-break
}
// interim, steps (1) to (3) only: impl From<String> for Message {}   (deleted in step 4)
trait Own { type Owned: Display + Debug + 'static; fn own(self) -> Self::Owned }       // Type and Name by clone, &str by Arc<str>, numbers by copy, a Display newtype
message!("Parameter #{} ${} of {} expects {}, {} given.", i, &name, &func, &expected, &given)   // deferred; each argument owned once
message!("Offset {dim} might not exist on {container}.")                              // captures too
```

- `message!` is a proc-macro in a small `phpstan-rs-macros` crate (a new crate: Part E 32's file set), because a
  `macro_rules!` cannot number its arguments and 464 of the checker's 849 `format!` lines use inline captures
  (`{dim}`, `{key:?}`). It parses the literal, owns every positional and captured argument through `Own` (once,
  at the call: `Type` and `Name` clones are refcount increments), accepts `{x:?}` and width specs (so
  `Own::Owned: Debug`), and defers the `format!`; its dependencies (`syn`, `quote`, `proc-macro2`) are judged on
  merit like the others, and the build-time gate is checked. A message built from text stays `Text`: an
  external rule's `RuleErrorBuilder::message("...")` costs what it does now.
- **Pending reports.** The checker's `diagnostics` vector holds `Entry::Pending(Pending { message, tips:
  Vec<Message>, span, level, identifier, non_ignorable, fix })` or `Entry::Done(Diagnostic)` (the ready-made
  ones above, until D6 retires the external splices); `report` pushes a `Pending`; `rewind_to` truncates, so a
  dropped report is never formatted and never converted to a line and column; `into_diagnostics()` renders the
  survivors and builds their ranges, then sorts. So "displayed" means "the file's check ended", which is after
  every drop the checker makes **except the trait path**: `check_trait_methods` renders its slice's messages
  (`rendered`) before it dedups, because the dedup needs message equality, so a trait method costs hosts ×
  findings renders. `Message` has no `PartialEq`; `rendered` is the only way to compare.
- **Tips and fixes** are `Message`s too (a tip is a message; a fix's description is). A fix's replacement text,
  which is code and not prose, stays a `String`.
- **Identifiers** are `&'static str` (CHECK-6.10's "a closed set"): the sites that build one with `format!` are
  about 52 (43 `report(` calls and 11 `DiagnosticBuilder::new(` calls, 21 files, e.g. `duplicates.rs`'s
  `"{kind}.duplicateEnumCase"`), and they become members of the families the batch builds: a const table, kind ×
  suffix, debug-asserted against the closed set, in step (3), so the sites are static now and D6 adopts it;
  `Identifier` keeps its `Cow` for rules outside the package.
- **`RuleError`** (D6) is a `Pending` without the file: `RuleErrorBuilder::message(impl Into<RuleMessage>)`,
  `.identifier(...)`, `.tip(...)`, `.non_ignorable()`, `.fix(...)`, where `RuleMessage: From<String> +
  From<Message>`, so an extension's string still works while the checker's own methods take `Message` only. The
  dispatcher turns each into a `Pending` of the checker it runs in (or straight into a `Diagnostic` where no
  checker is), attaching the file and the trait file.
- **The engine boundary.** `FileCheck.diagnostics` holds rendered `Diagnostic`s, exact values that compare and
  persist (H1.5b, H4.6); a `Message` never crosses it. A warm run that reads a persisted check renders nothing.

**Order.** (1) `Message`, `Own`, `message!`, `Entry`, `report` taking `impl Into<Message>`, `into_diagnostics`
rendering; the interim is `report`'s `impl Into<Message>` signature and `impl From<String> for Message`, so every
existing call site compiles; the trait path's `rendered` and the `Entry::Done` pushes come here too. (2)
`report_missing_offset` (CHECK-4.1's measured site). (3) CHECK-6.10's 37 rules, one commit each family, in two
kinds: a site whose element is rarely reported carries borrowed parts or `Name`/`Type` newtypes (a lazy `Display`
subject, as 20 G designs it) to the report and formats there, because `message!` allocates a `Box` and an
`Arc<str>` on the kept path, and only a site that reports and then drops uses `message!`; kept-path allocations
are measured before and after in the check phase's callgrind, as the plan already promises for the scalar-only
sites. The identifier families (a const table) land here too. (4) Then every other site, about 440 of `report`
and 113 of `report_builder`: the checker's own methods stop accepting a `String` (they take `Message`, and the
interim `From<String>` is deleted), so the compiler names each remaining site and none is left to be converted
someday. A literal becomes `Message::from_static`, anything with arguments `message!`, and `Message::text` is the
explicit eager form: it is in `disallowed-methods` (a per-package list in `crates/phpstan-rs/clippy.toml`, so
package-wide until the split makes `checker/` a crate, and the 20 or more existing
`allow(clippy::disallowed_methods)` sites show how far that list already reaches), and each use carries
`#[expect(clippy::disallowed_methods, reason = "...")]` that an `xtask` counts, so the count ignores the older
allows. The end state is measured (callgrind of the check phase, O2): a net regression on the reports that are
kept would send the scalar-only sites back to `Message::text` by that explicit, counted `expect`. D6 builds
`RuleError` on the same `Message` when it is reached; the rule API takes `RuleMessage`, so an extension's string
costs what it does now.

**Checks.** `message!` renders as `format!` for each `Own` impl (integers, `&str`, `String`, `Type`, `Name`, a
`Display` newtype, a Debug capture); a dropped deferred message is never rendered (a test-only counter in the
closure); `rewind_to` then `into_diagnostics` gives the diagnostics of the kept passes only; a test of the trait
path (hosts × findings, the dedup by `rendered`); a targeted test per family between conversion commits, and O2
(every message and tip byte-identical) once, at the end of batch 7 (the user's cadence: validation at the end of
the item, never per commit). **Estimate, unmeasured:** CHECK-4.1's 22.5 M is for all 55 `offsetAccess` formats; the
22 the level filter drops are dropped in the finalizer, after the checker returned, and E5 removes them by not
running the rule, so this step saves about 13.5 M (33 messages, labelled an estimate) and the other 9 M waits for
E5; the 37 rules' temporaries (21,919 and 46,005 allocations are two of them); nothing is claimed before a run.

---


# Part K. The task-by-task review of 2026-10-18

Asked as: is every design the most proper, or does any still avoid churn. The first answer searched the text for
deferring phrases (the audit in Part E), which the user did not want as a method. This part went through the tasks in
the order of "The sequence this gives" and, for each, read its ledger row (or its finding), the plan's design for it
and the code it touches, and checked three things: that the design meets the row, that its facts hold in the code at
`52844b07`, and that it holds on the platforms the project ships (`CROSS_PLATFORM_PLAN.md`) and against the tasks it
meets. A verdict is "sound" (the row, the design and the code agree) or what changed. Nothing was built or run.

## K1. The tasks, in order

| # | task | verdict |
|---|---|---|
| 0 | the start condition | **changed.** Level 9 is now the cross-platform plan, not the items 1 to 3 fixes, which finished at `791a57c6` (R0 to R12 and R18 are built at `2c7bf898`, R13 to R17 open; this row's first version said R0 to R5, and the workflow review found it stale twice). Its rows edit the files Part A edits. The intro says so |
| 1 | A1, LOAD-2.2's last part | **redesigned twice.** First, a list of known vendor directories missed a nested project's `vendor/` (the name test skipped it) in a monorepo opened at its root, so the rule became local to the walk, `vendor_dir(dir)` where a listing holds `composer.json`. Then, in the audit of 2026-10-18, that rule was found to walk a nested project's out-of-subtree vendor directory, to need carried targets and a fact from the parent in the reach tables, and to miss R8's `trees` and `unbuilt`: a directory Composer wrote into is now recognised from its own listing, and the project's directory is `vendor_location`, over candidates (A1; the mechanism was ruled on 2026-10-18, M5d), and the install directories of packages Composer put elsewhere are Composer's too (COMP-7). It follows level 9's R7 (the case rule on the names) |
| 2 | A2, LOAD-9 | **amended.** The listing shortcut for the install path is exact only if `vendor/composer` is a real directory (a linked one is tested); discovery, spawned first, takes the one `vendor_location` answer; `DirListings` is safe to ask from both threads (read) |
| 3 | A3, REG-14.4 | **changed.** A predicate `Fn(&str) -> bool`, not `&[Param]`: item 9's step 1 turns the list into a view and the assert code then needs no second change |
| 4 | A4, REG-14.5 | **folded into REG-7.5** (Part E 41). Decision 8's final-level rule replaces `reconcile_with_native` and both its callers; the function is written there as A4 now describes |
| 5 | A5, SYN-1.1 | sound (the loop; the guard in `Drop` stays until step 2) |
| 6 | A6, bookkeeping | sound; one line removed with A4 |
| 7 | VAL-1.1 | sound (`1892b477` exists; the six listed differences are item 0.1's) |
| 8 | F10 | **already fixed in form.** `can_begin_expression` (`parser/expr.rs:211`) lists the tokens that cannot begin an expression, `and`, `or`, `xor`, `instanceof`, `&` and `\|>` among them, since `3b7e4d95` (2026-10-03, after the review's design); `php -l` accepts every shape the finding lists. Unbuilt: the exhaustive allow-list (a new token must be classified), its table test, the seven parser tests and the fixture. Tests and fixture in batch 0; the predicate on SYN-2's `TokenKind` in batch 1 |
| 9 | F20 | sound; after level 9's R10 (which edits `syntax::stack`, and is built) and before item 9, so its lint checks the recursion item 9 writes; its `stack` crate restores `LIMIT` from a drop guard so that it runs on unwind too (H2.8) |
| 10 | F57 | **already built** (checked in `parse_arrow_fn` and `parse_ternary`); its box in the review is stale |
| 11 | item 0's tools, VAL-1.2 to 1.12 | **reordered.** VAL-1.2's run includes the edit replay, so the oracle run follows the session and the replay; F21 precedes the printers (it changes `Repr::Mixed`) |
| 12 | level 3 | **recounted and placed** (K2) |
| 13 | the error-node model, G3 | **one addition.** A file's validity range (its marks) is its declarations' availability (decision 15, Q-10), which `declarations_named` now returns with each candidate |
| 14 | item 9, step 0, SYN-1.11 | sound (the fabrication sites B6 puts in step 0 exist: `static_call` and `constant_fetch`, `var_tag_types.rs:537`, and `phpdoc_type_compat.rs:268-300`; `calls.rs:777` follows in step 1 and the probe in step 2) |
| 15 | step 1, SYN-1.3 and 1.12 | sound |
| 16 | step 2, SYN-1.2, 1.4, 1.5, 1.7, 1.8 | sound against the five rows; **I1.5 gains closure and arrow-function doc owners** (the Stage B review's C2 needs them) |
| 17 | step 3, LOAD-3.5 | sound; the engine design is corrected to agree with it (K3) |
| 18 | batch 1, SYN-2 | **amended.** `LexEnd`'s second pass is the lex-the-rest after a failed parse, which the whole-file stream has none of; but a whole-file lex must stop at `__halt_compiler` (the parser calls `skip_to_eof` today, and the data after it is binary), a NUL byte is an `Error` token and not `Eof`, and templates, heredocs, casts and inline HTML need the rows I2 now has |
| 19 | batch 2, SYN-3.1 to 3.4, SYN-4.1 to 4.10 | sound; SYN-3.3 is not an interim (SYN-3.1 gives a context file no columns); SYN-4.1's lookup half goes (the lexer interns nothing); SYN-4.5 moves to batch 4, since it is `ConstName` |
| 20 | batch 3, SYN-1.9, Q-10, SYN-6, F77 | sound |
| 21 | batch 4, NAME-2.1 to 2.3, NAME-1.1 to 1.6 | **three notes.** NAME-2.2's weak pairs are for the type tables, and names are held strongly and swept (H1.3); NAME-1.2 converts the `String` keys of the machinery W2 and W3 delete, because NAME-1.3 cannot delete `FoldKey for str` otherwise; NAME-1.5's hash must not depend on chunking (S-1) |
| 22 | batch 5, PROD-5.1 to 5.3, 5.5, 5.6, 5.8 | **PROD-5.5 keyed by ordinal, not span** (H1.5a); PROD-5.4 is deleted by REG-16.4; PROD-5.7 not built (Part E 35) |
| 23 | batch 6, DOC-2.1 to 2.9, TYPE-5 | **amended.** TYPE-5 is its own commit after I4.5 step 5 (it is the ledger's rule; F14's design text is superseded), never inside the behaviour-exact `phpdoc_keyword` extraction; the extraction also completes the keyword table |
| 24 | batch 7, the check's temporaries | sound; CHECK-6.8 and 6.18 reuse only what holds no tree handle (G1); CHECK-4.2 goes with REG-18.4 (Part E 41) |
| 25 | batch 8, LOAD-5.4, CACHE-13.1, CACHE-8 | CACHE-13.1's stamp follows decision 13 (the stat tuple on known file systems, the content hash on Windows and elsewhere); **CACHE-8 waits for S-4** (Part E 41). The second pass moved LOAD-5.4 to Part A and CACHE-13.1 to W3 (Part E 15), which leaves batch 8 empty |
| 26 | spikes S-1 to S-11 | S-1 gains the hash contract; S-3 the member grain; S-6 the mixed-strategy cycles; S-7 is rewritten (K3); S-11 is new |
| 27 | W1, the fillers | LOAD-7.3 follows level 9's R6 and R7 (one matcher; `report_path` and the case rule are its inputs) |
| 28 | W2, REG-4.2, 4.12, 4.15, 16.2, 16.3, 7.1 to 7.12, 8.1 to 8.7, 9 to 12 | **H3.6 gains REG-4.15, REG-4.11 and REG-11**; REG-17 gets its layering (H3.8); salsa's cycle rule is stated (H3.3) |
| 29 | W3, REG-16.4, CACHE-13.2, LOAD-3.1, LOAD-5.x, TYPE-4, CHECK-7, CHECK-8, REG-17, F1 and F2 | **trees corrected** (K3); STUB-7.5 moves here; the persisted file is built for every platform and one platform's file is never read by another's (H4.4); TYPE-4.2's "provisional answers" cannot come from the hierarchy, which has no salsa cycle, and the relation reads resolved types, not constants' values |
| 30 | W4, STUB-7.x, 8, 12, CHECK-10, STUB-13.1 | **`effective_range` and the phar locator** added (J1.2) |
| 31 | W5, OVL, RES, EXT, SUM | rows written from the crate split's C6 text; the workflow review found omissions and stale figures (W-10 to W-12), which J2 now corrects (RES-7, OVL-3, OVL-1, EXT-1) |
| 32 | W6, STUB-10, 11, 13 | sound |
| 33 | W7, Stages D to G | reconciled in J3 against D1, D3 and D4 (read first), and by the workflow review against E1, E7 and G1; **not yet read:** D5, D6, E2 to E6, F1 to F5 and G2 to G5, whose crate-split text stays the authority for each phase |
| 34 | W8, VAL-1.13 to 1.15 | sound |

### K1b. The wall's rows, one by one

Each row below was read in the ledger and set against H1 to H4 and J1 to J3. "H3.1" and the like say where it is
designed.

| row | verdict |
|---|---|
| REG-2 | sound (an `xtask` inventory) |
| REG-4.5, 4.7, 4.13, 4.14 | sound; flags and options carry no position (H1.5a); 4.14 is H2.7's determinism rule |
| REG-4.9 | built with REG-4.12: its `Option<&T>` and slices are the `Provider`'s `&'db T` |
| REG-4.10 | folded into DOC-2.9 (batch 6): the builder parses each doc comment once by reading the file's `doc_blocks` (H2.3) |
| REG-5 | sound; its case delta is F50's |
| REG-6, REG-18.1, 18.2, 18.3, 18.5, 18.6, CHECK-11 | sound |
| REG-18.4 | sound; takes CHECK-4.2 (Part E 41) and `ConstName` (H1.2) |
| LOAD-7.1, 7.4 to 7.7 | sound; 7.3 follows level 9's R6 and R7; 7.2's PHPUnit half still needs the user's ruling; 7.8 is each setting's phase |
| REG-4.2 | sound (H1.2: `FoldedName` keys; two names differing in case are one name, so two declarations become `Many` and a duplicate report) |
| REG-4.12 | sound (H2.4: about 280 sites become `Provider` calls; the grain of a read is the query that answers it) |
| REG-4.15 | its span field withdrawn (H1.5a, Part E 34) |
| REG-4.11 | sound; after REG-8.5 and 10 (H3.6 step 5) |
| REG-16.2 | "never reused" reworded (Part E 38); the ids are named in the engine module only (H1.6) |
| REG-16.3 | sound; purity and global variables are queries (H2.3, H3.7), further than the row's "run again per revision" |
| REG-7.1 to 7.5, 7.12 | H3.1; 7.5 is decision 8's rule at the final level and replaces `reconcile_with_native` (A4) |
| REG-7.6 | H3.3 (`cycle_result`; the fallback per constant for the PHPDoc case is S-6's) |
| REG-7.7 | H3.5 (`ClassDecl` keeps the scratch the row names) |
| REG-7.8 | `finish_loading` stays until W3, then `Session::load` (H3.5, H4.1) |
| REG-7.9 | H2.8 and H3.3 (a cycle is homogeneous) |
| REG-7.10, 7.11 | H3.5 |
| REG-8.1, 8.3, 8.5 | H3.5, with `file_decls` an input in W2 (H3.6) |
| REG-8.2 | superseded (the recorded reads) |
| REG-8.4 | H3.5 (`Provider`; no `ReflectionBuilder` is split off) |
| REG-8.6 | re-specified (H1.3) |
| REG-8.7, 9, 10 | H3.5, H3.6 step 4 |
| REG-11.1 to 11.16 | H3.6 step 6; 11.14 is superseded; 11.4 and 11.5 are F75 and F35, F55 |
| REG-12 | H3.6 step 7; its "resolves no class the file does not reach" is a count from the event callback |
| REG-16.4 | H4.1, H4.7; trees corrected (K3); needs SYN-1.9 |
| CACHE-13.2 | H4.6; keyed per config key (Part E 37); the stamp follows decision 13 |
| CACHE-9 to 11 | after S-4; one format bump |
| LOAD-3.1, 3.3's `update_file` half | H2.9: a tree is kept by (role, open): `parse_analysed` for an analysed file, under `lru` in the server; a context file's is never kept |
| LOAD-4.7's index half, PROD-5.4, PROD-5.7 | deleted by REG-16.4 (H4.8, Part E 35) |
| LOAD-5.1 | E1's collectors (Part E 39); the use-site index is `file_products.use_sites` |
| LOAD-5.2 | memory only; S-7 decides (K3) |
| TYPE-4.1 to 4.4, NAME-2.4 | H2.6: the memo is cleared by the first access in a later revision |
| REG-14.3 | after REG-8.3 (`FileDecls`' parameter vector) |
| REG-16.5 | H3.5 |
| REG-17.1 to 17.4 | H3.8 (new in this review) |
| CHECK-7, CHECK-8 | after REG-7.6 and REG-7.5; constants return value and throws (H3.3) |
| STUB-7.1 to 7.4, 7.6 to 7.12 | J1; `effective_range` and the phar locator added; 7.5 moves to W3 (H4.1) |
| STUB-8, STUB-12.1 to 12.5, CHECK-10 | J1.3 to J1.5 |
| STUB-10.1, 11, 13.1, 13.2 | J1.3; the builtin input holds a runtime corpus until STUB-11 (J1.2) |
| OVL-1 to 8, RES-1 to 7, EXT-1 to 4, SUM-1 to 3 | J2, read against the crate split's C6 text: its oracle-miss lists become each row's fixtures (RES-7 is the extension point and the three behaviours the first version dropped) |

## K2. Level 3: what the 136 are

The count overstates the work. `STAGE_B_FIX_REVIEW.md` has 88 open findings, a validation and 16 pointer lines (the
earlier reviews' items that a finding re-opens, listed again under "Full review"); `STAGE_B_REVIEW.md` has 15 and a
validation; `STAGE_C_REVIEW.md` has 14 and a validation, of which O1 and F5 are fixed (`e06f470c`, `1cdfe80d`; the
fix review says so) and two are the same bug as a fix-review finding (Stage C's F4 is F55, its F8 is F30). That is
about 115 findings. Whether each still reproduces is settled by running its probe on the gate binary in batch 0, not by
reading. **The rule for when each is fixed:** a finding in code that a later task rewrites gets its fixture now and
its fix inside that task; one that changes what the baseline records is fixed before the baseline; the rest go in the
review's own order between heavy runs.

| finding | meets | placed |
|---|---|---|
| F10 | the item 0 baseline (a skipped file has no tree dump) | its symptom is fixed; tests and fixture before the baseline, the exhaustive predicate in batch 1 |
| F21 | the canonical printers (`Repr::Mixed` changes) | before them |
| F20 | level 9's R10; item 9 | after R10 and before item 9, so the lint checks the recursion item 9 writes |
| F77 | the item 0 baseline (a refused file must show in O2) and SYN-1.9 | batch 0, for analysed files; its context-file half is a run-level notice that leaves the exit status (ruled 2026-10-18, M5); batch 3 feeds it from `errors()` |
| the seven fixes the crate split's F1 puts first (debounce stack, `latest_text`, `didClose`, `--generate-baseline`, `--fix`, the closed pipe, rename) | none: they are on today's code and independent | batch 0, each with its test (all open at `ffa7cccb`) |
| F22 (closure `use` list), F8 and F7 (the use-site walk) | item 9 step 1 (`Visitor` over handles) | in step 1 |
| F14, F41, F19 (the PHPDoc bullets) | DOC-2.1, TYPE-5 | batch 6 |
| F69, T2 | CHECK-9, REG-17.2 | F69's fixtures in batch 0; F69 and CHECK-9 in D2 (they sit on the recorders D2 replaces), T2 with REG-17.2. T2 asks "decide whether callee throw points count" and is a decision row: they do, as PHPStan's `MissingCheckedExceptionInMethodThrowsRule` reads the throw points of calls, and F69's throw points for implicit calls and REG-17.2's inferred throws are what make it complete, so T2 is built with them and the rule reports both a direct call and `$o[…]` on an `ArrayAccess` object |
| F5 (definedness certainty), F6 (closure-literal alias), F45 (reference groups), F61 (unset properties) | D1 and D5 (holder storage): each design adds per-variable state to `Locals`, and the fix review's own first step is one `ExpressionHolder` per narrowing key | fixture now, fix inside D1 and D5 (J3); F6's design stores an `Rc<Expr>`, which G1 and G2 replace by a rooted alias that D1 deletes |
| F18 | the `&&`/`\|\|` and nested-`match` causes are Stage D's one inference; the trait chain is REG-7.2's; the `.` rope and the `elseif` suffix are local | fixture now (the scale scripts), local ones now, the rest in D3 and W2 |
| F35, F50, F55 (Stage C F4), F75, F78, Stage B D1 | REG-4.2, REG-5, REG-11.4 and 11.5, REG-8.3, REG-7.5 | fixture now, fix in W1 and W2 |
| F76 | REG-7.10 (namespace cycles) | W2 |
| F58 | G1.9 and G4a (no per-function clone; the `Resolved` is built only when the anchor matches, in both walkers) | resolved by the design |
| F33 (anonymous class names, infer cache assumption) | Part E 34, G1.5, H1.4 | the name is the shown path and the line, as its design states (`File.shown`, H2.2); the infer cache's context epoch and the env-gated recompute-on-hit assertion are G1.5's |
| F30 (Stage C F8) | REG-4.15's builder | fixture now |
| Stage C F1 | decision 11, REG-8.1 | W2 |
| Stage C F5, F6, F7 | REG-11.10 (F5, fixed), REG-18 and J1 (F6, F7) | W1, W4 |
| Stage C F2, F3 | LOAD-7.3 and 7.4, STUB-7.3 | W1, W4 |
| Stage C F9, F10, F12 | LOAD-7.6, LOAD-7.7 | W1 |
| Stage C F11 | VAL-1.3, REG-8.5 | batch 0, W2 |
| F74 | the composer follow-ups of Part E 40 | with them |
| Stage B "Typed purity", D2 | REG-17.4 and SUM-3, REG-11.6 and EXT-2 | W2, W5 |
| the comments (F24, F34, F37, F43, F51, F59, F80, F88, Stage C F13) and the fixtures (F23) | none | any gap; F23 before the baseline |
| everything else (F2 to F6, F9, F11 to F13, F15 to F17, F25 to F32, F36, F38 to F40, F42 to F49, F52 to F56, F60 to F73, F79, F81 to F87, F89; Stage B W1, C2, M1, CMP-1, DF-1 to DF-7) | none | in the review's own order, F4 first |

## K3. The correction the review found in the engine

H2.9 had the batch run keep trees as a tracked `parse` with `lru`, evicted by `trigger_lru_eviction` between chunks.
That frees every evicted tree on the one thread that evicts, which is the cost LOAD-3.3 measured and removed (−138.6 M
Ir and −214 MiB peak when a context tree's drop moved to the worker that built it), and most trees are context trees.
The design now is: a context file's tree is built in the worker's scratch builder inside `file_products` and cleared
(LOAD-3.5), never a memo; an analysed file's tree is the memo `parse_analysed`, kept for the run in the batch and
under `lru` in the server, so the batch and the server hold trees alike. LOAD-5.2 is memory only, item 9 shrinks what it
frees from 151 MB (MEAS-2), and S-7 decides whether bounding the batch run is worth its frees (W3 builds LOAD-5.2 as
the ledger has it until S-7 says otherwise: H2.9). REG-16.4's phrase "a tree is a
parse result the engine evicts" holds for the analysed trees and not for context trees; that is a deviation from the
ledger's wording, for the user to see.

---


# Part L. The second pass, at the grain of the ledger's rows (2026-10-18)

Asked again as: "go through all tasks, one by one, in order and review them. maximum churn/properness". Part K
went through the tasks in groups and marked many "sound"; this pass reads each row's ledger text (its task, state and
needs), then the code it touches or the document it cites, and asks of each: (1) does the row's stated need hold;
(2) is it the end-state design or a structure a later task deletes (the 25 G report's no-interim rule, which Part
E 35 and 41 apply); (3) does it hold on every platform level 9 makes the project ship on, and against the module
moves level 9's R18 makes; (4) what checks it. A change made to an earlier part is made in that part and listed
here. Nothing was built or run.

## L1. Findings and what changed, in sequence order

| # | task | what the pass found | changed |
|---|---|---|---|
| 1 | the start condition | level 9 now has **R18**, the module graph, built before R6 and partly in the tree: `file_id` and `shard` go to reflection, `Completion`/`Reach`/`BlockCompletion` and `is_truthy_literal` to `syntax`, the mention bits to `checker::mentions`, `set_current_file` takes the shown path, `path_bytes` becomes a crate, `allocator` is one binary module | intro: rows R0 to R18, the start condition and what R18 moves; `File.shown` (H2.2) |
| 2 | the conventions | two gates hold every row that moves code, and the plan named neither: `cargo xtask modgraph` (a `crate` or `module` line in `gates/module-graph.txt`; a module held only by the root is rejected) and `cargo xtask tests` (a renamed, merged or deleted test needs `old -> new` or `old -> REMOVED: reason` in `gates/test-renames/*.txt`). Item 9's test rewrite and VAL-1.3's oracle deletions are the large cases | intro: conventions |
| 3 | A1, LOAD-2.2's last part | the walk counts, for every directory, the walks that reach it (`Starts::below`, `reaching`), filtering by excluded **name**; a vendor rule that only the listing tells would leave the reach counting a walk that lists nothing there, and the carried-state oracle (`reach_by_ancestors`) would disagree | A1: "The walk's reach follows from the listing" (a vendor directory's own task drops the walks that have the rule; `reaching` and `reach_by_ancestors` make the same cut; covering roots are `Excluded::is_empty()`; the first version's `vendor_child` went with the carried targets, M5d), the tests |
| 4 | A5a, LOAD-5.4 (new in Part A) | `FileState::ast()` returns `&[]` in release when a file kept no tree; item 9's new API would need an empty-tree constructor for the one case that must not happen. Four readers, all of files that keep trees. It is a type change an end-state invariant needs, not a structure W3 deletes | Part A table and A5a; bookkeeping |
| 5 | B3, I1.1, I1.7: the node ids | stored ids carried a debug-only tree tag, so `size_of::<ExprNode>()` would differ between the debug build the tests run in and the release build, and the size assertions could hold in only one | stored ids are raw 4-byte ids (`RawExpr`…); the tag is on the public `ExprId` and on handles |
| 6 | H1.4, Part E 34, anonymous classes | the plan keyed them by ordinal and said the displayed name stays `class@anonymous`. The ledger (decision 5, PROD-5.1, SYN-1.10, SYN-3.4) and the review's F33 design name them from their position, `class@anonymous/{path}:{line}[:{i}]`, which is also how types display them today (`types/src/parse_display.rs:91`); REG-11.13's `class@anonymous` is a display change of diagnostics, later. The ordinal was the plan's, to keep a line out of the global index | file-local entities with positional names, outside `decl_names` and `decl_index`, found by the provider through the file the name carries (`File.shown`, `FileSet`'s `shown` index) |
| 7 | CACHE-13.1 (batch 8) | MEAS-1 puts what a stat index replaces at ≈60-70 M Ir of reads and their system calls, with the ≈1,260 M of decode untouched; the persisted database holds `File.stamp` and deletes any index file. The stamp is permanent and its storage in batch 8 is not | built once as `File.stamp` at W3 (Part E 15); batch 8 is empty |
| 8 | REG-4.10 and DOC-2.9 (batch 6) | DOC-2.9 "needs REG-4.10 (level 2)", and REG-4.10 is the builder reading each doc comment's block once, which is what DOC-2.9's per-file product is; the need made item 14 close PART | REG-4.10 is folded into DOC-2.9; item 14 closes whole |
| 9 | Part C's table | it still had the first-pass order (products as batch 4, names as batch 5), "DOC-2.1 is deferred", the inference core in batch 7 and the error variants in step 1, against Part E 9, 11, 12, 14, 16 and 17 | C1, C2 notes 3 to 7 and C3 rewritten to the ruled order |
| 10 | W1's fillers | REG-4.9 is REG-4.12's change ("item 4's first step is the same change") and REG-4.10 is DOC-2.9's | W1 row, the tidying row |
| 11 | B3's store | it still listed `anon_classes` and `stmt_docs`, against I1.4 (one `classes` vector, an anonymous class is a `ClassNode` with no name) and I1.5 (one `docs` table keyed by owner kind) | B3 aligned to I1; a note that B5, B6 and Part I's `ExprId` means the raw id |
| 12 | G3.6, Part E 31 | an error node was said to infer `mixed`; F21 (batch 0, before recovery) makes the error type a kind of `mixed` that every unknown-type guard covers and that records a report was made | error nodes infer `Mixed(Error)` |
| 13 | B9 and DOC-2.9 | the plan said the comment's offset "is not carried by the parser, so that row adds it"; SYN-3.4 (batch 2) turns `doc_line` into the offset first | B9 |
| 14 | Part D and Part E | first-pass statements survived against the rulings: "DOC-2.1 is deferred", "batch 5" for names, "R7 lands", "four rows", "Part A's validation", W2's note, W3's open question, the sequence | each corrected or marked superseded |
| 15 | SYN-4.1, `WATCHED` | the plan said the watch list becomes a compile-time perfect hash; level 9's R18 composes `WATCHED` from three layers' lists (`stub_attributes`, `checker::mentions`, `file_declarations`), which `syntax` cannot see when it compiles | a `const fn` in the caller builds the table |
| 16 | step 1's module order | R18 moves the statement-completion rules into `syntax` | `completion` joins the syntax internals |
| 17 | J2, OVL/RES/EXT/SUM against C6 to C9 | read against the crate split's own text this time: faithful, with omissions in OVL-3 (the ≈60 narrower-parameter misses and the native-signature-base decision), EXT-1 (the trait's shape: `Name`s, memoized views, `TemplateParam`s, the enum-cases query; two renamed tests), EXT-2 (universal object crates; distance rule), EXT-3 (a used trait's mixins; template binding) and SUM-2 (expression typing) | rows completed |
| 18 | J3, D1 to D4 against the stage text | D1 (holder storage), D3 (per-node results), D4 (`RecordingNodeCallback`, `isReplayableConvergenceBody`, the `RefCell` channels as observer events) are as J3 says; D2's `syntax::annotations` needed a place in an immutable tree | J3's D2 row: side tables filled at `finish`, and `RemoveUnusedCodeByPhpVersionIdVisitor` a mark |
| 19 | Part E 32 | the `php-version` crate would have moved `KNOWN_PHP_VERSIONS` (5.3 to 8.5) and, with decision 18, been rebased to 7.0 in the same change | it moves as is; the rebasing is STUB-7.1's delta |
| 20 | the conventions | a heavy tool in CI would run on three systems that spell paths differently | cheap gates join `cargo xtask gates`; dumps and the oracle run stay local |
| 21 | F10 (the walk, T9) | the plan treated it as an open parser bug that gates the baseline; `3b7e4d95` (2026-10-03) made a bare `yield` end at any token in `can_begin_expression`'s list, and `php -l` accepts every shape the finding lists. The review's exhaustive `starts_expression`, its table test, seven parser tests and the fixture are unbuilt | tests and fixture in batch 0, the exhaustive predicate on SYN-2's `TokenKind` in batch 1 (Part E 22, K1 8, K2) |
| 22 | F77 (T34) | scheduled with parse recovery (batch 3), but its design needs none: a parse failure becomes `phpstanRs.parseError`, non-ignorable, and fails the run. Item 9's step 2 rewrites the parser, and a file it newly refuses must show in O2, not vanish into a count | F77 in batch 0, before the baseline; batch 3 feeds it from `errors()` (Part E 11 and 22, C1, the sequence) |
| 23 | G5 tool 4 (T15) | `Registry` has 31 fields, some scratch and some products the list did not name | the dump destructures it without `..`; each field printed or listed as scratch with its reason |
| 24 | G5 tool 6 (T17) | the in-process harness is `pub(super)` in `lsp/tests/helpers.rs` | it moves to the `testing` crate, which R18 makes the fixture runner's home |
| 25 | S-2 (T47, T48) | the spike tested the build on Linux only; salsa's default `inventory` feature registers ingredients at link time, and the project now ships and tests on four targets | the skeleton's tests run on the macOS, Windows and aarch64 legs; `default-features = false` is the fallback |
| 26 | W1, LOAD-7.5 (T50) | the row and the Q-11 table cite `lsp/workspace_config.rs`, which LOAD-1.3 deleted | the lookup is `lsp.rs:561`; corrected when the row is worked |
| 27 | Stage B T2 (T59) | filed as a finding in K2, it asks the user's tool to "decide whether callee throw points count" | decided by runtime truth and PHPStan's rule (`getThrowPoints()` of the statement result includes calls): they count; K2's row says so |


## L1b. The walk, item by item (the sequence's order; each item's real source read when it was reached)

| item | task | read | verdict |
|---|---|---|---|
| T0 | the start condition | `WORK_STACK.md:265-272`, `CROSS_PLATFORM_PLAN.md:23-25`, `git log` | changed: R18 first |
| T1 | A1 | `session.rs:335-394` (`Root::contains`, `unlisted_role`, `classify_unlisted`), `walk.rs:225-462` (`Scope`, `Reach`, `Starts`, `Reaching`, `below`) | the reach tables were missing: added, then simplified by the evidence rule (M5d) |
| T2 | A2 | `composer/lib.rs:92-141` (`PackageContext::load`), `manifest.rs:255-364`, `dir_listings.rs` | holds; `listings` is already a parameter |
| T3 | A3 | `function_sig.rs:822-842`, `asserts.rs:66-125` | holds |
| T4 | A4 | `types/decide.rs:1-60`, the four callers | holds |
| T5 | A5 | `ast.rs:1414-1428` | holds; the guard goes with the recursion (147 callers) |
| T6 | A5a (LOAD-5.4) | `project.rs:100-234`, `:770-830`, `:962-1000`, `:1812-1876`, `:2606-2626` | holds; all four readers read analysed or retained trees |
| T7 | A6 | `ITEMS_1_TO_3_FIXES_PLAN.md:1253-1254` | digest and counts match |
| T8 | VAL-1.1 | `git cat-file 1892b477`, `PROFILE_DRILL_PIWIGO_20G.md:613` | holds |
| T9 | F10 | `STAGE_B_FIX_REVIEW.md:543-580`, `parser/primary.rs:388-421`, `parser/expr.rs:211-285`, `git merge-base --is-ancestor 3b7e4d95 HEAD`, `php -l` on seven shapes | **already fixed in form** (`3b7e4d95`); tests, fixture and the exhaustive predicate remain |
| T10 | F20 | `STAGE_B_FIX_REVIEW.md:3446-3481`, `ls crates xtask/src`, `syntax/stack.rs` | open as said |
| T11 | F21 | `STAGE_B_FIX_REVIEW.md:3482-3518`, `types/queries.rs`, `lib.rs:375` | open as said |
| T12 | F57 | `closures_and_match.rs:79-107` | built (`parse_expression`) |
| T13 | G5 tool 2, printers | `Repr` (about 40 variants), `types/src` | holds; after F21 |
| T14 | G5 tool 3, O2 | `xtask/src/main.rs:37-88`, `gate-runner/src/main.rs` | holds |
| T15 | G5 tool 4, `registry-dump` | `registry.rs:262-453` (31 fields) | **a field rule added** (destructure without `..`; scratch listed with reasons) |
| T16 | G5 tool 5, expression dump | `project.rs:2359-2377`, `checker.rs:1328` (`set_dump_types`) | holds; a second switch of the same kind |
| T17 | G5 tool 6, server session | `lsp/tests/helpers.rs` | **the harness moves to `testing`** |
| T18 | G5 tool 7, edit replay | `main/incremental_bench.rs:9-69` | holds; the script is new, the entry points exist |
| T19-20 | G5 tools 8-9, memory and the `oracle` profile | `Cargo.toml:51-82` | holds |
| T21 | G5 tool 10, retiring the oracles | 54 `cfg(debug_assertions)` sites in 13 files | holds |
| T22-23 | G5 tool 11, the baseline; level 3 | K2 | holds |
| T24 | G3, the error-node model | `ParseError` sites (31), `LexWarning`, the `expect*` helpers (six) | holds |
| T25 | step 0, SYN-1.11 | `phpdoc_type_compat.rs:268-300`, `calls.rs:770-790` | holds |
| T26 | step 1, SYN-1.3 and 1.12 | counts: 134 files and 2,452 lines, 51 `Visitor` impls, 415 `Locals` mentions | holds |
| T27-28 | step 2 layout, step 3 | I1.2's arithmetic (`MethodCall` is 20 bytes) | holds |
| T29 | batch 1, SYN-2 | `lexer.rs`, `parser.rs:96-260` (`Supply`, `Lexed`, `Tok`, `VecDeque`, `fill`, `pop`) | holds; `Lexed` is the fuzz and test path |
| T30 | SYN-3.1 | 81 `LineIndex` mentions, 82 `.line_index` uses | holds |
| T31 | SYN-4.1 | `lexer.rs:243-262` (`WatchList::bits`) | holds; `keyword_slot` is the `const fn` precedent |
| T32-33 | SYN-4.2 to 4.10 | `token.rs:74-189`, `references.rs:23`, `name_resolution.rs:121`, `lexer.rs:1042` | holds |
| T34 | F77 | `STAGE_B_FIX_REVIEW.md:3724-3763` | **moved to batch 0** |
| T35 | SYN-1.9, Q-10, SYN-6 | Q-2, Q-10 records | holds |
| T36 | NAME-2.1 | `name.rs:22-80` | holds (three allocations a name today) |
| T37-39 | NAME-1.1, 1.2, 1.5, 1.6 | `checker.rs:281-287`, 209 `String`-keyed maps, `name_resolution.rs:424`, `:498`, `checker/expr.rs:55-890` | holds |
| T40 | PROD-5.8 | `references.rs:175-215` | holds; `Recorder::published` is PROD-5.2's seed |
| T41 | PROD-5.3 | `project.rs:3316`, `summaries.rs:380`, `global_vars.rs:551` | holds |
| T42 | DOC-2.3, 2.4 | `docblock.rs:391-497`, `:596` | holds |
| T43 | CHECK-6.4 | `types/lib.rs:539` (16 uses) | holds |
| T44 | CHECK-6.5, 6.8 | `checker.rs:421`, `:676`, `:691` | holds |
| T45 | F69, CHECK-9 | `try_catch.rs:234`, `throws.rs:105` | holds |
| T46-48 | S-1 to S-11 | ledger NAME-1 targets, `salsa-macros` `unsafe impl`, `salsa` features | S-2 extended to the other legs |
| T49-52 | W1 and W2 anchors | seven nested structs, `glob_matches`, `psr3_context_key_pattern`, `DEFAULT_REPORT_LEVEL`, `lsp.rs:561`, `class_info.rs:393-402`, `function_sig.rs:40`, 115 `finish_loading` mentions | holds; one stale citation |
| T53-55 | REG-4.15, REG-16.2 to 16.4 | `class_info.rs:986` (`from_def`), H1 to H4 | holds; the engine rows have no code to read, their anchors are the designs |
| T56 | F22 (the `use` list) | `visit.rs:595-605` | open as said (`Closure { .. } => {}`) |
| T57 | F7, F8 (the use-site walk) | `use_sites.rs:207-218`, `navigation.rs:489` | open as said |
| T58 | F14 / TYPE-5 | `types/parse.rs:1330-1349` | open as said; the arm I4 extracts into `phpdoc_keyword` |
| T59 | Stage B T2 (a callee's `@throws`) | `STAGE_B_REVIEW.md:321`, `vendor/phpstan-src/.../MissingCheckedExceptionInMethodThrowsRule.php:33` | a decision row, settled by PHPStan's rule: callee throw points count; built with F69 and REG-17.2 |

## L2. Checked, and holding

What this pass read and found as the plan has it (so that a "sound" in Part K has its evidence):

- **Part A.** The line references of A1 hold at HEAD (`session.rs:375`, `:386`, `:701`; `project.rs:1949`, `:3414`,
  `:3420`; `lsp.rs:541` for `classify_unlisted` has moved ten lines with R5); `phpstan_api.rs:268` is REG-18.6's, and
  `session.rs` (R8's `vendor_trees` and `unbuilt`) is a second reader of the name that this pass missed and the audit of
  2026-10-18 found by listing every `vendor` in the non-test code. A2's `read_installed` is as described (`manifest.rs:284-322`: two
  `resolve` calls per package, the install path joined to `vendor/composer` with `/`). A3's `param_names` has the three
  `resolve_assert_tags` calls and one `contains` (`function_sig.rs:828-835`, `asserts.rs:76-90`). Four readers call
  `FileState::ast` (A5a).
- **Batch 0.** F10's symptom is fixed and its design, an exhaustive predicate on the token, belongs on SYN-2's `TokenKind`; F20's is a `stack` crate
  and a recursion lint (after R10, which edits the guard); F21's is `Repr::Mixed(MixedKind, …)`, so G5's printers come
  after it; F57's arrow body and ternary error are in the parser. VAL-1.1 to 1.15's needs agree with G5's tool order,
  and VAL-1.3 (the oracles go) shrinks item 9's reader set as Part E 13 says.
- **Item 9.** The fabricated nodes exist where B1 says (`expr.rs:6054`, `:6071`, and the `NodeId::SYNTHETIC` test at
  `:4523`; `var_tag_types.rs:537`; `completion.rs:310-326`). The parser backtracks in one place only, the body skim's
  token pushback (`parser/functions.rs:219`), which the whole-file stream replaces by a cursor probe, so G3's "no node
  rollback" holds. Anonymous classes are named `class@anonymous/{file}:{line}[:{i}]` in `registry/anonymous_class.rs`.
- **Level 3.** The headings of the open findings were read against K2's placement; none that K2 leaves to "the review's
  order" sits in code that a later task deletes before its fixture exists.
- **The engine.** `run_finishing_passes` (`project.rs:1533`) runs the passes in the order H3.2 maps them (the
  assertion table after PHPDoc inheritance is the one difference, and the ledger's REG-7.2 already puts it at the own
  level); every pass H3.2 names exists where it says; salsa 0.28.5, `wide` 0.7.33 and `safe_arch` are in the registry
  cache or `Cargo.lock`, `triomphe` is in neither (S-1 fetches it); the checker's live filesystem reads are the
  `is_file` probes of `includes.rs` (`:272`, `:281`, `:282`, `:293`) and nothing else (H4.5); R11's cache directory is
  H4.4's; R18 step 7 (phpdoc may depend on types) is I4's placement.
- **The wall's text.** C6 to C9 against J2 and D1, D3, D4 against J3 (the last two with the changes of L1 17 and 18).

## L3. What this pass leaves for the user

No new ruling from this pass (the workflow review's rulings and changed answers are M5). Four answers changed or were
sharpened and want the user's eye with the rest of Part E: #15
(CACHE-13.1 is W3's), #32 (the `php-version` crate moves code as is), #34 (anonymous classes are file-local with
positional names, not ordinal; on 2026-10-18 the user ruled the identity ordinal, with the name still positional) and
#39's timing. Two edits to the ledger the plan would otherwise have left false were listed in B11 (decision 4's
layout bullet and SYN-1.4, on the arena, Part E 4); the user ruled on 2026-10-18 that the ledger's layout stands, so
neither edit is made. The pass changed nothing that was built, and ran nothing.

## L4. What the workflow review found wrong in this part (2026-10-18)

This part is the record of the second pass; the statements below were checked by the workflow review and found
wrong, and the parts they concern carry the corrections (the M3 id says where it was found). The rows above are
left as they were written.

| where | said | is | id |
|---|---|---|---|
| L1 #1, L1b T0, L2 start condition | level 9 is at R18 and R6 to R17 follow | R0 to R12 and R18 are built at `2c7bf898`; R13 to R17 are open | A-17, S-21, C-01 |
| L1 #4, L1b T6, L2 Part A | A5a has four readers | twelve non-test call sites in six files | A-04 |
| L1 #7 | what a stat index replaces is "reads and their system calls" in Ir | the ≈60-70 M Ir is read wrappers and two hashes; system calls and I/O are not in Ir | V-11 |
| L1 #22 | F77 fails the run for any file it refuses | analysed files only; the context half departs from decisions 9 and 15 and waits for a ruling | S-12, C-14 |
| L1 #24, L1b T17 | the in-process harness moves to the `testing` crate | `testing` has no `lsp` edge; the session API is public in `lsp` | V-01 |
| L1 #25 | `default-features = false` is the fallback | the pinned feature set without `inventory`, with every ingredient registered by hand; and a release-profile test on each leg | E2-22, X-06 |
| L1b T24 | `ParseError` is built at 31 sites | 28 constructions (31 lines name the type) | G-19 |
| L1b T29 | `Lexed` is the fuzz and test path | `tokenize_with_spans` and `with_comments` are used in production by `lsp/completion.rs`, `stub_attributes.rs` and `output/codeframe.rs` | T-07 |
| L1b T36 | a name takes three allocations today | two and a temporary; three only with capitals | N-18 |
| L1b T37 to 39 | 209 `String`-keyed maps | 181 outside tests (187 with); the plan carried three counts | N-12 |
| L1b T58 | the iterable arm is extracted into `phpdoc_keyword` | TYPE-5 is its own commit after I4.5 step 5 | P-01 |
| L2 engine | `triomphe` is in neither the registry cache nor `Cargo.lock`; the checker's live filesystem reads are the `is_file` probes and nothing else | `triomphe` 0.1.14 is in the registry cache; the SQL extension's live reflector is a second live read | X-22, E1-01 |
| L2 level 3 | no finding K2 leaves to the review's order sits in code a later task deletes | F5, F6, F45 and F61 add per-variable state to `Locals`, which D1 replaces | S-02 |

---


# Part N. The documents and edits this plan implies

Nothing here is done in plan mode. This part consolidates what Parts A, B, E, H, J and M scatter ("the ledger would
otherwise keep false"), so that the docs commit series of Part E 20 and the first commits of each batch have one
list. A ledger row is edited in the commit that builds it, except where a row says "at approval"; an edit of a ruled
decision's text is made only after the ruling, and the user ruled on 2026-10-18 (M5): the edits below are the
ones the rulings imply, and the decisions whose text does not change are named.

## N1. Where the plan's parts go (proposed)

| document | parts | why |
|---|---|---|
| `POST_LEVEL_9_PLAN.md` | A, C, D, E, F, G5, I2 to I5, K, L | the batches, their order, the answers and the designs the batches wait for |
| `FLAT_TREE_PLAN.md` | B, G1 to G4, I1 | item 9 and what it forces on the checker and the probe (B11) |
| `ENGINE_PLAN.md` | H, J | the engine, the pipeline, and the wall's data models and rows |
| `PLAN_REVIEW_LEDGER.md` | M | the workflow review's findings |

Each keeps its section numbers, so that a reference such as G1.5 or H4.6 stays valid, and opens with a table of where
the other parts went. The ledger keeps the rows and their states; the plans keep the designs.

## N2. The ledger (`PROFILE_DRILL_PIWIGO_LEDGER.md`)

| row | edit | from |
|---|---|---|
| SYN-1.2, 1.4, 1.5, 1.7, 1.8 | one shared design note pointing at `FLAT_TREE_PLAN.md`; need SYN-1.3; no ruled wording changes (ruled 2026-10-18): SYN-1.2's "owns its source" and SYN-1.4's "payloads are spans" are what B3 builds, and SYN-1.2's "settled first, by a measurement" is spike S-12 | B11, Part E 3 and 4 |
| SYN-1.3 | text "readers read through the handle API (step 1)"; need SYN-1.12, not SYN-1.2 | B2 D8, B11 |
| new SYN-1.11, SYN-1.12 | step 0 (the census; the fabricated nodes removed) and step 1's API (handles, views, the dump); SYN-1.12 is needed by SYN-1.3 | B7 |
| SYN-1.9, SYN-6, Q-10 | the error-node model of G3 (three arms, no member enum; the `incomplete` bit; marks); SYN-6's rows are marks and error kinds; a stray control byte, a NUL byte included, is a validity mark; a context file that fails to parse is a run-level notice that leaves the exit status | G3 |
| SYN-1.10 | keeps anonymous classes out of the declaration names and the global index; their identity is the ordinal | H1.4 |
| SYN-2 | needs SYN-1.2 (the file's names live in its tree); the stream of I2 (kind and span, no payload array, ends at `__halt_compiler`); the cast rule and its two deltas | I2 |
| SYN-3.2, 3.4 | the PHP line-terminator rule in one change (lexer sites, splitters, line index); the accept rule and the `wide` merit comparison; SYN-3.4 keeps `first_start` through the attribute re-key | I3, Part E 10 |
| SYN-4.1, 4.8, 4.9 | batch 1; SYN-4.1's lookup half goes and the watch table is a `const fn` in the caller; SYN-4.5 is `ConstName`, batch 4 | C1, C2 |
| NAME-1.2, 2.1, 2.2 | NAME-1.2 over values and maps, leading with the registry's entry points; NAME-2.1 is `ThinArc` only if S-1 finds a safe read path, else one `Arc` with one `Box<str>`, and the sweep lands with it; NAME-2.2's weak pairs are for the type tables | H1.7, Part E 29 |
| PROD-5.1, 5.4, 5.5, 5.7, 5.8 | PROD-5.1: file-local anonymous classes and the site table; PROD-5.4 and 5.7 "deleted by REG-16.4" (5.7 not built); PROD-5.5 keyed by ordinal; PROD-5.8 keyed by `FoldedName` and `ConstName` | Part E 34, 35, H1.4 |
| DOC-2.1, 2.2, 2.5, 2.7, 2.9; REG-4.10 | DOC-2.1 undeferred as F41, first in batch 6; DOC-2.2 keyed by the interned `Type` and the context id; DOC-2.9 absorbs REG-4.10 | Part E 14, I4 |
| TYPE-5 | its own commit after I4.5 step 5; F14's design text superseded | I4.5 |
| CHECK-6.1 to 6.3, 6.7, 6.13 to 6.15 | "built by D1 to D4" | J3 |
| CHECK-6.8, 6.18 | batch 7 builds the handle-free scratch reuse; "one checker per worker, reset for each file" is E7's | C2 note 7, G1.10 |
| CHECK-6.10, 4.1 | the `Message` of I5 | I5 |
| CHECK-4.2 | folded into REG-18.4 (batch 4b) | Part E 41 |
| CHECK-7, 9, 10 | CHECK-9 and CHECK-7's checker half in D2 (after F69), CHECK-10's operator sites in D3, its version table in W4 | Part E 17 |
| CHECK-11, LOAD-7.6, REG-18.4, LOAD-7.2 | their deltas are approved (2026-10-18) with the conditions of Part E 42: LOAD-7.6's key table is shown before the rename and the PHP range and the activation come from each file's owning manifest, CHECK-11 defines its template renderers, LOAD-7.2's PHPUnit half stays ungated | Part E 42 |
| REG-14.4 | "passes the parameters on" becomes the predicate | A3 |
| REG-14.5 | folded into REG-7.5 | A4 |
| REG-4.9 | built with REG-4.12 | Part D W1 |
| REG-4.11, 4.15 | REG-4.15 loses "a member's span is a field of its entry"; REG-4.11 becomes a record over (`Origin`, site) | H1.5a |
| REG-7.x, 8.x | the levels and queries of H3; there is no REG-8.8 | H3.5 |
| REG-8.6, 16.2 | REG-8.6 re-specified (one interner, counted and swept); REG-16.2 keeps "never reused" for `DeclKey`, which is immortal, and the name probes are collectable (H1.4, H1.6) | H1.3, Part E 38 |
| REG-16.4 | needs SYN-1.9 | H4.3 |
| REG-16.5 | the event callback plus the provider's trace of first readers | H3.5 |
| REG-17.x | the layering of H3.8 (the receiver-types query, the inference family) | H3.8 |
| LOAD-2.2 | stays PART until LOAD-7.8 deletes `path_under_excluded_dir` and the constant; its text, "skip Composer's vendor directory by its resolved path" (the fixes plan's decision 7), becomes "a directory Composer wrote into, or the install directory of a package it put elsewhere" (ruled 2026-10-18, M5d; COMP-7) | A6, A1 |
| REG-18.6 | `phpstan_class_files` is a provenance query, and "a PHPStan class is installed" asks the file's owning package and not a path component called `vendor` (`phpstan_api.rs:268`) | A1 |
| LOAD-5.1, 5.2, 5.4 | LOAD-5.1 moves to W3 (facts) and E1 (collection); LOAD-5.2 is W3's, needs both LOAD-5.1 and LOAD-3.1; LOAD-5.4 is Part A's "every reader asks `ast_of`" | Part E 39, A5a |
| LOAD-4.7 | the file-references half "deleted by REG-16.4" | H4.8 |
| LOAD-7.5, 7.x table | the LSP's lookup is `root.join("phpstan-rs.toml")` in `lsp.rs`, not `lsp/workspace_config.rs` | Part D W1 |
| LOAD-9 | DONE when A2 lands; leaves `WORK_STACK.md`'s "Outside the items" line | A6 |
| CACHE-13.1 | `File.stamp` at W3, with the racily-clean rule applied at the stat and an explicit file-system list; decision 13's "the index's own write time" is replaced | Part E 15 |
| CACHE-13.2 | H4.6; keyed by build, root and config key, the config key without the version range | Part E 37 |
| CACHE-8; CACHE-9 to 11 | after S-4; the form follows S-4 | Part E 41 |
| STUB-7.5 | moves to W3; needs the Composer maps to keep their keys, the root's own entries included, and gain PSR-0 (A7) | H4.1 |
| STUB-7.1, decision 18 | a range keeps exact version-id bounds and is open above when its constraint has no upper bound or none is declared (ruled 2026-10-18): the default is "7.0 to the newest, and later", a gate past `hi` is never, and one inside an open end is in part (`Availability`) | J1.1, Part E 40, M5d |
| STUB-12.2, 12.4 | behind the pre-8.0 spike; STUB-12.4 includes the `SignatureMapParser` port | J1.4, J2 |
| STUB-11 and OVL-2 | OVL-2 needs STUB-7.6, not STUB-11's build | J2 |
| VAL-1.2 to 1.12 | G5's order (the oracle run after the session and the replay); VAL-1.5 is item 9's dump; VAL-1.9's session API is public in `lsp` | Part E 13, G5 |
| VAL-2.9, 2.10 | the deltas the plan adds, approved on 2026-10-18 (the cast rule, `variable.undefined` after an error, `extract` or `include`, `extract` no longer truncating the body's diagnostics, the lone-CR rule, the interpolation and heredoc-indentation errors, the NUL mark, the anonymous-class consumers of H1.4, `Point<1, 2>`, `callable(int)`, the wildcard and `-9223372036854775808` and the other differences of I4.2 and I4.3, the string lexeme, CHECK-11, REG-18.4 and LOAD-7.6's keys and ranges) | I2 to I4, G3.6 |
| new rows | `OVL-1` to `OVL-8`, `RES-1` to `RES-7`, `EXT-1` to `EXT-4`, `SUM-1` to `SUM-3` (J2); a D-row and an E-row per phase of Stage D and E (J3); `COMP-1` to `COMP-8`, `SRV-1` to `SRV-7`, `CHECK-12` and `CHECK-13` (N6) | J2, J3, Part E 40, 22, G3.6, H1.4 |

Ruled decisions whose text changes (ruled on 2026-10-18, M5a): decision 3 (the member sentence only: a method by
folded name, a property and a class constant by exact name, an enum case being a class constant), decision 9 (one
sentence: a context file that fails to parse is reported as a run-level notice that leaves the exit status),
decision 13 (the racily-clean reference time is the clock reading at the file's own stat, not "the index's own write
time") and decision 17 (the widening sentence becomes "the iterate is the least fixpoint of the capped operator",
H3.3). **No text changes** in decision 4 (both sentences stand: the layout is what B3 builds, and the last sentence
is spike S-12), in decision 3's "is never reused" (it stands for `DeclKey`, which is immortal), in decision 14 (a
failed S-11 returns to the user before an eager fallback ships) or in decision 15 ("a context file is not reported"
gains decision 9's sentence by reference). **Two more texts change, ruled on 2026-10-18 (M5d, "agree"):** the fixes plan's
decision 7 ("by its resolved path" becomes "a directory Composer wrote into, or the install directory of a package it put
elsewhere") and the ledger's decision 18 ("to the newest version" becomes "and later").

## N3. `WORK_STACK.md`

Level 8 is reordered: item 0 (batch 0: VAL-1.1; the seven F1 fixes; F10's tests, F77, F20, F21, F69's fixtures; the
dump-changing level-3 findings and Q-11 follow-ups; the tooling and the baseline) before item 9; item 9 reads
"SYN-1.11, then 1.12 (+1.3), then 1.2/1.4/1.5/1.7/1.8, then step 3 (LOAD-3.5); `FLAT_TREE_PLAN.md`"; then the
batches of C1 in order (tokens, the lexer's second pass, parse recovery with the `php-version` crate, names, the
constant lookup, products, PHPDoc, the check's temporaries); Part A's rows come first, before item 0. Item 2's bullet
goes, LOAD-9 leaves "Outside the items", item 8 keeps REG-14.1 and 14.3, item 19 loses LOAD-5.4, the state line is
rewritten. The "Documents with no place in the stack" list gains this plan's four documents.

## N4. `CRATE_SPLIT_PLAN.md` (the docs commit series of Part E 20)

Marked in place, each pointing at the section that replaces it: Appendix F's "each level is a per-class `OnceLock`" and
"constant reference cycles type as `mixed`" (H3.1, decision 1); C4's "a change to any reflection input resets every
memo" (the recorded reads); F1's published snapshots (H4.3's cancellation); F1's text on line starts (the PHP terminator
rule is applied in the lexer too: I3); F2's resolved dependencies, fingerprints and entry sets (H4.4); §3.9's text
about the deleted `lsp/workspace_config.rs`; the line (6883-6891) "a file with parse errors is still not analysed",
superseded (ruled 2026-10-18, Part E 31); G's "26 new packages" count and Appendix A, which gain the engine and `php-version`
(J3); C6 to C9 and D to G gain the rows of J2 and J3. Appendix F's rule ("a bug in code a later phase rewrites gets a
fixture now and its fix as that phase's behaviour commit") is cited by F69, F5, F6, F45 and F61 (K2). B8's composer
follow-ups (`config.vendor-dir`, Composer's grammar, `psr-0`, the root package's autoload, a package's bundled `vendor/`)
point at A1, A7, A8 and Part E 40 once those are built; the clamp of an open upper bound to `MAX_PHP_VERSION` is marked
superseded (ruled 2026-10-18, M5d's third row).

## N5. Other documents

`CROSS_PLATFORM_PLAN.md`: R11's database-file follow-up and R16's `path_bytes` encoding pair (H4.6), R13's CRLF
probe files marked `-text` (G3.7.1). `CRASH_REPORTING_PLAN.md`: the stack guard's `LIMIT` restored on unwind and the
engine's two isolation modes (H2.8); the vendor-discovery panic-hook swap is deleted, not converted (Part E 40).
`UNSAFE_PLAN.md`: its end state changes only if S-2 finds the engine exception necessary (Part E 33).
`STAGE_B_FIX_REVIEW.md`: F14 superseded by TYPE-5's rule; F33's epoch lands in G1.5; F58's lazy construction in G1.9.

## N6. The new rows

`COMP-1` `config.vendor-dir` and `COMPOSER_VENDOR_DIR`, resolved as Composer does over candidates, and the recognition of
a Composer vendor directory by what Composer wrote in it (A1; the delta commit and the mechanism were ruled on 2026-10-18,
M5d); `COMP-2` Composer's constraint grammar by interval arithmetic (whitespace after an operator, a window inside a
minor, a bare version as Composer reads it; batch 0), and, apart, exact version-id bounds with an open upper end (J1.1;
ruled 2026-10-18, M5d); `COMP-3` the process-wide panic hook that `discover_vendor_context` swaps (the crash plan deletes it);
`COMP-4` `psr-0` in `Autoload` and the generated maps' keys (A7); `COMP-5` the root package's own autoload as context
(A7's maps keep its entries and W3's locator finds the class); `COMP-6` a package's bundled `vendor/` as context (A8);
`COMP-7` third-party code that Composer installed outside the vendor directory, left out of a degrade root's analysed walk
and admitted as context by discovery in every mode (A1: the seeds and the filter, two commits); `COMP-8` the vendor
contexts of a degrade root's nested Composer projects, found by a scan before the walk, with their owners and `unbuilt`
(A8). COMP-7 and COMP-8 were added on 2026-10-18 at the user's word ("also (a) and (b)"). All eight were audited against
the code and the corpora on 2026-10-18 (Part E 40), so each row is written as specified and none as OPEN to be audited. `SRV-1` to `SRV-7` are the seven fixes the crate split's F1 puts first: the debounce thread's stack size
and the scan's parses on the pool; `queue_change` writing `latest_text`; `didClose` restoring the disk's content;
`--generate-baseline` keeping the entries of files not analysed; `--fix` skipping a file changed since analysis; a
closed output pipe ending the run quietly; rename refusing qualified names, imports and aliases. Each has its test on
today's code (Part E 22). `CHECK-12` is exact include modelling (the included file's top-level assignments in the
includer's scope, and its return value as the include's type), built in Stage D on the scope model of D1 and D2, with
`any_variable_may_exist` as its fallback (G3.6); `CHECK-13` makes the class-shape parent-call rules include anonymous
classes (H1.4). Both are OPEN rows, each to be read against the code when it is worked.

## N7. The comparisons against an old implementation (Part E 13's policy)

Each is deleted in the commit after its stage: I2's old-versus-new lexer `(kind, span)` fuzz comparison (when the old
lexer is deleted); I4.5 step 3's per-`TypeNode` comparison, only if VAL-1.7's PHPDoc record cannot see what it finds;
H3.6 step 2's `Provider`-against-engine comparison and step 3's old passes, one level per stage. A2 keeps none.

---

