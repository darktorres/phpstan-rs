//! A small DQL SELECT-item parser (PHASE_BACKLOG Phase 168 168c) -
//! scoped to exactly what a query's own result-row *shape* depends on:
//! `alias`/`alias.field`/`IDENTITY(alias.assoc)`, an aggregate function
//! call, `CASE ... END`, `NEW Some\Dto(...)`, each optionally `AS`-
//! aliased, and a bare `DISTINCT` modifier (shape-irrelevant - it
//! changes cardinality, never which columns exist). No WHERE/HAVING/
//! GROUP BY/ORDER BY grammar at all - 168b's own chain-walker never
//! collects those clauses' arguments in the first place, since none of
//! them can ever add or remove a result column.
//!
//! This is a hand-rolled tokenizer/parser over a single select-item
//! fragment's own text, not a reuse of this project's PHP `Lexer` - DQL
//! is Doctrine's own, unrelated grammar.
//!
//! A scalar subquery nested inside a select item (`SELECT (SELECT
//! COUNT(...) FROM ...) AS cnt`, PHASE_BACKLOG Phase 168 168k) recurses
//! into this exact same grammar via `parse_select_statement_tokens` -
//! not a second, parallel one - only an uncorrelated inner query (a
//! complete, self-contained `FROM` of its own) is modeled; a subquery
//! referencing an outer alias isn't recognized and falls through to the
//! ordinary "undecidable" outcome the same as any other unrecognized
//! construct.

/// One select item's own operand - a bare alias (the whole entity), a
/// dotted scalar field, or an `IDENTITY(...)` association reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldRef {
    Alias(String),
    Field(String, String),
    Identity(String, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AggregateFn {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

impl AggregateFn {
    fn from_name(name: &str) -> Option<AggregateFn> {
        match name.to_ascii_uppercase().as_str() {
            "COUNT" => Some(AggregateFn::Count),
            "SUM" => Some(AggregateFn::Sum),
            "AVG" => Some(AggregateFn::Avg),
            "MIN" => Some(AggregateFn::Min),
            "MAX" => Some(AggregateFn::Max),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectItemKind {
    Field(FieldRef),
    Aggregate {
        func: AggregateFn,
        /// `None` for a shape-irrelevant argument this project doesn't
        /// otherwise resolve (`COUNT(*)`, a literal) - 168d falls back to
        /// this function's own default type for the aggregate rather
        /// than failing the whole item.
        operand: Option<FieldRef>,
    },
    /// `CASE WHEN ... THEN <arm> ... ELSE <arm> END` - only a bare
    /// `FieldRef` arm is modeled (every real corpus shape this project
    /// has seen); a `WHEN` condition's own text is never inspected here
    /// at all, matching 168b's WHERE-clause exclusion (a condition can
    /// never affect row shape).
    Case {
        then_arms: Vec<FieldRef>,
        else_arm: Option<FieldRef>,
    },
    New(String),
    /// A function-call-shaped item whose name isn't one of the five
    /// built-in aggregates - a custom, project-registered DQL function
    /// (PHASE_BACKLOG Phase 168 168h).
    Custom(String),
    /// A scalar subquery nested inside a select item (`SELECT (SELECT
    /// COUNT(...) FROM ...) AS cnt`, PHASE_BACKLOG Phase 168 168k) - the
    /// inner query's own resolved chain/items, exactly the same shape
    /// `parse_select_statement` itself returns. Only valid DQL when the
    /// inner query resolves to exactly one scalar column -
    /// `result_shape`'s own `single_scalar_column_type` is reused
    /// unchanged for this, the identical requirement
    /// `getSingleColumnResult()`/`getSingleScalarResult()` (168f)
    /// already have.
    Subquery(
        Box<super::query_builder::QueryBuilderChain>,
        Vec<SelectItem>,
    ),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectItem {
    pub(crate) kind: SelectItemKind,
    pub(crate) alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Ident(String),
    /// A backslash-qualified name (`App\Dto`) - kept distinct from a
    /// plain `Ident` since only `NEW`'s own class-name position accepts
    /// one.
    QualifiedName(String),
    Dot,
    Comma,
    LParen,
    RParen,
    Star,
    /// Anything outside this grammar's own vocabulary (an operator, a
    /// string/numeric literal) - never valid in a select-item position
    /// (`parse_field_ref`/`parse_select_item_kind` reject it there,
    /// reporting the item undecidable), but must still tokenize
    /// successfully so a `WHEN`/`THEN` condition's own text - never
    /// interpreted, just skipped over - doesn't fail the whole select
    /// item's tokenization before parsing even begins.
    Other,
}

fn tokenize(s: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '.' => {
                tokens.push(Token::Dot);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            _ if c.is_ascii_alphabetic() || c == '_' || c == '\\' => {
                let start = i;
                let mut saw_backslash = c == '\\';
                i += 1;
                while i < bytes.len() {
                    let c2 = bytes[i] as char;
                    if c2.is_ascii_alphanumeric() || c2 == '_' {
                        i += 1;
                    } else if c2 == '\\' {
                        saw_backslash = true;
                        i += 1;
                    } else {
                        break;
                    }
                }
                let text = &s[start..i];
                tokens.push(if saw_backslash {
                    Token::QualifiedName(text.trim_start_matches('\\').to_string())
                } else {
                    Token::Ident(text.to_string())
                });
            }
            // A quoted string literal (real corpus shape: a `WHEN
            // t.status = 'active'`-style condition, never interpreted -
            // just consumed as one opaque token so it doesn't break
            // tokenization of the rest of the item).
            '\'' | '"' => {
                let quote = c;
                i += 1;
                while i < bytes.len() && bytes[i] as char != quote {
                    i += 1;
                }
                i += 1; // the closing quote, if present - EOF otherwise
                tokens.push(Token::Other);
            }
            // A numeric literal or any other operator character
            // (`=`/`<`/`>`/`+`/`-`/etc.) - never valid in a select-item
            // position itself, but real, ordinary text inside a `WHEN`
            // condition, which this grammar skips over without
            // interpreting.
            _ => {
                i += 1;
                tokens.push(Token::Other);
            }
        }
    }
    Some(tokens)
}

/// Splits `s` on every top-level comma (never one nested inside
/// parentheses) - `select('t', 'COUNT(x) AS c')`'s own two arguments are
/// already split by 168b, but a real `select('t, COUNT(x) AS c')` single
/// comma-joined argument is equally legal DQL and must split here.
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn eat_ident_ci(&mut self, word: &str) -> bool {
        match self.peek() {
            Some(Token::Ident(s)) if s.eq_ignore_ascii_case(word) => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    /// `alias`, `alias.field`, or `IDENTITY(alias.assoc)`.
    fn parse_field_ref(&mut self) -> Option<FieldRef> {
        if self.eat_ident_ci("IDENTITY") {
            if self.next() != Some(Token::LParen) {
                return None;
            }
            let (alias, field) = self.parse_dotted_path()?;
            if self.next() != Some(Token::RParen) {
                return None;
            }
            return Some(FieldRef::Identity(alias, field));
        }
        match self.next()? {
            Token::Ident(alias) => {
                if matches!(self.peek(), Some(Token::Dot)) {
                    self.pos += 1;
                    match self.next()? {
                        Token::Ident(field) => Some(FieldRef::Field(alias, field)),
                        _ => None,
                    }
                } else {
                    Some(FieldRef::Alias(alias))
                }
            }
            _ => None,
        }
    }

    fn parse_dotted_path(&mut self) -> Option<(String, String)> {
        let Token::Ident(alias) = self.next()? else {
            return None;
        };
        if self.next() != Some(Token::Dot) {
            return None;
        }
        let Token::Ident(field) = self.next()? else {
            return None;
        };
        Some((alias, field))
    }

    fn parse_select_item(&mut self) -> Option<SelectItem> {
        let kind = self.parse_select_item_kind()?;
        let alias = if self.eat_ident_ci("AS") {
            match self.next()? {
                Token::Ident(a) => Some(a),
                _ => return None,
            }
        } else {
            None
        };
        if self.pos != self.tokens.len() {
            // Trailing, unrecognized tokens - undecidable rather than
            // silently ignored.
            return None;
        }
        Some(SelectItem { kind, alias })
    }

    fn parse_select_item_kind(&mut self) -> Option<SelectItemKind> {
        // A parenthesized scalar subquery (168k) - checked before the
        // generic `DISTINCT`/`NEW`/`CASE`/function-call dispatch below,
        // none of which ever start with `(`.
        if matches!(self.peek(), Some(Token::LParen))
            && ident_ci(&self.tokens, self.pos + 1, "SELECT")
        {
            let (chain, items, end) = parse_select_statement_tokens(&self.tokens, self.pos + 1)?;
            if self.tokens.get(end) != Some(&Token::RParen) {
                return None;
            }
            self.pos = end + 1;
            return Some(SelectItemKind::Subquery(Box::new(chain), items));
        }
        if self.eat_ident_ci("DISTINCT") {
            // Shape-irrelevant, but must still be consumed - the real
            // operand follows it.
            return self.parse_select_item_kind();
        }
        if self.eat_ident_ci("NEW") {
            return match self.next()? {
                Token::QualifiedName(name) | Token::Ident(name) => {
                    if self.next() != Some(Token::LParen) {
                        return None;
                    }
                    // The constructor's own arguments don't affect the
                    // row shape (it's always exactly `Object(Dto, [])`),
                    // so they're skipped, not parsed - just balance
                    // parens to find where they end.
                    self.skip_balanced_parens()?;
                    Some(SelectItemKind::New(name))
                }
                _ => None,
            };
        }
        if self.eat_ident_ci("CASE") {
            return self.parse_case();
        }
        // `IDENTITY(...)` is field-ref-shaped (resolves to the target
        // association's own id type), not a function call - checked
        // before the generic function-call dispatch below, which would
        // otherwise treat it as an unrecognized custom function.
        if matches!(self.peek(), Some(Token::Ident(s)) if s.eq_ignore_ascii_case("IDENTITY")) {
            return self.parse_field_ref().map(SelectItemKind::Field);
        }
        // A function-call shape: `Ident(...)` - an aggregate if `Ident`
        // is one of the five built-ins, otherwise a custom function
        // (168h).
        if let Some(Token::Ident(name)) = self.peek().cloned()
            && self.tokens.get(self.pos + 1) == Some(&Token::LParen)
        {
            self.pos += 2;
            if let Some(func) = AggregateFn::from_name(&name) {
                // `DISTINCT` inside an aggregate call (`COUNT(DISTINCT
                // x)`) only changes cardinality, never `COUNT`'s own
                // result type - consumed, not tracked.
                self.eat_ident_ci("DISTINCT");
                let operand = if matches!(self.peek(), Some(Token::Star)) {
                    self.pos += 1;
                    None
                } else {
                    self.parse_field_ref()
                };
                if self.next() != Some(Token::RParen) {
                    return None;
                }
                return Some(SelectItemKind::Aggregate { func, operand });
            }
            self.skip_balanced_parens()?;
            return Some(SelectItemKind::Custom(name));
        }
        self.parse_field_ref().map(SelectItemKind::Field)
    }

    /// Consumes tokens up to and including the matching `)` for a
    /// `(` already consumed by the caller - used where this project
    /// doesn't need an argument list's own contents (a `NEW`
    /// constructor's args; a custom function's own args, resolved via
    /// 168h's config surface instead of parsed here).
    fn skip_balanced_parens(&mut self) -> Option<()> {
        let mut depth = 1i32;
        loop {
            match self.next()? {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(());
                    }
                }
                _ => {}
            }
        }
    }

    fn parse_case(&mut self) -> Option<SelectItemKind> {
        let mut then_arms = Vec::new();
        let mut else_arm = None;
        loop {
            if self.eat_ident_ci("WHEN") {
                // The condition itself never affects row shape - skip to
                // the matching `THEN` by balancing any parens it
                // contains, then require `THEN` immediately.
                while !self.eat_ident_ci("THEN") {
                    self.next()?;
                }
                then_arms.push(self.parse_field_ref()?);
            } else if self.eat_ident_ci("ELSE") {
                else_arm = Some(self.parse_field_ref()?);
            } else if self.eat_ident_ci("END") {
                break;
            } else {
                return None;
            }
        }
        Some(SelectItemKind::Case {
            then_arms,
            else_arm,
        })
    }
}

fn parse_one_select_item(fragment: &str) -> Option<SelectItem> {
    let tokens = tokenize(fragment.trim())?;
    let mut parser = Parser { tokens, pos: 0 };
    parser.parse_select_item()
}

/// Parses every `select()`/`addSelect()` argument (168b's own
/// `select_args`) into its real select items, splitting each argument on
/// its own top-level commas first (a single comma-joined argument is
/// equally legal DQL). `None` the moment any one item can't be parsed -
/// an unrecognized construct anywhere makes the whole query undecidable,
/// not just that one item.
pub(crate) fn parse_select_items(select_args: &[String]) -> Option<Vec<SelectItem>> {
    let mut items = Vec::new();
    for arg in select_args {
        for fragment in split_top_level_commas(arg) {
            if fragment.trim().is_empty() {
                continue;
            }
            items.push(parse_one_select_item(fragment)?);
        }
    }
    Some(items)
}

/// Splits `tokens` on every top-level `Comma` (never one nested inside
/// parentheses) - `split_top_level_commas`'s own token-level twin, used
/// by `parse_select_statement` below so a full DQL string's own select
/// list doesn't need a separate round trip back through raw text.
fn split_top_level_commas_tokens(tokens: &[Token]) -> Vec<&[Token]> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, t) in tokens.iter().enumerate() {
        match t {
            Token::LParen => depth += 1,
            Token::RParen => depth -= 1,
            Token::Comma if depth == 0 => {
                parts.push(&tokens[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&tokens[start..]);
    parts
}

fn ident_ci(tokens: &[Token], pos: usize, word: &str) -> bool {
    matches!(tokens.get(pos), Some(Token::Ident(s)) if s.eq_ignore_ascii_case(word))
}

/// The index of the first top-level (paren-depth 0) `Ident` token
/// matching `word`, searching from `start` - used to find the boundary
/// between the select-item list and `FROM`.
fn find_top_level_keyword(tokens: &[Token], start: usize, word: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, t) in tokens.iter().enumerate().skip(start) {
        match t {
            Token::LParen => depth += 1,
            Token::RParen => depth -= 1,
            Token::Ident(s) if depth == 0 && s.eq_ignore_ascii_case(word) => return Some(i),
            _ => {}
        }
    }
    None
}

/// A `FROM`/`JOIN` target's own qualified name (`App\Entity`) or dotted
/// alias path (`alias.assoc`), consumed starting at `pos` - returns the
/// joined text (matching 168b's own `JoinClause::target` format exactly,
/// so the same `result_shape` resolution logic applies unchanged) plus
/// the position just past it.
fn parse_path_text(tokens: &[Token], pos: usize) -> Option<(String, usize)> {
    let mut pos = pos;
    let first = match tokens.get(pos)? {
        Token::Ident(s) | Token::QualifiedName(s) => s.clone(),
        _ => return None,
    };
    pos += 1;
    if matches!(tokens.get(pos), Some(Token::Dot)) {
        pos += 1;
        let Some(Token::Ident(second)) = tokens.get(pos) else {
            return None;
        };
        let joined = format!("{first}.{second}");
        return Some((joined, pos + 1));
    }
    Some((first, pos))
}

const JOIN_BOUNDARY_KEYWORDS: [&str; 7] =
    ["INNER", "LEFT", "OUTER", "JOIN", "WHERE", "GROUP", "ORDER"];

fn at_join_boundary(tokens: &[Token], pos: usize) -> bool {
    JOIN_BOUNDARY_KEYWORDS
        .iter()
        .any(|kw| ident_ci(tokens, pos, kw))
        || ident_ci(tokens, pos, "HAVING")
}

/// A full `SELECT ... FROM Entity alias [JOIN ...]*` DQL statement -
/// PHASE_BACKLOG Phase 168 168g's own literal-`createQuery($dql)` entry
/// point, feeding a real DQL string directly into the same select-item
/// grammar 168b's own `QueryBuilder`-chain-derived arguments use, rather
/// than needing a second, parallel grammar. No WHERE/GROUP BY/HAVING/
/// ORDER BY parsing - stops at the first one, matching 168b/168c's own
/// identical exclusion (none of those clauses can ever add or remove a
/// result column). Returns a `QueryBuilderChain` with an empty
/// `select_args` (the select items are already parsed, returned
/// alongside it) so the exact same `result_shape` functions 168b's own
/// chain feeds apply here completely unchanged.
pub(crate) fn parse_select_statement(
    dql: &str,
) -> Option<(super::query_builder::QueryBuilderChain, Vec<SelectItem>)> {
    let tokens = tokenize(dql)?;
    let (chain, items, _end) = parse_select_statement_tokens(&tokens, 0)?;
    Some((chain, items))
}

/// `parse_select_statement`'s own real implementation, operating
/// directly on an already-tokenized buffer starting at `start` - shared
/// with a select item's own nested-subquery production (PHASE_BACKLOG
/// Phase 168 168k, `SelectItemKind::Subquery`) so the two never risk
/// drifting into two subtly different grammars. Returns the position
/// just past the parsed statement (its caller decides what, if
/// anything, legally follows - the top-level `dql: &str` entry point
/// requires nothing else; a subquery's own caller requires exactly a
/// closing `)`).
fn parse_select_statement_tokens(
    tokens: &[Token],
    start: usize,
) -> Option<(
    super::query_builder::QueryBuilderChain,
    Vec<SelectItem>,
    usize,
)> {
    if !ident_ci(tokens, start, "SELECT") {
        return None;
    }
    let from_idx = find_top_level_keyword(tokens, start + 1, "FROM")?;
    let mut items = Vec::new();
    for fragment in split_top_level_commas_tokens(&tokens[start + 1..from_idx]) {
        if fragment.is_empty() {
            continue;
        }
        let mut parser = Parser {
            tokens: fragment.to_vec(),
            pos: 0,
        };
        items.push(parser.parse_select_item()?);
    }

    let (from_entity, mut pos) = parse_path_text(tokens, from_idx + 1)?;
    let Some(Token::Ident(from_alias)) = tokens.get(pos) else {
        return None;
    };
    let from_alias = from_alias.clone();
    pos += 1;

    let mut joins = Vec::new();
    while ident_ci(tokens, pos, "INNER")
        || ident_ci(tokens, pos, "LEFT")
        || ident_ci(tokens, pos, "OUTER")
        || ident_ci(tokens, pos, "JOIN")
    {
        while ident_ci(tokens, pos, "INNER")
            || ident_ci(tokens, pos, "LEFT")
            || ident_ci(tokens, pos, "OUTER")
        {
            pos += 1;
        }
        if !ident_ci(tokens, pos, "JOIN") {
            return None;
        }
        pos += 1;
        let (target, next_pos) = parse_path_text(tokens, pos)?;
        pos = next_pos;
        let Some(Token::Ident(join_alias)) = tokens.get(pos) else {
            return None;
        };
        joins.push(super::query_builder::JoinClause {
            target,
            alias: join_alias.clone(),
        });
        pos += 1;
        // `WITH <condition>` - the condition never affects row shape
        // (same WHERE-clause exclusion as everywhere else in this
        // phase) - skipped by running to the next real join/terminating
        // keyword, or a closing `)` (a nested subquery's own own
        // terminator - `at_join_boundary` alone wouldn't stop there).
        // A condition's own comparison/literal tokens all tokenize as
        // `Token::Other`, never as an `Ident` this scan would mistake
        // for one of those keywords.
        if ident_ci(tokens, pos, "WITH") {
            pos += 1;
            while pos < tokens.len()
                && !at_join_boundary(tokens, pos)
                && tokens[pos] != Token::RParen
            {
                pos += 1;
            }
        }
    }

    Some((
        super::query_builder::QueryBuilderChain {
            from: Some((from_entity, from_alias)),
            joins,
            select_args: Vec::new(),
            dml: false,
        },
        items,
        pos,
    ))
}

#[cfg(test)]
mod tests;
