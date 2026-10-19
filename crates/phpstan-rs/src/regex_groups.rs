//! Parses a PCRE pattern literal's own syntactic structure - never
//! executes it against a subject string - to enumerate its capturing
//! groups for `preg_match`/`preg_match_all`'s by-ref `$matches` shape
//! inference (`checker/expr.rs`). A hand-rolled recursive-descent scanner
//! over PCRE's own group/alternation/quantifier grammar, deliberately
//! narrower than a real regex engine: it never claims to know whether a
//! pattern matches anything, only what its capturing groups *are*.
//! Bails (`None`) on anything outside its own understood subset - the
//! only correct response to that is to leave `$matches` untouched, never
//! to guess.

/// One capturing group found in a pattern, in left-to-right numbering
/// order (matching real PCRE's own numbering, which counts every
/// capturing group - named or not - by the position of its opening
/// paren).
#[derive(Clone)]
pub(crate) struct RegexCaptureGroup {
    pub number: usize,
    pub name: Option<String>,
    /// Whether the group can go unmatched: its own `own_optional`, or its
    /// enclosing capturing group's `optional`.
    pub optional: bool,
    /// Whether the group can go unmatched while its enclosing capturing
    /// group matches - a zero-min quantifier or a multi-branch
    /// alternation between the two.
    pub own_optional: bool,
    /// The index in the scanned groups of the nearest enclosing capturing
    /// group, `None` for a top-level group.
    pub parent: Option<usize>,
    /// `Some(values)` only when this group's own body is provably a
    /// finite, enumerable set of exact strings (e.g. `(cat|dog)` ->
    /// `Some(["cat", "dog"])`); `None` for anything not statically
    /// enumerable by this scanner's own (deliberately narrower than real
    /// PHPStan's) understanding - the safe default, never a guess.
    pub literal_values: Option<Vec<String>>,
    /// `true` iff this group isn't nested inside another *capturing*
    /// group - a non-capturing wrapper doesn't break top-level-ness.
    /// Mirrors real PHPStan's own `isTopLevel()`; feeds the "exactly one
    /// optional top-level group" / "exactly one top-level alternation"
    /// tagged-union dispatch (`checker::expr::build_matched_shape`).
    pub top_level: bool,
    /// `true` only when this group is directly the atom a zero-min
    /// quantifier applies to (`(a)?`) - not propagated through an
    /// intervening non-capturing wrapper (`(?:(a))?` does not set this
    /// on group 1), mirroring real PHPStan's `inOptionalQuantification`:
    /// captured as a constructor parameter at the exact moment a group
    /// object is built, then unconditionally reset to `false` before
    /// recursing into children unless the current node is itself a
    /// quantification.
    pub in_optional_quantification: bool,
    /// Which real (2+ branch) alternation, and which branch, this
    /// group's own *innermost enclosing* alternation is - `None` if
    /// none.
    pub alternation: Option<(u32, u32)>,
    /// Set only by `checker::expr::build_matched_shape`'s own
    /// alternation-variant builder (never by this scanner) - `true`
    /// overrides this group's own value entirely to the empty string for
    /// one specific tagged-union variant, for a group known to sit
    /// before the variant's own matched branch - it's always present
    /// there, backfilled, never entirely absent. Just a `bool`, not a
    /// richer "forced value" type: the one
    /// dispatch that sets this (the "exactly one top-level alternation"
    /// case) is already scoped to `!unmatched_as_null` callers only, so
    /// the only forced value ever needed here is the empty string - a
    /// `PREG_UNMATCHED_AS_NULL` variant would need `null` instead, but
    /// nothing in this project's own call graph can reach that
    /// combination yet, and adding a case for it now would be dead code
    /// with no caller to exercise it.
    pub forced_empty: bool,
    /// A tri-state fallback for a group whose body isn't a finite,
    /// exactly-enumerable set (`literal_values: None`) but is still
    /// statically provable to be shaped a certain way - `NumericString`
    /// when every possible match consists solely of ASCII digit
    /// characters and can never be empty (e.g. `(\d+)`, `([0-9]+)`),
    /// `NonEmptyString` when it can never be empty but isn't provably
    /// digits-only (e.g. `([a-z]+)`), `Unknown` when neither is provable
    /// (degrading to plain `string`). Computed purely from the group's
    /// own body shape (length/character-set), independent of
    /// `literal_values` and of how many times the group itself might be
    /// repeated from outside - only consulted by
    /// `checker::expr::preg_shapes::group_leaf_type` when
    /// `literal_values` is `None` (an exact enumeration is always more
    /// precise, never overridden by this fallback).
    pub fallback: RegexGroupFallback,
}

/// See `RegexCaptureGroup::fallback`'s own doc comment.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) enum RegexGroupFallback {
    #[default]
    Unknown,
    NumericString,
    NonEmptyString,
}

/// PCRE's limit on nested parentheses: a pattern nested deeper doesn't
/// compile, so `preg_match` returns `false` and there are no groups to
/// read.
const MAX_GROUP_DEPTH: usize = 250;

/// Caps every literal-value cross-product step (concatenation, bounded-
/// quantifier repetition, alternation union) - smaller than real
/// PHPStan's own `LITERALS_LIMIT` (100), consistent with this project's
/// own tighter caps elsewhere.
/// Exceeding it degrades *that one span* to `None`, never the whole
/// parse.
const LITERAL_ENUM_LIMIT: usize = 32;

/// Caps a bounded quantifier's own `max` before attempting to repeat a
/// literal set that many times - independent of `LITERAL_ENUM_LIMIT`,
/// which only bounds the *result* size. A single-literal atom repeated
/// `{0,1000000}` times would keep the result at exactly one string the
/// whole way, never tripping the result-size cap, while still looping a
/// million times to get there - a real performance concern this bounds
/// directly.
const MAX_QUANTIFIER_REPEAT: u64 = 1000;

/// The full result of parsing a pattern's own structure: its capturing
/// groups, plus two pieces of per-alternation metadata
/// `RegexCaptureGroup` itself has no room for (they describe a whole
/// alternation, not any one group) - both feed
/// `checker::expr::build_matched_shape`'s own "exactly one top-level
/// alternation" tagged-union dispatch.
pub(crate) struct ParsedPattern {
    pub groups: Vec<RegexCaptureGroup>,
    /// `(alternation_id, branch_count)` - the total number of `|`-
    /// separated branches a given alternation has, *regardless* of
    /// whether every branch contains a capturing group. Needed to detect
    /// a branch with zero groups (real PCRE fact: matching it still
    /// produces `$matches` with only field `"0"`).
    pub alternation_branch_counts: Vec<(u32, usize)>,
    /// Which alternation ids sit inside a zero-min-quantified atom
    /// somewhere (`(?:(a)|(b))?`) - real PHPStan's own
    /// `RegexCapturingGroup::inOptionalAlternation()`, confirmed to do
    /// real work (not a no-op) via `/(?:(a)|(b))?/` still unioning in a
    /// bare `{0: subject}` possibility even though every branch already
    /// has a group.
    pub alternation_wrapped_in_optional_quantifier: Vec<u32>,
    /// The fewest bytes a match of the whole pattern has.
    pub min_len: u64,
    /// Whether a match of the whole pattern holds only ASCII digits.
    pub digits_only: bool,
    /// Whether a match is the whole subject, but for a final newline when
    /// `final_newline` is set: the pattern starts with `^` or `\A`, ends
    /// with `$`, `\z` or `\Z`, has no top-level `|`, and no `m` modifier.
    pub fully_anchored: bool,
    /// Whether the end anchor also matches before a final newline (`$`
    /// without the `D` modifier, or `\Z`), so the subject may hold one
    /// after the match.
    pub final_newline: bool,
}

/// Parses a PHP PCRE pattern literal's own delimiters/modifiers and
/// structure to enumerate its capturing groups, in left-to-right
/// numbering order. Returns `None` for anything outside this function's
/// own understood subset.
pub(crate) fn parse_capture_groups(pattern_literal: &str) -> Option<ParsedPattern> {
    let (body, modifiers) = split_delimiters_and_modifiers(pattern_literal)?;
    // `J` (duplicate named groups, excluded by real PHPStan itself as
    // "too complicated") changes the grammar in a way this scanner
    // doesn't attempt - bail before structural scanning even starts. `i`
    // (case-insensitive) doesn't change the grammar, only whether
    // literal-value tracking is safe - handled below, not a full bail.
    // `x` (free-spacing mode) only changes tokenization (unescaped
    // whitespace and `#`-comments outside a character class are skipped
    // between tokens), which `Scanner::skip_free_spacing` handles
    // inline, so it needs no bail either.
    if modifiers.contains('J') {
        return None;
    }
    let mut scanner = Scanner::new(
        body,
        modifiers.contains('i'),
        modifiers.contains('x'),
        modifiers.contains('u') || body.contains("(*UCP)"),
    );
    let whole = scanner.scan_alternation()?;
    let anchor = (!modifiers.contains('m'))
        .then(|| whole_subject_anchor(body, modifiers.contains('D')))
        .flatten();
    if scanner.remaining_input() {
        // Leftover, unconsumed text - a stray unmatched `)`, or a
        // construct the atom dispatch below didn't recognize as ending
        // cleanly. Never guess past this point.
        return None;
    }
    // A `(?|...)` branch-reset construct (see
    // `Scanner::scan_branch_reset_alternation`) legitimately reuses the
    // same public group `.number` across its own branches, matching
    // real PCRE fact - e.g. `(?|(\d+)|([a-z]+))` has exactly one group,
    // number 1, shared by both branches, never two. Resolving which
    // branch's value should show up under that one shared key -
    // correctly excluding a sibling branch's same-numbered entry rather
    // than wrongly treating it as a distinct field to backfill with `''`
    // - is something `checker::expr::build_matched_shape`'s tagged-union
    // dispatch only does right in one specific shape (the whole
    // top-level pattern being exactly that one alternation);
    // `preg_match_all`'s flat, unconditional shape-builders never go
    // through that dispatch at all. Rather than risk emitting a shape
    // with two same-numbered fields for every other placement, this
    // bails outright on any duplicate number, always safe (`$matches`
    // is simply left untouched), at the cost of not typing `$matches`
    // for a branch-reset construct whose branches do each capture
    // something. A branch-reset with at most one group total across all
    // its branches (no reuse possible), or one combined with otherwise-
    // ordinary structure, still resolves correctly, and a group declared
    // after one still numbers correctly too (`(?|(a)(b)|(c))(d)`
    // renumbers `(d)` to 3, matching real PCRE's "resume from the
    // highest number reached in any branch" rule).
    let mut seen_numbers: Vec<usize> = Vec::with_capacity(scanner.groups.len());
    for g in &scanner.groups {
        if seen_numbers.contains(&g.number) {
            return None;
        }
        seen_numbers.push(g.number);
    }
    Some(ParsedPattern {
        groups: scanner.groups,
        alternation_branch_counts: scanner.alternation_branch_counts,
        alternation_wrapped_in_optional_quantifier: scanner
            .alternation_wrapped_in_optional_quantifier,
        min_len: whole.min_len,
        digits_only: whole.digits_only,
        fully_anchored: anchor.is_some(),
        final_newline: anchor.unwrap_or(true),
    })
}

/// When `body` starts with `^` or `\A`, ends with an unescaped `$` or a
/// `\z` / `\Z` escape, and has no `|` outside a group or character class:
/// whether its end also matches before a final newline (`$` without the
/// `D` modifier, or `\Z`).
fn whole_subject_anchor(body: &str, dollar_end_only: bool) -> Option<bool> {
    if !(body.starts_with('^') || body.starts_with("\\A")) {
        return None;
    }
    let bytes = body.as_bytes();
    let (mut depth, mut in_class) = (0i32, false);
    let mut last: Option<(u8, bool)> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' {
            last = bytes.get(i + 1).map(|&c| (c, true));
            i += 2;
            continue;
        }
        match b {
            b'[' if !in_class => in_class = true,
            b']' if in_class => in_class = false,
            b'(' if !in_class => depth += 1,
            b')' if !in_class => depth -= 1,
            b'|' if !in_class && depth == 0 => return None,
            _ => {}
        }
        last = Some((b, in_class));
        i += 1;
    }
    match last? {
        (b'$', false) => Some(!dollar_end_only),
        (b'z', true) => Some(false),
        (b'Z', true) => Some(true),
        _ => None,
    }
}

/// Splits a PHP PCRE pattern literal into its raw body and modifier
/// letters, stripping the delimiter pair. The first non-whitespace
/// character is the delimiter, unless it's one of PCRE's four bracket
/// pairs, in which case the matching closing bracket is the real closer;
/// the body runs up to the *last* occurrence of the closing delimiter (so
/// a bracket-delimited pattern's own body may itself legitimately contain
/// the same bracket type nested inside, e.g. `(foo(bar))i`), and
/// everything after that (short of trailing whitespace) is the modifier
/// string.
///
/// Real PHP's `ext/pcre` skips leading whitespace before reading the
/// delimiter (and tolerates trailing whitespace after the modifiers) -
/// `preg_split(" /abc/x", ...)` (a literal leading space) compiles and
/// runs with no error. This isn't a contrived case: PHP's flexible
/// heredoc/nowdoc indentation stripping (7.3+) only removes whitespace
/// common with the closing marker's own indentation, so a multi-line
/// `/x`-flagged regex written with nested, readable indentation
/// (matching the pattern's own `(?(DEFINE)...)` nesting depth, deeper
/// than the closing marker) legitimately keeps literal leading spaces
/// before its real delimiter. Treating the literal first byte as the
/// delimiter regardless would produce a false "Unknown modifier"
/// `regexp.pattern` diagnostic on that exact, real, working pattern.
pub(crate) fn split_delimiters_and_modifiers(pattern: &str) -> Option<(&str, &str)> {
    fn is_php_pattern_whitespace(c: char) -> bool {
        matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{0B}' | '\u{0C}')
    }
    let pattern = pattern.trim_start_matches(is_php_pattern_whitespace);
    let open = pattern.chars().next()?;
    let close = match open {
        '(' => ')',
        '{' => '}',
        '[' => ']',
        '<' => '>',
        c => c,
    };
    let rest = &pattern[open.len_utf8()..];
    let last_close = rest.rfind(close)?;
    let body = &rest[..last_close];
    let modifiers =
        rest[last_close + close.len_utf8()..].trim_end_matches(is_php_pattern_whitespace);
    Some((body, modifiers))
}

/// One atom's own contribution when scanned: its capturing-group handles
/// (unchanged from before literal tracking existed), its own literal
/// value set if fully enumerable, whether this atom was specifically a
/// `\Q...\E` block (needed so a quantifier directly following one
/// doesn't wrongly cross-product the whole block - see `scan_branch`),
/// and - only when this atom is *directly and only* one bare or named
/// capturing group with nothing wrapping it - that group's own index
/// (needed to scope `in_optional_quantification` correctly: `None` for
/// every other form, even one whose own interior contains capturing
/// groups - a non-capturing wrapper, a lookaround, `\Q...\E`, a literal,
/// a character class).
struct AtomResult {
    handles: Vec<usize>,
    literal_values: Option<Vec<String>>,
    is_quoted_block: bool,
    direct_group: Option<usize>,
    /// The *minimum* number of characters this atom could ever
    /// contribute - `0` for anything that could legitimately match
    /// nothing (a zero-width assertion, an atom this scanner doesn't
    /// know the shape of at all, ...), matching real PCRE fact for fact
    /// (never a guess in the "could be non-empty" direction). Combined
    /// across a branch's own atoms by summing (concatenation) and across
    /// an alternation's own branches by taking the minimum (only one
    /// branch runs) - see `Scanner::combine_branches`/`Scanner::
    /// scan_branch`. Feeds `RegexGroupFallback` alongside `digits_only`.
    min_len: u64,
    /// `true` iff this atom, *whenever it contributes any character at
    /// all*, only ever contributes ASCII digit characters - independent
    /// of `min_len` (an atom that might contribute nothing still has a
    /// definite answer for "if it does, is it a digit"), and
    /// deliberately independent of `Scanner::literal_globally_suppressed`
    /// (the `i` modifier folds a digit's *case*, which digits don't
    /// have, so it never turns a digit into a non-digit or vice versa -
    /// unlike exact literal-value enumeration, which genuinely does need
    /// to bail under `/i`). The vacuous default (`true`) for a
    /// zero-width construct that never contributes any character at all.
    digits_only: bool,
}

impl AtomResult {
    /// No groups, not literal, and no known length/character-set shape -
    /// the maximally conservative default for anything this scanner
    /// can't or won't say anything more precise about (never wrongly
    /// claims non-emptiness or a digit-only charset).
    fn opaque() -> Self {
        AtomResult {
            handles: Vec::new(),
            literal_values: None,
            is_quoted_block: false,
            direct_group: None,
            min_len: 0,
            digits_only: false,
        }
    }

    /// No groups, contributes exactly `value` (a single known string -
    /// often one character, sometimes the empty string for a zero-width
    /// construct). `min_len`/`digits_only` are derived directly from
    /// `value` itself, so a zero-width construct built this way (`^`,
    /// `$`, an empty `\Q\E`) correctly gets the vacuous-true/zero-length
    /// identity for free.
    fn literal(value: String) -> Self {
        let min_len = value.chars().count() as u64;
        let digits_only = value.chars().all(|c| c.is_ascii_digit());
        AtomResult {
            handles: Vec::new(),
            literal_values: Some(vec![value]),
            is_quoted_block: false,
            direct_group: None,
            min_len,
            digits_only,
        }
    }
}

/// Cross-product of two literal-value sets end to end (concatenation) -
/// `None` if either side isn't literal. The empty-string singleton
/// (`Some(vec![""])`) is concatenation's own identity value, used as
/// `scan_branch`'s starting accumulator so a branch with zero atoms
/// correctly contributes `""` rather than needing special-casing.
fn concat_literals(a: Option<Vec<String>>, b: Option<Vec<String>>) -> Option<Vec<String>> {
    let a = a?;
    let b = b?;
    let mut result = Vec::with_capacity(a.len() * b.len());
    for x in &a {
        for y in &b {
            result.push(format!("{x}{y}"));
            if result.len() > LITERAL_ENUM_LIMIT {
                return None;
            }
        }
    }
    Some(result)
}

/// Cross-product of `literals` repeated `min..=max` times, deduplicated -
/// mirrors real PHPStan's own `repeatLiterals`. `None` past either cap.
fn repeat_literals(literals: &[String], min: u64, max: u64) -> Option<Vec<String>> {
    if max > MAX_QUANTIFIER_REPEAT {
        return None;
    }
    let mut collected: Vec<String> = Vec::new();
    if min == 0 {
        collected.push(String::new());
    }
    let mut current: Vec<String> = vec![String::new()];
    for k in 1..=max {
        let mut next = Vec::with_capacity(current.len() * literals.len());
        for prefix in &current {
            for lit in literals {
                next.push(format!("{prefix}{lit}"));
                if next.len() > LITERAL_ENUM_LIMIT {
                    return None;
                }
            }
        }
        current = next;
        if k >= min {
            for v in &current {
                if !collected.contains(v) {
                    collected.push(v.clone());
                }
            }
            if collected.len() > LITERAL_ENUM_LIMIT {
                return None;
            }
        }
    }
    Some(collected)
}

/// Classifies a single-character backslash escape's own known
/// `(min_len, digits_only)` shape, independent of `Scanner::
/// literal_globally_suppressed` (an escape class's character set is
/// fixed, never affected by the `i` modifier - see `AtomResult::
/// digits_only`'s own doc comment). `None` for anything this scanner
/// doesn't specifically know the shape of (a digit backreference `\1`,
/// `\K`, `\p{...}`, ...) - callers fall back to the fully-unknown
/// `(0, false)` default rather than guess.
fn escape_class_info(c: char) -> Option<(u64, bool)> {
    match c {
        // Exactly one digit character, always.
        'd' => Some((1, true)),
        // Exactly one character, but not knowably a digit.
        'D' | 'w' | 'W' | 's' | 'S' | 'h' | 'H' | 'v' | 'V' | 'N' => Some((1, false)),
        // Zero-width assertions - never contribute a character, so
        // vacuously both "always empty" and "digits only" (there's
        // nothing to violate either claim).
        'b' | 'B' | 'A' | 'Z' | 'z' | 'G' | 'K' => Some((0, true)),
        _ => None,
    }
}

struct Scanner {
    chars: Vec<char>,
    pos: usize,
    depth: usize,
    groups: Vec<RegexCaptureGroup>,
    next_number: usize,
    /// `true` once literal tracking is known to be unsafe for the rest
    /// of the pattern - initialized from the pattern's own top-level `i`
    /// modifier, and one-way (never cleared) if an inline `(?i...)`
    /// modifier group is encountered anywhere. A deliberately simple
    /// choice over precisely scoping an inline modifier group's effect
    /// to just its own body/branch remainder: PCRE scopes `(?i:...)` to
    /// its own body and bare `(?i)` to the rest of the enclosing branch,
    /// but a coarser, always-safe "suppress everything from here on" is
    /// preferred over the complexity of exact scoping for such a rarely
    /// used construct.
    literal_globally_suppressed: bool,
    /// The indices of the capturing groups whose bodies are being scanned,
    /// innermost last.
    enclosing_groups: Vec<usize>,
    next_alternation_id: u32,
    alternation_branch_counts: Vec<(u32, usize)>,
    alternation_wrapped_in_optional_quantifier: Vec<u32>,
    /// The pattern's own top-level `x` modifier (free-spacing mode) -
    /// unescaped whitespace and `#`-to-end-of-line comments outside a
    /// character class are skipped between tokens when this is set (see
    /// `skip_free_spacing`). Fixed for the whole scan, unlike
    /// `literal_globally_suppressed`: consistent with this scanner's
    /// existing, documented choice for inline `i`, an inline
    /// `(?x)`/`(?x:...)` modifier group still bails the whole parse
    /// (`scan_group_inner`'s own `"imsxuJU-"` branch) rather than
    /// dynamically toggling this mid-scan.
    free_spacing: bool,
    /// Whether `\d` and `[:digit:]` match every Unicode decimal digit, as
    /// PHP's `u` modifier (which turns on PCRE2's UCP) or a `(*UCP)` verb
    /// makes them, so they aren't ASCII digits.
    unicode_digits: bool,
}

impl Scanner {
    fn new(body: &str, case_insensitive: bool, free_spacing: bool, unicode_digits: bool) -> Self {
        Scanner {
            chars: body.chars().collect(),
            pos: 0,
            depth: 0,
            groups: Vec::new(),
            next_number: 1,
            literal_globally_suppressed: case_insensitive,
            enclosing_groups: Vec::new(),
            next_alternation_id: 0,
            alternation_branch_counts: Vec::new(),
            alternation_wrapped_in_optional_quantifier: Vec::new(),
            free_spacing,
            unicode_digits,
        }
    }

    /// Free-spacing mode's own tokenization effect: skips unescaped
    /// ASCII whitespace and `#`-to-end-of-line comments, repeatedly,
    /// until neither is at the current position - a no-op when `x`
    /// wasn't in the pattern's own modifiers. Only ever called from
    /// `scan_branch`'s own atom loop (between atoms, and between an atom
    /// and its own quantifier), never from inside a character class
    /// (real PCRE fact: whitespace stays literal there even under `x`)
    /// or `\Q...\E` (already consumed verbatim as one atom before this
    /// could ever run again).
    fn skip_free_spacing(&mut self) {
        if !self.free_spacing {
            return;
        }
        loop {
            match self.peek() {
                Some(c) if c.is_ascii_whitespace() => self.pos += 1,
                Some('#') => {
                    while !matches!(self.peek(), None | Some('\n')) {
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
    }

    fn remaining_input(&self) -> bool {
        self.pos < self.chars.len()
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// `alternation := branch ('|' branch)*`. Returns the group-handle
    /// indices (into `self.groups`) discovered across every branch,
    /// retroactively marking all of them `optional` when there were 2+
    /// branches - a branch that didn't run means its groups never
    /// captured. Only ever adds `optional`, never removes it, so this
    /// composes soundly with a quantifier's own optionality below. Its
    /// own literal contribution is the union of every branch's own set,
    /// `None` if any branch isn't literal.
    fn scan_alternation(&mut self) -> Option<AtomResult> {
        let mut branches: Vec<AtomResult> = vec![self.scan_branch()?];
        while self.eat('|') {
            branches.push(self.scan_branch()?);
        }
        Some(self.combine_branches(branches))
    }

    /// `(?|branch ('|' branch)*)` - PCRE's own "branch reset" group:
    /// each branch's own capturing groups restart numbering from the
    /// same point (the group number that would have come next just
    /// before this construct), so `(?|(a)|(b))` has exactly one
    /// capturing group, number N, shared by both branches - never two.
    /// Numbering resumes, after the whole construct, from one past the
    /// highest number reached in any one branch:
    /// `(?|(a)(b)|(c))(d)` numbers `(d)` as group 3, not 2, since branch
    /// 1 alone reached as high as 2. Otherwise behaves exactly like
    /// `scan_alternation` (multi-branch optionality, alternation-id
    /// bookkeeping, literal-value union, `min_len`/`digits_only`
    /// combination) via the same shared `combine_branches` - this only
    /// ever intercepts `next_number` between branches. The construct
    /// itself is *not* a capturing group (mirrors `finish_non_capturing_
    /// group`, not `finish_capturing_group`) - only what's inside it is.
    /// A resulting duplicate `.number` across branches (the common case:
    /// every branch captures something) is resolved, or safely bailed
    /// on, by `parse_capture_groups`'s own dedicated check after the
    /// whole pattern has been scanned - seeing only one branch at a time
    /// here, this has no way to know whether a number it's about to
    /// reuse collides with a sibling branch's own.
    fn scan_branch_reset_alternation(&mut self) -> Option<AtomResult> {
        let start_number = self.next_number;
        let mut max_number = start_number;
        let mut branches: Vec<AtomResult> = Vec::new();
        loop {
            self.next_number = start_number;
            branches.push(self.scan_branch()?);
            max_number = max_number.max(self.next_number);
            if !self.eat('|') {
                break;
            }
        }
        self.next_number = max_number;
        Some(self.combine_branches(branches))
    }

    /// Shared by `scan_alternation` and `scan_branch_reset_alternation`:
    /// given every branch's own already-scanned `AtomResult`, does every
    /// bit of "this is a real, 2+-branch alternation" bookkeeping that's
    /// independent of how each branch's own numbering was produced -
    /// retroactive `optional`/`alternation` marking, the alternation's
    /// own id/branch-count record, literal-value union, and the
    /// `min_len` (minimum across branches - only one ever runs) /
    /// `digits_only` (AND across branches - whichever runs must still
    /// satisfy it) combination.
    fn combine_branches(&mut self, branches: Vec<AtomResult>) -> AtomResult {
        let multi_branch = branches.len() > 1;
        let alt_id = self.next_alternation_id;
        if multi_branch {
            self.next_alternation_id += 1;
            self.alternation_branch_counts
                .push((alt_id, branches.len()));
        }
        let mut handles = Vec::new();
        let mut literal_values: Option<Vec<String>> = Some(Vec::new());
        let mut min_len: Option<u64> = None;
        let mut digits_only = true;
        for (branch_index, branch) in branches.into_iter().enumerate() {
            if multi_branch {
                for &idx in &branch.handles {
                    self.groups[idx].optional = true;
                    if !self.groups[idx]
                        .parent
                        .is_some_and(|p| branch.handles.contains(&p))
                    {
                        self.groups[idx].own_optional = true;
                    }
                    // Only claim a group not already claimed by a *more
                    // inner* real alternation - an inner `scan_alternation`
                    // call's own retroactive marking always finishes
                    // before this outer fixup runs, so `is_none()` here
                    // correctly means "no more-inner alternation owns
                    // this."
                    if self.groups[idx].alternation.is_none() {
                        self.groups[idx].alternation = Some((alt_id, branch_index as u32));
                    }
                }
            }
            min_len = Some(match min_len {
                None => branch.min_len,
                Some(m) => m.min(branch.min_len),
            });
            digits_only &= branch.digits_only;
            handles.extend(branch.handles);
            literal_values = match (literal_values, branch.literal_values) {
                (Some(mut acc), Some(values)) => {
                    for v in values {
                        if !acc.contains(&v) {
                            acc.push(v);
                        }
                    }
                    if acc.len() > LITERAL_ENUM_LIMIT {
                        None
                    } else {
                        Some(acc)
                    }
                }
                _ => None,
            };
        }
        AtomResult {
            handles,
            literal_values,
            is_quoted_block: false,
            direct_group: None,
            min_len: min_len.unwrap_or(0),
            digits_only,
        }
    }

    /// `branch := atom*`, stopping at `|`, `)`, or end of input. Its own
    /// literal contribution is the cross-product of every atom's own
    /// set, in order - `Some(vec![""])` (concatenation's identity) for a
    /// genuinely empty branch, matching real PCRE's own "an empty
    /// alternative always captures the empty string" semantics.
    fn scan_branch(&mut self) -> Option<AtomResult> {
        let mut handles = Vec::new();
        let mut literal_values: Option<Vec<String>> = Some(vec![String::new()]);
        let mut min_len: u64 = 0;
        let mut digits_only = true;
        loop {
            self.skip_free_spacing();
            match self.peek() {
                None | Some('|') | Some(')') => break,
                _ => {}
            }
            let atom = self.scan_atom()?;
            self.skip_free_spacing();
            let quantifier = self.maybe_consume_quantifier();
            if let Some((min, _)) = quantifier
                && min == 0
            {
                for &idx in &atom.handles {
                    self.groups[idx].optional = true;
                    if !self.groups[idx]
                        .parent
                        .is_some_and(|p| atom.handles.contains(&p))
                    {
                        self.groups[idx].own_optional = true;
                    }
                    // Broad, deliberately using `atom.handles` (not
                    // `atom.direct_group`) - real PHPStan's own
                    // `inOptionalAlternation()`, confirmed to do real
                    // work via `/(?:(a)|(b))?/` still unioning in a bare
                    // `{0: subject}` possibility even though every
                    // branch already has a group. This is a *different*
                    // signal than `in_optional_quantification` below,
                    // scoped to the whole alternation, not one group.
                    if let Some((alt_id, _)) = self.groups[idx].alternation
                        && !self
                            .alternation_wrapped_in_optional_quantifier
                            .contains(&alt_id)
                    {
                        self.alternation_wrapped_in_optional_quantifier.push(alt_id);
                    }
                }
                // Narrow, using `atom.direct_group` - see
                // `RegexCaptureGroup::in_optional_quantification`'s own
                // doc comment for why this must not use `atom.handles`.
                if let Some(idx) = atom.direct_group {
                    self.groups[idx].in_optional_quantification = true;
                }
            }
            // "Repeated more than once" disables constant typing for
            // *that specific group* (mirrors real PHPStan's own
            // `allowConstantTypes`'s `repeatedMoreThanOnce`) - only the
            // group's own already-computed `literal_values` is cleared;
            // `optional` is an orthogonal concern, untouched here.
            if let Some((_, max)) = quantifier
                && max.is_none_or(|m| m > 1)
            {
                for &idx in &atom.handles {
                    self.groups[idx].literal_values = None;
                }
            }
            let contributed = match quantifier {
                None => atom.literal_values,
                // PCRE binds a quantifier following `\Q...\E` only to
                // the block's last character, never the whole block -
                // this scanner treats the block as one indivisible atom,
                // so cross-producting it as a whole would be wrong; bail
                // rather than guess.
                Some(_) if atom.is_quoted_block => None,
                Some((_, None)) => None, // unbounded - min/max both required
                Some((min, Some(max))) => atom
                    .literal_values
                    .and_then(|lits| repeat_literals(&lits, min, max)),
            };
            literal_values = concat_literals(literal_values, contributed);
            // Same "binds to the quoted block's last character only" fact
            // as the literal-value cross-product just above applies here
            // too - staying with the safe, fully-unknown default
            // (`0`/`false`) for that one atom rather than guessing at
            // the more intricate length this would actually produce.
            let (contributed_min_len, contributed_digits_only) = match quantifier {
                None => (atom.min_len, atom.digits_only),
                Some(_) if atom.is_quoted_block => (0, false),
                Some((min, _)) => (atom.min_len.saturating_mul(min), atom.digits_only),
            };
            min_len = min_len.saturating_add(contributed_min_len);
            digits_only &= contributed_digits_only;
            handles.extend(atom.handles);
        }
        Some(AtomResult {
            handles,
            literal_values,
            is_quoted_block: false,
            direct_group: None,
            min_len,
            digits_only,
        })
    }

    /// If the input at the current position forms a valid quantifier
    /// (`?`, `*`, `+`, `{n}`, `{n,}`, `{n,m}` - digits only, correctly
    /// shaped - optionally suffixed by a lazy `?` or possessive `+`),
    /// consumes it whole and returns its `(min, max)` range (`max: None`
    /// meaning unbounded). Otherwise consumes nothing and returns `None`;
    /// a malformed `{...}` (real PCRE treats it as literal text, e.g.
    /// `a{b`) is left untouched for the next atom scan, not treated as
    /// an attempted-and-failed quantifier.
    fn maybe_consume_quantifier(&mut self) -> Option<(u64, Option<u64>)> {
        let (min, max) = match self.peek() {
            Some('?') => {
                self.pos += 1;
                (0, Some(1))
            }
            Some('*') => {
                self.pos += 1;
                (0, None)
            }
            Some('+') => {
                self.pos += 1;
                (1, None)
            }
            Some('{') => {
                let start = self.pos;
                self.pos += 1;
                let n = self.scan_digits();
                let range = if self.eat(',') {
                    let m = self.scan_digits();
                    n.map(|n| (n, m))
                } else {
                    n.map(|n| (n, Some(n)))
                };
                let Some(range) = range else {
                    self.pos = start;
                    return None;
                };
                if !self.eat('}') {
                    self.pos = start;
                    return None;
                }
                range
            }
            _ => return None,
        };
        // A trailing lazy `?` or possessive `+` doesn't affect the
        // range, but must still be consumed - left alone, it would be
        // misread as the start of a brand new atom by the next loop
        // iteration, corrupting everything after it.
        if matches!(self.peek(), Some('?') | Some('+')) {
            self.pos += 1;
        }
        Some((min, max))
    }

    /// Digits only, no sign - `None` if there are none at all (an empty
    /// `{}`/`{,5}` is malformed, not zero).
    fn scan_digits(&mut self) -> Option<u64> {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == start {
            return None;
        }
        self.chars[start..self.pos]
            .iter()
            .collect::<String>()
            .parse()
            .ok()
    }

    /// `atom := group | class | escape | literal`.
    fn scan_atom(&mut self) -> Option<AtomResult> {
        match self.peek()? {
            '(' => {
                self.pos += 1;
                self.scan_group()
            }
            '[' => {
                let digits_only = self.scan_character_class()?;
                Some(AtomResult {
                    handles: Vec::new(),
                    literal_values: None,
                    is_quoted_block: false,
                    direct_group: None,
                    // A well-formed `[...]` (a leading `]` is always a
                    // literal member, never an empty class - real PCRE
                    // fact) always matches exactly one character when it
                    // matches at all.
                    min_len: 1,
                    digits_only,
                })
            }
            '\\' => self.scan_escape_atom(),
            // Zero-width assertions - never consume a character,
            // unconditionally, so contributing the empty string is a
            // known fact, not a guess, regardless of the `i` modifier
            // (there's no case to fold in an empty string).
            '^' | '$' => {
                self.pos += 1;
                Some(AtomResult::literal(String::new()))
            }
            // The wildcard - consumes exactly one character, just not a
            // knowably-literal or knowably-digit one.
            '.' => {
                self.pos += 1;
                Some(AtomResult {
                    handles: Vec::new(),
                    literal_values: None,
                    is_quoted_block: false,
                    direct_group: None,
                    min_len: 1,
                    digits_only: false,
                })
            }
            c => {
                self.pos += 1;
                if self.literal_globally_suppressed {
                    // Case folding never turns a digit into a non-digit
                    // or vice versa (digits have no case), and never
                    // changes how many characters this contributes - so
                    // `min_len`/`digits_only` stay knowable even though
                    // the *exact* literal value doesn't (see
                    // `AtomResult::digits_only`'s own doc comment).
                    Some(AtomResult {
                        handles: Vec::new(),
                        literal_values: None,
                        is_quoted_block: false,
                        direct_group: None,
                        min_len: 1,
                        digits_only: c.is_ascii_digit(),
                    })
                } else {
                    Some(AtomResult::literal(c.to_string()))
                }
            }
        }
    }

    /// Consumes a character class's interior, given `self.pos` already
    /// past the opening `[`. Priority order matters: an escape is always
    /// checked first (so `[\]]`'s escaped `]` is never misread as the
    /// real closer), then a candidate POSIX class, then a leading-`]`
    /// literal, then an ordinary member. Character classes never contain
    /// real groups, and are never literal-tracked (not worth enumerating
    /// even a trivial `[a]`). Returns whether the
    /// class is provably digit-only (`class_body_is_digits_only`,
    /// computed before consuming, since that check is a read-only
    /// lookahead over the same span this then consumes for real) -
    /// `None` means "malformed/unterminated."
    fn scan_character_class(&mut self) -> Option<bool> {
        let digits_only = self.class_body_is_digits_only(self.pos + 1);
        self.pos += 1; // the opening `[`
        let mut first = true;
        if self.peek() == Some('^') {
            self.pos += 1;
        }
        loop {
            match self.peek() {
                None => return None, // unterminated class
                Some('\\') => {
                    self.scan_escape()?;
                    first = false;
                }
                Some('[') if self.peek_at(1) == Some(':') => {
                    if !self.try_consume_posix_class() {
                        self.pos += 1; // just the literal `[`
                    }
                    first = false;
                }
                Some(']') if first => {
                    self.pos += 1;
                    first = false;
                }
                Some(']') => {
                    self.pos += 1;
                    return Some(digits_only);
                }
                Some(_) => {
                    self.pos += 1;
                    first = false;
                }
            }
        }
    }

    /// A conservative, read-only lookahead classifying a `[...]` class
    /// body (given `i` at the char immediately after the opening `[` -
    /// *not* yet past a leading `^`, unlike `scan_character_class`'s own
    /// `self.pos` at that point) as provably digit-only - never advances
    /// `self.pos`, mirrors `scan_character_class`'s own member-boundary
    /// rules (leading `]` is literal, `\` escapes, POSIX `[:name:]`)
    /// closely enough to stay in sync, but independently, since this
    /// only ever needs to answer "would this class ever produce anything
    /// but an ASCII digit?", not actually consume the pattern. Any
    /// construct it doesn't specifically recognize as digit-safe (an
    /// ordinary non-digit literal, a non-`\d`/`[:digit:]` escape/POSIX
    /// class, a negated class, an unterminated class) makes it return
    /// `false` - the always-safe default, never a guess. A digit range
    /// (`0-9`, or any subrange like `3-7`) is recognized explicitly;
    /// `digits_only` only ever asserts the characters this class *can*
    /// produce are digits, never that it covers all ten, so a subrange
    /// is exactly as digit-safe as the full range.
    fn class_body_is_digits_only(&self, i: usize) -> bool {
        if self.chars.get(i) == Some(&'^') {
            return false; // negated - never provably digit-only
        }
        let mut i = i;
        let mut first = true;
        loop {
            match self.chars.get(i) {
                None => return false, // unterminated
                Some('\\') => {
                    if self.unicode_digits || self.chars.get(i + 1) != Some(&'d') {
                        return false;
                    }
                    i += 2;
                    first = false;
                }
                Some('[') if self.chars.get(i + 1) == Some(&':') => {
                    let name_start = i + 2;
                    let mut j = name_start;
                    loop {
                        match self.chars.get(j) {
                            None => return false,
                            Some(']') => return false, // outer closer first - malformed
                            Some(':') if self.chars.get(j + 1) == Some(&']') => break,
                            Some(_) => j += 1,
                        }
                    }
                    let name: String = self.chars[name_start..j].iter().collect();
                    if self.unicode_digits || name != "digit" {
                        return false;
                    }
                    i = j + 2;
                    first = false;
                }
                Some(']') if first => return false, // literal `]` member - not a digit
                Some(']') => return true, // real closer - every member so far was digit-safe
                Some(&c) if c.is_ascii_digit() => {
                    // A possible range `c-c2`, both endpoints digits.
                    if self.chars.get(i + 1) == Some(&'-')
                        && let Some(&c2) = self.chars.get(i + 2)
                        && c2.is_ascii_digit()
                        && c <= c2
                    {
                        i += 3;
                    } else {
                        i += 1;
                    }
                    first = false;
                }
                Some(_) => return false, // any other literal member
            }
        }
    }

    /// Called with `self.pos` at a `[` immediately followed by `:` (a
    /// candidate POSIX class, `[:alpha:]`). Looks ahead, without
    /// consuming, for a closing `:]` before any unescaped `]` or the
    /// input ending. If found, consumes the whole `[:...:]` span and
    /// returns `true`, so its own interior `]` never prematurely closes
    /// the outer class; if not found (a malformed, single-colon
    /// `[:foo]`, which real PCRE treats as the literal characters `:`,
    /// `f`, `o`, `o`), leaves `self.pos` untouched and returns `false`.
    fn try_consume_posix_class(&mut self) -> bool {
        let mut i = self.pos + 2; // past "[:"
        loop {
            match self.chars.get(i) {
                None => return false,
                Some(']') => return false, // hit the outer class's own closer first
                Some(':') if self.chars.get(i + 1) == Some(&']') => {
                    self.pos = i + 2;
                    return true;
                }
                Some(_) => i += 1,
            }
        }
    }

    /// Consumes a backslash escape - one following character, except
    /// `\Q` which switches to literal mode until `\E` or end-of-pattern.
    /// Deliberately semantics-agnostic and never literal-tracked: this
    /// is only ever called from inside a character class (which never
    /// tracks literals at all) or as a shared helper for consuming a
    /// `\Q...\E` span's text (see `consume_quoted_block`).
    fn scan_escape(&mut self) -> Option<()> {
        self.pos += 1; // the backslash
        match self.peek() {
            Some('Q') => {
                self.pos += 1;
                self.consume_quoted_block();
                Some(())
            }
            Some(_) => {
                self.pos += 1;
                Some(())
            }
            None => None, // a trailing lone backslash - malformed
        }
    }

    /// The top-level, literal-tracking-aware counterpart of
    /// `scan_escape` - used only from `scan_atom`, never from inside a
    /// character class. `\Q...\E` contributes its own verbatim text
    /// (and is flagged as a quoted block - see `scan_branch`'s own
    /// quantifier handling); `\` + one ASCII-punctuation character
    /// contributes that one character (PCRE's own rule: escaping
    /// non-alphanumeric punctuation is always just that literal
    /// character); a handful of specific multi-character escapes whose
    /// own length/character-set *is* safely knowable even without
    /// knowing their exact matched text (`escape_class_info`) contribute
    /// that; anything else consumes normally but contributes nothing
    /// literal *and* nothing about length/charset either - never
    /// special-casing a real escape meaning this scanner isn't
    /// specifically sure of (a digit backreference, `\p{...}`, ...),
    /// since getting one wrong would produce a wrong literal/fallback
    /// type, not just a missed one.
    fn scan_escape_atom(&mut self) -> Option<AtomResult> {
        self.pos += 1; // the backslash
        match self.peek() {
            Some('Q') => {
                self.pos += 1;
                let text = self.consume_quoted_block();
                // `min_len`/`digits_only` are derived from `text` itself
                // regardless of `literal_globally_suppressed` - see
                // `AtomResult::digits_only`'s own doc comment for why
                // that's always safe, unlike exact literal enumeration.
                let min_len = text.chars().count() as u64;
                let digits_only = text.chars().all(|c| c.is_ascii_digit());
                let literal_values = if self.literal_globally_suppressed {
                    None
                } else {
                    Some(vec![text])
                };
                Some(AtomResult {
                    handles: Vec::new(),
                    literal_values,
                    is_quoted_block: true,
                    direct_group: None,
                    min_len,
                    digits_only,
                })
            }
            Some(c) if c.is_ascii_punctuation() => {
                self.pos += 1;
                // Punctuation is never an ASCII digit, so `digits_only`
                // is `false` regardless of case-folding either way; only
                // the *exact* literal value needs the `/i` bail.
                if self.literal_globally_suppressed {
                    Some(AtomResult {
                        handles: Vec::new(),
                        literal_values: None,
                        is_quoted_block: false,
                        direct_group: None,
                        min_len: 1,
                        digits_only: false,
                    })
                } else {
                    Some(AtomResult::literal(c.to_string()))
                }
            }
            Some(c) => {
                self.pos += 1;
                let (min_len, digits_only) = escape_class_info(c).unwrap_or((0, false));
                let digits_only = digits_only && !(c == 'd' && self.unicode_digits);
                Some(AtomResult {
                    handles: Vec::new(),
                    literal_values: None,
                    is_quoted_block: false,
                    direct_group: None,
                    min_len,
                    digits_only,
                })
            }
            None => None, // a trailing lone backslash - malformed
        }
    }

    /// Consumes `\Q...\E`, given `self.pos` already past the `\Q`.
    /// Returns the verbatim text between them - group-counting call
    /// sites (`scan_escape`) discard it; literal-tracking ones
    /// (`scan_escape_atom`) use it directly.
    fn consume_quoted_block(&mut self) -> String {
        let start = self.pos;
        loop {
            match self.peek() {
                None => break,
                Some('\\') if self.peek_at(1) == Some('E') => break,
                Some(_) => self.pos += 1,
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        if self.peek() == Some('\\') && self.peek_at(1) == Some('E') {
            self.pos += 2;
        }
        text
    }

    /// `group := '(' ... ')'`, given `self.pos` already past the opening
    /// `(`. Peeks for a following `?` to distinguish a real numbered
    /// capturing group from every other `(?...)` shape.
    fn scan_group(&mut self) -> Option<AtomResult> {
        self.depth += 1;
        let result = if self.depth > MAX_GROUP_DEPTH {
            None
        } else {
            stack::guard(|| self.scan_group_inner())
        };
        self.depth -= 1;
        result
    }

    fn scan_group_inner(&mut self) -> Option<AtomResult> {
        if self.peek() != Some('?') {
            return self.finish_capturing_group(None);
        }
        self.pos += 1; // the `?`
        match self.peek() {
            Some(':') => {
                self.pos += 1;
                self.finish_non_capturing_group()
            }
            Some('|') => {
                self.pos += 1;
                self.finish_branch_reset_group()
            }
            Some('=') | Some('!') => {
                self.pos += 1;
                self.finish_lookaround()
            }
            Some('<') if matches!(self.peek_at(1), Some('=') | Some('!')) => {
                self.pos += 2;
                self.finish_lookaround()
            }
            Some('<') => {
                self.pos += 1;
                let name = self.scan_name_until('>')?;
                self.finish_capturing_group(Some(name))
            }
            Some('\'') => {
                self.pos += 1;
                let name = self.scan_name_until('\'')?;
                self.finish_capturing_group(Some(name))
            }
            Some('P') => match self.peek_at(1) {
                Some('<') => {
                    self.pos += 2;
                    let name = self.scan_name_until('>')?;
                    self.finish_capturing_group(Some(name))
                }
                Some('=') | Some('>') => {
                    // Named backreference / subroutine call - atomic, no
                    // handle, no recursion, not literal (a backreference
                    // depends on a runtime capture, never statically
                    // known).
                    self.consume_atomic_to_close_paren()?;
                    Some(AtomResult::opaque())
                }
                _ => None, // unrecognized `(?P...` shape
            },
            Some('#') => {
                // A comment - consume verbatim to the first `)` (PCRE
                // comments cannot contain a literal `)` at all, even
                // escaped, so no escape handling is needed here).
                // Zero-width, so contributes `""` like `^`/`$` do -
                // `AtomResult::opaque()`'s own `None` literal_values
                // would otherwise wrongly break literal enumeration for
                // a branch merely containing a comment (`(a(?#x)b)`
                // really is always `"ab"`).
                loop {
                    match self.advance() {
                        None => return None,
                        Some(')') => return Some(AtomResult::literal(String::new())),
                        Some(_) => {}
                    }
                }
            }
            Some('(') => None, // a conditional group - bail the whole parse
            Some(c) if c.is_ascii_digit() || matches!(c, '+' | '-' | '&' | 'R') => {
                // Recursion/subroutine-call forms: (?R), (?1), (?+1),
                // (?-1), (?&name) - atomic, no handle, no recursion (per
                // PCRE these never contain further parens), not literal
                // (dynamic, like a backreference).
                self.consume_atomic_to_close_paren()?;
                Some(AtomResult::opaque())
            }
            Some(c) if "imsxuJU-".contains(c) => {
                let mods_start = self.pos;
                while matches!(self.peek(), Some(m) if "imsxuJU-".contains(m)) {
                    self.pos += 1;
                }
                let mods: String = self.chars[mods_start..self.pos].iter().collect();
                if mods.contains('x') || mods.contains('J') {
                    // Continuing under this scanner's normal rules when
                    // the pattern is actually meant to switch modes
                    // partway through would silently produce a wrong
                    // group count, not just an unsupported one.
                    return None;
                }
                // An inline `i` modifier (scoped `(?i:...)` or bare,
                // "rest of branch" `(?i)`) makes literal tracking unsafe
                // from here on - PCRE scopes this precisely to the
                // modifier's own body/branch remainder, but this scanner
                // suppresses literal tracking for the rest of the whole
                // pattern instead of building exact scoping for such a
                // rarely used construct - coarser, but never wrong.
                if mods.contains('i') {
                    self.literal_globally_suppressed = true;
                }
                if self.eat(':') {
                    self.finish_non_capturing_group()
                } else if self.eat(')') {
                    // A bare `(?i)` - zero-width, same reasoning as the
                    // `(?#...)` comment case just above.
                    Some(AtomResult::literal(String::new()))
                } else {
                    None
                }
            }
            _ => None, // a malformed or unrecognized `(?X...)` shape
        }
    }

    fn finish_capturing_group(&mut self, name: Option<String>) -> Option<AtomResult> {
        let number = self.next_number;
        self.next_number += 1;
        let idx = self.groups.len();
        let parent = self.enclosing_groups.last().copied();
        self.groups.push(RegexCaptureGroup {
            number,
            name,
            optional: false,
            own_optional: false,
            parent,
            literal_values: None,
            top_level: parent.is_none(),
            in_optional_quantification: false,
            alternation: None,
            forced_empty: false,
            fallback: RegexGroupFallback::Unknown,
        });
        self.enclosing_groups.push(idx);
        let body = self.scan_alternation();
        self.enclosing_groups.pop();
        let body = body?;
        if !self.eat(')') {
            return None;
        }
        self.groups[idx].literal_values = body.literal_values.clone();
        // Computed unconditionally (not only when `literal_values` ends
        // up `None`) - `group_leaf_type` is the one that decides literal
        // enumeration always wins when both are available, so there's
        // no need to duplicate that priority check here too.
        self.groups[idx].fallback = if body.min_len == 0 {
            RegexGroupFallback::Unknown
        } else if body.digits_only {
            RegexGroupFallback::NumericString
        } else {
            RegexGroupFallback::NonEmptyString
        };
        let mut handles = vec![idx];
        handles.extend(body.handles);
        Some(AtomResult {
            handles,
            literal_values: body.literal_values,
            is_quoted_block: false,
            direct_group: Some(idx),
            min_len: body.min_len,
            digits_only: body.digits_only,
        })
    }

    fn finish_non_capturing_group(&mut self) -> Option<AtomResult> {
        let body = self.scan_alternation()?;
        if !self.eat(')') {
            return None;
        }
        Some(body)
    }

    /// `(?|...)` - see `scan_branch_reset_alternation`'s own doc comment
    /// for the numbering semantics; the construct itself is never a
    /// capturing group (mirrors `finish_non_capturing_group`).
    fn finish_branch_reset_group(&mut self) -> Option<AtomResult> {
        let body = self.scan_branch_reset_alternation()?;
        if !self.eat(')') {
            return None;
        }
        Some(body)
    }

    /// A lookahead/lookbehind (`(?=...)`, `(?!...)`, `(?<=...)`,
    /// `(?<!...)`) - zero-width, so its own contribution to whatever
    /// contains it is always the empty string, never its interior's
    /// literal computation: `(a(?=b))` genuinely captures only `"a"`,
    /// so treating a lookaround like an ordinary non-capturing group
    /// would silently compute the wrong literal `"ab"`. The interior is
    /// still fully scanned as normal for group-counting and for any
    /// nested capturing group's `literal_values` (`(?=(a))`'s group 1
    /// really is `'a'` if it participates) - only this construct's own
    /// contribution is forced.
    fn finish_lookaround(&mut self) -> Option<AtomResult> {
        let body = self.scan_alternation()?;
        if !self.eat(')') {
            return None;
        }
        Some(AtomResult {
            handles: body.handles,
            literal_values: Some(vec![String::new()]),
            is_quoted_block: false,
            direct_group: None,
            // Same forcing as `literal_values` just above, same reason:
            // zero-width, so this construct's own contribution is always
            // the empty string, never its interior's real shape.
            min_len: 0,
            digits_only: true,
        })
    }

    /// Scans a group name (anything but `terminator`) up to and
    /// including `terminator`.
    fn scan_name_until(&mut self, terminator: char) -> Option<String> {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c != terminator) {
            self.pos += 1;
        }
        if self.peek() != Some(terminator) {
            return None; // unterminated name
        }
        let name: String = self.chars[start..self.pos].iter().collect();
        self.pos += 1; // the terminator
        Some(name)
    }

    /// Consumes characters verbatim up to and including the first `)` -
    /// used for the atomic backreference/subroutine-call/recursion forms,
    /// which per PCRE never contain further parens.
    fn consume_atomic_to_close_paren(&mut self) -> Option<()> {
        loop {
            match self.advance() {
                None => return None,
                Some(')') => return Some(()),
                Some(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests;
