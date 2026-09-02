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
pub(crate) struct RegexCaptureGroup {
    pub number: usize,
    pub name: Option<String>,
    pub optional: bool,
    /// `Some(values)` only when this group's own body is provably a
    /// finite, enumerable set of exact strings (e.g. `(cat|dog)` ->
    /// `Some(["cat", "dog"])`); `None` for anything not statically
    /// enumerable by this scanner's own (deliberately narrower than real
    /// PHPStan's) understanding - the safe default, never a guess.
    pub literal_values: Option<Vec<String>>,
}

/// A deeply-nested pattern is a real stack-overflow risk via this
/// scanner's own recursive descent, not just a slowness one - the same
/// class of bug Phase 20 found and fixed for the main expression parser
/// (`src/parser.rs`'s `MAX_EXPR_DEPTH`). This scanner already returns
/// `Option` everywhere, so exceeding this just bails like any other
/// unsupported construct, no panic needed.
const MAX_GROUP_DEPTH: usize = 200;

/// Caps every literal-value cross-product step (concatenation, bounded-
/// quantifier repetition, alternation union) - smaller than real
/// PHPStan's own `LITERALS_LIMIT` (100), consistent with this project's
/// own tighter caps elsewhere (`checker::expr::MAX_UNION_MEMBERS` is 12).
/// Exceeding it degrades *that one span* to `None`, never the whole
/// parse.
const LITERAL_ENUM_LIMIT: usize = 32;

/// Caps a bounded quantifier's own `max` before attempting to repeat a
/// literal set that many times - independent of `LITERAL_ENUM_LIMIT`,
/// which only bounds the *result* size. A single-literal atom repeated
/// `{0,1000000}` times would keep the result at exactly one string the
/// whole way, never tripping the result-size cap, while still looping a
/// million times to get there - a real performance concern this bounds
/// directly, the same "don't trust an untrusted-shaped input without an
/// explicit termination argument" precedent Phase 20 already established.
const MAX_QUANTIFIER_REPEAT: u64 = 1000;

/// Parses a PHP PCRE pattern literal's own delimiters/modifiers and
/// structure to enumerate its capturing groups, in left-to-right
/// numbering order. Returns `None` for anything outside this function's
/// own understood subset.
pub(crate) fn parse_capture_groups(pattern_literal: &str) -> Option<Vec<RegexCaptureGroup>> {
    let (body, modifiers) = split_delimiters_and_modifiers(pattern_literal)?;
    // `x` (free-spacing mode) and `J` (duplicate named groups, excluded
    // by real PHPStan itself as "too complicated") both change the
    // grammar in ways this scanner doesn't attempt - bail before
    // structural scanning even starts. `i` (case-insensitive) doesn't
    // change the grammar, only whether literal-value tracking is safe -
    // handled below, not a full bail.
    if modifiers.contains('x') || modifiers.contains('J') {
        return None;
    }
    let mut scanner = Scanner::new(body, modifiers.contains('i'));
    scanner.scan_alternation()?;
    if scanner.remaining_input() {
        // Leftover, unconsumed text - a stray unmatched `)`, or a
        // construct the atom dispatch below didn't recognize as ending
        // cleanly. Never guess past this point.
        return None;
    }
    Some(scanner.groups)
}

/// Splits a PHP PCRE pattern literal into its raw body and modifier
/// letters, stripping the delimiter pair. The first character is the
/// delimiter, unless it's one of PCRE's four bracket pairs, in which case
/// the matching closing bracket is the real closer; the body runs up to
/// the *last* occurrence of the closing delimiter (so a bracket-delimited
/// pattern's own body may itself legitimately contain the same bracket
/// type nested inside, e.g. `(foo(bar))i`), and everything after that is
/// the modifier string.
fn split_delimiters_and_modifiers(pattern: &str) -> Option<(&str, &str)> {
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
    let modifiers = &rest[last_close + close.len_utf8()..];
    Some((body, modifiers))
}

/// One atom's own contribution when scanned: its capturing-group handles
/// (unchanged from before literal tracking existed), its own literal
/// value set if fully enumerable, and whether this atom was specifically
/// a `\Q...\E` block (needed so a quantifier directly following one
/// doesn't wrongly cross-product the whole block - see `scan_branch`).
struct AtomResult {
    handles: Vec<usize>,
    literal_values: Option<Vec<String>>,
    is_quoted_block: bool,
}

impl AtomResult {
    /// No groups, not literal - the default for anything this scanner
    /// can't or won't enumerate.
    fn opaque() -> Self {
        AtomResult {
            handles: Vec::new(),
            literal_values: None,
            is_quoted_block: false,
        }
    }

    /// No groups, contributes exactly `value` (a single known string -
    /// often one character, sometimes the empty string for a zero-width
    /// construct).
    fn literal(value: String) -> Self {
        AtomResult {
            handles: Vec::new(),
            literal_values: Some(vec![value]),
            is_quoted_block: false,
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

struct Scanner {
    chars: Vec<char>,
    pos: usize,
    depth: usize,
    groups: Vec<RegexCaptureGroup>,
    next_number: usize,
    /// `true` once literal tracking is known to be unsafe for the rest
    /// of the pattern - initialized from the pattern's own top-level `i`
    /// modifier, and one-way (never cleared) if an inline `(?i...)`
    /// modifier group is encountered anywhere. A real, deliberately
    /// simple choice over precisely scoping an inline modifier group's
    /// own effect to just its own body/branch remainder: real PCRE scopes
    /// `(?i:...)` to its own body and bare `(?i)` to the rest of the
    /// enclosing branch, but no corpus evidence anywhere in this
    /// project's own history uses either form, so a coarser, always-safe
    /// "suppress everything from here on" is preferred over the
    /// complexity of exact scoping for a construct with zero evidenced
    /// real usage.
    literal_globally_suppressed: bool,
}

impl Scanner {
    fn new(body: &str, case_insensitive: bool) -> Self {
        Scanner {
            chars: body.chars().collect(),
            pos: 0,
            depth: 0,
            groups: Vec::new(),
            next_number: 1,
            literal_globally_suppressed: case_insensitive,
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
        let multi_branch = branches.len() > 1;
        let mut handles = Vec::new();
        let mut literal_values: Option<Vec<String>> = Some(Vec::new());
        for branch in branches {
            if multi_branch {
                for &idx in &branch.handles {
                    self.groups[idx].optional = true;
                }
            }
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
        Some(AtomResult {
            handles,
            literal_values,
            is_quoted_block: false,
        })
    }

    /// `branch := atom*`, stopping at `|`, `)`, or end of input. Its own
    /// literal contribution is the cross-product of every atom's own
    /// set, in order - `Some(vec![""])` (concatenation's identity) for a
    /// genuinely empty branch, matching real PCRE's own "an empty
    /// alternative always captures the empty string" semantics.
    fn scan_branch(&mut self) -> Option<AtomResult> {
        let mut handles = Vec::new();
        let mut literal_values: Option<Vec<String>> = Some(vec![String::new()]);
        loop {
            match self.peek() {
                None | Some('|') | Some(')') => break,
                _ => {}
            }
            let atom = self.scan_atom()?;
            let quantifier = self.maybe_consume_quantifier();
            if let Some((min, _)) = quantifier
                && min == 0
            {
                for &idx in &atom.handles {
                    self.groups[idx].optional = true;
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
                // Real PCRE binds a quantifier following `\Q...\E` only
                // to the block's own last character, never the whole
                // block - this scanner treats the block as one
                // indivisible atom, so cross-producting it as a whole
                // would be wrong. No corpus pattern combines the two;
                // bail rather than guess.
                Some(_) if atom.is_quoted_block => None,
                Some((_, None)) => None, // unbounded - min/max both required
                Some((min, Some(max))) => atom
                    .literal_values
                    .and_then(|lits| repeat_literals(&lits, min, max)),
            };
            literal_values = concat_literals(literal_values, contributed);
            handles.extend(atom.handles);
        }
        Some(AtomResult {
            handles,
            literal_values,
            is_quoted_block: false,
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
                self.scan_character_class()?;
                Some(AtomResult::opaque())
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
            // The wildcard - consumes a character, just not a knowably-
            // literal one.
            '.' => {
                self.pos += 1;
                Some(AtomResult::opaque())
            }
            c => {
                self.pos += 1;
                if self.literal_globally_suppressed {
                    Some(AtomResult::opaque())
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
    /// real groups, and are never literal-tracked (no corpus evidence
    /// calls for enumerating even a trivial `[a]`).
    fn scan_character_class(&mut self) -> Option<()> {
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
                    return Some(());
                }
                Some(_) => {
                    self.pos += 1;
                    first = false;
                }
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
    /// character); anything else consumes normally but contributes
    /// nothing literal - never special-casing a real multi-character
    /// escape meaning (`\d`, `\w`, `\n`, a backreference, ...), since
    /// getting one wrong would produce a *wrong* literal type.
    fn scan_escape_atom(&mut self) -> Option<AtomResult> {
        self.pos += 1; // the backslash
        match self.peek() {
            Some('Q') => {
                self.pos += 1;
                let text = self.consume_quoted_block();
                Some(AtomResult {
                    handles: Vec::new(),
                    literal_values: if self.literal_globally_suppressed {
                        None
                    } else {
                        Some(vec![text])
                    },
                    is_quoted_block: true,
                })
            }
            Some(c) if c.is_ascii_punctuation() => {
                self.pos += 1;
                if self.literal_globally_suppressed {
                    Some(AtomResult::opaque())
                } else {
                    Some(AtomResult::literal(c.to_string()))
                }
            }
            Some(_) => {
                self.pos += 1;
                Some(AtomResult::opaque())
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
            self.scan_group_inner()
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
                loop {
                    match self.advance() {
                        None => return None,
                        Some(')') => return Some(AtomResult::opaque()),
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
                // A real, if coarse, correctness fix: an inline `i`
                // modifier (scoped `(?i:...)` or bare, "rest of branch"
                // `(?i)`) makes literal tracking unsafe from here on -
                // real PCRE scopes this precisely to the modifier's own
                // body/branch remainder, but no corpus evidence anywhere
                // uses either form, so this scanner suppresses literal
                // tracking for the *rest of the whole pattern* instead
                // of building exact scoping for a construct with zero
                // evidenced real usage - coarser, but never wrong.
                if mods.contains('i') {
                    self.literal_globally_suppressed = true;
                }
                if self.eat(':') {
                    self.finish_non_capturing_group()
                } else if self.eat(')') {
                    Some(AtomResult::opaque())
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
        self.groups.push(RegexCaptureGroup {
            number,
            name,
            optional: false,
            literal_values: None,
        });
        let body = self.scan_alternation()?;
        if !self.eat(')') {
            return None;
        }
        self.groups[idx].literal_values = body.literal_values.clone();
        let mut handles = vec![idx];
        handles.extend(body.handles);
        Some(AtomResult {
            handles,
            literal_values: body.literal_values,
            is_quoted_block: false,
        })
    }

    fn finish_non_capturing_group(&mut self) -> Option<AtomResult> {
        let body = self.scan_alternation()?;
        if !self.eat(')') {
            return None;
        }
        Some(body)
    }

    /// A lookahead/lookbehind (`(?=...)`, `(?!...)`, `(?<=...)`,
    /// `(?<!...)`) - zero-width, so its own contribution to whatever
    /// contains it is always the empty string, *never* its own
    /// interior's literal computation (a real bug found and fixed while
    /// designing this: `(a(?=b))` genuinely captures only `"a"`,
    /// confirmed live - treating a lookaround like an ordinary
    /// non-capturing group would silently compute the wrong literal
    /// `"ab"`). The interior is still fully scanned as normal for
    /// group-counting and for any *nested* capturing group's own
    /// `literal_values` (`(?=(a))`'s own group 1 really is `'a'` if it
    /// participates) - only this construct's own contribution is forced.
    fn finish_lookaround(&mut self) -> Option<AtomResult> {
        let body = self.scan_alternation()?;
        if !self.eat(')') {
            return None;
        }
        Some(AtomResult {
            handles: body.handles,
            literal_values: Some(vec![String::new()]),
            is_quoted_block: false,
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
mod tests {
    use super::*;

    fn numbers_and_names(groups: &[RegexCaptureGroup]) -> Vec<(usize, Option<&str>, bool)> {
        groups
            .iter()
            .map(|g| (g.number, g.name.as_deref(), g.optional))
            .collect()
    }

    fn literals(groups: &[RegexCaptureGroup]) -> Vec<Option<Vec<String>>> {
        groups.iter().map(|g| g.literal_values.clone()).collect()
    }

    #[test]
    fn bare_numbered_groups_are_required() {
        let groups = parse_capture_groups(r"/(\d+)-(\d+)/").unwrap();
        assert_eq!(
            numbers_and_names(&groups),
            vec![(1, None, false), (2, None, false)]
        );
    }

    #[test]
    fn named_groups_capture_both_number_and_name() {
        let groups = parse_capture_groups(r"/(?<year>\d+)-(?<month>\d+)/").unwrap();
        assert_eq!(
            numbers_and_names(&groups),
            vec![(1, Some("year"), false), (2, Some("month"), false)]
        );
    }

    #[test]
    fn optional_group_via_question_mark() {
        let groups = parse_capture_groups("/(a)(b)?/").unwrap();
        assert_eq!(
            numbers_and_names(&groups),
            vec![(1, None, false), (2, None, true)]
        );
    }

    #[test]
    fn optional_group_via_alternation() {
        let groups = parse_capture_groups("/(a)|(b)/").unwrap();
        assert_eq!(
            numbers_and_names(&groups),
            vec![(1, None, true), (2, None, true)]
        );
    }

    #[test]
    fn optionality_propagates_through_a_non_capturing_wrapper() {
        let groups = parse_capture_groups("/(?:(a))?/").unwrap();
        assert_eq!(numbers_and_names(&groups), vec![(1, None, true)]);
    }

    #[test]
    fn p_angle_and_quote_named_group_syntax_are_equivalent() {
        let angle = parse_capture_groups("/(?P<x>a)/").unwrap();
        let quote = parse_capture_groups("/(?'x'a)/").unwrap();
        assert_eq!(numbers_and_names(&angle), vec![(1, Some("x"), false)]);
        assert_eq!(numbers_and_names(&quote), vec![(1, Some("x"), false)]);
    }

    #[test]
    fn a_leading_bracket_and_a_posix_class_dont_miscount_group_boundaries() {
        let groups = parse_capture_groups("/[]a[:digit:]]+/").unwrap();
        assert!(groups.is_empty());
    }

    #[test]
    fn a_malformed_posix_looking_construct_nested_in_a_class_falls_back_to_literal() {
        // The `[:` lookahead only ever triggers on an inner `[` while a
        // class is already open, since real POSIX syntax `[:name:]` is
        // itself bracket-delimited - a bare top-level `/[:foo](x)/` never
        // reaches this code path at all.
        let groups = parse_capture_groups("/[[:foo](x)/").unwrap();
        assert_eq!(numbers_and_names(&groups), vec![(1, None, false)]);
    }

    #[test]
    fn q_e_literal_block_does_not_open_a_group() {
        let groups = parse_capture_groups(r"/\Q(\E/").unwrap();
        assert!(groups.is_empty());
    }

    #[test]
    fn a_literal_brace_not_forming_a_quantifier_is_plain_text() {
        let groups = parse_capture_groups("/a{b/").unwrap();
        assert!(groups.is_empty());
    }

    #[test]
    fn duplicate_names_on_alternation_branches_dont_crash() {
        let groups = parse_capture_groups("/(?<x>a)|(?<x>b)/").unwrap();
        assert_eq!(
            numbers_and_names(&groups),
            vec![(1, Some("x"), true), (2, Some("x"), true)]
        );
    }

    #[test]
    fn a_conditional_group_bails() {
        assert!(parse_capture_groups("/(?(1)a|b)/").is_none());
    }

    #[test]
    fn the_x_and_j_modifiers_both_bail() {
        assert!(parse_capture_groups("/(a)/x").is_none());
        assert!(parse_capture_groups("/(a)/J").is_none());
    }

    #[test]
    fn malformed_patterns_bail() {
        assert!(parse_capture_groups("/(a/").is_none());
        assert!(parse_capture_groups("/").is_none());
        assert!(parse_capture_groups("").is_none());
    }

    #[test]
    fn delimiter_variety_all_strip_correctly() {
        assert!(parse_capture_groups("/(a)/").is_some());
        assert!(parse_capture_groups("#(a)#").is_some());
        assert!(parse_capture_groups("~(a)~").is_some());
        assert!(parse_capture_groups("(a)").is_some());
        assert!(parse_capture_groups("{a}").is_some());
        assert!(parse_capture_groups("[a]").is_some());
        assert!(parse_capture_groups("<a>").is_some());
    }

    #[test]
    fn the_real_qdaterangescope_pattern_produces_the_expected_shape() {
        // src/Piwigo/Search/QDateRangeScope.php:51 - a year/month/day
        // date parser. Group 1 (year) required; groups 2 and 3
        // (month/day) each optional via their own trailing `?`.
        let groups = parse_capture_groups(
            "/([0-9]{4})-?((?:1[0-2])|(?:0?[1-9]))?-?((?:(?:[1-3][0-9])|(?:0?[1-9])))?/",
        )
        .unwrap();
        assert_eq!(
            numbers_and_names(&groups),
            vec![(1, None, false), (2, None, true), (3, None, true)]
        );
    }

    #[test]
    fn pathological_input_does_not_hang_or_overflow() {
        let long_flat = format!("/{}/", "a".repeat(5000));
        assert!(parse_capture_groups(&long_flat).is_some());

        let mut deeply_nested = String::from("/");
        for _ in 0..500 {
            deeply_nested.push_str("(?:");
        }
        deeply_nested.push('a');
        for _ in 0..500 {
            deeply_nested.push(')');
        }
        deeply_nested.push('/');
        // Exceeds MAX_GROUP_DEPTH - must bail cleanly, not overflow.
        assert!(parse_capture_groups(&deeply_nested).is_none());
    }

    // --- Task 1a: literal enumeration ---

    #[test]
    fn a_plain_literal_group_is_fully_enumerable() {
        let groups = parse_capture_groups("/(cat)/").unwrap();
        assert_eq!(literals(&groups), vec![Some(vec!["cat".to_string()])]);
    }

    #[test]
    fn an_alternation_of_literals_unions_their_values() {
        let groups = parse_capture_groups("/(cat|dog)/").unwrap();
        assert_eq!(
            literals(&groups),
            vec![Some(vec!["cat".to_string(), "dog".to_string()])]
        );
    }

    #[test]
    fn q_e_contributes_its_own_verbatim_text() {
        let groups = parse_capture_groups(r"/(\Qa.b\E)/").unwrap();
        assert_eq!(literals(&groups), vec![Some(vec!["a.b".to_string()])]);
    }

    #[test]
    fn an_escaped_punctuation_character_is_literal() {
        let groups = parse_capture_groups(r"/(a\.b)/").unwrap();
        assert_eq!(literals(&groups), vec![Some(vec!["a.b".to_string()])]);
    }

    #[test]
    fn a_bounded_quantifier_repeats_a_literal_atom() {
        let groups = parse_capture_groups("/(a{2})/").unwrap();
        assert_eq!(literals(&groups), vec![Some(vec!["aa".to_string()])]);
    }

    #[test]
    fn a_bounded_range_quantifier_enumerates_every_repeat_count() {
        let groups = parse_capture_groups("/(a{1,2})/").unwrap();
        let mut values = literals(&groups)[0].clone().unwrap();
        values.sort();
        assert_eq!(values, vec!["a".to_string(), "aa".to_string()]);
    }

    #[test]
    fn the_i_modifier_disables_literal_tracking_but_not_group_counting() {
        let groups = parse_capture_groups("/(cat)/i").unwrap();
        assert_eq!(numbers_and_names(&groups), vec![(1, None, false)]);
        assert_eq!(literals(&groups), vec![None]);
    }

    #[test]
    fn a_character_class_is_never_literal_tracked() {
        let groups = parse_capture_groups("/([a])/").unwrap();
        assert_eq!(literals(&groups), vec![None]);
    }

    #[test]
    fn a_wildcard_dot_is_never_literal_tracked() {
        let groups = parse_capture_groups("/(a.b)/").unwrap();
        assert_eq!(literals(&groups), vec![None]);
    }

    #[test]
    fn anchors_contribute_the_empty_string_not_a_bail() {
        let groups = parse_capture_groups("/(a^b)/").unwrap();
        assert_eq!(literals(&groups), vec![Some(vec!["ab".to_string()])]);
    }

    #[test]
    fn an_unbounded_quantifier_disables_literal_tracking_for_that_atom() {
        let groups = parse_capture_groups("/(a+)/").unwrap();
        assert_eq!(literals(&groups), vec![None]);
    }

    #[test]
    fn an_unrecognized_escape_disables_literal_tracking_for_that_atom() {
        let groups = parse_capture_groups(r"/(a\db)/").unwrap();
        assert_eq!(literals(&groups), vec![None]);
    }

    #[test]
    fn a_bounded_quantifier_directly_after_a_quoted_block_bails_that_span() {
        let groups = parse_capture_groups(r"/(\Qab\E{2})/").unwrap();
        assert_eq!(literals(&groups), vec![None]);
    }

    #[test]
    fn a_group_repeated_more_than_once_externally_loses_its_own_literal_value() {
        let groups = parse_capture_groups("/(?:(a)){2,}/").unwrap();
        assert_eq!(literals(&groups), vec![None]);
        assert_eq!(numbers_and_names(&groups), vec![(1, None, false)]);
    }

    #[test]
    fn a_lookahead_contributes_the_empty_string_not_its_own_interior() {
        let groups = parse_capture_groups("/(a(?=b))/").unwrap();
        assert_eq!(literals(&groups), vec![Some(vec!["a".to_string()])]);
    }

    #[test]
    fn a_capturing_group_inside_a_lookahead_still_gets_its_own_correct_value() {
        let groups = parse_capture_groups("/(?=(a))/").unwrap();
        assert_eq!(literals(&groups), vec![Some(vec!["a".to_string()])]);
    }

    #[test]
    fn the_literal_enum_limit_degrades_one_span_without_failing_the_whole_parse() {
        // 6 branches of 2 chars each, repeated 6 times -> 6^6 = 46656
        // combinations, well past LITERAL_ENUM_LIMIT (32).
        let groups = parse_capture_groups("/((?:aa|bb|cc|dd|ee|ff){6})(x)/").unwrap();
        assert_eq!(
            numbers_and_names(&groups),
            vec![(1, None, false), (2, None, false)]
        );
        assert_eq!(literals(&groups)[0], None);
        assert_eq!(literals(&groups)[1], Some(vec!["x".to_string()]));
    }
}
