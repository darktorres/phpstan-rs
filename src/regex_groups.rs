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
}

/// A deeply-nested pattern is a real stack-overflow risk via this
/// scanner's own recursive descent, not just a slowness one - the same
/// class of bug Phase 20 found and fixed for the main expression parser
/// (`src/parser.rs`'s `MAX_EXPR_DEPTH`). This scanner already returns
/// `Option` everywhere, so exceeding this just bails like any other
/// unsupported construct, no panic needed.
const MAX_GROUP_DEPTH: usize = 200;

/// Parses a PHP PCRE pattern literal's own delimiters/modifiers and
/// structure to enumerate its capturing groups, in left-to-right
/// numbering order. Returns `None` for anything outside this function's
/// own understood subset.
pub(crate) fn parse_capture_groups(pattern_literal: &str) -> Option<Vec<RegexCaptureGroup>> {
    let (body, modifiers) = split_delimiters_and_modifiers(pattern_literal)?;
    // `x` (free-spacing mode) and `J` (duplicate named groups, excluded
    // by real PHPStan itself as "too complicated") both change the
    // grammar in ways this scanner doesn't attempt - bail before
    // structural scanning even starts.
    if modifiers.contains('x') || modifiers.contains('J') {
        return None;
    }
    let mut scanner = Scanner::new(body);
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

struct Scanner {
    chars: Vec<char>,
    pos: usize,
    depth: usize,
    groups: Vec<RegexCaptureGroup>,
    next_number: usize,
}

impl Scanner {
    fn new(body: &str) -> Self {
        Scanner {
            chars: body.chars().collect(),
            pos: 0,
            depth: 0,
            groups: Vec::new(),
            next_number: 1,
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
    /// composes soundly with a quantifier's own optionality below.
    fn scan_alternation(&mut self) -> Option<Vec<usize>> {
        let mut branches: Vec<Vec<usize>> = vec![self.scan_branch()?];
        while self.eat('|') {
            branches.push(self.scan_branch()?);
        }
        let multi_branch = branches.len() > 1;
        let all: Vec<usize> = branches.into_iter().flatten().collect();
        if multi_branch {
            for &idx in &all {
                self.groups[idx].optional = true;
            }
        }
        Some(all)
    }

    /// `branch := atom*`, stopping at `|`, `)`, or end of input.
    fn scan_branch(&mut self) -> Option<Vec<usize>> {
        let mut handles = Vec::new();
        loop {
            match self.peek() {
                None | Some('|') | Some(')') => break,
                _ => {}
            }
            let atom_handles = self.scan_atom()?;
            if let Some(is_zero_min) = self.maybe_consume_quantifier()
                && is_zero_min
            {
                for &idx in &atom_handles {
                    self.groups[idx].optional = true;
                }
            }
            handles.extend(atom_handles);
        }
        Some(handles)
    }

    /// If the input at the current position forms a valid quantifier
    /// (`?`, `*`, `+`, `{n}`, `{n,}`, `{n,m}` - digits only, correctly
    /// shaped - optionally suffixed by a lazy `?` or possessive `+`),
    /// consumes it whole and returns whether its minimum is 0. Otherwise
    /// consumes nothing and returns `None` - a malformed `{...}` (real
    /// PCRE treats it as literal text, e.g. `a{b`) is left untouched for
    /// the next atom scan, not treated as an attempted-and-failed
    /// quantifier.
    fn maybe_consume_quantifier(&mut self) -> Option<bool> {
        let is_zero_min = match self.peek() {
            Some('?') | Some('*') => {
                self.pos += 1;
                true
            }
            Some('+') => {
                self.pos += 1;
                false
            }
            Some('{') => {
                let start = self.pos;
                self.pos += 1;
                let n_start = self.pos;
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    self.pos += 1;
                }
                let has_n = self.pos > n_start;
                let n_is_zero = has_n
                    && self.chars[n_start..self.pos]
                        .iter()
                        .collect::<String>()
                        .parse::<u64>()
                        == Ok(0);
                if self.eat(',') {
                    while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                        self.pos += 1;
                    }
                }
                if !has_n || !self.eat('}') {
                    // Malformed - rewind, treat the `{` as literal text.
                    self.pos = start;
                    return None;
                }
                n_is_zero
            }
            _ => return None,
        };
        // A trailing lazy `?` or possessive `+` doesn't affect
        // optionality, but must still be consumed - left alone, it would
        // be misread as the start of a brand new atom by the next loop
        // iteration, corrupting everything after it.
        if matches!(self.peek(), Some('?') | Some('+')) {
            self.pos += 1;
        }
        Some(is_zero_min)
    }

    /// `atom := group | class | escape | literal`.
    fn scan_atom(&mut self) -> Option<Vec<usize>> {
        match self.peek()? {
            '(' => {
                self.pos += 1;
                self.scan_group()
            }
            '[' => {
                self.scan_character_class()?;
                Some(Vec::new())
            }
            '\\' => {
                self.scan_escape()?;
                Some(Vec::new())
            }
            _ => {
                self.pos += 1;
                Some(Vec::new())
            }
        }
    }

    /// Consumes a character class's interior, given `self.pos` already
    /// past the opening `[`. Priority order matters: an escape is always
    /// checked first (so `[\]]`'s escaped `]` is never misread as the
    /// real closer), then a candidate POSIX class, then a leading-`]`
    /// literal, then an ordinary member. Character classes never contain
    /// real groups.
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
    /// `\Q` which switches to literal mode until `\E` or end-of-pattern,
    /// consuming everything in between verbatim with no metacharacter
    /// interpretation at all. Deliberately semantics-agnostic: this
    /// scanner never needs to know what an escape *means* - only whether
    /// a character opens or closes a paren/bracket/quantifier matters,
    /// and consuming exactly the backslash plus one character is sound
    /// for every escape form regardless of what it actually represents.
    fn scan_escape(&mut self) -> Option<()> {
        self.pos += 1; // the backslash
        match self.peek() {
            Some('Q') => {
                self.pos += 1;
                loop {
                    match self.peek() {
                        None => break,
                        Some('\\') if self.peek_at(1) == Some('E') => {
                            self.pos += 2;
                            break;
                        }
                        Some(_) => self.pos += 1,
                    }
                }
                Some(())
            }
            Some(_) => {
                self.pos += 1;
                Some(())
            }
            None => None, // a trailing lone backslash - malformed
        }
    }

    /// `group := '(' ... ')'`, given `self.pos` already past the opening
    /// `(`. Peeks for a following `?` to distinguish a real numbered
    /// capturing group from every other `(?...)` shape.
    fn scan_group(&mut self) -> Option<Vec<usize>> {
        self.depth += 1;
        let result = if self.depth > MAX_GROUP_DEPTH {
            None
        } else {
            self.scan_group_inner()
        };
        self.depth -= 1;
        result
    }

    fn scan_group_inner(&mut self) -> Option<Vec<usize>> {
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
                self.finish_non_capturing_group()
            }
            Some('<') if matches!(self.peek_at(1), Some('=') | Some('!')) => {
                self.pos += 2;
                self.finish_non_capturing_group()
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
                    // handle, no recursion.
                    self.consume_atomic_to_close_paren()?;
                    Some(Vec::new())
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
                        Some(')') => return Some(Vec::new()),
                        Some(_) => {}
                    }
                }
            }
            Some('(') => None, // a conditional group - bail the whole parse
            Some(c) if c.is_ascii_digit() || matches!(c, '+' | '-' | '&' | 'R') => {
                // Recursion/subroutine-call forms: (?R), (?1), (?+1),
                // (?-1), (?&name) - atomic, no handle, no recursion (per
                // PCRE these never contain further parens).
                self.consume_atomic_to_close_paren()?;
                Some(Vec::new())
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
                if self.eat(':') {
                    self.finish_non_capturing_group()
                } else if self.eat(')') {
                    Some(Vec::new())
                } else {
                    None
                }
            }
            _ => None, // a malformed or unrecognized `(?X...)` shape
        }
    }

    fn finish_capturing_group(&mut self, name: Option<String>) -> Option<Vec<usize>> {
        let number = self.next_number;
        self.next_number += 1;
        let idx = self.groups.len();
        self.groups.push(RegexCaptureGroup {
            number,
            name,
            optional: false,
        });
        let mut handles = vec![idx];
        handles.extend(self.scan_alternation()?);
        if !self.eat(')') {
            return None;
        }
        Some(handles)
    }

    fn finish_non_capturing_group(&mut self) -> Option<Vec<usize>> {
        let handles = self.scan_alternation()?;
        if !self.eat(')') {
            return None;
        }
        Some(handles)
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
}
