//! A body kept as text is read as the statements it was parsed from give:
//! with the names in effect where it is declared, through the cache's
//! form, and from a file that is not UTF-8.

use super::super::*;

/// The bodies of `src`, as a context file's are kept, and as the file's
/// statements give them.
fn both(src: &[u8]) -> (FileBodies, FileBodies) {
    let parsed = syntax::parse(src).expect("the source parses");
    let skimmed = FileBodies::of_file(
        &parsed.stmts,
        &parsed.facts,
        Some(syntax::Source::Bytes(src)),
    );
    (skimmed, FileBodies::of(&parsed.stmts))
}

fn assert_same(skimmed: &FileBodies, eager: &FileBodies) {
    assert_eq!(skimmed.spans(), eager.spans());
    for span in eager.spans() {
        assert_eq!(
            skimmed.summary(span).map(|summary| format!("{summary:?}")),
            eager.summary(span).map(|summary| format!("{summary:?}")),
        );
        assert_eq!(
            skimmed.init_flow(span).map(|flow| format!("{flow:?}")),
            eager.init_flow(span).map(|flow| format!("{flow:?}")),
        );
    }
}

const NAMESPACED: &[u8] = b"<?php
namespace App\\One {
    use function Lib\\helper;
    use Lib\\Tool as T;
    class Box {
        private int $n = 0;
        public function set(int $n): void { $this->n = $n; helper($n); }
        public function make(): T { return new T(); }
        public function again(): void {
            top:
            if ($this->n > 0) { $this->n--; goto top; }
        }
    }
    function free(): int { return strlen('a') + helper(1); }
}
namespace App\\Two {
    use function Other\\helper;
    function free(): int { return helper(2); }
}
";

#[test]
fn bodies_are_kept_as_text_and_read_as_the_tree_gives_them() {
    let (skimmed, eager) = both(NAMESPACED);
    assert!(!eager.spans().is_empty());
    assert!(
        eager
            .spans()
            .into_iter()
            .all(|span| skimmed.is_skimmed(span))
    );
    assert_same(&skimmed, &eager);
}

#[test]
fn the_names_in_effect_where_a_body_is_declared_are_the_names_it_is_read_with() {
    let (skimmed, _) = both(NAMESPACED);
    let text = String::from_utf8_lossy(NAMESPACED);
    let summaries: Vec<String> = skimmed
        .spans()
        .into_iter()
        .map(|span| format!("{:?}", skimmed.summary(span).expect("read")))
        .collect();
    assert!(
        summaries.iter().any(|s| s.contains("Lib\\\\helper")),
        "{text}"
    );
    assert!(
        summaries.iter().any(|s| s.contains("Other\\\\helper")),
        "{text}"
    );
}

#[test]
fn bodies_stored_as_text_are_read_again_from_the_files_bytes() {
    let (skimmed, eager) = both(NAMESPACED);
    let stored = postcard::to_allocvec(&skimmed).expect("encodes");
    let decoded: FileBodies = postcard::from_bytes(&stored).expect("decodes");
    assert_eq!(
        postcard::to_allocvec(&decoded).expect("encodes"),
        stored,
        "what is decoded is stored as it was"
    );
    let decoded = decoded.with_source(NAMESPACED);
    assert!(
        eager
            .spans()
            .into_iter()
            .all(|span| decoded.is_skimmed(span))
    );
    assert_same(&decoded, &eager);
}

#[test]
fn a_body_is_stored_as_text_whether_or_not_it_was_read() {
    let (skimmed, _) = both(NAMESPACED);
    let before = postcard::to_allocvec(&skimmed).expect("encodes");
    for span in skimmed.spans() {
        skimmed.summary(span);
    }
    assert_eq!(postcard::to_allocvec(&skimmed).expect("encodes"), before);
}

#[test]
fn a_file_that_is_not_utf_8_is_read_as_its_bytes_are() {
    let mut src = b"<?php function f(): string { return \"\xff\xfe\"; }\nfunction g(): int { return f() === '' ? 1 : 2; }".to_vec();
    src.extend_from_slice(b"\n// \xc3\n");
    let (skimmed, eager) = both(&src);
    assert!(
        eager
            .spans()
            .into_iter()
            .all(|span| skimmed.is_skimmed(span))
    );
    assert_same(&skimmed, &eager);
    let decoded: FileBodies =
        postcard::from_bytes(&postcard::to_allocvec(&skimmed).expect("encodes")).expect("decodes");
    assert_same(&decoded.with_source(&src), &eager);
}

#[test]
fn a_file_with_a_declaration_in_a_body_keeps_every_body_read() {
    for src in [
        &b"<?php function outer() { function inner() { return 1; } return inner(); }"[..],
        b"<?php class A { function m() { class B {} } }",
        b"<?php function f() { return new class { function g() {} }; }",
        b"<?php function f() { return function () { function h() {} }; }",
    ] {
        let (skimmed, eager) = both(src);
        assert!(
            eager
                .spans()
                .into_iter()
                .all(|span| !skimmed.is_skimmed(span)),
            "{}",
            String::from_utf8_lossy(src)
        );
        assert_same(&skimmed, &eager);
    }
}
