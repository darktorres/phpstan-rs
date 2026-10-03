//! Pure unit tests, no `Project`/I-O: the token-scan classifier, the
//! receiver-boundary scanner, the live-buffer backward name scanner,
//! and the auto-import decision. Full end-to-end coverage (probing a
//! real receiver type, the registry-backed bare candidate list) lives
//! in `src/lsp/tests/completion.rs`, driven through the real wire
//! protocol.

use super::*;

fn resolve_import_at_end(stmts: &[Stmt], fqn: &str) -> (String, Option<TextEdit>) {
    resolve_import(
        stmts,
        &LineIndex::default(),
        &context_at(stmts, u32::MAX),
        fqn,
    )
}

/// The byte offset right after the first `needle` in `text`.
fn pos_after(text: &str, needle: &str) -> u32 {
    (text.find(needle).unwrap() + needle.len()) as u32
}

fn parse(text: &str) -> Vec<Stmt> {
    let tokens = syntax::lexer::Lexer::new(text).tokenize_with_spans();
    syntax::parser::Parser::new(tokens).parse_program().unwrap()
}

#[test]
fn classify_finds_a_bare_arrow_trigger() {
    let src = "<?php $x->";
    let tokens = tokenize(src);
    match classify(&tokens, pos_after(src, "->")) {
        Trigger::Member {
            is_static: false, ..
        } => {}
        _ => panic!("expected a non-static Member trigger"),
    }
}

#[test]
fn classify_finds_a_bare_double_colon_trigger() {
    let src = "<?php Foo::";
    let tokens = tokenize(src);
    match classify(&tokens, pos_after(src, "::")) {
        Trigger::Member {
            is_static: true, ..
        } => {}
        _ => panic!("expected a static Member trigger"),
    }
}

#[test]
fn classify_finds_a_member_trigger_with_a_partial_name_already_typed() {
    let src = "<?php $x->fo";
    let tokens = tokenize(src);
    match classify(&tokens, pos_after(src, "fo")) {
        Trigger::Member {
            is_static: false, ..
        } => {}
        _ => panic!("expected a Member trigger even with a partial name typed"),
    }
}

#[test]
fn classify_is_bare_everywhere_else() {
    let src = "<?php $x = Fo";
    let tokens = tokenize(src);
    assert!(matches!(
        classify(&tokens, pos_after(src, "Fo")),
        Trigger::Bare
    ));
}

#[test]
fn receiver_start_idx_stops_at_a_preceding_semicolon() {
    let src = "<?php foo(); $x->";
    let tokens = tokenize(src);
    let Trigger::Member { anchor_idx, .. } = classify(&tokens, pos_after(src, "->")) else {
        panic!("expected a Member trigger");
    };
    let start = receiver_start_idx(&tokens, anchor_idx);
    let receiver = slice_span(
        src,
        Span {
            start: tokens[start].1.start,
            end: tokens[anchor_idx].1.start,
        },
    )
    .unwrap();
    assert_eq!(receiver.trim(), "$x");
}

#[test]
fn receiver_start_idx_stops_at_an_unmatched_opening_brace() {
    let src = "<?php if ($cond) { $x->";
    let tokens = tokenize(src);
    let Trigger::Member { anchor_idx, .. } = classify(&tokens, pos_after(src, "->")) else {
        panic!("expected a Member trigger");
    };
    let start = receiver_start_idx(&tokens, anchor_idx);
    let receiver = slice_span(
        src,
        Span {
            start: tokens[start].1.start,
            end: tokens[anchor_idx].1.start,
        },
    )
    .unwrap();
    assert_eq!(receiver.trim(), "$x");
}

#[test]
fn receiver_start_idx_keeps_a_multi_arg_call_receiver_whole() {
    let src = "<?php compute($a, $b)->";
    let tokens = tokenize(src);
    let Trigger::Member { anchor_idx, .. } = classify(&tokens, pos_after(src, "->")) else {
        panic!("expected a Member trigger");
    };
    let start = receiver_start_idx(&tokens, anchor_idx);
    let receiver = slice_span(
        src,
        Span {
            start: tokens[start].1.start,
            end: tokens[anchor_idx].1.start,
        },
    )
    .unwrap();
    assert_eq!(receiver.trim(), "compute($a, $b)");
}

#[test]
fn find_enclosing_names_finds_a_top_level_function() {
    let src = "<?php function helper() { $x->";
    let tokens = tokenize(src);
    let Trigger::Member { anchor_idx, .. } = classify(&tokens, pos_after(src, "->")) else {
        panic!("expected a Member trigger");
    };
    let (function_name, class_name) = find_enclosing_names(&tokens, anchor_idx);
    assert_eq!(function_name.as_deref(), Some("helper"));
    assert_eq!(class_name, None);
}

#[test]
fn find_enclosing_names_finds_a_method_and_its_class() {
    let src = "<?php class Foo { function bar() { $x->";
    let tokens = tokenize(src);
    let Trigger::Member { anchor_idx, .. } = classify(&tokens, pos_after(src, "->")) else {
        panic!("expected a Member trigger");
    };
    let (function_name, class_name) = find_enclosing_names(&tokens, anchor_idx);
    assert_eq!(function_name.as_deref(), Some("bar"));
    assert_eq!(class_name.as_deref(), Some("Foo"));
}

#[test]
fn find_enclosing_names_finds_nothing_at_top_level_script_scope() {
    let src = "<?php $x->";
    let tokens = tokenize(src);
    let Trigger::Member { anchor_idx, .. } = classify(&tokens, pos_after(src, "->")) else {
        panic!("expected a Member trigger");
    };
    let (function_name, class_name) = find_enclosing_names(&tokens, anchor_idx);
    assert_eq!(function_name, None);
    assert_eq!(class_name, None);
}

#[test]
fn find_enclosing_names_does_not_mistake_an_if_block_for_a_function() {
    let src = "<?php function helper() { if ($cond) { $x->";
    let tokens = tokenize(src);
    let Trigger::Member { anchor_idx, .. } = classify(&tokens, pos_after(src, "->")) else {
        panic!("expected a Member trigger");
    };
    let (function_name, class_name) = find_enclosing_names(&tokens, anchor_idx);
    assert_eq!(function_name.as_deref(), Some("helper"));
    assert_eq!(class_name, None);
}

#[test]
fn find_use_insertion_point_lands_after_the_last_use() {
    let stmts = parse("<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\nclass C {}\n");
    let point = find_use_insertion_point(&stmts);
    assert!(point.is_some());
}

#[test]
fn find_use_insertion_point_lands_after_a_bare_namespace_with_no_use() {
    let stmts = parse("<?php\nnamespace App;\nclass C {}\n");
    assert!(find_use_insertion_point(&stmts).is_some());
}

#[test]
fn find_use_insertion_point_is_none_with_neither_namespace_nor_use() {
    let stmts = parse("<?php\nclass C {}\n");
    assert!(find_use_insertion_point(&stmts).is_none());
}

#[test]
fn resolve_import_needs_no_edit_when_already_reachable_in_the_same_namespace() {
    let stmts = parse("<?php\nnamespace App;\nclass Other {}\n");
    let (insert_text, edit) = resolve_import_at_end(&stmts, "App\\Foo");
    assert_eq!(insert_text, "Foo");
    assert!(edit.is_none());
}

#[test]
fn resolve_import_needs_no_edit_when_already_use_imported() {
    let stmts = parse("<?php\nuse App\\Models\\User;\nclass Other {}\n");
    let (insert_text, edit) = resolve_import_at_end(&stmts, "App\\Models\\User");
    assert_eq!(insert_text, "User");
    assert!(edit.is_none());
}

#[test]
fn resolve_import_adds_a_use_edit_for_an_unimported_class() {
    let stmts = parse("<?php\nnamespace App;\nuse Other\\Thing;\nclass C {}\n");
    let (insert_text, edit) = resolve_import_at_end(&stmts, "Vendor\\Widget");
    assert_eq!(insert_text, "Widget");
    let edit = edit.expect("expected an auto-import edit");
    assert!(edit.new_text.contains("use Vendor\\Widget;"));
}

#[test]
fn resolve_import_falls_back_to_fully_qualified_on_a_short_name_collision() {
    let stmts = parse("<?php\nuse Vendor\\Widget;\nclass C {}\n");
    // A second, differently-namespaced `Widget` collides with the
    // already-imported one - must not silently shadow it with a second
    // `use` of the same short name.
    let (insert_text, edit) = resolve_import_at_end(&stmts, "Other\\Widget");
    assert_eq!(insert_text, "\\Other\\Widget");
    assert!(edit.is_none());
}
