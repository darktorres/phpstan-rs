export const meta = {
  name: 'phpstan-parity',
  description: 'Gap-analyze phpstan-rs against real PHPStan: rule coverage (src/Rules/*) and type-system parity (src/Type/*)',
  phases: [
    { title: 'Read' },
    { title: 'Synthesize' },
    { title: 'Combine' },
  ],
}

// ---- shared context handed to every reading agent ----
const REPO_CONTEXT = `
You are analyzing two codebases in the same git repo:
- vendor/phpstan-src/  — the real PHPStan, written in PHP. Ground truth for what a full
  PHP static analyzer needs to check.
- src/ — phpstan-rs, a from-scratch Rust rewrite aiming for feature parity. Its checker
  lives in src/checker/ (assignment.rs, calls.rs, control_flow.rs, generics.rs,
  narrowing.rs, oop.rs, resolve.rs, stmt.rs, expr.rs, expr/*), its type system in
  src/types.rs, parsing in src/parser/ and src/lexer/, docblock handling in
  src/docblock/.

Do not trust naming similarity alone — read the actual PHP source for what a rule/type
checks, then grep/read the actual Rust source to see if equivalent logic exists. A
missing 1:1 file is not evidence of a missing feature — phpstan-rs consolidates many
PHPStan rules into single match arms inside larger files. Conversely, a file that exists
with a similar name may only be a partial or stale implementation — check the logic, not
just presence.
`

const RULE_STATUS_ITEM = {
  type: 'object',
  properties: {
    rule_name: { type: 'string', description: 'PHPStan class name, e.g. ArrayUnpackingRule' },
    phpstan_path: { type: 'string', description: 'path under vendor/phpstan-src/' },
    checks: { type: 'string', description: 'one sentence: what this rule actually flags' },
    status: { type: 'string', enum: ['implemented', 'partial', 'missing', 'not_applicable'] },
    our_location: { type: 'string', description: 'file:function in src/ implementing it, if any; empty if missing' },
    notes: { type: 'string', description: 'why partial/missing/not_applicable, or a caveat on the implemented behavior' },
  },
  required: ['rule_name', 'checks', 'status'],
}

const BUCKET_RESULT_SCHEMA = {
  type: 'object',
  properties: {
    category: { type: 'string' },
    items: { type: 'array', items: RULE_STATUS_ITEM },
    summary: { type: 'string', description: '2-3 sentences: the headline finding for this category' },
  },
  required: ['category', 'items', 'summary'],
}

const SYNTHESIS_SCHEMA = {
  type: 'object',
  properties: {
    report_markdown: { type: 'string', description: 'full markdown report section for this phase' },
    top_gaps: {
      type: 'array',
      items: { type: 'string' },
      description: 'ranked list of the highest-value missing/partial items to close next, most valuable first',
    },
  },
  required: ['report_markdown', 'top_gaps'],
}

// ---- Phase A: rule-coverage buckets (vendor/phpstan-src/src/Rules/*) ----
const RULE_BUCKETS = [
  {
    key: 'functions',
    dirs: ['vendor/phpstan-src/src/Rules/Functions/', 'vendor/phpstan-src/src/Rules/Generators/'],
    focus: 'function declarations, calls, parameter/return checks, generators',
  },
  {
    key: 'classes-traits',
    dirs: ['vendor/phpstan-src/src/Rules/Classes/', 'vendor/phpstan-src/src/Rules/Traits/'],
    extraFiles: [
      'vendor/phpstan-src/src/Rules/ClassCaseSensitivityCheck.php',
      'vendor/phpstan-src/src/Rules/ClassForbiddenNameCheck.php',
      'vendor/phpstan-src/src/Rules/ClassNameCheck.php',
    ],
    focus: 'class/interface/enum declarations, inheritance, trait usage',
  },
  {
    key: 'properties',
    dirs: [
      'vendor/phpstan-src/src/Rules/Properties/',
      'vendor/phpstan-src/src/Rules/EnumCases/',
      'vendor/phpstan-src/src/Rules/Whitespace/',
    ],
    focus: 'property declarations/access, enum cases',
  },
  {
    key: 'methods',
    dirs: [
      'vendor/phpstan-src/src/Rules/Methods/',
      'vendor/phpstan-src/src/Rules/Keywords/',
      'vendor/phpstan-src/src/Rules/Debug/',
      'vendor/phpstan-src/src/Rules/Pure/',
    ],
    extraFiles: [
      'vendor/phpstan-src/src/Rules/FunctionCallParametersCheck.php',
      'vendor/phpstan-src/src/Rules/FunctionDefinitionCheck.php',
      'vendor/phpstan-src/src/Rules/FunctionReturnTypeCheck.php',
      'vendor/phpstan-src/src/Rules/UnusedFunctionParametersCheck.php',
    ],
    focus: 'method declarations/calls, signature checks, purity/keyword misuse',
  },
  {
    key: 'phpdoc',
    dirs: [
      'vendor/phpstan-src/src/Rules/PhpDoc/',
      'vendor/phpstan-src/src/Rules/Missing/',
      'vendor/phpstan-src/src/Rules/Types/',
      'vendor/phpstan-src/src/Rules/Ignore/',
    ],
    extraFiles: [
      'vendor/phpstan-src/src/Rules/AttributesCheck.php',
      'vendor/phpstan-src/src/Rules/NullsafeCheck.php',
      'vendor/phpstan-src/src/Rules/IssetCheck.php',
      'vendor/phpstan-src/src/Rules/MissingTypehintCheck.php',
    ],
    focus: 'docblock tag validation, missing typehints, ignore-comment handling',
  },
  {
    key: 'comparison-deadcode',
    dirs: [
      'vendor/phpstan-src/src/Rules/Comparison/',
      'vendor/phpstan-src/src/Rules/DeadCode/',
      'vendor/phpstan-src/src/Rules/TooWideTypehints/',
    ],
    focus: 'always-true/false conditions, unreachable code, overly-wide declared types. NOTE: our ROADMAP.md already tracks Comparison/DeadCode/TooWideTypehints as largely unimplemented (Phase 15) — confirm/refine that, don\'t just restate it blindly.',
    extraNote: 'IMPORTANT: a previous run of this exact bucket returned placeholder/template data (rule_name "Foo", phpstan_path "a/b.php", a single fake item) instead of real analysis — that is a failure, not an acceptable answer. You must actually open and read the real files in vendor/phpstan-src/src/Rules/Comparison/, vendor/phpstan-src/src/Rules/DeadCode/, and vendor/phpstan-src/src/Rules/TooWideTypehints/ (list them with a directory listing first), and every returned item must name a real PHPStan class from one of those directories with genuine, specific checks/notes text grounded in what you actually read.',
  },
  {
    key: 'generics-variables-constants',
    dirs: [
      'vendor/phpstan-src/src/Rules/Generics/',
      'vendor/phpstan-src/src/Rules/Variables/',
      'vendor/phpstan-src/src/Rules/Constants/',
    ],
    focus: 'template/generic bound checks, variable definiteness, class/enum constants',
  },
  {
    key: 'arrays-cast-operators-regex',
    dirs: [
      'vendor/phpstan-src/src/Rules/Arrays/',
      'vendor/phpstan-src/src/Rules/Cast/',
      'vendor/phpstan-src/src/Rules/Operators/',
      'vendor/phpstan-src/src/Rules/Regexp/',
    ],
    focus: 'array shape/offset checks, casts, operator type checks, regex pattern validation',
  },
  {
    key: 'exceptions-api-restricted',
    dirs: [
      'vendor/phpstan-src/src/Rules/Exceptions/',
      'vendor/phpstan-src/src/Rules/Api/',
      'vendor/phpstan-src/src/Rules/RestrictedUsage/',
      'vendor/phpstan-src/src/Rules/Playground/',
      'vendor/phpstan-src/src/Rules/Namespaces/',
    ],
    focus: 'throws-type checking, @internal/@deprecated/@final API enforcement, restricted class/function usage, namespace checks. NOTE: Api/Playground/RestrictedUsage are largely about *PHPStan\'s own* extension ecosystem and its public playground — judge case by case whether an equivalent is meaningful for phpstan-rs at all (mark not_applicable where it genuinely is not), rather than assuming everything is a gap.',
  },
]

function ruleBucketPrompt(b) {
  const dirList = b.dirs.map(d => `  - ${d}`).join('\n')
  const extra = b.extraFiles ? `\nAlso these standalone files:\n${b.extraFiles.map(f => `  - ${f}`).join('\n')}\n` : ''
  const note = b.extraNote ? `\n${b.extraNote}\n` : ''
  return `${REPO_CONTEXT}${note}
Category: "${b.key}" — ${b.focus}

List every *Rule.php (and relevant Check.php) class in these directories:
${dirList}${extra}

For each one: read enough of it to know precisely what it flags (skip pure DI/wiring
files, error-message-only subclasses, and abstract base classes with no rule logic of
their own — but do call those out by name in your summary if you skip more than a
couple). Then search src/checker/ (and src/types.rs, src/parser/, src/docblock/ if
relevant) for equivalent logic. Classify each as implemented / partial / missing /
not_applicable (not_applicable = the rule only makes sense for PHPStan's own extension
API, config system, or IDE integration, not for a type-checker rewrite — justify briefly
when you use it).

Return one item per PHPStan rule class you examined, plus a 2-3 sentence summary of the
headline finding for this category.`
}

// ---- Phase B: type-system buckets (vendor/phpstan-src/src/Type/*) ----
const TYPE_BUCKETS = [
  {
    key: 'core-types',
    prompt: `${REPO_CONTEXT}
Category: "core-types" — PHPStan's core Type class hierarchy.

List the top-level (non-subdirectory) *.php files directly in
vendor/phpstan-src/src/Type/ (e.g. UnionType, IntersectionType, ObjectType, ArrayType,
StringType, IntegerType, CallableType, ClosureType, StaticType, ThisType, VoidType,
NeverType, MixedType, NullType, BooleanType, and the TypeCombinator/TypeUtils/
VerbosityLevel helper classes, etc — there are roughly 100 files here).

For each: what does it represent, and does src/types.rs (the phpstan-rs Type enum and
its subtyping/combination logic) have an equivalent variant or code path? Read
src/types.rs directly rather than guessing from the enum names alone — check whether the
*behavior* (subtyping rules, narrowing, combination/union simplification) matches, not
just whether a variant with a similar name exists.

Return one item per PHPStan core type class, classified implemented/partial/missing/
not_applicable, plus a summary.`,
  },
  {
    key: 'generics-accessory-constant',
    prompt: `${REPO_CONTEXT}
Category: "generics-accessory-constant" — PHPStan's generic/template types, accessory
types, and constant (literal) types.

List the *.php files in:
  - vendor/phpstan-src/src/Type/Generic/  (TemplateType and friends, GenericObjectType, ...)
  - vendor/phpstan-src/src/Type/Accessory/ (AccessoryNonEmptyStringType, NonEmptyArrayType,
    OversizedArrayType, AccessoryNumericStringType, AccessoryLiteralStringType, ...)
  - vendor/phpstan-src/src/Type/Constant/ (ConstantArrayType, ConstantStringType,
    ConstantIntegerType, ConstantBooleanType, ...)

For each: what does it represent, and does phpstan-rs (src/types.rs, src/checker/generics.rs)
have equivalent representation and subtyping/narrowing behavior? Pay particular attention
to template-bound checking and array-shape (ConstantArrayType) fidelity — these are
called out in ROADMAP.md as areas with known partial coverage (e.g. Type::Array having no
key-type parameter) — confirm/refine rather than restate blindly.

Return one item per class, classified, plus a summary.`,
  },
  {
    key: 'php-dynamic-extensions',
    prompt: `${REPO_CONTEXT}
Category: "php-dynamic-extensions" — PHPStan's ~190 per-function/per-method dynamic
return-type extensions in vendor/phpstan-src/src/Type/Php/ (e.g. ArrayMapFunctionReturnTypeExtension,
CurlGetinfoFunctionDynamicReturnTypeExtension, PregMatchTypeSpecifyingExtension,
StrSplitFunctionReturnTypeExtension, VersionCompareFunctionDynamicReturnTypeExtension, ...).
This directory is too large to review file-by-file — instead:

1. List all files and group them into rough families by the stdlib area they cover
   (array_*, string/str_*, preg_*/regex, curl_*, date/DateTime, json_*, class-reflection
   (get_class/instanceof/property_exists-shaped), sorting, math, sprintf/printf, misc).
2. For each family, report the file count, and grep src/checker/ (especially calls.rs and
   expr.rs) for whether phpstan-rs special-cases those same function names for dynamic
   return typing.
3. Report per-family coverage (roughly what fraction of that family's functions phpstan-rs
   special-cases) and name a handful of concrete, currently-unhandled functions from the
   family with the largest gap as examples — don't try to enumerate all ~190.

Note: ROADMAP.md already documents this as an intentionally curated subset (not aiming
for all ~190), grown incrementally against real corpus false-positives — your job is to
size the *current* gap precisely, not to argue we should implement everything.

Return items where each item is one family (not one PHPStan file), status = your rough
coverage classification (implemented = most of family covered, partial = some, missing =
none), plus a summary with the top 2-3 highest-value uncovered families.`,
  },
  {
    key: 'type-misc',
    prompt: `${REPO_CONTEXT}
Category: "type-misc" — remaining Type subsystem pieces.

List the *.php files in:
  - vendor/phpstan-src/src/Type/Regex/ (regex pattern -> capture-group type inference)
  - vendor/phpstan-src/src/Type/Traits/ (shared trait implementations for Type classes —
    skip pure trait plumbing with no independent behavior, note if you do)
  - vendor/phpstan-src/src/Type/Traverser/, Type/PHPStan/, Type/Helper/, Type/Enum/

For each meaningful one: what does it do, and does phpstan-rs have an equivalent? For
Regex/ specifically, cross-check against src/checker (regex-related tests exist at
src/checker/tests/preg_and_regex.rs and src/regex_groups/) — this is likely to be one of
phpstan-rs's stronger areas; confirm precisely how far it goes (e.g. does capture-group
typing from a literal pattern work, and for which preg_* functions).

Return one item per meaningful class/file, classified, plus a summary.`,
  },
]

// ---- run all reads in parallel (independent, no cross-item dependency needed yet) ----
phase('Read')
const allBuckets = [
  ...RULE_BUCKETS.map(b => ({ phase: 'rules', key: b.key, prompt: ruleBucketPrompt(b) })),
  ...TYPE_BUCKETS.map(b => ({ phase: 'types', key: b.key, prompt: b.prompt })),
]

const bucketResults = await parallel(
  allBuckets.map(b => () =>
    agent(b.prompt, { label: `read:${b.key}`, phase: 'Read', schema: BUCKET_RESULT_SCHEMA })
  )
)

const ruleResults = bucketResults
  .map((r, i) => ({ r, meta: allBuckets[i] }))
  .filter(x => x.r && x.meta.phase === 'rules')
  .map(x => x.r)
const typeResults = bucketResults
  .map((r, i) => ({ r, meta: allBuckets[i] }))
  .filter(x => x.r && x.meta.phase === 'types')
  .map(x => x.r)

log(`Read phase done: ${ruleResults.length}/${RULE_BUCKETS.length} rule buckets, ${typeResults.length}/${TYPE_BUCKETS.length} type buckets returned.`)

// ---- synthesize each phase (needs all of that phase's bucket results together) ----
phase('Synthesize')

function synthPrompt(phaseName, results, note) {
  return `You are synthesizing a "${phaseName}" gap-analysis pass comparing phpstan-rs
(a Rust rewrite of PHPStan, mid-implementation) against real PHPStan's source
(vendor/phpstan-src/). ${note}

Below is the raw per-category findings as JSON, produced by independent reading agents
who each covered one category:

${JSON.stringify(results, null, 2)}

Write a markdown report section (## heading for the phase, ### subheadings per category)
that:
- Gives an implemented/partial/missing count per category and overall.
- Highlights genuinely notable findings — surprises, categories that are further along or
  further behind than ROADMAP.md's own notes suggest, and anything marked not_applicable
  with its justification.
- Is honest about "missing" being expected at this stage of a mid-implementation rewrite —
  don't editorialize that every gap is a problem.

Also return a ranked list (top_gaps) of the ~8 highest-value missing/partial items to
close next, most valuable first, each as one line naming the specific rule/type and why
it matters (e.g. real-world frequency, or that it blocks other work).`
}

const [ruleSynthesis, typeSynthesis] = await parallel([
  () => agent(
    synthPrompt('Rule coverage', ruleResults, 'This pass compared PHPStan\'s src/Rules/* classes (408 files) against phpstan-rs\'s checker.'),
    { label: 'synthesize:rules', phase: 'Synthesize', schema: SYNTHESIS_SCHEMA }
  ),
  () => agent(
    synthPrompt('Type-system parity', typeResults, 'This pass compared PHPStan\'s src/Type/* hierarchy (392 files) against phpstan-rs\'s src/types.rs and related type-inference code.'),
    { label: 'synthesize:types', phase: 'Synthesize', schema: SYNTHESIS_SCHEMA }
  ),
])

// ---- final combined report ----
phase('Combine')

const combined = await agent(
  `Combine these two independently-written gap-analysis report sections into one cohesive
markdown document titled "# phpstan-rs vs PHPStan: gap analysis" with a short intro
paragraph (what was compared, how — parallel agents reading vendor/phpstan-src/src/Rules/*
and src/Type/* against phpstan-rs's src/), then the two sections as-is (light editing for
consistency only — do not re-derive findings), then a final "## Overall priority list"
that merges and re-ranks both phases' top_gaps into a single ordered list of the ~10
highest-value next steps across rule coverage AND type-system work combined, with a
one-line rationale each.

Rule coverage section:
${ruleSynthesis ? ruleSynthesis.report_markdown : '(this pass failed to return a result)'}

Rule coverage top gaps: ${JSON.stringify(ruleSynthesis ? ruleSynthesis.top_gaps : [])}

Type-system parity section:
${typeSynthesis ? typeSynthesis.report_markdown : '(this pass failed to return a result)'}

Type-system parity top gaps: ${JSON.stringify(typeSynthesis ? typeSynthesis.top_gaps : [])}

Return the full final markdown document as your answer, nothing else.`,
  { label: 'combine-report', phase: 'Combine' }
)

return {
  report_markdown: combined,
  rule_synthesis: ruleSynthesis,
  type_synthesis: typeSynthesis,
  bucket_count: { rules: ruleResults.length, types: typeResults.length },
}
