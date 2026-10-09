use std::{env, fs};

use php_semantics::{
    BehaviorGraph, Block, Bytes, CatalogEntry, CatalogManifest, Digest, Domain, DomainCoverage,
    EntryKey, Event, EvidenceKind, EvidenceRecord, GraphRecord, Operand, Precision, ReleaseVersion,
    SCHEMA_MAJOR, TargetPredicate, Terminator, VersionRange,
};
use sha2::{Digest as _, Sha256};

fn main() {
    let source_path = env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .expect("pass the locked PHP 8.5.4 Zend/zend_builtin_functions.c path");
    let source = fs::read(&source_path).expect("read pinned strlen source");
    let source_digest: [u8; 32] = Sha256::digest(&source).into();
    let source_text = std::str::from_utf8(&source).expect("PHP source is UTF-8");
    assert_eq!(
        source_digest,
        [
            0x0d, 0x44, 0x77, 0x17, 0x0a, 0x78, 0x54, 0x01, 0x31, 0x89, 0x3a, 0x8c, 0xfc, 0xa2,
            0xb9, 0xd1, 0x03, 0x3c, 0x79, 0xea, 0xb7, 0x54, 0xf9, 0xca, 0x3d, 0x85, 0xe0, 0x5a,
            0xe6, 0x91, 0x95, 0x05,
        ],
        "source differs from the locked PHP 8.5.4 translation unit"
    );
    let strlen_start = source_text
        .find("ZEND_FUNCTION(strlen)")
        .expect("pinned source contains strlen implementation");
    let function_end = source_text[strlen_start..]
        .find("/* }}} */")
        .map(|offset| strlen_start + offset)
        .expect("strlen implementation has a section terminator");
    assert!(
        source_text[strlen_start..function_end].contains("RETVAL_LONG(ZSTR_LEN(s));"),
        "strlen source no longer returns the byte length of its parsed string"
    );
    let evidence = EvidenceRecord {
        kind: EvidenceKind::SourceTransfer,
        source_digest: Digest(source_digest),
        site: Some(Bytes(
            b"Zend/zend_builtin_functions.c:ZEND_FUNCTION(strlen):ZSTR_LEN(s)".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-strlen-return-v1".to_vec()),
        assumptions: vec![],
        dependencies: vec![],
    };
    let evidence_id = evidence.id().expect("evidence encodes");
    let graph = BehaviorGraph {
        schema_major: SCHEMA_MAJOR,
        entry: 0,
        blocks: vec![Block::new(
            vec![Event::Unknown {
                domains: Domain::ALL
                    .iter()
                    .copied()
                    .filter(|domain| *domain != Domain::Return)
                    .collect(),
                reason: Bytes(
                    b"effects and non-normal exits are not modeled in this slice".to_vec(),
                ),
            }],
            Terminator::Return(Some(Operand::StringByteLength(0))),
        )],
        coverage: Domain::ALL
            .iter()
            .copied()
            .map(|domain| DomainCoverage {
                domain,
                coverage: if domain == Domain::Return {
                    php_semantics::Coverage::Closed
                } else {
                    php_semantics::Coverage::Open {
                        reasons: vec![Bytes(b"return-only first slice".to_vec())],
                    }
                },
                precision: Precision::Exact,
                dependencies: vec![],
                evidence: vec![evidence_id],
                assumptions: vec![],
            })
            .collect(),
    };
    let graph_id = graph.canonical_digest().expect("graph encodes");
    let applicability = TargetPredicate {
        release: Some(VersionRange::exact(ReleaseVersion {
            major: 8,
            minor: 5,
            patch: 4,
        })),
        ..TargetPredicate::any()
    };
    let manifest = CatalogManifest {
        schema_major: SCHEMA_MAJOR,
        entries: vec![CatalogEntry {
            key: EntryKey::Function {
                extension: Bytes(b"standard".to_vec()),
                name: Bytes(b"strlen".to_vec()),
            },
            graph: graph_id,
            applicability,
        }],
        graphs: vec![GraphRecord {
            id: graph_id,
            graph,
        }],
        evidence: vec![(evidence_id, evidence)],
    };
    manifest.validate().expect("generated manifest validates");
    println!("{}", manifest.to_json().expect("manifest is JSON"));
}
