export const meta = {
  name: 'phpstan-pilot-implementation',
  description: 'Implement 6 independent ROADMAP.md phases (16, 40, 47, 59, 72, 77) in parallel, each in its own git worktree/branch for review before merge',
  phases: [{ title: 'Implement' }],
}

const PILOT = {"16": {"roadmap_text": "**Phase 16 - CI-friendly output formats (PLANNED).** No JSON/checkstyle/\njunit/etc. output. Realistic starting subset: `json` (most universally\nconsumable) plus `checkstyle` or `junit`. Needs real source positions to\nbe useful beyond plain text - Phase 11 already shipped those, so this is\notherwise unblocked. Unaffected by the exhaustive audit (not a PHPStan\nRules/Type-shaped feature - PHPStan's own output formatters live outside\n`src/Rules`/`src/Type`) beyond one dependency worth noting: Phase 47's\ndiagnostic-identifier work is a natural prerequisite for a JSON format\na CI tool would actually want to consume (a stable per-check identifier\nper finding, not just a message string).", "primary_rust_locations": ["src/main.rs: run_check_project() (L284-~410), specifically the diagnostic-printing loop at L374-390 - needs to become format-dispatching rather than a fixed println! - plus the smaller ad-hoc printing loops in run_incremental_bench() (L226-237) and run_demo() (~L648-656) if they should also honor a shared format", "src/main.rs: Cli/Command::CheckProject struct (L410-460) - needs a new #[arg(long)] format: Option<String> (or enum) field alongside the existing level/baseline/scan-dir/config args", "src/checker.rs: pub struct Diagnostic (L71-76) - the JSON/checkstyle/junit serializers consume this struct's existing location/message/span/level fields directly; no struct change is structurally required, but a stable per-check identifier (absent today) would make the JSON output far more useful", "New module likely needed (e.g. src/output.rs) for the json/checkstyle emitters themselves - serde/serde_json are already a Cargo.toml dependency, so no new crate is required for the json format"], "notes_from_audit": "Diagnostic struct already has real Span-based positions (Phase 11, done) confirmed at src/checker.rs L71-76, so the roadmap's 'otherwise unblocked' claim checks out. Phase 47's identifier work is referenced as a 'natural prerequisite' but the roadmap's own wording ('beyond one dependency worth noting') and the fact that JSON output is fully buildable against the current 4-field Diagnostic struct make this a soft dependency, not a hard blocker."}, "47": {"roadmap_text": "**Phase 47 - Diagnostic identifier & metadata infrastructure (NEW,\nPLANNED).** `Diagnostic` (`src/checker.rs:71`) is a flat 4-field struct,\njust `{location, message, span, level}`, populated via ad hoc struct\nliterals/`format!()` calls scattered across `src/checker/*.rs`, with no\nidentifier field, no tip/canned-tip helper, no metadata bag, no\n`nonIgnorable` flag, and no `fixNode`/auto-fix support, and no fluent\nbuilder to assemble any of it consistently (the Rust equivalent of\n`RuleErrorBuilder`). This is what real PHPStan's `@phpstan-ignore\n<identifier>` matches against, what CI output formats (Phase 16) want\nfor machine consumption, and what lets a \"possibly impure\" tip (see\nPhase 41) be appended to a constant-condition diagnostic without a\nsecond diagnostic. The core struct change is small; real parity requires\nthreading an identifier convention through dozens of existing\ncall-sites and deciding a Rust-idiomatic builder API. Prerequisite for\nPhase 44 (`@phpstan-ignore`, which already names this as a blocker),\nPhase 16 (CI output formats), and Phase 41's possibly-impure tip\ndecoration. Size: M.", "primary_rust_locations": ["src/checker.rs: Diagnostic struct (L71-76, confirmed exactly {location, message, span, level}), report() (L263-270), into_diagnostics (L272+)", "38 call sites of self.report(...) across src/checker/*.rs (confirmed via grep: oop.rs, expr.rs, assignment.rs, stmt.rs, calls.rs, resolve.rs, generics.rs, narrowing.rs) - each an ad hoc format!() call with no identifier, needing a builder/threading pass", "src/main.rs print loop (L377-389) - currently prints location/message/level only; would eventually surface identifier/tip, though that may land in Phase 16 instead"], "notes_from_audit": "47's own edits (struct + one fn) are isolated to a single spot in src/checker.rs, so it is a wide-blast-radius prerequisite for identifier-bearing diagnostics rather than a phase likely to produce hard merge conflicts by itself - the real cost of concurrent work is call sites written against the old report() signature needing a follow-up pass, not line-level collisions."}, "40": {"roadmap_text": "**Phase 40 - PHPStan-API-usage checks (PLANNED).** Checks PHP code that\nconsumes `phpstan/phpstan` itself as a Composer library (third-party\nPHPStan rule/extension authors) for references into `PHPStan\\*` that\naren't marked `@api` - see the Phase 18 note above for why this is a\ndifferent feature. A pure namespace-prefix check (`PHPStan\\*` minus\n`PHPStan\\PhpDocParser\\`/`PHPStan\\BetterReflection\\`) plus a\nvendor-directory file-path check; `@api`/`@api-do-not-implement`\nrecognized via Phase 36's own tag infrastructure; gated behind\ndetecting a `phpstan/extension-installer`/`phpstan/phpstan` dependency\nin the analyzed project's own `composer.json` (mirrors the real\nPest-config-detection precedent, Phase 21). The exhaustive audit's own\n`Api/*` findings (`ApiClassConstFetchRule`, `ApiMethodCallRule`, etc.,\n11 items) were all classified `not_applicable` for phpstan-rs *itself*,\nsince they gate on referencing PHPStan's own `PHPStan\\*` namespace, which\nthis project has none of. This phase remains the mirror-image, correct\nfeature: checking *other* PHP code that consumes real PHPStan's API,\nnot phpstan-rs protecting its own.", "primary_rust_locations": ["new checker module (no existing file covers this - e.g. src/checker/phpstan_api.rs) implementing the PHPStan\\* namespace-prefix check plus vendor-path check", "src/composer.rs: discover_vendor_context (L43-70) currently only checks composer.json's existence via .is_file() (L44) and never parses its contents - Phase 40 needs a new function actually parsing composer.json's require/require-dev via serde_json (already a Cargo.toml dependency, confirmed) to detect phpstan/phpstan or phpstan/extension-installer", "src/docblock.rs: apply_tag (L142-277) - new \"api\"/\"api-do-not-implement\" arms (confirmed NOT already present anywhere in Phase 36's own explicitly-listed sub-items, so Phase 40 must add these itself, following the established two-pass plain/@phpstan- prefix pattern already in docblock.rs)", "src/main.rs: run_check/run_check_project pipeline - wiring to gate the new check on the composer.json dependency detection"], "notes_from_audit": "Genuinely self-contained new feature - the roadmap text's 'recognized via Phase 36's own tag infrastructure' means reusing the already-existing two-pass tag-parsing pattern in docblock.rs (which Phase 36 formalized/extended but which already worked before Phase 36, confirmed by reading apply_tag directly), not a hard dependency on Phase 36 landing first. composer.rs currently does no JSON parsing at all (only a .is_file() existence check), so this phase is the first to actually read composer.json's contents - no other open phase appears to touch composer.rs."}, "72": {"roadmap_text": "**Phase 72 - Regex capture-group inference extensions (distinct from,\nnot blocked by, Phase 46) (NEW, PLANNED).** Two additive improvements\nto `regex_groups.rs`'s Scanner that are about *type precision for\n`$matches`*, not pattern *validity* (so independent of Phase 46's PCRE-\nvalidation work): (a) a tri-state fallback for non-enumerable capture\ngroups (`(\\d+)` -> `numeric-string`, `([a-z]+)` -> `non-empty-string`,\ninstead of degrading to plain `string`) plus `(?|...)` branch-reset and\n`x`-modifier support; (b) resolving a statically-foldable `Concat`\npattern argument (e.g. `'/^' . $prefix . '$/'`) instead of only\nmatching `ExprKind::Literal(String)` in `literal_string_value()`.\nUnlocks improvements to `RegexAstWalkResult`, `RegexCapturingGroup`,\n`RegexGroupParser`, `RegexGroupWalkResult`, `RegexNonCapturingGroup`,\n`RegexExpressionHelper`. Size: M+S - high value; `preg_match()`\n`$matches` shape typing is high-frequency, and a non-literal group\n(extremely common: `(\\d+)`) currently silently degrades.", "primary_rust_locations": ["src/regex_groups.rs (not src/checker/regex_groups.rs as the roadmap text names it - the module lives at crate root, src/regex_groups.rs, 959 lines) - Scanner (struct at L269) and RegexCaptureGroup (L17-64, `literal_values: Option<Vec<String>>` at L26) are the real targets for the tri-state numeric-string/non-empty-string fallback", "src/regex_groups.rs:119 (parse_capture_groups) and its `(?|...)` branch-reset / `x`-modifier handling - not yet supported per the module's own doc comment scope", "src/checker/assignment.rs:525-530 (`literal_string_value`, currently `ExprKind::Literal(Literal::String(s)) => Some(s.as_str())` only, `_ => None`) - the Concat-folding extension lands here; this is a widely shared helper (also used by src/checker/calls.rs:386 for sprintf/printf, src/checker/narrowing.rs:26, src/checker/expr.rs:299,344, src/pest_config.rs:63,268), so widening its return type from `Option<&str>` to something that can represent a folded owned String touches every call site mechanically", "src/checker/expr.rs:299,344 (preg_match/preg_match_all's own `literal_string_value(pattern_arg)` calls feeding regex_groups) - consumers of both extension points"], "notes_from_audit": ""}, "59": {"roadmap_text": "**Phase 59 - Unused-symbol detection subsystem (NEW, PLANNED).**\nphpstan-rs has no dead-code/unused-variable-usage-scanning category at\nall. Build one variable/member-usage walker (with the\n`func_get_args()`/`compact()`/`get_defined_vars()`/dynamic-variable-name\nescape hatches that force \"treat everything as used\"), then apply it to\nfour sites: function/method parameters, closure `use()` captures, and\nprivate class members (constants/methods/properties). Unlocks\n`UnusedConstructorParametersRule`, `UnusedClosureUsesRule`,\n`UnusedPrivateConstantRule`, `UnusedPrivateMethodRule`,\n`UnusedPrivatePropertyRule` (the property variant additionally needs 5\ndistinct read/write messages and `@phpstan-always-read`/`-written` tag\nsupport). Size: L - code-cleanliness signal rather than a\ncorrectness-bug catcher for the parameter/closure-use half, but the\nunused-private-member half is a very common, high-signal real finding\nafter refactors.", "primary_rust_locations": ["New module needed (e.g. src/checker/unused.rs) \u2014 confirmed via grep that no unused/func_get_args/compact/get_defined_vars handling exists anywhere in the checker today", "src/checker.rs: check_function (~L438+, confirmed) \u2014 needs param-usage tracking for function/method/constructor parameters, and closure use()-capture usage (ExprKind::Closure's uses: Vec<ClosureUse> field, confirmed at src/ast.rs:481, backed by the ClosureUse struct at src/ast.rs:388)", "src/checker.rs: check_program (~L313-345, confirmed) \u2014 needs a new post-pass per class checking which private consts/methods/properties (ast::Visibility, already parsed) were never referenced anywhere in that class's own checked bodies", "src/registry/class_info.rs: ClassInfo.consts/methods/properties (confirmed struct) \u2014 supplies the private-member candidate set to scan for zero references"], "notes_from_audit": "Confirmed genuinely new: no usage-walker of any kind exists in the checker today. The func_get_args()/compact()/get_defined_vars()/dynamic-variable-name escape hatches the phase calls out are a real, separate risk worth flagging up front \u2014 a naive AST walker for 'is $x read anywhere' would need to special-case all four (plus dynamic property/method access via $$var or variable-variables) to avoid false positives, which is likely most of this phase's real difficulty rather than the walker's happy path."}, "77": {"roadmap_text": "**Phase 77 - Misc built-in-class/runtime-capability modeling batch\n(NEW, PLANNED).** Batch several independent, lower-value stdlib-\nmodeling gaps: `get_class`/`get_called_class`/`get_parent_class`/\n`gettype`/`get_debug_type` (late-static-binding + runtime-string\nmapping tables); `date()`/`idate()` format-character-to-type table\n(static, no runtime simulation needed - unlike the DateTime/DOM/\nSimpleXML cluster in Phase 78); hardcoded constant-array-shape tables\nfor `curl_getinfo` (extend the existing ~20-entry table to the full\n~75), `stat`/`dio_stat`, `pathinfo`, `parse_url`, `sscanf`; runtime-\ncapability-list snapshots for `hash_algos()`, `openssl_get_cipher_methods()`,\n`mb_list_encodings()`; Reflection-class modeling in the registry;\n`SimpleXMLElement` magic-property resolution. Size: M - low individual\nvalue; batch to amortize planning overhead, skip or deprioritize unless\na corpus surfaces heavy Reflection/XML/hash usage.", "primary_rust_locations": ["src/checker/expr/builtin_calls.rs: extends the EXISTING `curl_getinfo_component_type` helper function (~L1019 onward, a standalone match over CURLINFO_* constant names returning a Type) from ~20 to ~75 entries - this is a data-table edit inside a helper function, not the `curl_getinfo` dispatch arm itself (~L740-747), so it's isolated from anything else touching that dispatch match", "src/checker/expr/builtin_calls.rs: new sibling arms alongside the EXISTING `get_class` arm (~L759-763, currently 1-arg form only, resolves from the argument's own Object type) for get_called_class/get_parent_class (late-static-binding, a materially different mechanism - needs $this/static class context, not the argument) plus brand-new arms for gettype/get_debug_type/date/idate/pathinfo/parse_url/sscanf/hash_algos/openssl_get_cipher_methods/mb_list_encodings/stat (confirmed zero existing coverage for all of these via grep)", "src/registry.rs: 'Reflection-class modeling in the registry' - confirmed no existing Reflection-specific code (only a doc comment at ~L18 noting ReflectionClass isn't used as an implementation detail); would be genuinely new registry surface, location within registry.rs not yet clear from current structure", "SimpleXMLElement magic-property resolution - no existing magic-property-resolution infrastructure found for this class specifically; would likely hook into whatever class-reflection-extension mechanism already exists for other magic-property classes (not located in this pass)"], "notes_from_audit": "This is a batch of many small, independent match-arm additions plus one data-table extension to code that already exists in a clean, isolated form (curl_getinfo_component_type) - the internal batch composition carries essentially no self-conflict risk, and none against the other phases in this set of 6."}}

const CONVENTIONS = `
Project conventions (phpstan-rs, a from-scratch Rust rewrite of PHPStan):
- Tests live alongside the module under test: src/checker/tests/*.rs (one file per
  theme, e.g. array_functions.rs, closures_and_callables.rs), or a tests.rs/tests/
  submodule next to other modules (src/types/tests.rs, src/docblock/tests.rs, etc).
  Find the existing file that best matches your feature's theme and add to it,
  or create a new one following the exact same #[test] fn style as its neighbors
  if nothing fits - don't invent a different test framework or style.
- Match existing code style closely. For a new dynamic-return-type match arm in
  src/checker/expr/builtin_calls.rs, look at how "curl_getinfo" is handled there
  (a match arm keyed on the function name, with a doc comment above it citing the
  real PHP stub/behavior it's modeling) and follow that shape.
- A pre-commit hook (.githooks/pre-commit) runs \`cargo fmt --check\` then
  \`cargo clippy --all-targets --quiet -- -D warnings -A dead_code\` and BLOCKS
  the commit if either fails. Before every commit: run \`cargo fmt\` (not just
  --check, actually apply it), then run the clippy command above yourself and fix
  everything it flags, then run the relevant tests (\`cargo test\`, or a scoped
  \`cargo test <module>\` if the full suite is slow) and confirm they pass. Never
  use --no-verify to bypass the hook - if it fails, fix the real issue.
- Commit message style: descriptive, references "Phase N" from ROADMAP.md, present-
  tense-ish, explaining what changed and often why (see \`git log --oneline -10\`
  for real examples already in this repo's history). If your phase has natural
  sub-pieces, make one focused commit per piece rather than one giant commit at the
  end - this matches how every phase in this project's own completed history was
  built.
- Do NOT modify ROADMAP.md - marking a phase DONE and writing its history entry
  happens later, after this work is reviewed and merged, not as part of this pass.
- Do NOT merge your branch into master and do NOT push anywhere. You are in an
  isolated git worktree specifically so a human can review your diff before it
  lands. Just commit your work on your current branch and stop there.
- If, while implementing, you find the roadmap phase's own text is wrong or stale
  about something (a cited file:line that's moved, a described gap that turns out
  to already be partly handled, a scope that's bigger or smaller than described) -
  earlier audits on this project have found and corrected exactly this kind of
  thing more than once - implement the CORRECT behavior and clearly say what you
  found and changed in your final report. Don't silently follow stale guidance.
`

function phasePrompt(n, p) {
  const locs = p.primary_rust_locations.map(l => `  - ${l}`).join('\n')
  return `You are implementing one phase of phpstan-rs's roadmap in an isolated git
worktree. Full phase text from ROADMAP.md:

${p.roadmap_text}

A prior audit pass already confirmed these specific current-source locations are
where this phase's work belongs (verify line numbers against the actual file, since
they may have drifted slightly, but these are a reliable starting map, not a guess):

${locs}

That same audit's notes on this phase (may include corrections to the roadmap text
above - trust these over the roadmap text where they conflict, they were verified
against the real current source):
${p.notes_from_audit}

${CONVENTIONS}

Implement this phase as fully and correctly as its scope and Size estimate suggest
is reasonable for one focused pass. Write real tests that exercise the new behavior
(both a case that should now be caught/handled, and at least one case that should
NOT be flagged, to guard against false positives - this project treats false
positives as seriously as missed detections). Run the full verification (fmt,
clippy, tests) before considering any commit final.

When done (or if you get genuinely stuck - report that honestly rather than faking
completion), report back: your current branch name, the worktree path you're in
(\`git rev-parse --show-toplevel\`), a summary of what you implemented, every file
you changed, your commit subject lines, what tests you added, the real output of
your final \`cargo test\`/clippy runs, anything you deviated from vs. the roadmap
text and why, and an honest self-assessed status.`
}

const REPORT_SCHEMA = {
  type: 'object',
  properties: {
    phase: { type: 'number' },
    branch: { type: 'string' },
    worktree_path: { type: 'string' },
    summary: { type: 'string' },
    files_changed: { type: 'array', items: { type: 'string' } },
    commits: { type: 'array', items: { type: 'string' } },
    tests_added: { type: 'array', items: { type: 'string' } },
    test_result: { type: 'string' },
    clippy_result: { type: 'string' },
    fmt_result: { type: 'string' },
    deviations_or_issues: { type: 'string' },
    self_assessed_status: { type: 'string', enum: ['complete', 'partial', 'blocked'] },
  },
  required: ['phase', 'branch', 'worktree_path', 'summary', 'files_changed', 'commits', 'test_result', 'clippy_result', 'self_assessed_status'],
}

phase('Implement')
const phaseNums = Object.keys(PILOT).map(Number)
log(`Launching ${phaseNums.length} isolated worktree agents: ${phaseNums.join(', ')}`)

const results = await parallel(
  phaseNums.map(n => () =>
    agent(phasePrompt(n, PILOT[String(n)]), {
      label: `implement:phase${n}`,
      phase: 'Implement',
      schema: REPORT_SCHEMA,
      isolation: 'worktree',
    })
  )
)

const reports = phaseNums.map((n, i) => results[i]).filter(Boolean)
log(`${reports.length}/${phaseNums.length} agents returned a report`)

return { reports, missing: phaseNums.filter((n,i) => !results[i]) }
