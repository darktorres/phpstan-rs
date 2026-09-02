use std::collections::HashMap;

use crate::ast::*;
use crate::checker::{Checker, Locals, literal_string_value, purge_stale_narrowing_for};
use crate::registry::{FunctionSig, ParamSig};
use crate::token::Span;
use crate::types::{self, Type};

/// `check_args`'s own call bundle. `callee_label`/`label` are two
/// adjacent, identically-typed `&str` parameters (the callee's own
/// display name vs. the enclosing function/method being checked) - a
/// real mis-ordering risk a positional call site can't catch, which
/// named struct fields fix. `args` is the raw, as-written argument list
/// (needed for the two count-based checks `check_args` does, both of
/// which are about how many arguments were actually written, not which
/// parameter each one resolves to); `resolved` is Item 1's own per-
/// parameter resolution of that same list (`resolve_call_args`), used
/// for the "too few" check's own missing-required-parameter test
/// (correct for a named call that fills required parameters out of
/// order, where a raw argument *count* alone can't tell) and for the
/// per-argument type-checking loop.
pub(crate) struct CheckArgsCall<'a> {
    pub(crate) callee_label: &'a str,
    pub(crate) sig: &'a FunctionSig,
    pub(crate) args: &'a [Arg],
    pub(crate) resolved: &'a [Option<&'a Expr>],
    pub(crate) locals: &'a Locals,
    pub(crate) label: &'a str,
    pub(crate) defining_class: Option<&'a str>,
    pub(crate) span: Span,
}

impl<'a> Checker<'a> {
    /// A bare-variable argument that isn't defined yet is either a real bug
    /// or PHP's by-reference "output parameter" idiom
    /// (`preg_match($pattern, $subject, $matches)` defines `$matches` as a
    /// side effect). Without tracking every stdlib function's exact
    /// by-ref positions, the safe default for a call to an *unknown*
    /// function is to assume it might be exactly that and define the
    /// variable (as Mixed) rather than flag it; a *known* function only
    /// gets this leniency for the specific parameter position actually
    /// marked by-ref.
    /// `sig: None` (an unresolvable call) conservatively treats *any*
    /// bare-variable argument, in its own written position, as a
    /// definition site - the real reason this function exists at all
    /// (see its own callers' comments on `isset`/`empty`). Once a real
    /// signature is known, `resolved` (Item 1's own per-parameter
    /// resolution, already computed by the caller) is used instead of the
    /// raw argument list - a named or reordered-named by-ref argument
    /// needs to be matched against its *real* declared parameter, not
    /// whichever parameter happens to sit at the same index it was
    /// written at.
    pub(crate) fn predefine_byref_or_unknown_args(
        &self,
        sig: Option<&FunctionSig>,
        args: &[Arg],
        resolved: &[Option<&Expr>],
        locals: &mut Locals,
    ) {
        let Some(sig) = sig else {
            for arg in args {
                let ExprKind::Variable(name) = &arg.value.kind else {
                    continue;
                };
                locals.entry(name.clone()).or_insert(Type::Mixed);
            }
            return;
        };
        for (i, slot) in resolved.iter().enumerate() {
            let Some(arg_expr) = slot else {
                continue;
            };
            let ExprKind::Variable(name) = &arg_expr.kind else {
                continue;
            };
            // Both the by-ref flag and (Phase 22 Finding 3) the declared
            // parameter itself need the identical primary-then-variadic-
            // fallback lookup - `resolved` can extend past `sig.params`'
            // own length for a variadic tail argument (confirmed via
            // `resolve_call_args`'s own `resolved.push(...)` there).
            let param = sig
                .params
                .get(i)
                .or_else(|| sig.params.last().filter(|p| p.variadic));
            let Some(param) = param else {
                continue;
            };
            if !param.by_ref {
                continue;
            }
            if !locals.contains_key(name) {
                locals.insert(name.clone(), Type::Mixed);
                continue;
            }
            // Phase 22 Finding 3: an *already-defined* variable passed
            // by-ref was previously left untouched entirely (only an
            // undefined one got the "assume Mixed" leniency above) - a
            // real corpus example (`Piwigo\Admin\Install\InstallService::
            // installDbConnect(array &$infos, array &$errors, ...)`
            // called as `installDbConnect($infos, $errors, ...)` where
            // `$errors` was already `$errors = [];`) kept its stale,
            // narrow pre-call type through the call even though the
            // callee is known (by signature) to mutate it by reference.
            // Widen to the *declared parameter's own type*, not a
            // blanket `Type::Mixed` - `array &$errors` becomes
            // `Type::Array(Mixed)`, preserving the real, guaranteed-by-
            // the-type-hint information that it's still an array, which
            // blanket `Mixed` would needlessly discard. `substitute_
            // template` with empty bindings replaces any unresolved
            // `TemplateParam` in the declared type with `Mixed` too -
            // proper substitution would need the receiver's own type
            // arguments, not available at this call site, so an
            // unresolved generic by-ref parameter degrades to the same
            // safe `Mixed` the undefined-variable case already uses.
            //
            // Only trust the substituted declared type when it's itself
            // plainly array-shaped (`Type::Array`/`Type::ArrayShape`) -
            // found via real corpus verification, not assumed: several
            // real stdlib by-ref parameters (`array_walk(array|object
            // &$array, ...)`, confirmed in the vendored phpstorm-stubs)
            // declare a *union* wider than what the already-array-typed
            // variable being passed in could actually become - the
            // parameter type says what the callee *accepts*, not what an
            // already-narrower-typed argument turns into, so blindly
            // trusting a `Union`/`Object`/other declared shape here
            // overshoots into new noise once the corpus is checked, not
            // just less precision. Anything else falls back to `Mixed`,
            // exactly this function's own pre-existing behavior for the
            // undefined-variable case above.
            let substituted = types::substitute_template(&param.ty, &HashMap::new());
            let widened = match substituted {
                Type::Array(_) | Type::ArrayShape(_) => substituted,
                _ => Type::Mixed,
            };
            // Phase 33 Finding 5: a by-ref call argument is exactly as
            // much a "this variable now holds something else" event as a
            // direct assignment - without this, a stale composite key
            // rooted at `name` (from narrowing before this call) would
            // keep answering for whatever the callee may have replaced it
            // with.
            purge_stale_narrowing_for(locals, name);
            locals.insert(name.clone(), widened);
        }
    }

    /// Checks a closure/arrow-fn literal's body with `$this` seeded to
    /// `this_ty` instead of whatever (if anything) the enclosing scope's
    /// own `locals` would otherwise provide - used by `Closure::bind`/
    /// `->bindTo()` (Finding 49), both of which explicitly rebind `$this`
    /// independently of where the closure was lexically declared. A
    /// clone of `locals`, not a mutation of it: the seeded `$this` (and
    /// anything the closure's own body binds) must not leak back into the
    /// caller's real scope, matching how `ExprKind::Closure`'s own
    /// ordinary handling already keeps `closure_locals` separate.
    pub(crate) fn check_closure_literal_with_this(
        &mut self,
        closure_expr: &Expr,
        locals: &Locals,
        label: &str,
        this_ty: Type,
    ) {
        let mut seeded = locals.clone();
        seeded.insert("this".to_string(), this_ty);
        self.check_expr(closure_expr, &mut seeded, label);
    }

    pub(crate) fn check_args(&mut self, call: CheckArgsCall) {
        let CheckArgsCall {
            callee_label,
            sig,
            args,
            resolved,
            locals,
            label,
            defining_class,
            span,
        } = call;
        let required = sig
            .params
            .iter()
            .filter(|p| !p.variadic && !p.has_default)
            .count();
        let missing_required = resolved
            .iter()
            .take(required)
            .filter(|s| s.is_none())
            .count();
        if missing_required > 0 {
            self.report(
                label.to_string(),
                format!(
                    "{callee_label}() expects at least {required} argument(s), {} given",
                    args.len()
                ),
                span,
                0,
            );
        }

        // Finding 40: calling a non-variadic function/method with more
        // positional arguments than it can possibly accept is a real PHP
        // fatal error ("Too many arguments... expected exactly N"), just
        // as basic a check as the "too few" one right above it - but
        // nothing here caught it before, since the per-argument loop
        // below simply stops (via its own fallback once `sig.params.get(i)`
        // runs out and the last param isn't variadic) rather than
        // reporting anything. Uses the raw `args.len()` deliberately, not
        // `resolved`'s own length - a non-variadic signature's resolved
        // slice is always exactly `params.len()` long by
        // `resolve_call_args`'s own design, which would make this check
        // always read `false` if it read from that shape instead.
        let last_is_variadic = sig.params.last().is_some_and(|p| p.variadic);
        if !last_is_variadic && args.len() > sig.params.len() {
            let quantifier = if required == sig.params.len() {
                "exactly"
            } else {
                "at most"
            };
            self.report(
                label.to_string(),
                format!(
                    "{callee_label}() expects {quantifier} {} argument(s), {} given",
                    sig.params.len(),
                    args.len()
                ),
                span,
                0,
            );
        }

        for (i, slot) in resolved.iter().enumerate() {
            let Some(arg) = slot else {
                continue;
            };
            let param = match sig.params.get(i) {
                Some(p) => p,
                None => match sig.params.last() {
                    Some(p) if p.variadic => p,
                    _ => continue,
                },
            };
            let arg_ty = self.infer_expr(arg, locals);
            if arg_ty == Type::Mixed {
                continue;
            }
            let param_ty = match defining_class {
                Some(c) => self.resolve_pseudo(&param.ty, c),
                None => param.ty.clone(),
            };
            if !self.registry.is_assignable(&arg_ty, &param_ty) {
                self.report(
                    label.to_string(),
                    format!(
                        "{callee_label}() argument #{} (${}) expects {}, {} given",
                        i + 1,
                        param.name,
                        param_ty,
                        arg_ty
                    ),
                    arg.span,
                    5,
                );
            }
        }
    }

    /// Phase 12 item 3: validates a `sprintf`/`printf` call's format
    /// string against the arguments actually passed - only when the
    /// format string is a literal (mirrors `check_shape_key_access`'s
    /// own "only a literal, never a dynamic value" precedent) and
    /// `parse_format_specifiers` confidently understood every specifier
    /// in it (bails silently on anything it doesn't, e.g. a positional
    /// `%1$s` specifier). Two checks, both deliberately narrow to avoid
    /// false positives: too few arguments for the placeholders present
    /// (real PHP throws `ValueError` for this - unambiguously a bug,
    /// never intentional) - checked instead of "too many," since passing
    /// extra arguments is valid, unremarkable PHP that real `sprintf`
    /// just ignores; and an argument whose type couldn't possibly satisfy
    /// its specifier (an array, or an object for a numeric specifier).
    pub(crate) fn check_format_string_call(
        &mut self,
        name: &str,
        args: &[Arg],
        locals: &Locals,
        label: &str,
    ) {
        let Some(fmt_expr) = args.first() else {
            return;
        };
        let Some(fmt) = literal_string_value(&fmt_expr.value) else {
            return;
        };
        let Some(specs) = parse_format_specifiers(fmt) else {
            return;
        };
        let provided = &args[1..];
        if specs.len() > provided.len() {
            self.report(
                label.to_string(),
                format!(
                    "{name}() format string expects {} argument(s), {} given",
                    specs.len(),
                    provided.len()
                ),
                fmt_expr.value.span,
                0,
            );
            return;
        }
        for (spec, arg) in specs.iter().zip(provided) {
            let arg_ty = self.infer_expr(&arg.value, locals);
            if !format_specifier_accepts(*spec, &arg_ty) {
                self.report(
                    label.to_string(),
                    format!("{name}() format specifier %{spec} does not accept {arg_ty}"),
                    arg.value.span,
                    5,
                );
            }
        }
    }
}

/// PHP superglobals are implicitly defined in every scope without any
/// assignment ever appearing in source - undefined-variable detection must
/// never flag these.
/// Real PHP language constructs that are syntactically indistinguishable
/// from an ordinary function call in this parser (none of `isset`,
/// `empty`, `unset`, `eval`, `exit`, `die` are lexed as keywords - see
/// `token.rs` - so `isset($x)` parses as `ExprKind::Call(Name("isset"), ...)`
/// exactly like a real call). None of them are declared in any stub
/// source (they're compiler intrinsics, not functions - a real PHP
/// function can be called through a variable, `$f = 'isset'; $f();` is a
/// fatal error), so without this list every real codebase's constant use
/// of `isset`/`empty`/`unset` would be flagged as calling an undefined
/// function. Matched case-insensitively since PHP itself treats function
/// (and construct) names case-insensitively.
pub(crate) fn is_language_construct(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "isset"
            | "empty"
            | "unset"
            | "eval"
            | "exit"
            | "die"
            // The parser deliberately models these four as a call to a
            // synthetic name (see parser.rs's own comment on that
            // decision) rather than a dedicated AST node - real language
            // constructs, not functions, so they belong on this list for
            // exactly the same reason as isset/empty/unset above.
            | "include"
            | "include_once"
            | "require"
            | "require_once"
            // Same synthetic-call modeling, for the same reason.
            | "print"
    )
}

/// Parses a `sprintf`/`printf`-style format string into its ordered list
/// of conversion specifiers (`%d`, `%s`, ...), skipping `%%` (a literal
/// percent, not a placeholder). Returns `None` - "don't know, don't
/// guess" - for anything this doesn't confidently understand: a
/// positional specifier (`%1$s`, real and common but out of scope for
/// this pass, since a lone specifier count no longer maps 1:1 to argument
/// count once positions can repeat or skip), an unknown specifier
/// character, or a malformed/truncated `%` sequence. `%b`/`%c`/`%d`/`%e`/
/// `%E`/`%f`/`%F`/`%g`/`%G`/`%o`/`%s`/`%u`/`%x`/`%X` are real PHP
/// specifier characters (confirmed against the language's own
/// documented format).
pub(crate) fn parse_format_specifiers(fmt: &str) -> Option<Vec<char>> {
    let bytes = fmt.as_bytes();
    let mut specs = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        let &next = bytes.get(i)?;
        if next == b'%' {
            i += 1;
            continue;
        }
        // A positional prefix (`%1$s`) is digits immediately followed by
        // `$` - checked before flags/width, since a bare width (`%5d`)
        // is also just digits and must NOT be mistaken for one.
        let digits_start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if bytes.get(i) == Some(&b'$') {
            return None;
        }
        i = digits_start;
        while bytes
            .get(i)
            .is_some_and(|b| matches!(b, b'-' | b'+' | b' ' | b'0'))
        {
            i += 1;
        }
        if bytes.get(i) == Some(&b'\'') {
            i += 2; // custom padding char: the quote plus the pad character itself
        }
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if bytes.get(i) == Some(&b'.') {
            i += 1;
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        let &spec = bytes.get(i)?;
        if !b"bcdeEfFgGosuxX".contains(&spec) {
            return None;
        }
        specs.push(spec as char);
        i += 1;
    }
    Some(specs)
}

/// Whether a real PHP conversion specifier could plausibly accept
/// `arg_ty` - deliberately lenient (PHP casts numeric strings and floats
/// for `%d`, and almost anything with a `__toString` for `%s`), so this
/// only flags the unambiguous cases: an array (or a known array shape)
/// for any specifier, or an object for a numeric one - both produce
/// either a fatal error or the literal, useless string `"Array"` in real
/// PHP, never a legitimate value.
pub(crate) fn format_specifier_accepts(spec: char, arg_ty: &Type) -> bool {
    match spec {
        'b' | 'c' | 'd' | 'e' | 'E' | 'f' | 'F' | 'g' | 'G' | 'o' | 'u' | 'x' | 'X' => !matches!(
            arg_ty,
            Type::Array(_) | Type::ArrayShape(_) | Type::Object(_, _)
        ),
        's' => !matches!(arg_ty, Type::Array(_) | Type::ArrayShape(_)),
        _ => true,
    }
}

/// Item 1: resolves a call's own argument list against the declared
/// parameter list it's being checked against, producing one slot per
/// declared parameter - `Some(&Expr)` when some argument (positional or
/// named) actually fills that parameter, `None` when it doesn't (an
/// omitted optional parameter, or anything past the first spread
/// argument, whose real contents can't be resolved statically at all).
/// Every consumer that used to assume `args[i]` lines up with
/// `params[i]` - true only for the overwhelmingly common all-positional
/// case - now goes through this first, so a named or reordered-named
/// call resolves against the *real* parameter, not whichever one happens
/// to sit at the same index in the written argument list.
///
/// The returned `Vec` can be *longer* than `params.len()` when the last
/// parameter is variadic: real PHP has no way to fill a variadic
/// parameter by name (there's no single name that means "the rest of the
/// arguments"), so once a variadic parameter is in play, any further
/// positional argument is necessarily extra, appended past the initial
/// `params.len()`-sized block - the same shape `check_args`'s own
/// existing variadic fallback (`sig.params.last()` when `i` runs past
/// `params.len()`) already expects.
///
/// A positional argument fills the next open slot by declaration order,
/// tracked via a running count of positional arguments seen so far (not
/// "the next unfilled slot overall" - the two only diverge for a
/// positional argument written *after* a named one, which real PHP
/// itself forbids as a parse-time error; this project's own parser
/// doesn't enforce that ordering, so such input could reach here anyway,
/// but only needs to avoid panicking on it, not produce a meaningful
/// result for syntactically-invalid PHP).
pub(crate) fn resolve_call_args<'a>(args: &'a [Arg], params: &[ParamSig]) -> Vec<Option<&'a Expr>> {
    let mut resolved: Vec<Option<&Expr>> = vec![None; params.len()];
    let mut positional_index = 0usize;
    for arg in args {
        if arg.spread {
            break;
        }
        if let Some(name) = &arg.name {
            if let Some(pos) = params.iter().position(|p| &p.name == name) {
                resolved[pos] = Some(&arg.value);
            }
            continue;
        }
        if positional_index < resolved.len() {
            resolved[positional_index] = Some(&arg.value);
        } else if params.last().is_some_and(|p| p.variadic) {
            resolved.push(Some(&arg.value));
        }
        positional_index += 1;
    }
    resolved
}
