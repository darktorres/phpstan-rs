//! Ids as git computes them, how a text is cut and that it comes back
//! whole, and a run given a reference.

use std::io::Write;
use std::path::PathBuf;

use crate::split::{Emit, Splitter};
use crate::tree::{Mode, TreeBuilder, TreeEntry, tree_id};
use crate::{
    Difference, Dump, DumpWriter, FileEntity, Id, Kind, Manifest, escape, first_difference,
    manifest_path, trees,
};

struct Collected(Vec<(String, Vec<u8>)>);

impl Emit for Collected {
    fn entity(&mut self, path: &str, text: &[u8]) -> std::io::Result<()> {
        let Collected(entities) = self;
        entities.push((path.to_string(), text.to_vec()));
        Ok(())
    }
}

/// `text` cut as `kind`, given to the splitter `piece` bytes at a time.
fn cut(kind: Kind, text: &[u8], piece: usize) -> Vec<(String, Vec<u8>)> {
    let mut splitter = Splitter::new(kind).unwrap();
    let mut collected = Collected(Vec::new());
    for piece in text.chunks(piece) {
        splitter.push(piece, &mut collected).unwrap();
    }
    splitter.finish(&mut collected).unwrap();
    collected.0
}

fn paths(entities: &[(String, Vec<u8>)]) -> Vec<&str> {
    entities.iter().map(|(path, _)| path.as_str()).collect()
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "phpstan_rs_dumps_test_{label}_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn blob(name: &str, text: &[u8]) -> TreeEntry {
    TreeEntry {
        name: name.to_string(),
        mode: Mode::Blob,
        id: Id::blob(text),
    }
}

#[test]
fn ids_are_git_s_in_its_sha256_object_format() {
    // `git hash-object` and `git mktree` in a repository made with
    // `--object-format=sha256`.
    assert_eq!(
        Id::blob(b"hello\n").to_string(),
        "2cf8d83d9ee29543b34a87727421fdecb7e3f3a183d337639025de576db9ebb4"
    );
    assert_eq!(
        tree_id(&mut [blob("a", b"c"), blob(".0", b"ab")]).to_string(),
        "d298ee862b296669549863468675ebf61e7b6b8be2c633ea808130d97c0bb610"
    );
    let sub = tree_id(&mut [blob("z", b"x\n")]);
    assert_eq!(
        sub.to_string(),
        "9a968a7649fe11b6955662f2872eef2461613a883d1f283a478e2dfbf3883961"
    );
    let top = tree_id(&mut [
        blob("a0", b"x\n"),
        TreeEntry {
            name: "a".to_string(),
            mode: Mode::Tree,
            id: sub,
        },
        blob("a.b", b"y"),
        blob("%2Egit", b"y"),
    ]);
    assert_eq!(
        top.to_string(),
        "0459b5d97dec099a407420e36b81ce08b9e7e42a267cd3b631c9caed519f2b3b"
    );
    assert_eq!(top.to_string().parse::<Id>().unwrap(), top);
    assert!("0459".parse::<Id>().is_err());
}

#[test]
fn a_key_escapes_to_one_name_git_takes_as_it_is() {
    for (key, name) in [
        (&b""[..], "%"),
        (b"Foo\\Bar", "Foo\\Bar"),
        (b"'a.php'", "'a.php'"),
        (b"src/a.php", "src%2Fa.php"),
        (b".git", "%2Egit"),
        (b"..", "%2E."),
        (b"a~2", "a%7E2"),
        (b"100%", "100%25"),
        (b"\"q\"", "%22q%22"),
        (b"a\tb\n", "a%09b%0A"),
        (b"\xA9t\xC3\xA9", "%A9t\u{e9}"),
        (b"a b", "a b"),
    ] {
        assert_eq!(escape(key), name, "{key:?}");
    }
}

const REGISTRY: &str = "
== files
[
  'a.php' => File {
    role: 'analysed'
  }
  'b.php' => File {
    role: 'context'
  }
]

== dependents
[]

== registry over stub-base
Registry {
  classes: Over {
    changed: [
      Foo => ClassReflection {
        name: Foo
      }
      Bar => 1
    ]
    dropped: [
      Baz
    ]
  }
  functions: []
  constants: [
    A => 1
    B => 2
  ]
}

== project
Project {
  working_dir: '.'
  finished: true
}
";

#[test]
fn a_registry_dump_is_cut_at_its_sections_fields_entries_and_files() {
    let entities = cut(Kind::Tree, REGISTRY.as_bytes(), 1 << 16);
    let texts: Vec<(&str, &str)> = entities
        .iter()
        .map(|(path, text)| (path.as_str(), std::str::from_utf8(text).unwrap()))
        .collect();
    assert_eq!(
        texts,
        [
            (".0", "\n"),
            ("files/.0", "== files\n"),
            ("files/.1", "[\n"),
            (
                "files/'a.php'",
                "  'a.php' => File {\n    role: 'analysed'\n  }\n"
            ),
            (
                "files/'b.php'",
                "  'b.php' => File {\n    role: 'context'\n  }\n"
            ),
            ("files/.2", "]\n\n"),
            ("dependents", "== dependents\n[]\n\n"),
            ("registry over stub-base/.0", "== registry over stub-base\n"),
            ("registry over stub-base/.1", "Registry {\n"),
            ("registry over stub-base/classes/.0", "  classes: Over {\n"),
            (
                "registry over stub-base/classes/changed/.0",
                "    changed: [\n"
            ),
            (
                "registry over stub-base/classes/changed/Foo",
                "      Foo => ClassReflection {\n        name: Foo\n      }\n"
            ),
            (
                "registry over stub-base/classes/changed/Bar",
                "      Bar => 1\n"
            ),
            ("registry over stub-base/classes/changed/.1", "    ]\n"),
            (
                "registry over stub-base/classes/dropped",
                "    dropped: [\n      Baz\n    ]\n"
            ),
            ("registry over stub-base/classes/.1", "  }\n"),
            ("registry over stub-base/functions", "  functions: []\n"),
            (
                "registry over stub-base/constants",
                "  constants: [\n    A => 1\n    B => 2\n  ]\n"
            ),
            ("registry over stub-base/.2", "}\n\n"),
            (
                "project",
                "== project\nProject {\n  working_dir: '.'\n  finished: true\n}\n"
            ),
        ]
    );
}

#[test]
fn an_item_is_named_by_its_first_field_and_a_repeated_name_is_told_apart() {
    let text = "== s\n[\n  Node {\n    name: A\n  }\n  Node {\n    name: A\n  }\n  plain\n  plain\n  'x => y' => z\n]";
    assert_eq!(
        paths(&cut(Kind::Tree, text.as_bytes(), 5)),
        [
            "s/.0",
            "s/.1",
            "s/A",
            "s/A~2",
            "s/plain",
            "s/plain~2",
            "s/'x => y'",
            "s/.2"
        ]
    );
}

#[test]
fn a_section_or_a_field_none_of_whose_parts_spans_lines_is_one_entity() {
    let text = "== files\nanalysed\ta.php\ncontext\tb.php\n== registry\n\n== diagnostics\nx\t[]\n";
    let entities = cut(Kind::Tree, text.as_bytes(), 4);
    let texts: Vec<(&str, &str)> = entities
        .iter()
        .map(|(path, text)| (path.as_str(), std::str::from_utf8(text).unwrap()))
        .collect();
    assert_eq!(
        texts,
        [
            ("files", "== files\nanalysed\ta.php\ncontext\tb.php\n"),
            ("registry", "== registry\n\n"),
            ("diagnostics", "== diagnostics\nx\t[]\n"),
        ]
    );
}

#[test]
fn lines_are_cut_into_runs_alike_before_the_separator() {
    let entities = cut(
        Kind::ByPrefix(b':'),
        b"a.php:1 x\na.php:2 y\nb.php:1 z\na.php:3 w\nnone",
        3,
    );
    let texts: Vec<(&str, &[u8])> = entities
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_slice()))
        .collect();
    assert_eq!(
        texts,
        [
            ("a.php", &b"a.php:1 x\na.php:2 y\n"[..]),
            ("b.php", b"b.php:1 z\n"),
            ("a.php~2", b"a.php:3 w\n"),
            ("none", b"none"),
        ]
    );
}

/// A generator of numbers, the same from the same seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let Rng(state) = self;
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// A text of lines such as prints are made of, nested at random, and some
/// that no print has.
fn random_text(rng: &mut Rng) -> Vec<u8> {
    const LINES: [&[u8]; 16] = [
        b"== section",
        b"== other",
        b"field: value",
        b"node: Node {",
        b"list: [",
        b"Key => Value {",
        b"'a => b' => [",
        b"Item {",
        b"}",
        b"]",
        b"}, Next {",
        b"plain",
        b"",
        b"   ",
        b"\xff\xfe odd",
        b"x: '",
    ];
    let mut text = Vec::new();
    for _ in 0..rng.below(60) {
        let indent = match rng.below(4) {
            0 => 0,
            1 => 2 * rng.below(6),
            2 => rng.below(13),
            _ => 2,
        };
        let line = LINES[rng.below(LINES.len())];
        if !line.starts_with(b"==") {
            text.extend(std::iter::repeat_n(b' ', indent));
        }
        text.extend_from_slice(line);
        text.push(b'\n');
    }
    if rng.below(2) == 0 {
        text.pop();
    }
    text
}

#[test]
fn any_text_comes_back_from_its_cut_in_any_pieces() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..3000 {
        let text = random_text(&mut rng);
        for kind in [Kind::Tree, Kind::ByPrefix(b':')] {
            let whole = cut(kind, &text, text.len().max(1));
            let joined: Vec<u8> = whole.iter().flat_map(|(_, text)| text.clone()).collect();
            assert_eq!(joined, text, "{}", String::from_utf8_lossy(&text));
            for piece in [1, 2, 7] {
                assert_eq!(cut(kind, &text, piece), whole, "in pieces of {piece}");
            }
            let mut tree = TreeBuilder::new(true);
            for (path, entity) in &whole {
                assert!(!entity.is_empty(), "{path}");
                tree.add(path, Id::blob(entity))
                    .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&text)));
            }
            tree.finish().unwrap();
        }
    }
}

/// Writes `text` as a dump at `dir`/`name`, given `reference`; its manifest.
fn write_dump(
    dir: &std::path::Path,
    name: &str,
    text: &str,
    reference: Option<&std::path::Path>,
) -> PathBuf {
    let path = dir.join(name);
    let mut writer = DumpWriter::create(&path, Kind::Tree, reference).unwrap();
    for piece in text.as_bytes().chunks(10) {
        writer.write_all(piece).unwrap();
    }
    writer.finish().unwrap();
    manifest_path(&path)
}

#[test]
fn a_run_given_a_reference_writes_only_the_entities_that_differ() {
    let dir = temp_dir("reference");
    let first = write_dump(&dir, "first.txt", REGISTRY, None);
    assert_eq!(
        std::fs::read_to_string(dir.join("first.txt")).unwrap(),
        REGISTRY
    );
    let changed = REGISTRY.replace("Bar => 1", "Bar => 2");
    let second = write_dump(&dir, "second.txt", &changed, Some(&first));
    assert_eq!(
        std::fs::read_to_string(dir.join("second.txt")).unwrap(),
        "      Bar => 2\n"
    );
    let mut dump = Dump::open(&second).unwrap();
    let mut text = Vec::new();
    dump.write_text(&mut text).unwrap();
    assert_eq!(String::from_utf8(text).unwrap(), changed);

    let first = Manifest::read(&first).unwrap();
    let difference = first_difference(&first, &dump.manifest).unwrap();
    let at = dump
        .manifest
        .entries
        .iter()
        .position(|entry| entry.offset.is_some());
    assert_eq!(difference, Difference { a: at, b: at });
    assert_eq!(
        dump.manifest.entries[at.unwrap()].path,
        "registry over stub-base/classes/changed/Bar"
    );

    let same = write_dump(
        &dir,
        "same.txt",
        REGISTRY,
        Some(&manifest_path(&dir.join("first.txt"))),
    );
    assert_eq!(std::fs::read(dir.join("same.txt")).unwrap(), b"");
    let same = Manifest::read(&same).unwrap();
    assert_eq!(first_difference(&first, &same), None);
    assert_eq!(same.root, first.root);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_text_cut_without_a_run_makes_the_run_s_manifest() {
    let dir = temp_dir("cut");
    let written = Manifest::read(&write_dump(&dir, "dump.txt", REGISTRY, None)).unwrap();
    let mut given = Vec::new();
    let manifest = crate::cut(
        Kind::Tree,
        &mut REGISTRY.as_bytes(),
        &mut |path, id, text| {
            assert_eq!(id, Id::blob(text));
            given.push((path.to_string(), text.to_vec()));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(manifest.root, written.root);
    assert_eq!(manifest.entries, written.entries);
    assert_eq!(given, cut(Kind::Tree, REGISTRY.as_bytes(), 1));
    assert!(crate::cut(Kind::Files, &mut [].as_slice(), &mut |_, _, _| Ok(())).is_err());
}

#[test]
fn a_manifest_s_trees_make_its_root_with_each_directory_s_order() {
    let dir = temp_dir("trees");
    let manifest = Manifest::read(&write_dump(&dir, "dump.txt", REGISTRY, None)).unwrap();
    let trees = trees(&manifest).unwrap();
    let root = trees.last().unwrap();
    assert_eq!((root.path.as_str(), root.id), ("", manifest.root));
    assert_eq!(
        root.order.as_deref(),
        Some(&b".0\nfiles\ndependents\nregistry over stub-base\nproject\n"[..])
    );
    let changed = trees
        .iter()
        .find(|tree| tree.path == "registry over stub-base/classes/changed")
        .unwrap();
    assert_eq!(changed.order.as_deref(), Some(&b".0\nFoo\nBar\n.1\n"[..]));
    let names: Vec<&str> = changed
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, [".0", ".1", ".order", "Bar", "Foo"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dump_of_files_lists_them_by_path_whatever_order_they_come_in() {
    let dir = temp_dir("files");
    let files: [(&[&[u8]], &[u8]); 4] = [
        (&[b"analysed", b"src", b"b.php"], b"b\n"),
        (&[b"context", b"x.php"], b"x\n"),
        (&[b"analysed", b"src", b"a.php"], b"a\n"),
        (&[b"analysed", b"..", b"c.php"], b"c\n"),
    ];
    let mut roots = Vec::new();
    for (run, order) in [[0, 1, 2, 3], [3, 2, 1, 0]].into_iter().enumerate() {
        let path = dir.join(format!("files{run}.txt"));
        let mut writer = DumpWriter::create(&path, Kind::Files, None).unwrap();
        for index in order {
            let (components, text) = files[index];
            writer
                .file(FileEntity::new(components.iter().copied(), text))
                .unwrap();
        }
        roots.push(writer.finish().unwrap());
        let mut dump = Dump::open(&manifest_path(&path)).unwrap();
        let listed: Vec<String> = dump
            .manifest
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        assert_eq!(
            listed,
            [
                "analysed/%2E./c.php",
                "analysed/src/a.php",
                "analysed/src/b.php",
                "context/x.php"
            ]
        );
        assert_eq!(dump.text(1).unwrap(), b"a\n");
        assert_eq!(
            trees(&dump.manifest).unwrap().last().unwrap().id,
            dump.manifest.root
        );
    }
    assert_eq!(roots[0], roots[1]);
    let _ = std::fs::remove_dir_all(&dir);
}
