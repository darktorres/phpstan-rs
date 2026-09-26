//! The checked-in stub symbol index (`stub_sources::INDEX`) is what the
//! generator makes of the embedded sources; `cargo xtask stub-index`
//! rewrites it.

#[path = "../../../xtask/src/stub_index.rs"]
mod stub_index;

#[test]
fn the_checked_in_stub_index_is_up_to_date() {
    let corpora = vec![
        stub_index::Corpus {
            tag: "c",
            files: stub_sources::PHPSTORM_STUBS_PATHS
                .iter()
                .zip(stub_sources::PHPSTORM_STUBS_SOURCES)
                .zip(stub_sources::PHPSTORM_STUBS_EXTENSIONS)
                .map(|((path, source), extension)| (*path, *source, Some(*extension)))
                .collect(),
        },
        stub_index::Corpus {
            tag: "s",
            files: stub_sources::PHPSTAN_SRC_PATHS
                .iter()
                .zip(stub_sources::PHPSTAN_SRC_SOURCES)
                .map(|(path, source)| (*path, *source, None))
                .collect(),
        },
    ];
    assert!(
        stub_index::generate(&corpora) == stub_sources::INDEX,
        "crates/stub-sources/index.tsv is stale: run `cargo xtask stub-index`"
    );
}
