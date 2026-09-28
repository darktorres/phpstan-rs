# Isolate PCRE2 FFI in one audited crate, make the whole PCRE path PHP-exact, forbid `unsafe_code` elsewhere

## Context

The codebase has 5 `unsafe` blocks:

- **PCRE2 FFI.** `crates/phpstan-rs/src/pcre_validate.rs:147,158,166` make raw `pcre2-sys` calls. The whole module is exempted by `#[allow(unsafe_code)]` at `crates/phpstan-rs/src/lib.rs:23`.
- **Test env mutation.** `crates/sql/src/config/tests.rs:36,43` call `std::env::set_var`/`remove_var`. This is a real data race under parallel `cargo test`.

The chosen approach is option 2: move the FFI into a tiny single-purpose crate with an RAII, documented, safe API, and set `forbid` everywhere else. The safe `pcre2` crate (0.2.11) was rejected because it can't set J, U, n, A, D or `r`, and it exposes errors only through its `Display` text.

The PCRE path was audited against `php_pcre.c` at php-8.5.4, the PCRE2 10.46 source, and the installed `php` 8.5.4. Every finding below is a runtime-correctness issue in exactly the code being rewritten, and each was confirmed by a probe:

1. **Build-machine PCRE2.** `pcre2-sys` links the system `libpcre2-8` when pkg-config finds one. This checkout does: `cargo:rustc-link-lib=pcre2-8`. Results then vary by distro, and the x86_64 release binary needs the `.so` at runtime. The bundled source is PCRE2 10.46, the same version as PHP 8.5.4's.
2. **`u`** is `PCRE2_UTF | PCRE2_UCP` in PHP; we set UTF only.
3. **`r`** is `PCRE2_EXTRA_CASELESS_RESTRICT`, set through a compile context; we ignore it.
4. **The delimiter and modifier front end isn't PHP's.** `rfind(close)` instead of a forward scan with escape skipping and bracket nesting makes `/a/b/i` and `(a)b(c)` valid here, but `Unknown modifier 'b'` in PHP. Also:
   - `' '`, `\n` and `\r` modifiers are accepted by PHP, while we reject `/a/ i`;
   - a `\t` modifier is rejected by PHP, while we trim it;
   - NUL modifier, `Empty regular expression`, the `" a"` delimiter check after whitespace, and `No ending delimiter` vs `No ending matching delimiter` all differ.
5. **Chars instead of bytes.** PHP works on bytes. Our checker skips every non-UTF-8 literal (`PhpString::as_str()`), and the parser works on `char`s.
   - `"/\xff/u"` gives PHP's `UTF-8 error: illegal byte (0xfe or 0xff) at offset 0`, which we never report.
   - `"éaé"` gives `Unknown modifier '\xA9'`, because PHP's delimiter is the single byte `0xC3`; we read it as valid.
6. **Validity only sees literals.** `check_preg_pattern_validity` (`checker/preg_pattern_validity.rs:51`) only reads string literals. The match-shape inference (`expr.rs:818`, `narrowing.rs:647`) already resolves constants via `constant_string_value` (`checker/var_tag_types.rs:64`), which drops non-UTF-8 values through `to_string()`. `Type::constant_strings` (`crates/types/src/lib.rs:639`) also returns `&str` and drops non-UTF-8 members. So `preg_match(self::BAD, …)` is never validated.
7. **Group inference ignores `n`.** `/(a)(?<x>b)/n` infers `array{0: string, 1: 'a', 2: 'b', x: 'b'}`; PHP gives `[0 => 'ab', 'x' => 'b', 1 => 'b']`. That's a wrong type, probed with the debug binary at e1c26a53.
8. **Inline options aren't modelled.** Inline `(?x)`/`(?J)` bail, inline `(?n)` falls through to `mixed`, and inline `(?i)` suppresses literal tracking for the rest of the pattern. PHP probes confirm PCRE2's scoping: an option change applies to the rest of its enclosing group, including that group's later alternatives, and ends when the group closes. `/(a(?n)(b)|(c))(d)/` on `abd` gives `["abd", "ab", "d"]`: both `(b)` and `(c)` are non-capturing, and `(d)` captures again.
9. **`J` bails entirely.** PHP semantics, probed:
   - the named key holds the **last matched** group among the duplicates: `add_named` in `php_pcre.c:961` overwrites on a matched group and only adds on an unmatched one;
   - it sits in the first duplicate's position;
   - unmatched duplicates are `""`, or `null` under `PREG_UNMATCHED_AS_NULL`;
   - trailing unmatched numbered groups are trimmed as usual.

   For example, `/(?<a>x)|(?<a>y)/J` gives `{0:'x', a:'x', 1:'x'}` on `x` and `{0:'y', a:'y', 1:'', 2:'y'}` on `y`.

10. **Group inference ignores that non-`u` PCRE works on bytes.** Without `u`, PCRE's unit is the byte, so `é?` is `\xC3` followed by an optional `\xA9`. The scanner works on `char`s. `/^(é?)$/` infers `array{0: string, 1: ''|'é'}`, but PHP matches `"\xC3"` with group 1 = `"\xC3"`, and doesn't match `""` at all. The real set is `"\xC3"|'é'`, so our `''` is impossible and `"\xC3"` is missing. With `u`, `''|'é'` is correct. Probed with `php` and with the debug binary.

11. **The key layout of `$m` is guessed by a hand-written scanner, while PHP takes it from PCRE2.** PHP builds `$m` from `PCRE2_INFO_CAPTURECOUNT` and the `NAMETABLE` (`make_subpats_table`, `php_pcre.c:521`), then fills it with `populate_subpat_array`. Every numbering bug above (`n`, inline options, byte units) comes from the scanner re-deriving what PCRE2 already knows, and more scanner divergences can't be ruled out without a full audit. With the compiled pattern available, the keys and names should come from PCRE2 itself.

Also checked, no change needed:
- **JIT failures** in 10.46 are only `NOMEMORY`, `JIT_UNSUPPORTED`, `JIT_BADOPTION`, `INTERNAL` or `NULL`, never pattern syntax.
- **Error messages** are at most 108 bytes, so PHP's 128-byte buffer never truncates them.
- **Locale tables** apply only after a runtime non-C `setlocale`, which can't be known statically, so PCRE2's default C tables are right.
- **`NEVER_BACKSLASH_C`** is on the PHP-8.5 branch but not in the 8.5.4 release, so it's not set.

End state:
- The 12 single-call `unsafe` blocks, each with a clippy-enforced `// SAFETY:` comment, are all in `crates/pcre/src/lib.rs`.
- No `allow(unsafe_code)`, and `forbid` in every other crate.
- PCRE2 10.46 is static and asserted at test time.
- The front end, the validation inputs and the group inference follow PHP byte for byte.

## Working copy

Another instance has 47 uncommitted files in the main checkout, including `checker/expr/preg_shapes.rs`. So:
- work in a separate worktree, initialising only `vendor/phpstan-src` via `--reference`;
- commit each step with an explicit pathspec, right after its own scoped verification.

## Commit 1: `sql` injects the env lookup

- `crates/sql/src/config.rs:91`: `resolved_password()` delegates to a new `pub(crate) fn resolved_password_with(&self, env: impl Fn(&str) -> Option<String>)`. The public function passes `|n| std::env::var(n).ok()`, and the error text is unchanged.
- `config/tests.rs`: the tests pass closures. Remove the `#[allow(unsafe_code)]`, both `unsafe` blocks, and the "distinct env var name" comment.
- Verify: `cargo test -p phpstan-rs-sql --lib config::`, then clippy.

## Commit 2: crate `crates/pcre` (package `phpstan-rs-pcre`, lib `pcre`) and static PCRE2 10.46

- **`crates/pcre/Cargo.toml`:**
  - uses the workspace package fields;
  - depends on `pcre2-sys = "=0.2.10"`, moved here from `crates/phpstan-rs/Cargo.toml:26`;
  - has its own lint table, since a workspace `forbid` can't be overridden: `unreachable_pub`, `unsafe_op_in_unsafe_fn`, `rustdoc::broken_intra_doc_links`, `clippy::undocumented_unsafe_blocks` and `clippy::multiple_unsafe_ops_per_block`, all `deny`, plus a one-line reason.
- **Workspace `Cargo.toml`:** add `pcre = { package = "phpstan-rs-pcre", path = "crates/pcre" }`.
- **`.cargo/config.toml`:** add `[env] PCRE2_SYS_STATIC = { value = "1", force = true }`, which `pcre2-sys/build.rs:25,32` reads. This forces the bundled PCRE2 10.46, the same version PHP 8.5.4 uses.
- **`crates/pcre/src/lib.rs`** holds the API and knows nothing about PHP:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompileOptions { pub caseless: bool, pub multiline: bool, pub dotall: bool,
    pub extended: bool, pub anchored: bool, pub dollar_endonly: bool, pub ungreedy: bool,
    pub no_auto_capture: bool, pub dupnames: bool, pub utf: bool, pub ucp: bool,
    pub caseless_restrict: bool }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileError { pub code: i32, pub offset: usize, pub message: String }
/// What PHP builds `$matches` from (`make_subpats_table`, `populate_subpat_array`).
pub struct PatternInfo { pub capture_count: u32, pub names: Vec<(u16, Vec<u8>)> /* name-table order */ }
pub fn compile(pattern: &[u8], options: CompileOptions) -> Result<PatternInfo, CompileError>
pub fn version() -> String
```

- **Implementation:**
  - Private `bits()` and `extra_bits()` map the options to PCRE2 flags; there's no raw-`u32` entry point.
  - RAII `CompileContext(NonNull<…>)` and `Code(NonNull<…>)`, each with a `Drop` that frees it.
  - A context is always created and its extra options always set, as PHP does.
  - The error message goes into a `[u8; 256]` buffer, falling back to `PCRE2 compile error {code}` if PCRE2 can't produce one.
  - `version()` calls `pcre2_config_8(PCRE2_CONFIG_VERSION)`.
  - The crate doc covers thread safety (a fresh context per call) and the C-locale tables.
- **The 12 `unsafe` blocks,** one FFI call each, each with its own SAFETY comment:
  1. context create;
  2. set extra options;
  3. compile;
  4. code free;
  5. context free;
  6. error message;
  7. config version;
  8–11. `pcre2_pattern_info_8` for `CAPTURECOUNT`, `NAMECOUNT`, `NAMEENTRYSIZE` and `NAMETABLE`;
  12. `slice::from_raw_parts` over the name table, whose length is `NAMECOUNT × NAMEENTRYSIZE` and which lives as long as `Code`. The names are copied out before `Code` drops.
- **More tests:** `PatternInfo` is checked for `/(a)(?<x>b)/` (count 2, `[(2, "x")]`), for the same pattern under `no_auto_capture` (count 1, `[(1, "x")]`), for `/(?<a>x)|(?<a>y)/` under `dupnames` (count 2, `[(1, "a"), (2, "a")]`), and for `(?|(a)|(b))` (count 1).
- **Tests** in `crates/pcre/src/tests.rs`:
  - a valid pattern gives `Ok`;
  - an unclosed group gives the exact message and offset;
  - `dupnames` off gives an error, and on gives `Ok`;
  - `no_auto_capture` with `\1` gives an error;
  - `utf` + `ucp` with a multibyte literal passes;
  - `caseless_restrict` with `(?i)ß` passes;
  - an empty pattern passes;
  - `b"a\0b"` passes;
  - invalid UTF-8 gives an error under `utf` and `Ok` without it;
  - `version()` starts with `"10.46 "`.
- **Verify:**
  - `cargo test -p phpstan-rs-pcre`;
  - the pcre2-sys build output shows no `rustc-link-lib=pcre2-8`;
  - `ldd` of the binary shows no `libpcre2`;
  - clippy.

## Commit 3: PHP-exact, byte-level front end; `phpstan-rs` uses `pcre`

- **Manifest:** in `crates/phpstan-rs/Cargo.toml`, `pcre = { workspace = true, optional = true }` and `pcre-validate = ["dep:pcre"]`. The feature comment at lines 45–54 is rewritten. `pcre` stays an FFI-only crate and is linked only with the feature.
- **The PHP-level options type lives in `phpstan-rs`.** `pcre_validate.rs` gets `pub(crate) struct PhpPatternOptions`, with one field per PHP modifier effect: `caseless multiline no_auto_capture dotall extended anchored dollar_endonly caseless_restrict ungreedy utf ucp dupnames`. It is always compiled, so group inference never depends on the FFI crate. `impl From<PhpPatternOptions> for pcre::CompileOptions` sits behind `cfg(feature = "pcre-validate")`, next to the one `compile_check` call.
- **`pcre_validate.rs`** gets `pub(crate) fn parse_php_pattern(literal: &[u8]) -> Result<PhpPattern<'_>, String>`, with `PhpPattern { body: &[u8], options: PhpPatternOptions }`. It follows `php_pcre.c` step by step:
  1. Skip C-`isspace`; if nothing is left, `Empty regular expression`.
  2. An alphanumeric, `\` or NUL delimiter gives the delimiter error.
  3. Bracket pairs come from PHP's `"([{< )]}> )]}>"` table.
  4. Scan forward with `\`-skip, plus a nesting counter for pairs.
  5. With no close, `No ending delimiter '%c' found` or `No ending matching delimiter '%c' found`.
  6. One `match` mirrors PHP's modifier `switch`: `i m n s x A D r S X U u J`, with `u` → `utf` + `ucp` and `r` → `caseless_restrict`. `' ' \n \r` are skipped, NUL gives `NUL byte is not a valid modifier`, and anything else gives `Unknown modifier '%c'`.
- **Byte rendering:** printable ASCII is printed as is and other bytes as `\xNN`, because a diagnostic is UTF-8 text while PHP writes the raw byte.
- **Replacements:** this match replaces `VALID_MODIFIERS` and `modifier_options`. `split_delimiters_and_modifiers`, `closing_delimiter` and `mod ffi` are deleted.
- **`validate_pattern(&[u8])`:** `parse_php_pattern`, then `pcre::compile`, then `Compilation failed: {message} at offset {offset}`. The checker passes `PhpString::as_bytes()`, and `as_string_literal` returns `&PhpString`.
- **`regex_groups::parse_capture_groups(&[u8])`:** `parse_php_pattern(..).ok()?`, then scanning with the typed options. The UTF-8 gate is temporary, until commit 5 moves the scanner to PCRE units. The inference rework is commit 5; in this commit `dupnames` still bails, and `caseless`/`extended` feed `Scanner::new` as today.
- **Cleanup:**
  - `lib.rs:21-23`: remove the comment and the `#[allow(unsafe_code)]`.
  - Rewrite the `pcre_validate.rs` module doc in house style.
  - Update the docs: `README.md:58`; `release.yml:42-46` (PCRE2 is now always built via `cc`, and the cross step at `:94` still applies); `CRATE_SPLIT_PLAN.md:7078`; `ROADMAP.md:1019` where it describes the current state.
  - Refresh `fuzz/Cargo.lock`.
- **Tests** in `pcre_validate/tests.rs`, each asserting PHP's exact text:
  - `/a/b/i`, `(a)b(c)`, `"/a/\ti"`;
  - `"/a/ i\r"` (valid), `"   "`, `" a"`, `" /a/"` (valid), `"/a/\0"`;
  - `/a\/b/` (valid), `(a(b)c)` (valid), `{a}}`;
  - `/a/ui` (valid), `/(?i)ß/r` (valid);
  - `b"/\xff/"` (valid), `b"/\xff/u"`, `"éaé"`, `"/a/é"`.

  The existing tests are kept.
- **Verify:**
  - `cargo test --lib pcre_validate regex_groups preg`;
  - `cargo test -p phpstan-rs-pcre`;
  - `cargo build --no-default-features`, then confirm that `cargo tree -e normal --no-default-features` lists neither `pcre` nor `pcre2-sys`;
  - clippy.

## Commit 4: validity check and shape inference read constant string types as bytes

- **`crates/types`:**
  - Add `Type::constant_php_strings(&self) -> Vec<&PhpString>`, with the same union and intersection rules as `constant_strings` (`lib.rs:639`), which becomes `constant_php_strings().filter_map(as_str)`.
  - Add `Checker::constant_string_bytes(e, locals) -> Option<PhpString>` next to `constant_string_value` (`var_tag_types.rs:64`), using `string_literal_value()` (already `&PhpString`) instead of `to_string()`.
- **`check_preg_pattern_validity`:**
  - It gets `locals` from its call site at `expr.rs:715`, where `locals` is in scope.
  - It validates every member of `infer_expr(pattern).constant_php_strings()`.
  - For `PlainOrArrayValues`, it validates the value types of each `constant_arrays()` shape; for `ArrayKeys`, the string keys of the shape.
  - Diagnostics go on the argument span and are deduplicated by message. The literal-only `as_string_literal` path is subsumed.
- **Shape inference:** `expr.rs:818` and `narrowing.rs:647` switch to `constant_string_bytes`, so non-UTF-8 constant patterns reach `parse_capture_groups(&[u8])`.
- **Tests:**
  - a class constant holding an invalid pattern is reported;
  - a `'/a/' . 'b/i'` concatenation is reported;
  - a union of one valid and one invalid constant reports the invalid one;
  - a constant array of patterns is checked;
  - a non-UTF-8 constant is reported.
- **Verify:** the checker's preg tests, then clippy.

## Commit 5: group inference with typed options and PCRE2-scoped inline options

- **The scanner works in PCRE's own units.**
  - Without `utf`, `Scanner.units` is the body's bytes; with `utf`, it is the body's code points. Under `utf`, the body is guaranteed valid UTF-8, because PHP fails the compile otherwise and the validation step reports it.
  - This removes commit 3's `from_utf8` gate.
  - Literal values become `PhpString` byte strings instead of `String`, since a group can capture `"\xC3"`. Their consumers in `preg_shapes`/`expr.rs`/`narrowing.rs` build `Type::StringLiteral(PhpString)` directly; the type already holds bytes.
  - Tests:
    - `/^(é?)$/` → literals `"\xC3"`, `"é"`;
    - `/^(é?)$/u` → `""`, `"é"`;
    - a non-UTF-8 body `"/(\xff)/"` gives one group, literal `"\xff"`;
    - checker `assertType` shapes for all three, matching `php`.
- **`Scanner`** (`regex_groups.rs:404`) also replaces `free_spacing` and `literal_globally_suppressed` with an option-scope stack. The current scope holds `caseless`, `extended`, `extended_more` and `no_auto_capture`.
  - The initial scope comes from `PhpPattern.options`.
  - Entering any group pushes a copy of the current scope; its close pops it.
  - A bare `(?flags)` mutates the current scope, so it applies to the rest of the group, including later alternatives.
  - `(?flags:…)` pushes a scope just for that body.
  - Flags: `i m n s x xx U J`, with `-` unsetting and `(?^)` resetting `imnsx` off, per PCRE2.
- **Mode effects:**
  - Literal tracking is suppressed exactly where `caseless` is on, not globally. Literal tracking is the scanner's record of the exact strings a group can match.
  - `skip_free_spacing` reads the current `extended`; with `extended_more`, whitespace inside character classes is skipped too.
  - A bare `(` numbers a group only when `no_auto_capture` is off.
- **Bail changes:** the inline `"imsxuJU-"` bail at `regex_groups.rs:1231` is removed, since `u` isn't a valid inline flag in PCRE2 and that stays a bail. The coarse-suppression doc comments on the struct fields are replaced.
- **Tests** (group lists and literal sets), each confirmed against `php`:
  - `/(a)(?<x>b)/n` → `[x→1]`;
  - `/(a(?n)(b)|(c))(d)/` → groups 1 and 2, where 2 is `(d)`;
  - `/(?i)a(?-i)(b)/` → group 1, literal `b`;
  - `/(?i:a)(b)/` → literal `b`;
  - `/(?x) ( a ) /` → one group, literal `a`;
  - `/(?^)…/` resets;
  - the checker `assertType` for `/(a)(?<x>b)/n` has PHP's keys `0`, `x`, `1`.

## Commit 6: `J` duplicate-name modelling

- `regex_groups` records duplicate names; each `RegexCaptureGroup` keeps its name, and a name maps to its ordered group list.
- `preg_shapes` (`checker/expr/preg_shapes.rs`) builds the named key once, at the first duplicate's position:
  - its type is the union of each duplicate's matched type, because whichever matched last wins;
  - it can be `""`, or `null` under `PREG_UNMATCHED_AS_NULL`, only when every duplicate can go unmatched;
  - numbered keys and trailing trimming are unchanged.
- This file is also dirty in the other instance's working tree, so the change is made on this worktree's base and merged afterwards.
- **Tests,** each shape checked against `php` output:
  - `/(?<a>x)|(?<a>y)/J`;
  - `/(?<a>x)?(?<a>y)/J`;
  - `/(?<a>x)(?<a>y)?/J` with and without `PREG_UNMATCHED_AS_NULL`.

## Commit 6b: `$m` key layout from PCRE2, with the scanner cross-checked

- `parse_php_pattern` plus `pcre::compile` yields `PatternInfo` when the feature is on. `preg_shapes` builds the key set and order the way `populate_subpat_array` does: for `i` in `0..=capture_count`, the named key goes first when `names` has `i` (with add/update semantics for duplicates), then key `i`.
- The scanner supplies only per-group value facts: optionality, literal sets, and alternation data. Its group count and name→number map must equal `PatternInfo`.
  - When they agree, the full precision is used.
  - When they disagree, the scanner's facts are dropped and each group's value becomes `string` (plus `''`/`null` for unmatched). The keys stay exact instead of the result falling back to `mixed`.
- Without `pcre-validate`, the scanner-only layout is used, as today.
- **Tests:**
  - a pattern the scanner can't handle, such as a conditional group, still gets exact keys from `PatternInfo`;
  - a forced disagreement uses the fallback.
- **Differential fuzz target** `fuzz_targets/groups_vs_pcre2.rs`: arbitrary pattern bytes plus modifiers go through the scanner, whose count and names must equal `PatternInfo` whenever both succeed. Each mismatch found is a scanner bug, fixed in its own commit with a `php`-confirmed test.

## Commit 6c: match oracle for the inferred value types (fuzz-only)

After 6b, the keys are exact by construction. The per-group value facts (optionality, literal sets, unmatched `''`/`null`) are still the scanner's claims, and nothing checks them against the engine.

- **`pcre` crate, behind a non-default `oracle` feature** that only `fuzz/` enables, so the production binary never contains it:
  - `pub fn match_captures(pattern: &[u8], options: CompileOptions, subject: &[u8]) -> Result<Option<Vec<Option<(usize, usize)>>>, CompileError>`;
  - RAII `MatchData`;
  - 6 more single-call `unsafe` blocks: match-data create, `pcre2_match_8`, ovector pointer, ovector count, `slice::from_raw_parts` over the ovector, match-data free;
  - the default match context, since PHP's backtrack and recursion limits only affect runtime failures, not the shape of `$m`.
- **`phpstan-rs` helper** `pub(crate) fn php_matches_array(info, ovector, flags) -> PhpArrayValue`, available under `cfg(fuzzing)` or `cfg(test)`. It ports `populate_subpat_array` and `add_named` line for line:
  - trailing unmatched groups are trimmed (`count`);
  - with `PREG_UNMATCHED_AS_NULL`, the tail is filled with `null`;
  - with `PREG_OFFSET_CAPTURE`, each value is a `[string, int]` pair;
  - duplicate names use add/update.
- **Fuzz target** `fuzz_targets/preg_shape_oracle.rs`, whose input is (pattern bytes, modifiers, subject bytes, flags):
  1. Infer the `preg_match` `$m` type through the same code path the checker uses.
  2. Run `match_captures`.
  3. On a match, build the concrete array as `Type::array_shape` of string, int and null constants (`constructors.rs:112`), and assert that `Relation::is_super_type_of(inferred, concrete)` (`crates/types/src/relation/super_type.rs:21`) is `Yes`, not merely `!= No`. The relation uses the checker's own `Checker::relation()` (`checker.rs:1288`), since only scalars and arrays are involved.
  4. On no match, assert that the inferred "no match" shape holds.

  Every counterexample is a real unsound type, fixed in its own commit with a `php`-confirmed regression test. Seeds come from all the probe patterns, with subjects that match them.
- **Unit tests** for `php_matches_array` against recorded `php` output: the `J`, `n`, `(?|…)`, trailing-unmatched and `PREG_UNMATCHED_AS_NULL` cases above.

## Commit 7: fuzz the FFI boundary and the byte parser

Miri can't run through FFI, so fuzzing is the dynamic check.

- `fuzz/fuzz_targets/pcre_compile.rs`: arbitrary bytes plus an `arbitrary`-derived options wrapper, fed to `compile_check`. There must be no crash, and on `Err`, `offset <= len` and the message is non-empty.
- `fuzz/fuzz_targets/php_pattern.rs`: arbitrary bytes fed to `validate_pattern` and `parse_capture_groups`, with no panic.
- `fuzz/Cargo.toml` gets the `pcre` path dependency and the two `[[bin]]` entries; `seed_corpus.sh` gets seeds from every probe above.
- Verify: `cargo +nightly fuzz run <target> -- -max_total_time=300` for each target, one at a time.

## Commit 8: forbid

- Root `Cargo.toml:34`: `unsafe_code = "forbid"`. Every other member already inherits the workspace lints.
- `fuzz/Cargo.toml`: add `[lints.rust] unsafe_code = "forbid"`. If the `fuzz_target!` expansion (`#[no_mangle]`) trips it, drop the line and report it instead of adding an `allow`.
- Verify:
  - `cargo clippy --workspace --all-targets --all-features`;
  - `git grep -nE 'unsafe *(\{|fn|impl)|allow\(unsafe_code\)' -- ':!vendor' ':!crates/pcre'` is empty;
  - `crates/pcre/src/lib.rs` has exactly 12 `unsafe {` outside `#[cfg(feature = "oracle")]` and 18 in total;
  - `cargo tree -e features -p phpstan-rs` doesn't show `oracle`.

## End-to-end verification

- **Order:** `cargo fmt` before every build, test or clippy run.
- **PHP oracle:**
  - A scratchpad probe dir holds every pattern above, plus about 20 PCRE2 compile errors, used both as literals and as class constants.
  - `php` produces the expected warnings and `$m` dumps; `check-project --no-composer <dir>` produces ours.
  - Each `Regex pattern is invalid: X` must equal `preg_match(): X`, PHP-valid patterns must produce no diagnostic, and every `assertType` shape must match PHP's `$m` keys and values.
- **Full suite:** `cargo test --lib` (debug).
- **Piwigo corpus:**
  - save the run to `.corpus-runs/<scope>/<hash>.txt`;
  - `LC_ALL=C sort` both it and the previous run, then diff;
  - explain each `regexp.pattern` or preg-shape change by a Context item, confirmed with `php` on that pattern.
