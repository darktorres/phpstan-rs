# Stage C review: findings

Findings from reviewing the Stage C work done so far: C1 and C2 (one
commit), C3's four commits, the rename to reflection names, and the two
fixes that followed on the reflection types. Judged on runtime correctness
and design, not parity with PHPStan. Each task names where the problem is,
what is wrong, and what done looks like.

The earlier Stage C reviews sampled the work and were folded into the
crate-split plan as the C1–C4 follow-ups; those tasks stay in the plan and
are listed below for reference. The full review reads every commit's diff
in order; its coverage is tracked at the end, and its findings are the F
tasks.

Legend: `[x]` done (commit given), `[>]` moved to the crate-split plan
(where given), `[ ]` open.

## Validation

- [ ] **V1. The fixture suite and corpus** run once, at the HEAD the Stage B
  review's V1 and V2 use, which includes Stage C; a failure or delta traced
  to a commit below is judged here.

## Earlier reviews (in the crate-split plan)

- [>] **C1 follow-ups:** nested validation and the full `ignore_errors`
  shape; missing settings; one settings assembly for the CLI and the LSP;
  activation reaching library typing; paths and matching; entries validated
  at load; config discovery; one walk per file; key names; PHPStan configs
  (ROADMAP Phase 183); tests and docs.
- [>] **C2 follow-ups:** the settings gate; predefined constants over a
  version range and both integer sizes; `ConstantResolver` in `reflection`;
  initializers evaluated as `InitializerExprTypeResolver`; one folded
  constant key; `generalize`; `RuleLevelHelper` owning the rule.
- [>] **C3 follow-ups:** C3a's `property.unused` delta; `is_phpstan_code` as
  a provenance query; attribute arguments evaluated to types (C8); the rest
  of the data model in C4.

## Full review: findings

Grouped by severity, most severe first. Each names the commit that
introduced it or, for a module read in its final state, the module;
"probed" means reproduced with an `assertType` or diagnostic probe
through `check-project`.

## Full review: coverage

Every Stage C commit's full diff (`crates`, `xtask`, `gates`), read in
order. A commit is ticked once its whole diff has been read and its
findings added above.

- [ ] `4c18d661` refactor: config owner sections, settings leave the registry
- [ ] `dcc67ce5` refactor: every declaration records its provenance (C3a)
- [ ] `bfce0e9f` refactor: the registry holds no function bodies (C3b)
- [ ] `49e6d9b1` refactor: attributes carry resolved names and arguments (C3c)
- [ ] `41874e77` refactor: reflection types are read through accessors only (C3d)
- [ ] `ca63ef85` refactor: the reflection types take their reflection names
- [ ] `a11f7012` fix: composer hygiene credits a package with what its files declare
- [ ] `69b97cb1` fix: an anonymous class is never reported unused

### Final state, module by module

Each module Stage C created or reshaped, read whole in its final state
after the commit pass.

- [ ] config: `lib`, `tests`
- [ ] app: the config schema and `--set` (`app.rs`, `app/check_project.rs`,
  `app/tests.rs`)
- [ ] settings owners: every `Section` implementor and `CheckSettings`
- [ ] checker: `constant_resolver`, `predefined_constants`, the
  `RuleLevelHelper`
- [ ] registry: the reflection types (`class_info`, functions, parameters,
  properties, constants, attributes, provenance)
