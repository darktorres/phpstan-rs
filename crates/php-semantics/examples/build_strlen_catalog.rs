use std::{env, fs};

use php_semantics::{
    BehaviorGraph, Block, Bytes, CatalogEntry, CatalogManifest, DiagnosticKind, Digest, Domain,
    DomainCoverage, EntryKey, Event, EvidenceKind, EvidenceRecord, GraphRecord, Operand, Precision,
    Predicate, ReleaseVersion, SCHEMA_MAJOR, TargetPredicate, Terminator, VersionRange,
};
use sha2::{Digest as _, Sha256};

fn main() {
    let mut args = env::args_os().skip(1);
    let source_path = args
        .next()
        .map(std::path::PathBuf::from)
        .expect("pass the locked PHP 8.5.4 Zend/zend_builtin_functions.c path");
    let witness_path = args
        .next()
        .map(std::path::PathBuf::from)
        .expect("pass the extracted zif_strlen LLVM witness JSON path");
    let runtime_path = args
        .next()
        .map(std::path::PathBuf::from)
        .expect("pass the paired PHP 8.5.4 weak/strict runtime witness JSON path");
    let source = fs::read(&source_path).expect("read pinned strlen source");
    let source_digest: [u8; 32] = Sha256::digest(&source).into();
    let source_text = std::str::from_utf8(&source).expect("PHP source is UTF-8");
    let runtime_bytes = fs::read(&runtime_path).expect("read PHP runtime witness");
    let runtime: serde_json::Value =
        serde_json::from_slice(&runtime_bytes).expect("runtime witness is JSON");
    assert_eq!(
        runtime["weak_fixture"]["expected"]["lengths"],
        serde_json::json!([3, 3, 1, 0, 0])
    );
    assert!(
        runtime["weak_fixture"]["expected"]["deprecations"]
            .as_array()
            .is_some_and(|messages| messages.iter().any(|message| {
                message
                    .as_str()
                    .is_some_and(|message| message.contains("Passing null"))
            }))
    );
    assert_eq!(
        runtime["result"]["weak_tailcall_call_outputs_identical"].as_bool(),
        Some(true)
    );
    assert_eq!(
        runtime["result"]["strict_tailcall_call_outputs_identical"].as_bool(),
        Some(true)
    );
    for rejected in ["int", "float", "bool", "null"] {
        assert!(
            runtime["strict_fixture"]["expected"]["rejected"]
                .as_array()
                .is_some_and(|types| types.iter().any(|ty| ty.as_str() == Some(rejected)))
        );
    }
    let runtime_digest: [u8; 32] = Sha256::digest(&runtime_bytes).into();
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
    let source_evidence = EvidenceRecord {
        kind: EvidenceKind::SourceTransfer,
        source_digest: Digest(source_digest),
        site: Some(Bytes(
            b"Zend/zend_builtin_functions.c:ZEND_FUNCTION(strlen):ZSTR_LEN(s)".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-strlen-return-v1".to_vec()),
        assumptions: vec![],
        dependencies: vec![],
    };
    let source_evidence_id = source_evidence.id().expect("source evidence encodes");
    let witness: serde_json::Value =
        serde_json::from_slice(&fs::read(&witness_path).expect("read linked LLVM witness"))
            .expect("LLVM witness is JSON");
    assert_eq!(witness["schema"], 1, "unsupported LLVM witness schema");
    assert_eq!(
        witness["source"]["sha256"].as_str(),
        Some("0d4477170a78540131893a8cfca2b9d1033c79eab754f9ca3d85e05ae6919505"),
        "LLVM witness refers to a different source file"
    );
    assert_eq!(witness["source"]["return_line"].as_u64(), Some(382));
    assert_eq!(
        witness["compiler"].as_str(),
        Some("Ubuntu clang version 21.1.8 (6ubuntu1)")
    );
    assert_eq!(
        witness["mapping"]["zend_string_length_field_index"].as_u64(),
        Some(2)
    );
    assert_eq!(
        witness["target"]["triple"].as_str(),
        Some("x86_64-pc-linux-gnu")
    );
    assert_eq!(
        witness["target"]["datalayout_has_64_bit_i64"].as_bool(),
        Some(true)
    );
    assert_eq!(
        witness["zend_string_layout"]["length_field_index"].as_u64(),
        Some(2)
    );
    assert_eq!(
        witness["zend_string_layout"]["length_field_name"].as_str(),
        Some("len")
    );
    assert_eq!(
        witness["zend_string_layout"]["length_field_type"].as_str(),
        Some("size_t")
    );
    assert_eq!(
        witness["zend_string_layout"]["fields"][2]["name"].as_str(),
        Some("len")
    );
    let layout_digest = decode_digest(
        witness["zend_string_layout"]["sha256"]
            .as_str()
            .expect("Zend layout-header digest"),
    );
    let layout_evidence = EvidenceRecord {
        kind: EvidenceKind::SourceTransfer,
        source_digest: Digest(layout_digest),
        site: Some(Bytes(
            b"Zend/zend_types.h:struct _zend_string:field[2]=len:size_t".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-zend-string-layout-v1".to_vec()),
        assumptions: vec![],
        dependencies: vec![],
    };
    let layout_evidence_id = layout_evidence.id().expect("Zend layout evidence encodes");
    assert_eq!(
        witness["parser_source"]["sha256"].as_str(),
        Some("ba6d163b75905a3fc122186452a4cc08ab06b25e7fc51b22d830b38d5daca23b"),
        "LLVM witness refers to different Zend parser source"
    );
    assert_eq!(
        witness["parser_source"]["functions"].as_array(),
        Some(&vec![
            serde_json::Value::String("zend_parse_arg_str_weak".into()),
            serde_json::Value::String("zend_parse_arg_str_slow".into()),
            serde_json::Value::String("zend_flf_parse_arg_str_slow".into()),
        ])
    );
    for required_fact in [
        "if (UNEXPECTED(ZEND_ARG_USES_STRICT_TYPES())) {\n\t\treturn 0;\n\t}",
        "if (UNEXPECTED(ZEND_FLF_ARG_USES_STRICT_TYPES())) {\n\t\treturn 0;\n\t}",
    ] {
        assert!(
            witness["parser_source"]["contract_facts"]
                .as_array()
                .is_some_and(|facts| facts
                    .iter()
                    .any(|fact| fact.as_str() == Some(required_fact)))
        );
    }
    let parser_digest = decode_digest(
        witness["parser_source"]["sha256"]
            .as_str()
            .expect("Zend parser source digest"),
    );
    let parser_evidence = EvidenceRecord {
        kind: EvidenceKind::SourceTransfer,
        source_digest: Digest(parser_digest),
        site: Some(Bytes(
            b"Zend/zend_API.c:Z_PARAM_STR:strict-gates-to-weak-string-parser".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-zend-string-parser-source-v1".to_vec()),
        assumptions: vec![],
        dependencies: vec![],
    };
    let parser_evidence_id = parser_evidence.id().expect("Zend parser evidence encodes");
    let diagnostic_evidence = EvidenceRecord {
        kind: EvidenceKind::Observation,
        source_digest: Digest(runtime_digest),
        site: Some(Bytes(
            b"PHP 8.5.4 TAILCALL/CALL weak strlen(null) deprecation witness".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-strlen-null-deprecation-observation-v1".to_vec()),
        assumptions: vec![],
        dependencies: vec![parser_evidence_id],
    };
    let diagnostic_evidence_id = diagnostic_evidence
        .id()
        .expect("diagnostic evidence encodes");
    assert_eq!(witness["mapping"]["loaded_as"].as_str(), Some("i64"));
    for fact in [
        "debug_line_matches_source_return",
        "stored_as_return_zval_payload",
        "parameter_failure_path_present",
        "parsed_argument_flows_to_length_load",
        "parser_output_written_through_destination",
        "object_cast_handler_indirect_call",
    ] {
        assert_eq!(
            witness["mapping"][fact].as_bool(),
            Some(true),
            "missing LLVM witness fact: {fact}"
        );
    }
    let parse_lines = witness["source"]["parameter_parse_lines"]
        .as_array()
        .expect("parameter-parse source lines");
    assert_eq!(parse_lines.len(), 3);
    let implicit_operations = witness["configured_implicit_operations"]
        .as_array()
        .expect("configured implicit-operation mapping");
    let expected_operations = [
        ("parameter_count_error", "zend_wrong_parameters_count_error"),
        ("flf_strictness_gate", "zend_flf_parse_arg_str_slow"),
        ("general_strictness_gate", "zend_parse_arg_str_slow"),
        ("type_error", "zend_wrong_parameter_error"),
        ("weak_string_parse", "zend_parse_arg_str_weak"),
        ("weak_string_conversion", "_convert_to_string"),
        ("nullable_string_diagnostic", "zend_null_arg_deprecated"),
        (
            "object_string_cast_callback",
            "indirect Zend object handler",
        ),
        ("object_release_during_cast", "zend_objects_store_del"),
    ];
    for (operation, callee) in expected_operations {
        assert!(
            implicit_operations.iter().any(|item| {
                item["operation"].as_str() == Some(operation)
                    && item["llvm_callee"].as_str() == Some(callee)
                    && item["catalog_domain"].as_str() == Some("open")
            }),
            "missing configured implicit-operation mapping: {operation}"
        );
    }
    for (function, callee) in [
        ("zend_flf_parse_arg_str_slow", "zend_parse_arg_str_weak"),
        ("zend_parse_arg_str_slow", "zend_parse_arg_str_weak"),
        ("zend_parse_arg_str_weak", "_convert_to_string"),
        ("zend_parse_arg_str_weak", "zend_null_arg_deprecated"),
    ] {
        assert!(
            witness["helper_witnesses"][function]["calls"]
                .as_array()
                .is_some_and(|calls| calls.iter().any(|call| call.as_str() == Some(callee)))
        );
    }
    let module_digest = decode_digest(
        witness["linked_bitcode"]["sha256"]
            .as_str()
            .expect("linked bitcode digest"),
    );
    assert_eq!(
        module_digest,
        [
            0xe3, 0x42, 0x7c, 0xe8, 0x1f, 0x98, 0xf9, 0xa0, 0x4d, 0xb3, 0xe0, 0x4c, 0xe8, 0x22,
            0xc3, 0xe0, 0xf1, 0xc7, 0x31, 0x13, 0x7f, 0x0d, 0x63, 0xe0, 0x44, 0x0f, 0x86, 0x2b,
            0x1e, 0x61, 0xc9, 0x3e,
        ],
        "linked module differs from the measured PHP 8.5.4 Clang profile"
    );
    let llvm_evidence = EvidenceRecord {
        kind: EvidenceKind::SourceTransfer,
        source_digest: Digest(module_digest),
        site: Some(Bytes(
            b"php-cli.bc:zif_strlen:line-382:_zend_string.field[2]->return-zval.i64".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-clang-21.1.8-strlen-return-v1".to_vec()),
        assumptions: vec![],
        dependencies: vec![source_evidence_id, layout_evidence_id, parser_evidence_id],
    };
    let llvm_evidence_id = llvm_evidence.id().expect("LLVM evidence encodes");
    let return_evidence = vec![
        source_evidence_id,
        layout_evidence_id,
        parser_evidence_id,
        llvm_evidence_id,
    ];
    let graph = BehaviorGraph {
        schema_major: SCHEMA_MAJOR,
        entry: 0,
        blocks: vec![
            Block::new(
                vec![Event::Unknown {
                    domains: Domain::ALL
                        .iter()
                        .copied()
                        .filter(|domain| *domain != Domain::Return && *domain != Domain::Diagnostic)
                        .collect(),
                    reason: Bytes(
                        b"effects and non-normal exits are not modeled in this slice".to_vec(),
                    ),
                }],
                Terminator::Branch {
                    predicate: Predicate::Host(Bytes(b"strlen-weak-null-deprecation".to_vec())),
                    yes: 1,
                    no: 2,
                },
            ),
            Block::new(
                vec![Event::EmitDiagnostic {
                    kind: DiagnosticKind::NullToInternalScalarParameterDeprecated,
                    argument: 0,
                }],
                Terminator::Goto(2),
            ),
            Block::new(
                vec![],
                Terminator::Return(Some(Operand::StringByteLength(0))),
            ),
        ],
        coverage: Domain::ALL
            .iter()
            .copied()
            .map(|domain| DomainCoverage {
                domain,
                coverage: if domain == Domain::Return || domain == Domain::Diagnostic {
                    php_semantics::Coverage::Closed
                } else {
                    php_semantics::Coverage::Open {
                        reasons: vec![Bytes(b"return-only first slice".to_vec())],
                    }
                },
                precision: Precision::Exact,
                dependencies: vec![],
                evidence: if domain == Domain::Return {
                    return_evidence.clone()
                } else if domain == Domain::Diagnostic {
                    vec![parser_evidence_id, diagnostic_evidence_id]
                } else {
                    return_evidence.clone()
                },
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
        evidence: vec![
            (source_evidence_id, source_evidence),
            (layout_evidence_id, layout_evidence),
            (parser_evidence_id, parser_evidence),
            (diagnostic_evidence_id, diagnostic_evidence),
            (llvm_evidence_id, llvm_evidence),
        ],
    };
    manifest.validate().expect("generated manifest validates");
    println!("{}", manifest.to_json().expect("manifest is JSON"));
}

fn decode_digest(text: &str) -> [u8; 32] {
    assert_eq!(text.len(), 64, "digest must contain 64 hexadecimal digits");
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .expect("digest contains only hexadecimal digits");
    }
    digest
}
