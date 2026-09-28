//! The type-access ratchet: every path naming a `types::Type` variant
//! outside `crates/types` (a pattern, a construction, a macro argument),
//! counted per file against `gates/type-access.txt`. A count may only
//! fall, and a fall is blessed into the file so it can't rise back.

use std::collections::BTreeMap;
use std::path::Path;

use syn::parse::{ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::visit::Visit;

use crate::modgraph::walk_dir;

const BASELINE: &str = "gates/type-access.txt";

/// `(path, count)` for every file outside `crates/types` naming a variant.
fn count(root: &Path) -> Result<BTreeMap<String, usize>, String> {
    let variants = variant_names(root)?;
    let mut files = Vec::new();
    walk_dir(&root.join("crates"), &mut files)?;
    walk_dir(&root.join("xtask"), &mut files)?;
    files.sort();
    let mut counts = BTreeMap::new();
    for path in &files {
        if path.starts_with(root.join("crates/types")) {
            continue;
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let ast = syn::parse_file(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut counter = Counter {
            variants: &variants,
            found: 0,
        };
        counter.visit_file(&ast);
        if counter.found > 0 {
            let shown = path.strip_prefix(root).unwrap_or(path);
            counts.insert(shown.display().to_string(), counter.found);
        }
    }
    Ok(counts)
}

/// The variant names of `pub enum Type` in `crates/types/src/lib.rs`.
fn variant_names(root: &Path) -> Result<Vec<String>, String> {
    let lib = root.join("crates/types/src/lib.rs");
    let text = std::fs::read_to_string(&lib).map_err(|e| format!("{}: {e}", lib.display()))?;
    let ast = syn::parse_file(&text).map_err(|e| format!("{}: {e}", lib.display()))?;
    ast.items
        .iter()
        .find_map(|item| match item {
            syn::Item::Enum(e) if e.ident == "Type" => {
                Some(e.variants.iter().map(|v| v.ident.to_string()).collect())
            }
            _ => None,
        })
        .ok_or_else(|| format!("{}: no `enum Type`", lib.display()))
}

struct Counter<'v> {
    variants: &'v [String],
    found: usize,
}

impl<'ast> Visit<'ast> for Counter<'_> {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        let mut tail = path.segments.iter().rev();
        if let (Some(variant), Some(ty)) = (tail.next(), tail.next())
            && ty.ident == "Type"
            && self.variants.iter().any(|v| variant.ident == v)
        {
            self.found += 1;
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let tokens = mac.tokens.clone();
        if mac.path.is_ident("matches") {
            if let Ok((expr, pat, guard)) = Parser::parse2(parse_matches, tokens) {
                self.visit_expr(&expr);
                self.visit_pat(&pat);
                if let Some(guard) = guard {
                    self.visit_expr(&guard);
                }
            }
        } else if let Ok(args) = Parser::parse2(
            Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
            tokens.clone(),
        ) {
            for arg in &args {
                self.visit_expr(arg);
            }
        } else if let Ok((value, len)) = Parser::parse2(parse_repeat, tokens) {
            self.visit_expr(&value);
            self.visit_expr(&len);
        }
        syn::visit::visit_macro(self, mac);
    }
}

/// `matches!(expr, pat)` or `matches!(expr, pat if guard)`.
fn parse_matches(input: ParseStream) -> syn::Result<(syn::Expr, syn::Pat, Option<syn::Expr>)> {
    let expr: syn::Expr = input.parse()?;
    input.parse::<syn::Token![,]>()?;
    let pat = syn::Pat::parse_multi_with_leading_vert(input)?;
    let guard = if input.parse::<Option<syn::Token![if]>>()?.is_some() {
        Some(input.parse()?)
    } else {
        None
    };
    input.parse::<Option<syn::Token![,]>>()?;
    Ok((expr, pat, guard))
}

/// `vec![value; len]`.
fn parse_repeat(input: ParseStream) -> syn::Result<(syn::Expr, syn::Expr)> {
    let value = input.parse()?;
    input.parse::<syn::Token![;]>()?;
    Ok((value, input.parse()?))
}

fn read_baseline(root: &Path) -> Result<BTreeMap<String, usize>, String> {
    let path = root.join(BASELINE);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (n, file) = l
                .split_once('\t')
                .ok_or_else(|| format!("{BASELINE}: bad line `{l}`"))?;
            let n = n
                .parse()
                .map_err(|_| format!("{BASELINE}: bad count in `{l}`"))?;
            Ok((file.to_string(), n))
        })
        .collect()
}

fn render(counts: &BTreeMap<String, usize>) -> String {
    let total: usize = counts.values().sum();
    let mut out = format!(
        "# Paths naming a `types::Type` variant outside crates/types, per file.\n\
         # A count may only fall; `cargo xtask arch --bless-types` records a fall.\n\
         # total {total}\n"
    );
    for (file, n) in counts {
        out.push_str(&format!("{n}\t{file}\n"));
    }
    out
}

/// Rewrites the baseline from the current source.
pub(crate) fn bless(root: &Path) -> Result<(), String> {
    let counts = count(root)?;
    let path = root.join(BASELINE);
    std::fs::write(&path, render(&counts)).map_err(|e| format!("{}: {e}", path.display()))
}

/// The violations: a file whose count rose, or fell without a bless.
pub(crate) fn check(root: &Path) -> Result<Vec<String>, String> {
    let counts = count(root)?;
    let baseline = read_baseline(root)?;
    let mut violations = Vec::new();
    let files: std::collections::BTreeSet<&String> = counts.keys().chain(baseline.keys()).collect();
    for file in files {
        let now = counts.get(file).copied().unwrap_or(0);
        let was = baseline.get(file).copied().unwrap_or(0);
        if now > was {
            violations.push(format!(
                "{file}: {now} paths name a `Type` variant, {was} allowed - ask the type through a method"
            ));
        } else if now < was {
            violations.push(format!(
                "{file}: {now} paths name a `Type` variant, down from {was} - run `cargo xtask arch --bless-types`"
            ));
        }
    }
    Ok(violations)
}
