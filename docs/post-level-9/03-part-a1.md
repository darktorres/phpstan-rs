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
