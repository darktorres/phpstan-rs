use crate::lexer::{Lexer, is_ident_continue};
use crate::token::{Span, Token};

/// Finds the next single- or double-quoted string literal in `s` (quote-
/// agnostic - the real stub corpus mixes `'...'` and `"..."` freely for
/// this attribute), returning its unquoted content and the remainder of
/// `s` right after the closing quote. No escape handling - every type
/// string this attribute carries is a plain identifier/union, never
/// containing a quote of its own.
fn take_quoted_string(s: &str) -> Option<(&str, &str)> {
    let start = s.find(['\'', '"'])?;
    let quote = s.as_bytes()[start] as char;
    let after_open = &s[start + quote.len_utf8()..];
    let end = after_open.find(quote)?;
    Some((&after_open[..end], &after_open[end + quote.len_utf8()..]))
}

/// Parses a `LanguageLevelTypeAware` version key (`"8.1"`) into a
/// `(major, minor)` pair for numeric comparison - a plain string
/// comparison would wrongly rank `"8.10"` below `"8.9"`, and while no
/// version this high exists yet, comparing as numbers costs nothing extra.
fn parse_version(s: &str) -> Option<(u32, u32)> {
    let (major, minor) = s.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// Extracts the highest-versioned type string from a captured
/// `LanguageLevelTypeAware(['V.V' => 'TYPE', ...], default: 'TYPE')`
/// attribute's raw text (including its own `#[...` wrapper - callers
/// don't need to trim it first, only `default:`'s presence and the
/// quoted pairs before it matter). This project's own established
/// "assume the modern/open-ended interpretation" precedent (already used
/// for `#[PhpStormStubsElementAvailable]`'s closed-range parameter
/// dropping) - real code being checked is running some real, current PHP
/// version, so the highest-versioned declared type is the one actually
/// in effect. A version key that fails to parse as `major.minor` is
/// skipped, not fatal to the rest of the scan. Falls back to `default:`'s
/// own value only when no version pair parsed at all (shouldn't happen -
/// every real occurrence has at least one - but `default:` is always
/// present regardless, confirmed against the full vendored corpus).
fn extract_modern_type(raw: &str) -> Option<String> {
    let (array_part, default_part) = match raw.find("default:") {
        Some(idx) => (&raw[..idx], Some(&raw[idx + "default:".len()..])),
        None => (raw, None),
    };
    let mut best: Option<((u32, u32), String)> = None;
    let mut rest = array_part;
    while let Some((key, after_key)) = take_quoted_string(rest) {
        let Some(after_arrow) = after_key.trim_start().strip_prefix("=>") else {
            break;
        };
        let Some((ty, tail)) = take_quoted_string(after_arrow) else {
            break;
        };
        if let Some(version) = parse_version(key)
            && best.as_ref().is_none_or(|(v, _)| version > *v)
        {
            best = Some((version, ty.to_string()));
        }
        rest = tail;
    }
    best.map(|(_, ty)| ty).or_else(|| {
        default_part
            .and_then(take_quoted_string)
            .map(|(ty, _)| ty.to_string())
    })
}

/// Splices a fresh `@return <override>` line into `doc` right before its
/// closing `*/` - `docblock::parse` processes `@return` tags in source
/// order with no early exit, so a later one simply overwrites an earlier
/// one's value in the returned `DocBlock`. Appending (rather than
/// surgically replacing the original `@return` line's own type
/// substring) sidesteps needing to reimplement `take_type_token`'s own
/// type-vs-description boundary logic here.
pub(crate) fn append_return_type_override(doc: &str, override_ty: &str) -> String {
    match doc.rfind("*/") {
        Some(idx) => format!(
            "{}\n * @return {override_ty}\n {}",
            &doc[..idx],
            &doc[idx..]
        ),
        None => doc.to_string(),
    }
}

/// Phase 32 Finding B: the parameter-position sibling of
/// `append_return_type_override` - splices a fresh `@param <override>
/// $<name>` line into `doc` right before its closing `*/`, so
/// `resolve_type`'s own "docblock wins" priority (each `@param` tag
/// matched by name, last one wins - see `docblock::parse`'s
/// `HashMap::insert`) makes the modernization override win over a stale
/// legacy `@param` tag for the same parameter, without touching
/// `resolve_type`'s general priority rule at all.
pub(crate) fn append_param_type_override(doc: &str, name: &str, override_ty: &str) -> String {
    match doc.rfind("*/") {
        Some(idx) => format!(
            "{}\n * @param {override_ty} ${name}\n {}",
            &doc[..idx],
            &doc[idx..]
        ),
        None => doc.to_string(),
    }
}

/// Phase 37: the index one past the last token in `tokens` that isn't
/// part of a trailing, complete `#[Attr(...)]` group - zero or more such
/// groups are skipped back over, in case several are stacked
/// (`#[Pure] #[LanguageLevelTypeAware(...)]`, the real phpstorm-stubs
/// `substr()` shape this exists for). Depth-counts backward the same way
/// a forward scanner would count `[`/`]` (an attribute group's own
/// opening `#[` counts as an open, exactly like a plain `[` inside its
/// arguments would, since both close with the identical `RBracket`
/// token) - so a nested array-literal argument (`#[ArrayShape([...])]`)
/// doesn't stop the walk short. Falls back to returning `tokens.len()`
/// unchanged (skip nothing) if the depth count runs off the start without
/// balancing - malformed input, not worth guessing at.
pub(crate) fn skip_trailing_attribute_groups(tokens: &[(Token, Span)]) -> usize {
    let mut end = tokens.len();
    while matches!(tokens.get(end.wrapping_sub(1)), Some((Token::RBracket, _))) {
        let mut depth = 1i32;
        let mut i = end - 1;
        loop {
            if i == 0 {
                return tokens.len();
            }
            i -= 1;
            match &tokens[i].0 {
                Token::RBracket => depth += 1,
                Token::AttributeStart | Token::LBracket => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
        }
        if !matches!(tokens[i].0, Token::AttributeStart) {
            // Balanced against a plain `[`, not `#[` - not actually a
            // trailing attribute group at all (shouldn't happen for a
            // token stream produced by this lexer, but bail rather than
            // guess).
            return tokens.len();
        }
        end = i;
    }
    end
}

impl<'a> Lexer<'a> {
    /// phpstorm-stubs represents "this parameter only existed in PHP
    /// versions X-Y" with `#[PhpStormStubsElementAvailable(from: 'X', to:
    /// 'Y')]` on the parameter itself, rather than maintaining separate
    /// per-version signatures - e.g. `array_map`'s real stub carries a
    /// modern, still-open `array $array` parameter *and* a superseded,
    /// closed-range, no-default `$arrays` one, side by side with the
    /// modern variadic `array ...$arrays`, both meant to represent the
    /// same conceptual parameter across two PHP-version eras, never both
    /// present on one real install. A plain attribute skip (this project's
    /// treatment of every other attribute, none of which carry
    /// information any checker feature here consumes) would leave that
    /// superseded parameter in the token stream as an ordinary,
    /// permanently-required one, miscounting the function's real minimum
    /// arity - and not just for this project's own bundled stub corpus:
    /// real projects commonly vendor `jetbrains/phpstorm-stubs` itself as
    /// a transitive phpstan dependency, so this needs to be a lexer-level
    /// fix, not something special-cased only in the stub-generation
    /// pipeline.
    ///
    /// Only a *closed* range (`to:` present) parameter with no `= value`
    /// default of its own gets dropped - along with, if present, its one
    /// trailing separator comma (a trailing comma before `)` is valid PHP
    /// syntax the parser already tolerates, so nothing needs repairing on
    /// the other side). An open-ended range (`from` only, still valid in
    /// the latest PHP) is a real, current parameter and is left alone, as
    /// is one that already has a real default - both fall back to an
    /// ordinary attribute-only skip. Whether this attribute gates a
    /// single parameter or a *whole* function/method declaration (real
    /// shape: `DateTimeImmutable::__construct`'s `to: '8.2'`/`from:
    /// '8.3'` pair) is checked before attempting the parameter-fragment
    /// scan at all, via `upcoming_is_function_declaration` - see
    /// `skip_whole_versioned_declaration`'s own doc comment for that
    /// case (Round 15 of the Piwigo corpus cleanup - previously left
    /// completely untouched, wrongly letting a superseded declaration's
    /// return type merge into the modern one's).
    pub(crate) fn skip_stub_availability_attribute(&mut self) {
        self.pos += "#[PhpStormStubsElementAvailable(".len();
        let attr_args_start = self.pos;
        let mut depth = 1i32;
        while depth > 0 && self.pos < self.src.len() {
            match self.peek() {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            self.pos += 1;
        }
        if depth != 0 {
            // Malformed/unexpected shape (EOF before the attribute
            // closed) - bail out with whatever's been consumed so far.
            return;
        }
        let is_closed_range = std::str::from_utf8(&self.src[attr_args_start..self.pos - 1])
            .is_ok_and(|s| s.contains("to:"));
        if self.peek() == b']' {
            self.pos += 1;
        }
        if !is_closed_range {
            return;
        }
        // Round 15 of the Piwigo corpus cleanup: this same attribute also
        // gates a *whole* function/method declaration (real shape:
        // `version_compare()`'s own two full declarations, one
        // `to: '7.4'` returning `int|bool|null`, one `from: '8.0'`
        // returning `int|bool`; `DateTimeImmutable::__construct`'s
        // `to: '8.2'`/`from: '8.3'` pair) - previously left completely
        // untouched (this function's own doc comment used to document
        // that as deliberate), so both declarations reached the parser
        // as ordinary, unconditional signatures and got unioned together
        // by the registry's own "second unconditional declaration merges
        // onto the first" real-overload handling, wrongly reintroducing
        // the pre-8.0 `null` return into a target this project otherwise
        // always treats as PHP 8.0+. A closed range (`to:` - superseded
        // on the modern PHP version this project models) gating a whole
        // declaration should leave no trace at all, the same principle
        // already applied to a closed-range *parameter* below.
        if self.upcoming_is_function_declaration() {
            self.skip_whole_versioned_declaration();
            return;
        }

        let param_start = self.pos;
        let mut pdepth = 0i32;
        let mut has_default = false;
        loop {
            if self.pos >= self.src.len() {
                // Reached EOF mid-parameter - leave as skipped so far.
                return;
            }
            match self.peek() {
                b'(' | b'[' => pdepth += 1,
                b')' | b']' if pdepth == 0 => break,
                b')' | b']' => pdepth -= 1,
                b',' if pdepth == 0 => break,
                b'{' | b';' if pdepth == 0 => {
                    // Not a simple parameter fragment - restore and leave
                    // the attribute-only skip already applied above.
                    self.pos = param_start;
                    return;
                }
                b'=' if pdepth == 0 => has_default = true,
                _ => {}
            }
            self.pos += 1;
        }
        if has_default {
            // Already counted correctly regardless of its version range.
            self.pos = param_start;
            return;
        }
        // Drop the parameter too, and its one trailing comma if present
        // (a closing delimiter is left for the real tokenizer to see).
        if self.peek() == b',' {
            self.pos += 1;
        }
    }

    /// Whether the attribute just consumed by `skip_stub_availability_attribute`
    /// gates a whole function/method declaration rather than a single
    /// parameter - real PHP only allows `public`/`private`/`protected`/
    /// `static`/`abstract`/`final` modifiers (in any combination) before
    /// the `function` keyword itself at this position; a parameter
    /// fragment can never start with any of them.
    fn upcoming_is_function_declaration(&self) -> bool {
        let mut p = self.pos;
        loop {
            while p < self.src.len() && (self.src[p] as char).is_ascii_whitespace() {
                p += 1;
            }
            let word_start = p;
            while p < self.src.len() && (self.src[p] as char).is_ascii_alphabetic() {
                p += 1;
            }
            if p == word_start {
                return false;
            }
            match std::str::from_utf8(&self.src[word_start..p]) {
                Ok("public" | "private" | "protected" | "static" | "abstract" | "final") => {}
                Ok("function") => return true,
                _ => return false,
            }
        }
    }

    /// Skips an entire closed-range-superseded function/method
    /// declaration (modifiers, `function`, name, parameter list, return
    /// type) up through its own terminator - either a bare `;` (an
    /// abstract/interface method, no body) or a balanced `{...}` body.
    /// `;` can never appear inside the parameter list itself (not even
    /// in a default value - PHP has no expression form that contains a
    /// bare `;`), so a simple paren-depth counter is enough to tell the
    /// declaration's own terminator apart from one inside the parameter
    /// list's parens.
    fn skip_whole_versioned_declaration(&mut self) {
        let mut paren_depth = 0i32;
        loop {
            if self.pos >= self.src.len() {
                return;
            }
            match self.peek() {
                b'\'' | b'"' => self.skip_string_literal(),
                b'(' => {
                    paren_depth += 1;
                    self.pos += 1;
                }
                b')' => {
                    paren_depth -= 1;
                    self.pos += 1;
                }
                b';' if paren_depth <= 0 => {
                    self.pos += 1;
                    return;
                }
                b'{' if paren_depth <= 0 => {
                    self.skip_balanced_braces();
                    return;
                }
                _ => self.pos += 1,
            }
        }
    }

    /// String-aware skip from an opening `{` (the current byte) through
    /// its matching `}`.
    fn skip_balanced_braces(&mut self) {
        self.pos += 1;
        let mut depth = 1i32;
        while depth > 0 && self.pos < self.src.len() {
            match self.peek() {
                b'\'' | b'"' => self.skip_string_literal(),
                b'{' => {
                    depth += 1;
                    self.pos += 1;
                }
                b'}' => {
                    depth -= 1;
                    self.pos += 1;
                }
                _ => self.pos += 1,
            }
        }
    }

    /// phpstorm-stubs represents "this function/param's type differs by
    /// PHP version" with `#[LanguageLevelTypeAware(['V.V' => 'MODERN',
    /// ...], default: 'LEGACY')]` - e.g. `substr()`'s real stub has *no*
    /// native return-type hint at all, so its only source of a return
    /// type is its own `@return string|false` doc comment, a pre-8.0
    /// legacy value this attribute exists specifically to override for a
    /// modern PHP install; `finfo_file`'s real stub gates its own first
    /// *parameter* the identical way (`#[LanguageLevelTypeAware(['8.1' =>
    /// 'finfo'], default: 'resource')] $finfo`, no separate native hint
    /// at all either). Captures the attribute's own argument text (via
    /// the same paren-depth, string-aware skip every other call site here
    /// already uses), extracts the highest-versioned type via
    /// `extract_modern_type`, and dispatches on which of the two shapes
    /// this is (`next_parameter_anchor` - an immediately-following `$`/
    /// `&`/`...` means a parameter, anything else means a return type):
    /// a parameter override becomes real, synthetic type-hint tokens
    /// queued in `pending_param_type_tokens` (inserted right before the
    /// upcoming `$name` token - no docblock involved, since a parameter's
    /// own `@param $name` tag may not exist at all for every gated
    /// parameter); a return override is stashed in
    /// `pending_return_type_override` for `tokenize_with_spans` to splice
    /// into the preceding doc comment instead.
    pub(crate) fn skip_language_level_type_aware_attribute(&mut self, matched_prefix: &str) {
        let attr_start = self.pos;
        self.pos += matched_prefix.len();
        let mut depth = 1i32;
        while depth > 0 && self.pos < self.src.len() {
            match self.peek() {
                b'\'' | b'"' => self.skip_string_literal(),
                b'(' => {
                    depth += 1;
                    self.pos += 1;
                }
                b')' => {
                    depth -= 1;
                    self.pos += 1;
                }
                _ => self.pos += 1,
            }
        }
        if depth != 0 {
            // Malformed/unexpected shape (EOF before the attribute
            // closed) - bail out with whatever's been consumed so far.
            return;
        }
        let raw = String::from_utf8_lossy(&self.src[attr_start..self.pos]).into_owned();
        if self.peek() == b']' {
            self.pos += 1;
        }
        let Some(override_ty) = extract_modern_type(&raw) else {
            return;
        };
        if self.next_parameter_anchor() {
            // Unlike a return type (routed through the docblock, parsed
            // by `types::parse_type_string` which understands PHPDoc-only
            // syntax like `TYPE[]`), an injected parameter override
            // becomes real *native* type-hint tokens, consumed by the
            // parser's native `parse_type_hint` - which has no notion of
            // `[]` shorthand at all. Real corpus proof this isn't
            // hypothetical: `strip_tags()`'s own `#[LanguageLevelTypeAware(
            // ["7.4" => "string[]|string|null"], ...)]` parameter crashed
            // the parser outright before this guard existed. None of this
            // pass's diagnosed parameter cases (`finfo`, `GdFont|int`,
            // `PgSql\Connection`) use `[]` shorthand, so skipping this
            // shape entirely - falling back to today's existing "leave it
            // alone" behavior - costs nothing evidenced, and is far safer
            // than guessing at a native-hint-compatible rewrite.
            if !override_ty.contains(['[', ']']) {
                let tokens: Vec<Token> = Lexer::new_in_php(&override_ty)
                    .tokenize()
                    .into_iter()
                    .filter(|t| *t != Token::Eof)
                    .collect();
                if !tokens.is_empty() {
                    self.pending_param_type_tokens = Some(tokens.clone());
                }
                // Finding B: also queue a doc-splice fallback, keyed by the
                // parameter's own name - a no-op if there's no enclosing
                // doc comment to find, but the only way to beat a
                // competing legacy `@param` tag for this same name when
                // one does exist (see this field's own doc comment).
                if !tokens.is_empty()
                    && let Some(name) = self.peek_param_name()
                {
                    self.pending_param_doc_override = Some((name, override_ty));
                }
            }
        } else {
            self.pending_return_type_override = Some(override_ty);
        }
    }

    /// Peeks (without consuming) the parameter name that
    /// `next_parameter_anchor` already confirmed follows - skips leading
    /// whitespace, an optional `&` (by-ref) or `...` (variadic) marker,
    /// then the `$` and its identifier. `None` for anything other than
    /// exactly this shape - conservative, matching every other bail-out in
    /// this file rather than guessing.
    fn peek_param_name(&self) -> Option<String> {
        let mut p = self.pos;
        while matches!(self.src.get(p), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            p += 1;
        }
        if self.src[p..].starts_with(b"...") {
            p += 3;
        } else if self.src.get(p) == Some(&b'&') {
            p += 1;
        }
        while matches!(self.src.get(p), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            p += 1;
        }
        if self.src.get(p) != Some(&b'$') {
            return None;
        }
        p += 1;
        let start = p;
        while self.src.get(p).is_some_and(|b| is_ident_continue(*b)) {
            p += 1;
        }
        if p == start {
            return None;
        }
        Some(String::from_utf8_lossy(&self.src[start..p]).into_owned())
    }

    /// Whether the next real token (skipping only plain ASCII whitespace -
    /// deliberately not a full trivia skip, since this only needs to
    /// distinguish "a parameter follows" from everything else) starts a
    /// parameter - a bare `$name`, a by-ref `&$name`, or a variadic
    /// `...$name` - as opposed to a return-type-gating position (a
    /// visibility keyword, `function`, or another attribute).
    fn next_parameter_anchor(&self) -> bool {
        let mut p = self.pos;
        while matches!(self.src.get(p), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            p += 1;
        }
        matches!(self.src.get(p), Some(b'$' | b'&')) || self.src[p..].starts_with(b"...")
    }
}
