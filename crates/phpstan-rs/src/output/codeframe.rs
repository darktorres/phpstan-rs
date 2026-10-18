//! The `friendly` format: every diagnostic with a frame of the source
//! lines around it, the reported span underlined, and (on a colour
//! terminal) the code syntax-highlighted.

use rustc_hash::FxHashMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};

use diagnostics::Diagnostic;
use syntax::lexer::Lexer;
use syntax::span::{LineCol, LineIndex};
use syntax::token::Token;

const CONTEXT_LINES: u32 = 2;

const RESET: &str = "\x1b[0m";
const BOLD_RED: &str = "\x1b[1;31m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";
const GRAY: &str = "\x1b[90m";

/// `(first column, last column exclusive or `None` for end of line,
/// colour)` per line.
type Highlights = FxHashMap<u32, Vec<(u32, Option<u32>, &'static str)>>;

fn token_colour(token: &Token) -> Option<&'static str> {
    match token {
        Token::Keyword(_) => Some(MAGENTA),
        Token::Variable(_) => Some(CYAN),
        Token::Integer(_) | Token::Float(_) => Some(YELLOW),
        Token::StringLit(_) | Token::InterpString(_) | Token::ShellExec(_) => Some(GREEN),
        Token::DocComment(_) => Some(GRAY),
        _ => None,
    }
}

fn add_range(highlights: &mut Highlights, start: LineCol, end: LineCol, colour: &'static str) {
    for line in start.line..=end.line {
        let from = if line == start.line { start.col } else { 1 };
        let to = (line == end.line).then_some(end.col);
        highlights.entry(line).or_default().push((from, to, colour));
    }
}

fn highlights_for(source: &str) -> Highlights {
    let mut highlights = Highlights::default();
    let (tokens, comments, _) = Lexer::new(source).tokenize_with_comments();
    let index = LineIndex::new(source.as_bytes());
    for (token, span, _) in &tokens {
        if let Some(colour) = token_colour(token) {
            let range = index.range(*span);
            add_range(&mut highlights, range.start, range.end, colour);
        }
    }
    for (span, _) in &comments {
        let range = index.range(*span);
        add_range(&mut highlights, range.start, range.end, GRAY);
    }
    highlights
}

fn paint(line_number: u32, text: &str, highlights: &Highlights) -> String {
    let Some(ranges) = highlights.get(&line_number) else {
        return text.to_string();
    };
    let mut out = String::new();
    let mut current: Option<&str> = None;
    for (i, c) in text.chars().enumerate() {
        let col = i as u32 + 1;
        let colour = ranges
            .iter()
            .rev()
            .find(|(from, to, _)| col >= *from && to.is_none_or(|t| col < t))
            .map(|(_, _, colour)| *colour);
        if colour != current {
            out.push_str(RESET);
            if let Some(colour) = colour {
                out.push_str(colour);
            }
            current = colour;
        }
        out.push(c);
    }
    if current.is_some() {
        out.push_str(RESET);
    }
    out
}

struct Painter {
    colour: bool,
}

impl Painter {
    fn wrap(&self, code: &str, text: &str) -> String {
        if self.colour {
            format!("{code}{text}{RESET}")
        } else {
            text.to_string()
        }
    }
}

fn frame(
    out: &mut String,
    lines: &[&str],
    highlights: Option<&Highlights>,
    d: &Diagnostic,
    painter: &Painter,
) {
    let line = d.span.start.line;
    if line == 0 || line as usize > lines.len() {
        return;
    }
    let first = line.saturating_sub(CONTEXT_LINES).max(1);
    let last = (line + CONTEXT_LINES).min(lines.len() as u32);
    let width = last.to_string().len();
    for n in first..=last {
        let text = lines[(n - 1) as usize];
        let code = match highlights {
            Some(h) => paint(n, text, h),
            None => text.to_string(),
        };
        let marker = if n == line { ">" } else { " " };
        let gutter = format!("{marker} {n:>width$} | ");
        let gutter = if n == line {
            painter.wrap(BOLD_RED, &gutter)
        } else {
            painter.wrap(DIM, &gutter)
        };
        let _ = writeln!(out, "  {gutter}{code}");
        if n == line {
            let start = d.span.start.col.max(1);
            let length = if d.span.end.line == line && d.span.end.col > start {
                d.span.end.col - start
            } else {
                1
            };
            let underline = format!(
                "{}{}",
                " ".repeat((start - 1) as usize),
                "^".repeat(length as usize)
            );
            let blank = painter.wrap(DIM, &format!("  {:>width$} | ", ""));
            let _ = writeln!(out, "  {blank}{}", painter.wrap(RED, &underline));
        }
    }
}

/// Renders `results` in the `friendly` format; `colour` adds ANSI colours
/// and syntax highlighting. Source text is read from each file's path.
pub fn render_friendly(results: &FxHashMap<PathBuf, Vec<Diagnostic>>, colour: bool) -> String {
    let painter = Painter { colour };
    let mut paths: Vec<&PathBuf> = results.keys().filter(|p| !results[*p].is_empty()).collect();
    paths.sort();
    let mut out = String::new();
    for path in paths {
        let source = std::fs::read_to_string(Path::new(path)).ok();
        let lines: Vec<&str> = source
            .as_deref()
            .map(|s| syntax::span::lines(s).collect())
            .unwrap_or_default();
        let highlights = if colour && path.extension().is_some_and(|e| e == "php") {
            source.as_deref().map(highlights_for)
        } else {
            None
        };
        for d in &results[path] {
            let _ = writeln!(
                out,
                " {}",
                painter.wrap(BOLD_RED, &format!("✖ {}", d.message))
            );
            let _ = writeln!(
                out,
                "  {}",
                painter.wrap(
                    DIM,
                    &format!(
                        "{}:{}:{} · {}",
                        path.display(),
                        d.span.start.line,
                        d.span.start.col,
                        d.identifier
                    )
                )
            );
            frame(&mut out, &lines, highlights.as_ref(), d, &painter);
            if let Some(tip) = d.tip_text() {
                let _ = writeln!(out, "  💡 {tip}");
            }
            out.push('\n');
        }
    }
    out
}
