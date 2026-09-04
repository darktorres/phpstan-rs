export const meta = {
  name: 'phpstan-batch2-implementation',
  description: 'Implement 6 more independent ROADMAP.md phases (37, 38, 48, 49, 54, 58) in parallel, each in its own git worktree/branch for review before merge',
  phases: [{ title: 'Implement' }],
}

const BATCH = {"37": {"roadmap_text": "**Phase 37 - Attribute (`#[...]`) AST support (PLANNED).**\n`#[Attribute(...)]` is lexed and thrown away entirely\n(`Lexer::skip_whitespace_and_comments`) - real attribute content is\nnever available anywhere in this project, only two narrowly\nspecial-cased stub-generation attributes get lexer-level text splicing\n(`#[PhpStormStubsElementAvailable]`, `#[LanguageLevelTypeAware]`).\nNeeds a real lexer-level fix (emit tokens for `#[`, the name, `(`,\nargs, `,`, `]` instead of skipping the span, reusing the existing\nstring-aware bracket-depth-counting logic those two special cases\nalready use), a new `Attribute { name, args }` node parsed onto every\ndeclaration site that can carry one (`ClassDecl`/`MethodDecl`/\n`PropertyDecl`/`FunctionDecl`/`Param`/`ClassConst`/`EnumCase`, plus\nclosures/arrow functions), and checker-side attribute-class-existence\nand target-validity checking, plus a `#[NoDiscard]` marker.\n\nThe exhaustive audit confirms this is the single most cross-cutting gap\nin the whole codebase - it independently surfaced as an identical,\nalready-verified-real (`attr_groups`/`AttrGroup` confirmed absent from\n`ast.rs`/`parser/classes.rs`) blocker in **six** separate audit\ncategories, and its consolidated unlock list spans nearly every\ndeclaration kind: `AttributesCheck`, `ClassAttributesRule`,\n`ClassConstantAttributesRule`, `NonClassAttributeClassRule`,\n`TraitAttributeClassRule`, `TraitAttributesRule`, `PropertyAttributesRule`,\n`PropertyHookAttributesRule`, `EnumCaseAttributesRule`,\n`MethodAttributesRule`, `ParamAttributesRule`, `FunctionAttributesRule`,\n`ClosureAttributesRule`, `ArrowFunctionAttributesRule`,\n`ConstantAttributesRule`, `CallToMethodStatementWithNoDiscardRule`,\n`CallToStaticMethodStatementWithNoDiscardRule`,\n`CallToFunctionStatementWithNoDiscardRule`. Prerequisite for the\n`#[Override]`/`#[ReturnTypeWillChange]` half of Phase 64's\noverride-compatibility rewrite.", "primary_rust_locations": ["src/lexer.rs: skip_whitespace_and_comments (L517-559) - the b'#' arm's plain '#[' branch (L528-550) currently skips the whole bracket-depth-counted span; must instead emit real tokens ('#[', name, '(', args, ',', ']')", "src/lexer.rs: skip_stub_availability_attribute/skip_language_level_type_aware_attribute (L586-~850) - existing string-aware bracket-depth-counting logic (skip_string_literal, shared with '{$...}' interpolation) to reuse rather than duplicate", "src/ast.rs: new Attribute{name,args} node + an attrs field added onto ClassDecl (L221), MethodDecl (L182), PropertyDecl (L147), FunctionDecl (L129), Param (L108), ClassConst (L198), EnumCase (L256) - confirmed absent (grep for attr_groups/AttrGroup/Attribute found nothing in ast.rs)", "src/parser/classes.rs: parse_class_body (L101) and parse_class_decl (L466) - attribute-list parsing before each declaration site", "src/parser/functions.rs: parse_params (L8) and parse_function_decl (L93)", "src/checker/stmt.rs: StmtKind::FunctionDecl/ClassDecl dispatch (L609-610) and src/checker/oop.rs - new attribute-class-existence/target-validity checker wired at this existing per-declaration dispatch point"], "notes_from_audit": "Foundational and self-contained: the lexer rewrite reuses its own existing string-aware depth-counter, the two new struct fields per declaration kind are additive, and nothing else currently reads/writes these AST nodes. Only the attribute-constructor-argument-validation sub-feature genuinely needs Phase 49's call-argument binding generalization first (named per Phase 49's own text); the core lex/parse/AST work does not."}, "38": {"roadmap_text": "**Phase 38 - Target-PHP-version configuration (PLANNED).** No\n`php_version` setting exists anywhere (`config.rs`/CLI) - every \"X only\nsupported on PHP Y.Z and later\" portability check is unbuildable\nwithout one. New `php_version: Option<(u8, u8)>`, following the exact\nprecedent of `strict_mixed_arrays` (Phase 11 Stage 5: `config.rs` field\n-> CLI flag -> `Project` setter), plus a small `PhpFeature -> (u8, u8)`\ntable.\n\nThe exhaustive audit found this is already correctly scoped, but is a\n**recurring blocker** across four other audit categories - worth\nresequencing earlier than its number implies, since it gates the\nversion-gate half of `FinalConstantRule`, `NativeTypedClassConstantRule`,\n`ConstantAttributesRule`, `DeprecatedCastRule`, `VoidCastRule`,\n`BacktickRule`, `UnsetCastRule`, `ArrayUnpackingRule`, `ReadOnlyClassRule`,\n`ConstantsInTraitsRule`, `InvalidPromotedPropertiesRule`,\n`NoncapturingCatchRule`, and `ThrowExpressionRule`.", "primary_rust_locations": ["src/config.rs: Config struct (L23-36) - add pub php_version: Option<String> (or Option<(u8,u8)>) alongside the existing strict_mixed_arrays: Option<bool> (L28)", "src/main.rs: Command::CheckProject variant (L422-455) - new #[arg(long)] php_version field, mirroring strict_mixed_arrays (L434-435); main()'s match arm destructuring/threading it (L476-485+) and the config-vs-CLI merge logic (around L531-532, same pattern as `strict_mixed_arrays || config.strict_mixed_arrays.unwrap_or(false)`)", "src/project.rs: new set_php_version setter (L88-95 area), mirroring set_strict_mixed_arrays which stores onto registry.strict_mixed_arrays (L93-95)", "new small PhpFeature -> (u8,u8) lookup table, likely a new file (e.g. src/php_version.rs) or appended to registry.rs"], "notes_from_audit": "Exactly mirrors the existing strict_mixed_arrays precedent end to end (config field -> CLI flag -> merge -> Project setter -> Registry field), confirmed by reading that precedent directly. It's a leaf/enabling phase - many other phases (39-branching rules named in its own unlock list) depend on it existing, not vice versa, so it should be sequenced early rather than needing anything else first."}, "48": {"roadmap_text": "**Phase 48 - Symbol-existence validation pipeline (classes, functions,\nuse-imports, typehints) (NEW, PLANNED).** `resolve_name_expr`\n(`src/checker/resolve.rs:39`) unconditionally returns\n`Type::Object(name, ..)` regardless of whether the class exists, and\n`resolve_type_hint` (`src/checker/resolve.rs:53`) silently accepts a\ntypo'd class name in a param/return/property type unless that name is\n*also* used at a `new` or call site elsewhere - confirmed by reading\nboth functions directly. Build the missing \"does this name actually\nresolve\" primitive once (`ClassKind` is already tracked per class,\n`src/ast.rs:213-218`, and the registry can already resolve a name to a\n`ClassInfo` - this is checker wiring reusing existing data, not a new\ndata model), then wire it into every site that currently trusts a name\nblindly: `use`/`use function`/`use const`/group-use validation,\ncatch-clause types, `extends`/`implements`/`instanceof`/trait-use\ntargets, and - the single highest-value application - parameter/\nreturn/property type hints.\n\nThis is the highest real-world-value item in the entire audit -\ntypo'd/missing class references are one of PHPStan's most common\ncatches, and it surfaced as an independent blocker in *seven* separate\naudit categories. Consolidated unlock list: `ExistingNamesInUseRule`,\n`ExistingNamesInGroupUseRule`, `CaughtExceptionExistenceRule`,\n`ExistingClassInClassExtendsRule` (partially done already -\n`check_final_class_extension`, `src/checker/oop.rs:16-34`, implements\nthis rule's final-class-extension sub-check using this same registry\ndata), `ExistingClassesInClassImplementsRule`,\n`ExistingClassesInEnumImplementsRule`, `ExistingClassesInInterfaceExtendsRule`,\n`ExistingClassInInstanceOfRule`, `ExistingClassInTraitUseRule`,\n`ExistingClassesInTypehintsRule`, `ExistingClassesInPropertiesRule`,\n`ExistingClassesInArrowFunctionTypehintsRule`,\n`ExistingClassesInClosureTypehintsRule`,\n`ExistingClassesInPropertyHookTypehintsRule`, `ClassConstantRule`,\n`ClassNameCheck`/`ClassCaseSensitivityCheck`/`ClassForbiddenNameCheck`\n(case-sensitivity + leaked-prefix checks fold in cheaply once the\nlookup exists). A fuller version of this same primitive - a centralized\n\"resolve a type, split union/intersection members, collect per-class\nexistence errors while respecting `class_exists()`/`interface_exists()`\nguards, tag each with a usage-location-specific message + identifier\"\npipeline matching PHPStan's `RuleLevelHelper::findTypeToCheck` +\n`ClassNameUsageLocation` taxonomy - is a stretch goal on top of this\nphase's MVP; it additionally needs Phase 36's attribute/assert/\ntemplate/sealed/mixin/requireExtends/requireImplements tag parsing to\nreach several of PHPStan's ~27 usage-location kinds, and unlocks\n`ClassNameNodePair`, `ClassNameUsageLocation`, `FoundTypeResult`, and\n`RuleLevelHelper`'s `findTypeToCheck()` half. Prerequisite for\n`InstantiationRule`'s remaining trait/enum-instantiation cases and\nseveral Phase 15-adjacent items. Size: L for the MVP, XL for the\nfull `RuleLevelHelper`-equivalent pipeline.", "primary_rust_locations": ["src/checker/resolve.rs: resolve_name_expr (L26-41, confirmed unconditionally returns Type::Object regardless of existence) and resolve_type_hint (L53-58, confirmed silently accepts any class name) - need existence-checking wired in, likely via a new reporting pass consuming these", "src/checker/oop.rs: check_final_class_extension (L16-34) - already does the identical extends-lookup for the finality sub-check; concrete place to add ExistingClassInClassExtendsRule's existence half", "src/checker/stmt.rs: try/catch handling (L474-520) already has a 'cannot catch X: it does not implement Throwable' check (L515) - CaughtExceptionExistenceRule extends this exact block to check existence before the implements-Throwable check", "src/checker/expr.rs: ExprKind::Instanceof arm (L681-690) for ExistingClassInInstanceOfRule; ExprKind::Closure (L1267) and ExprKind::ArrowFn (L1344) arms for ExistingClassesInClosureTypehintsRule/ExistingClassesInArrowFunctionTypehintsRule", "src/registry/name_context.rs: NameContext::add_uses (L67-79) - confirmed no checker code anywhere validates use/use-function/use-const imports; needs a new pass, likely called near the top of checker.rs's check_program (~L300-313)"], "notes_from_audit": "Confirmed the highest-value, most structurally independent phase of the six: resolve_name_expr/resolve_type_hint really do trust names blindly today, and check_final_class_extension really is a partial precedent for the existence-lookup pattern this phase generalizes. MVP has no hard cross-phase blocker; only the stretch-goal RuleLevelHelper-equivalent pipeline needs Phase 36's tag parsing."}, "49": {"roadmap_text": "**Phase 49 - Call-argument binding generalization (named args, spread,\nby-ref legality) (NEW, PLANNED).** `check_args`\n(`src/checker/calls.rs:262`) implements only the MVP: positional arity\nplus a per-positional type check. Extend it with named-argument handling\n(unknown/duplicate/positional-after-named), unpacked/spread arguments,\nand by-ref target legality - the last needs the same \"does this access\nchain contain a nullsafe operator\" walk `NullsafeCheck` provides in real\nPHPStan, which this project has never built despite already tracking\nnullsafe elsewhere. Unlocks `FunctionCallParametersCheck`,\n`NullsafeCheck`. Prerequisite for attribute constructor-argument\nvalidation (Phase 37). Size: L - one of the highest-value gaps found;\nnamed-argument and by-ref misuse are common in modern PHP.", "primary_rust_locations": ["src/checker/calls.rs: CheckArgsCall struct (L24-33) and check_args() (L263-361) - currently only counts required/positional arity and does a per-position type check; has no named-argument unknown/duplicate/positional-after-named validation at all", "src/checker/calls.rs: resolve_call_args() (L572-593) - `if arg.spread { break; }` at L577 silently stops resolving anything after the first spread argument instead of modeling unpacking", "src/checker/calls.rs: predefine_byref_or_unknown_args() (L55-240) - by-ref legality (rejecting a nullsafe-chain or non-lvalue expression passed by-ref) would need a new access-chain walk added here or as a sibling helper", "src/ast.rs: Arg struct (L303-307) already carries `name`/`spread` - no AST change needed, confirmed adequate for Item 1's existing resolve_call_args", "new helper (no existing counterpart): a 'does this expression's access chain contain a nullsafe operator' walk over PropertyAccess/MethodCall's own `nullsafe: bool` field (ast.rs L426), which nothing in the checker currently does generically - nullsafe is only ever read locally per-node (checker/expr.rs L526, L1154, L1685, L1770)"], "notes_from_audit": "Self-contained in calls.rs; doesn't touch types.rs/expr.rs's Type-system machinery at all, so it has essentially no overlap with 50/51/52/53/54. Phase 37 (attribute constructor-arg validation) depends on this phase landing first, not the reverse."}, "54": {"roadmap_text": "**Phase 54 - Generic template richness (`TemplateParam` bound/scope/\nvariance rewrite) (NEW, PLANNED).** `Type::TemplateParam(String)`\n(`src/types.rs:~60`) is a bare name with no bound, scope, variance, or\ndefault - one flat placeholder standing in for ~19 of PHPStan's\nbound-specific `Template*Type` subclasses plus the `TemplateType`\ninterface/trait. An unbound template can never be used as its own\nbound's type (indexed as an array, compared as a bool, treated as its\nbound's object methods) prior to substitution; it just becomes an\nopaque name until `class_template_bindings`/`substitute_template`\nresolve it. Add a `bound: Box<Type>` field (plus scope/variance/\ndefault), rewrite `substitute_template`/`class_template_bindings`/\n`check_variance` (`src/checker/generics.rs`) to compose variance\nthrough nested generics instead of the current flat class-level\n`covariant_templates`/`contravariant_templates` name-sets, and add\n`@template T = Default` PHPDoc parsing (currently silently dropped).\n\nThis is the single largest cluster by class count in the whole audit -\nPHPStan splits \"template bound to X\" into ~19 concrete subclasses where\nphpstan-rs uses one variant. Unlocks the 19-class `Template*Type`\nfamily (`TemplateArrayType`, `TemplateBooleanType`,\n`TemplateConstantArrayType`, `TemplateConstantIntegerType`,\n`TemplateConstantStringType`, `TemplateFloatType`,\n`TemplateGenericObjectType`, `TemplateIntegerType`,\n`TemplateIntersectionType`, `TemplateIterableType`, `TemplateKeyOfType`,\n`TemplateMixedType`, `TemplateNullType`, `TemplateObjectShapeType`,\n`TemplateObjectType`, `TemplateObjectWithoutClassType`,\n`TemplateStringType`, `TemplateUnionType`, plus `TemplateType`/\n`TemplateTypeTrait`/`TemplateTypeStrategy`), `TemplateTypeVariance`,\n`TemplateTypeVarianceMap`, `TemplateTypeMap`, `TemplateTypeHelper`,\n`TemplateTypeFactory`, `TemplateTypeScope`, `VarianceCheck`,\n`MethodSignatureVarianceRule`, `FunctionSignatureVarianceRule`,\n`PropertyVarianceRule`, `ClassAncestorsRule`, `GenericAncestorsCheck`,\n`GenericObjectTypeCheck`, `InterfaceAncestorsRule`,\n`CrossCheckInterfacesHelper`, `GenericStaticType` (`static` losing its\nown generic type arguments in generic classes). `TemplateKeyOfType`/\n`TemplateObjectShapeType` specifically also need Phase 80's `key-of<T>`/\n`object{...}` PHPDoc syntax as a prerequisite; everything else in this\nphase is independent of it. Size: XL - a genuine algorithmic rewrite,\nnot incremental wiring, but real-world value is high for\ngeneric-collection-heavy codebases (arg-count mismatches, fluent-builder\n`static` returns).", "primary_rust_locations": ["src/types.rs: Type::TemplateParam(String) (L60) - needs bound/scope/variance/default fields added", "src/types.rs: bind_template_param() (L939-960) and substitute_template() (L964-991) - both have a TemplateParam arm that would need bound-aware logic, plus their existing Array/ClassString/Union/Intersection/ArrayShape recursive arms stay structurally the same", "src/types.rs: is_subtype()'s TemplateParam arm (L894, `(Type::TemplateParam(_), _) | (_, Type::TemplateParam(_)) => true`) - today a blanket 'always compatible' escape hatch that a real bound would need to replace with an actual bound-aware comparison", "src/checker/generics.rs: check_variance() (L190-233) - rewrite from the current flat class-level covariant_templates/contravariant_templates HashSet<String> name-sets to real per-position variance composition through nested generics", "src/checker/generics.rs: class_template_bindings()/class_template_bindings_rec() (L263-335) and substitute_class_templates()/substitute_scoped() (L364-431) and collect_template_params() (L439-457) - all currently name-keyed HashMap<String,Type>/HashSet<String> machinery (TemplateBindings) that a bound/variance-carrying TemplateParam would need to thread through", "src/registry/class_info.rs: templates/template_bounds/covariant_templates/contravariant_templates fields (L57-89) and their construction from docblock data (L95-278)", "src/docblock.rs: the \"template\"/\"template-covariant\"/\"template-contravariant\" tag arms (L228-261) and parse_template_tag() (~L281+) - `@template T = Default` parsing is confirmed absent (only name + optional bound is parsed today)"], "notes_from_audit": "The roadmap's own 'everything else in this phase is independent of it' line about Phase 80 is why Phase 80 is listed as a soft dependency rather than a hard prerequisite - only two of the ~19 Template*Type subclasses actually need it."}, "58": {"roadmap_text": "**Phase 58 - `MissingTypehintCheck` completeness battery (level-6\nfamily) (NEW, PLANNED).** No `MissingTypehintCheck`-equivalent exists\nanywhere: no detection of implicit-mixed parameters/returns, bare\n`iterable`/`array` with no value type, non-generic use of a generic\nclass, or `callable` with no signature - for any declaration kind\n(function, method, closure, arrow-fn, property, class constant).\nUnlocks `MissingClassConstantTypehintRule`, `MissingMethodParameterTypehintRule`,\n`MissingMethodReturnTypehintRule`, `MissingPropertyTypehintRule`,\n`MissingFunctionParameterTypehintRule`, `MissingFunctionReturnTypehintRule`.\nSize: L - build the generic `Type`-tree walker once, apply it at five\ndeclaration-kind call sites. High value for teams targeting\nPHPStan-level-6-equivalent strictness; not a correctness-bug catcher,\nmore a completeness nudge.", "primary_rust_locations": ["New module needed (e.g. src/checker/missing_typehint.rs) \u2014 confirmed via grep that no MissingTypehintCheck-equivalent, 'no value type', or implicit-mixed detection exists anywhere in src/checker*", "src/checker.rs: check_program (~L286-356, confirmed) \u2014 needs new per-class-constant/per-property call sites parallel to the existing check_class_completeness/check_final_class_extension/... calls in the StmtKind::ClassDecl arm", "src/checker.rs: check_function (~L438+, confirmed entry point for every function/method/closure body check) \u2014 needs a new call for param/return typehint completeness alongside its existing per-function checks", "src/registry/class_info.rs (PropertySig.ty, ClassInfo.consts) and src/registry/function_sig.rs (FunctionSig/ParamSig) \u2014 read-only consumers supplying each declaration's already-resolved Type to walk for Mixed/bare-array/bare-iterable/non-generic-use/signature-less-callable", "Closure/arrow-fn literal checking sites in src/checker/expr.rs (wherever ExprKind::Closure/ArrowFn bodies are checked, e.g. check_closure_literal_with_this's siblings) \u2014 needed for the closure/arrow-fn declaration kinds the phase lists"], "notes_from_audit": "Confirmed via grep across src/checker.rs and src/checker/*.rs that no missing-typehint infrastructure exists today \u2014 this is genuinely new. The five target rules map to five distinct declaration kinds already fully resolved into Type by the registry (properties via PropertySig, consts via ClassInfo.consts, params/returns via FunctionSig/ParamSig), so the phase's own 'build the walker once, apply at five call sites' framing checks out against the actual code."}}

const CONVENTIONS = `
Project conventions (phpstan-rs, a from-scratch Rust rewrite of PHPStan):
- Tests live alongside the module under test: src/checker/tests/*.rs (one file per
  theme, e.g. array_functions.rs, closures_and_callables.rs), or a tests.rs/tests/
  submodule next to other modules (src/types/tests.rs, src/docblock/tests.rs, etc).
  Find the existing file that best matches your feature's theme and add to it,
  or create a new one following the exact same #[test] fn style as its neighbors
  if nothing fits - don't invent a different test framework or style.
- Match existing code style closely.
- This repo's master branch was JUST updated (six other phases: 16, 40, 47, 59, 72,
  77) with real, substantive changes to src/checker.rs (Diagnostic gained an
  \`identifier: String\` field plus tip/non_ignorable/metadata/fix - EVERY
  self.report(...) call now takes 5 arguments, the 5th a dot-separated identifier
  string like "class.notFound" - see existing call sites for the convention, and
  real PHPStan's own vendor/phpstan-src/src/Rules/**/*.php \`->identifier(...)\`
  calls for the exact string to match where a real PHPStan rule exists), and
  src/registry/class_info.rs/ClassInfo gained is_api/is_api_do_not_implement fields.
  You are starting from this already-updated master - read the CURRENT state of any
  file you touch, don't assume it matches what the roadmap text below describes if
  the roadmap text predates those phases (it was written before they were confirmed
  implemented). If you add any new diagnostic (self.report(...) call or a manually
  constructed Diagnostic{...}), you MUST supply a real identifier string - grep
  vendor/phpstan-src for the equivalent real rule's own ->identifier(...) call
  first, and only invent a new dot-separated identifier following this project's
  existing convention (see src/checker.rs's other call sites) when no real PHPStan
  equivalent exists.
- A pre-commit hook (.githooks/pre-commit) runs \`cargo fmt --check\` then
  \`cargo clippy --all-targets --quiet -- -D warnings -A dead_code\` and BLOCKS
  the commit if either fails. Before every commit: run \`cargo fmt\` (not just
  --check, actually apply it), then run the clippy command above yourself and fix
  everything it flags, then run the relevant tests (\`cargo test\`, or a scoped
  \`cargo test <module>\` if the full suite is slow) and confirm they pass. Never
  use --no-verify to bypass the hook - if it fails, fix the real issue. Note: one
  pre-existing, environment-specific test - tests/cli.rs's
  check_does_not_crash_on_realistically_deep_nested_expressions - fails at this
  sandbox's default stack size regardless of any change (a stack-overflow flake
  confirmed identical on a clean unmodified checkout, unrelated to any of this
  work) - don't chase it, and use RUST_MIN_STACK=67108864 when running the full
  test suite to sidestep it and see real signal.
- Commit message style: descriptive, references "Phase N" from ROADMAP.md, present-
  tense-ish, explaining what changed and often why (see \`git log --oneline -15\`
  for real examples already in this repo's history, including the six phases that
  just landed). If your phase has natural sub-pieces, make one focused commit per
  piece rather than one giant commit at the end.
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
they may have drifted since that audit and since six other phases just landed on
master - these are a reliable starting map, not a guess):

${locs}

That same audit's notes on this phase (may include corrections to the roadmap text
above - trust these over the roadmap text where they conflict, they were verified
against the real current source at the time):
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
const phaseNums = Object.keys(BATCH).map(Number)
log(`Launching ${phaseNums.length} isolated worktree agents: ${phaseNums.join(', ')}`)

const results = await parallel(
  phaseNums.map(n => () =>
    agent(phasePrompt(n, BATCH[String(n)]), {
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
