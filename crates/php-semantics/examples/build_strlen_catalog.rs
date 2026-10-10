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
        runtime["source_commit"].as_str(),
        Some("e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03"),
        "runtime witness refers to a different locked PHP source revision"
    );
    let repository_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("resolve repository root");
    for fixture in ["weak_fixture", "strict_fixture"] {
        read_witness_file(&repository_root, &runtime[fixture], "path", "sha256");
    }
    let weak_output = read_witness_file(
        &repository_root,
        &runtime["weak_fixture"],
        "output",
        "output_sha256",
    );
    let strict_output = read_witness_file(
        &repository_root,
        &runtime["strict_fixture"],
        "output",
        "output_sha256",
    );
    let weak_call_output = fs::read(
        repository_root.join(
            runtime["weak_fixture"]["call_output"]
                .as_str()
                .expect("weak CALL output path"),
        ),
    )
    .expect("read weak CALL output");
    let strict_call_output = fs::read(
        repository_root.join(
            runtime["strict_fixture"]["call_output"]
                .as_str()
                .expect("strict CALL output path"),
        ),
    )
    .expect("read strict CALL output");
    assert_eq!(weak_output, weak_call_output, "weak VM outputs differ");
    assert_eq!(
        strict_output, strict_call_output,
        "strict VM outputs differ"
    );
    validate_runtime_output(&weak_output, &runtime["weak_fixture"]["expected"], "weak");
    validate_runtime_output(
        &strict_output,
        &runtime["strict_fixture"]["expected"],
        "strict",
    );
    assert_eq!(runtime["result"]["exit_code"].as_u64(), Some(0));
    assert_eq!(
        runtime["profiles"]["record"].as_str(),
        Some("tools/php-catalog/experiments/comparison/results/runtime/profiles.json")
    );
    let profiles: serde_json::Value = serde_json::from_slice(
        &fs::read(
            repository_root.join(
                runtime["profiles"]["record"]
                    .as_str()
                    .expect("profile record path"),
            ),
        )
        .expect("read PHP VM profile record"),
    )
    .expect("profile record is JSON");
    for (runtime_profile, recorded_profile) in [("tailcall", "normal"), ("call", "call")] {
        assert_eq!(
            runtime["profiles"][runtime_profile]["vm_kind"],
            profiles[recorded_profile]["vm_kind"]
        );
        assert_eq!(
            runtime["profiles"][runtime_profile]["binary_sha256"],
            profiles[recorded_profile]["binary_sha256"]
        );
    }
    for (profile, expected_kind, expected_binary) in [
        (
            "tailcall",
            "ZEND_VM_KIND_TAILCALL",
            "abc41b0be90d2e9d1d744f2c53600c5f6a7e48af09bbd3ffc4eda382ce9236b5",
        ),
        (
            "call",
            "ZEND_VM_KIND_CALL",
            "866ee99f4de2644b3d28e45dfc2071a5a3926a65b04ab3127baef312102684af",
        ),
    ] {
        assert_eq!(
            runtime["profiles"][profile]["vm_kind"].as_str(),
            Some(expected_kind)
        );
        assert_eq!(
            runtime["profiles"][profile]["binary_sha256"].as_str(),
            Some(expected_binary),
            "runtime witness uses an unlocked {profile} profile"
        );
    }
    assert_eq!(
        runtime["weak_fixture"]["expected"]["lengths"],
        serde_json::json!([3, 3, 1, 0, 0])
    );
    assert_eq!(
        runtime["weak_fixture"]["expected"]["deprecations"],
        serde_json::json!([
            "strlen(): Passing null to parameter #1 ($string) of type string is deprecated"
        ])
    );
    assert_eq!(
        runtime["result"]["weak_tailcall_call_outputs_identical"].as_bool(),
        Some(true)
    );
    assert_eq!(
        runtime["result"]["strict_tailcall_call_outputs_identical"].as_bool(),
        Some(true)
    );
    assert_eq!(
        runtime["strict_fixture"]["expected"]["rejected"],
        serde_json::json!(["int", "float", "bool", "bool", "null"])
    );
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
        "convert_to_string(arg);",
        "zobj->handlers->cast_object(zobj, &obj, IS_STRING)",
        "OBJ_RELEASE(zobj);",
        "*dest = Z_STR_P(arg);",
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
    let parser_source_path = source_path
        .parent()
        .expect("Zend source has a parent directory")
        .join("zend_API.c");
    let parser_source_bytes =
        fs::read(&parser_source_path).expect("read pinned Zend argument-parser source");
    let parser_source_digest: [u8; 32] = Sha256::digest(&parser_source_bytes).into();
    assert_eq!(
        parser_source_digest, parser_digest,
        "parser source bytes differ from the source-linked LLVM witness"
    );
    let parser_source_text =
        std::str::from_utf8(&parser_source_bytes).expect("Zend argument-parser source is UTF-8");
    let parser_start = parser_source_text
        .find("zend_parse_arg_str_weak(zval *arg")
        .expect("locked weak string parser source exists");
    let parser_end = parser_source_text[parser_start..]
        .find("/* }}} */")
        .map(|offset| parser_start + offset)
        .expect("weak string parser has a section terminator");
    for required_fact in [
        "convert_to_string(arg);",
        "zobj->handlers->cast_object(zobj, &obj, IS_STRING)",
        "OBJ_RELEASE(zobj);",
        "*dest = Z_STR_P(arg);",
    ] {
        assert!(
            parser_source_text[parser_start..parser_end].contains(required_fact),
            "weak string parser no longer contains {required_fact}"
        );
    }
    let object_handlers_path = source_path
        .parent()
        .expect("Zend source has a parent directory")
        .join("zend_object_handlers.c");
    let object_handlers =
        fs::read(&object_handlers_path).expect("read pinned Zend object-handler source");
    let object_handlers_text =
        std::str::from_utf8(&object_handlers).expect("Zend object-handler source is UTF-8");
    let cast_start = object_handlers_text
        .find("ZEND_API zend_result zend_std_cast_object_tostring")
        .expect("pinned source contains the standard object cast handler");
    let cast_end = object_handlers_text[cast_start..]
        .find("/* }}} */")
        .map(|offset| cast_start + offset)
        .expect("standard cast handler has a section terminator");
    assert!(object_handlers_text[cast_start..cast_end].contains(
        "zend_call_known_instance_method_with_0_params(ce->__tostring, readobj, &retval)"
    ));
    for required_fact in [
        "GC_ADDREF(readobj);",
        "zend_object_release(readobj);",
        "EXPECTED(Z_TYPE(retval) == IS_STRING)",
        "zval_ptr_dtor(&retval);",
        "zend_throw_error(NULL, \"Method %s::__toString() must return a string value\"",
    ] {
        assert!(
            object_handlers_text[cast_start..cast_end].contains(required_fact),
            "standard cast handler no longer contains {required_fact}"
        );
    }
    assert!(object_handlers_text.contains("zend_std_cast_object_tostring,\t\t\t/* cast_object */"));
    let object_handlers_digest: [u8; 32] = Sha256::digest(&object_handlers).into();
    let object_cast_evidence = EvidenceRecord {
        kind: EvidenceKind::SourceTransfer,
        source_digest: Digest(object_handlers_digest),
        site: Some(Bytes(
            b"Zend/zend_object_handlers.c:zend_std_cast_object_tostring:ce->__tostring;std_object_handlers.cast_object".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-zend-stringable-cast-invocation-v1".to_vec()),
        assumptions: vec![],
        dependencies: vec![parser_evidence_id],
    };
    let object_cast_evidence_id = object_cast_evidence.id().expect("cast evidence encodes");
    let callback_runtime_path = repository_root
        .join("tools/php-catalog/experiments/comparison/results/runtime/tailcall-call-reference-8.5.4.json");
    let callback_runtime_bytes =
        fs::read(&callback_runtime_path).expect("read current TAILCALL/CALL runtime witness");
    let callback_runtime: serde_json::Value =
        serde_json::from_slice(&callback_runtime_bytes).expect("callback runtime witness is JSON");
    assert_eq!(
        callback_runtime["source_commit"].as_str(),
        Some("e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03")
    );
    assert!(
        callback_runtime["result"]["runtime_php_sentinels"]
            .as_array()
            .is_some_and(|sentinels| sentinels
                .iter()
                .any(|sentinel| sentinel.as_str() == Some("string-coercion-callback"))
                && sentinels
                    .iter()
                    .any(|sentinel| sentinel.as_str() == Some("string-coercion-throw")))
    );
    assert_eq!(callback_runtime["result"]["exit_code"].as_u64(), Some(0));
    assert_eq!(
        callback_runtime["result"]["output_files_identical"].as_bool(),
        Some(true)
    );
    let callback_fixture = &callback_runtime["fixture"];
    let callback_witness = read_witness_file(
        &repository_root,
        callback_fixture,
        "tailcall_output",
        "output_sha256",
    );
    for sentinel in [
        b"string-coercion-callback".as_slice(),
        b"string-coercion-throw",
    ] {
        assert!(
            callback_witness
                .split(|byte| *byte == b'\n')
                .any(|line| line == sentinel)
        );
    }
    let call_callback_witness_path = repository_root.join(
        callback_fixture["call_output"]
            .as_str()
            .expect("CALL callback output path"),
    );
    let call_callback_witness = read_witness_file(
        &repository_root,
        callback_fixture,
        "call_output",
        "output_sha256",
    );
    assert_eq!(
        callback_witness, call_callback_witness,
        "PHP VM profiles disagree on the Stringable callback witness"
    );
    assert!(call_callback_witness_path.exists());
    let mut callback_dependencies = vec![parser_evidence_id, object_cast_evidence_id];
    callback_dependencies.sort();
    let callback_observation = EvidenceRecord {
        kind: EvidenceKind::Observation,
        source_digest: Digest(Sha256::digest(&callback_runtime_bytes).into()),
        site: Some(Bytes(
            b"PHP 8.5.4 TAILCALL/CALL runtime:strlen(Stringable):normal-and-throw-callback"
                .to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-strlen-stringable-callback-observation-v2".to_vec()),
        assumptions: vec![],
        dependencies: callback_dependencies,
    };
    let callback_observation_id = callback_observation
        .id()
        .expect("callback observation evidence encodes");
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
    validate_zif_strlen_inventory(&witness["zif_strlen_operation_inventory"]);
    validate_native_return_demand(&witness);
    let inventory_evidence = EvidenceRecord {
        kind: EvidenceKind::SourceTransfer,
        source_digest: Digest(module_digest),
        site: Some(Bytes(
            b"php-cli.bc:zif_strlen:complete-llvm-opcode-inventory".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-clang-21.1.8-strlen-opcode-inventory-v1".to_vec()),
        assumptions: vec![],
        dependencies: vec![],
    };
    let inventory_evidence_id = inventory_evidence.id().expect("inventory evidence encodes");
    let mut native_demand_dependencies = vec![
        source_evidence_id,
        layout_evidence_id,
        parser_evidence_id,
        inventory_evidence_id,
    ];
    native_demand_dependencies.sort();
    let native_demand_evidence = EvidenceRecord {
        kind: EvidenceKind::SourceTransfer,
        source_digest: Digest(module_digest),
        site: Some(Bytes(
            b"php-cli.bc:zif_strlen:normal-return-value-demand".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-clang-21.1.8-strlen-native-return-demand-v1".to_vec()),
        assumptions: vec![],
        dependencies: native_demand_dependencies,
    };
    let native_demand_evidence_id = native_demand_evidence
        .id()
        .expect("native return demand evidence encodes");
    assert_eq!(
        module_digest,
        [
            0xe3, 0x42, 0x7c, 0xe8, 0x1f, 0x98, 0xf9, 0xa0, 0x4d, 0xb3, 0xe0, 0x4c, 0xe8, 0x22,
            0xc3, 0xe0, 0xf1, 0xc7, 0x31, 0x13, 0x7f, 0x0d, 0x63, 0xe0, 0x44, 0x0f, 0x86, 0x2b,
            0x1e, 0x61, 0xc9, 0x3e,
        ],
        "linked module differs from the measured PHP 8.5.4 Clang profile"
    );
    let mut llvm_dependencies = vec![
        source_evidence_id,
        layout_evidence_id,
        parser_evidence_id,
        inventory_evidence_id,
        native_demand_evidence_id,
    ];
    llvm_dependencies.sort();
    let llvm_evidence = EvidenceRecord {
        kind: EvidenceKind::SourceTransfer,
        source_digest: Digest(module_digest),
        site: Some(Bytes(
            b"php-cli.bc:zif_strlen:line-382:_zend_string.field[2]->return-zval.i64".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-clang-21.1.8-strlen-return-v2-inventoried".to_vec()),
        assumptions: vec![],
        dependencies: llvm_dependencies,
    };
    let llvm_evidence_id = llvm_evidence.id().expect("LLVM evidence encodes");
    let mut return_evidence = vec![
        source_evidence_id,
        layout_evidence_id,
        parser_evidence_id,
        inventory_evidence_id,
        native_demand_evidence_id,
        llvm_evidence_id,
    ];
    return_evidence.sort();
    let mut diagnostic_coverage_evidence = vec![parser_evidence_id, diagnostic_evidence_id];
    diagnostic_coverage_evidence.sort();
    let graph = BehaviorGraph {
        schema_major: SCHEMA_MAJOR,
        entry: 0,
        blocks: vec![
            Block::new(
                vec![Event::Unknown {
                    domains: vec![
                        Domain::Invocation,
                        Domain::Release,
                        Domain::PhpThrow,
                        Domain::NativeUnwind,
                        Domain::Fatal,
                        Domain::Suspension,
                    ],
                    reason: Bytes(
                        b"weak scalar parsing/coercion, object conversion, temporary release, and parser failure exits are only partially evidenced".to_vec(),
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
                Terminator::Goto(3),
            ),
            Block::new(
                vec![],
                Terminator::Branch {
                    predicate: Predicate::Host(Bytes(b"strlen-weak-stringable-cast".to_vec())),
                    yes: 3,
                    no: 4,
                },
            ),
            Block::new(
                vec![
                    Event::Invoke {
                        callable: Operand::Literal(Bytes(b"__toString".to_vec())),
                        arguments: vec![Operand::Argument(0)],
                        result: 0,
                    },
                    Event::Unknown {
                        domains: vec![
                            Domain::Invocation,
                            Domain::UserRead,
                            Domain::UserWrite,
                            Domain::ExternalRead,
                            Domain::ExternalWrite,
                            Domain::Autoload,
                            Domain::Output,
                            Domain::Release,
                            Domain::Retention,
                            Domain::PhpThrow,
                            Domain::NativeUnwind,
                            Domain::Fatal,
                            Domain::Suspension,
                        ],
                        reason: Bytes(
                            b"Stringable callback body, ordered effects, destructor consequences, invalid return, and abrupt callback outcomes are not interpreted".to_vec(),
                        ),
                    },
                ],
                Terminator::Goto(4),
            ),
            Block::new(
                vec![],
                Terminator::Return(Some(Operand::StringByteLength(0))),
            ),
        ],
        // The closed Return fact is the nonnegative byte-length envelope of
        // Argument(0). Its expression does not consume the __toString Invoke
        // result, and this graph contains no explicit user-state Read/Write.
        // The callback may affect other domains (left open below), but those
        // effects cannot make this value envelope negative, so Return's
        // mandatory transfer-dependency set is empty.
        coverage: Domain::ALL
            .iter()
            .copied()
            .map(|domain| DomainCoverage {
                domain,
                    coverage: if domain == Domain::Return || domain == Domain::Diagnostic {
                    php_semantics::Coverage::Closed
                } else {
                    php_semantics::Coverage::Open {
                            reasons: vec![Bytes(match domain {
                                Domain::Invocation => b"normal Stringable callback invocation is observed; callback body behavior remains open".to_vec(),
                                Domain::Release => b"source releases temporary objects; release timing and destructor consequences are not modeled".to_vec(),
                                Domain::PhpThrow => b"callback throw and parser failure are possible; only callback throw is observed".to_vec(),
                                Domain::NativeUnwind => b"native unwind is distinct from PHP throw and is not modeled by this graph".to_vec(),
                                Domain::Fatal => b"fatal and native bailout outcomes are not covered by the selected witness".to_vec(),
                                Domain::Suspension => b"callback suspension is not covered by the selected witness".to_vec(),
                                _ => b"callback effects and non-normal behavior are not modeled in this slice".to_vec(),
                            })],
                    }
                },
                precision: Precision::Exact,
                dependencies: vec![],
                evidence: if matches!(domain, Domain::Invocation | Domain::Release | Domain::PhpThrow) {
                    let mut evidence = return_evidence.clone();
                    evidence.extend([object_cast_evidence_id, callback_observation_id]);
                    evidence.sort();
                    evidence.dedup();
                    evidence
                } else if domain == Domain::Return {
                    return_evidence.clone()
                } else if domain == Domain::Diagnostic {
                    diagnostic_coverage_evidence.clone()
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
    let mut evidence = vec![
        (source_evidence_id, source_evidence),
        (layout_evidence_id, layout_evidence),
        (parser_evidence_id, parser_evidence),
        (object_cast_evidence_id, object_cast_evidence),
        (callback_observation_id, callback_observation),
        (diagnostic_evidence_id, diagnostic_evidence),
        (inventory_evidence_id, inventory_evidence),
        (native_demand_evidence_id, native_demand_evidence),
        (llvm_evidence_id, llvm_evidence),
    ];
    evidence.sort_by_key(|(id, _)| *id);
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
        evidence,
        assumptions: vec![],
    };
    manifest.validate().expect("generated manifest validates");
    println!("{}", manifest.to_json().expect("manifest is JSON"));
}

fn validate_native_return_demand(witness: &serde_json::Value) {
    let demand = &witness["native_return_demand"];
    assert_eq!(demand["query"]["function"].as_str(), Some("zif_strlen"));
    assert_eq!(demand["query"]["domain"].as_str(), Some("Return"));
    assert_eq!(
        demand["query"]["demand"].as_str(),
        Some("normal-exit return zval payload")
    );
    assert_eq!(demand["query"]["argument"].as_u64(), Some(0));
    assert_eq!(
        demand["source_expression"].as_str(),
        Some("RETVAL_LONG(ZSTR_LEN(s))")
    );
    assert_eq!(demand["parser_output_local_slot"].as_str(), Some("17"));
    assert_eq!(demand["string_length_field_index"].as_u64(), Some(2));
    assert_eq!(demand["length_load_register"].as_str(), Some("%193"));
    assert_eq!(
        demand["return_payload_field_address"].as_str(),
        Some("%195")
    );
    assert_eq!(demand["return_zval_output_slot"].as_str(), Some("%16"));
    assert_eq!(
        demand["parser_output_local_slot"],
        witness["argument_bridge"]["length_load_uses_local_slot"],
        "parser output and length load use different local slots"
    );
    assert_eq!(
        demand["parser_calls"],
        serde_json::json!(["zend_flf_parse_arg_str_slow", "zend_parse_arg_str_slow"])
    );
    let destinations = witness["argument_bridge"]["parser_destination_slots"]
        .as_array()
        .expect("both argument-parser destinations are recorded");
    assert_eq!(destinations.len(), 2);
    assert!(destinations.iter().all(|destination| {
        destination["strlen_local_slot"] == demand["parser_output_local_slot"]
    }));
    assert_eq!(
        demand["coverage"]["Return"].as_str(),
        Some("closed for normal returns after successful parameter parsing")
    );
    assert!(
        demand["coverage"]["non_normal_exits"]
            .as_str()
            .is_some_and(|coverage| coverage.starts_with("open;"))
    );
    assert!(
        demand["coverage"]["effects"]
            .as_str()
            .is_some_and(|coverage| coverage.starts_with("open;"))
    );
}

fn validate_zif_strlen_inventory(inventory: &serde_json::Value) {
    assert_eq!(inventory["instruction_count"].as_u64(), Some(241));
    assert_eq!(inventory["indirect_call_count"].as_u64(), Some(0));
    assert_eq!(
        inventory["accounting_scope"].as_str(),
        Some("all instructions inventoried; operation semantics remain subject to graph coverage")
    );
    assert_eq!(
        inventory["opcode_counts"],
        serde_json::json!({
            "add": 1,
            "alloca": 28,
            "br": 32,
            "call": 7,
            "getelementptr": 9,
            "icmp": 17,
            "load": 67,
            "phi": 3,
            "ret": 1,
            "sext": 6,
            "store": 39,
            "trunc": 8,
            "xor": 12,
            "zext": 11,
        }),
        "opcode inventory differs from the locked Clang module"
    );
    assert_eq!(
        inventory["operation_group_counts"],
        serde_json::json!({
            "calls": 7,
            "control_flow": 32,
            "exit": 1,
            "layout_access": 9,
            "native_storage": 134,
            "scalar_transfer": 58,
        }),
        "operation groups do not reconcile to the pinned LLVM inventory"
    );
    assert_eq!(
        inventory["direct_call_targets"],
        serde_json::json!([
            "llvm.assume",
            "llvm.assume",
            "llvm.assume",
            "zend_flf_parse_arg_str_slow",
            "zend_parse_arg_str_slow",
            "zend_wrong_parameter_error",
            "zend_wrong_parameters_count_error",
        ])
    );
}

fn read_witness_file(
    repository_root: &std::path::Path,
    record: &serde_json::Value,
    path_field: &str,
    digest_field: &str,
) -> Vec<u8> {
    let relative_path = record[path_field]
        .as_str()
        .unwrap_or_else(|| panic!("runtime witness {path_field} is a path"));
    let expected_digest = record[digest_field]
        .as_str()
        .unwrap_or_else(|| panic!("runtime witness {digest_field} is a SHA-256"));
    let bytes = fs::read(repository_root.join(relative_path))
        .unwrap_or_else(|error| panic!("read runtime witness file {relative_path}: {error}"));
    let actual_digest = Sha256::digest(&bytes);
    assert_eq!(
        hex_digest(actual_digest.as_slice()),
        expected_digest,
        "runtime witness digest differs for {relative_path}"
    );
    bytes
}

fn hex_digest(digest: &[u8]) -> String {
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut hex, "{byte:02x}").expect("writing to String cannot fail");
    }
    hex
}

fn validate_runtime_output(output: &[u8], expected: &serde_json::Value, mode: &str) {
    let observed: serde_json::Value =
        serde_json::from_slice(output).unwrap_or_else(|_| panic!("{mode} runtime output is JSON"));
    assert_eq!(
        &observed, expected,
        "{mode} runtime output differs from its expected values"
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_return_demand_witness() -> serde_json::Value {
        serde_json::json!({
            "native_return_demand": {
                "query": {
                    "function": "zif_strlen",
                    "domain": "Return",
                    "demand": "normal-exit return zval payload",
                    "argument": 0
                },
                "source_expression": "RETVAL_LONG(ZSTR_LEN(s))",
                "parser_output_local_slot": "17",
                "string_length_field_index": 2,
                "length_load_register": "%193",
                "return_payload_field_address": "%195",
                "return_zval_output_slot": "%16",
                "parser_calls": ["zend_flf_parse_arg_str_slow", "zend_parse_arg_str_slow"],
                "coverage": {
                    "Return": "closed for normal returns after successful parameter parsing",
                    "non_normal_exits": "open; represented by catalog domain coverage",
                    "effects": "open; represented by catalog domain coverage"
                }
            },
            "argument_bridge": {
                "length_load_uses_local_slot": "17",
                "parser_destination_slots": [
                    {"strlen_local_slot": "17"},
                    {"strlen_local_slot": "17"}
                ]
            }
        })
    }

    fn valid_strlen_inventory() -> serde_json::Value {
        serde_json::json!({
            "instruction_count": 241,
            "indirect_call_count": 0,
            "accounting_scope": "all instructions inventoried; operation semantics remain subject to graph coverage",
            "opcode_counts": {
                "add": 1, "alloca": 28, "br": 32, "call": 7,
                "getelementptr": 9, "icmp": 17, "load": 67, "phi": 3,
                "ret": 1, "sext": 6, "store": 39, "trunc": 8,
                "xor": 12, "zext": 11
            },
            "operation_group_counts": {
                "calls": 7, "control_flow": 32, "exit": 1,
                "layout_access": 9, "native_storage": 134, "scalar_transfer": 58
            },
            "direct_call_targets": [
                "llvm.assume", "llvm.assume", "llvm.assume",
                "zend_flf_parse_arg_str_slow", "zend_parse_arg_str_slow",
                "zend_wrong_parameter_error", "zend_wrong_parameters_count_error"
            ]
        })
    }

    #[test]
    fn zif_strlen_inventory_validator_accepts_the_locked_operation_accounting() {
        validate_zif_strlen_inventory(&valid_strlen_inventory());
    }

    #[test]
    fn zif_strlen_inventory_validator_rejects_missing_or_invented_operations() {
        let mut missing = valid_strlen_inventory();
        missing["opcode_counts"]
            .as_object_mut()
            .unwrap()
            .remove("load");
        assert!(std::panic::catch_unwind(|| validate_zif_strlen_inventory(&missing)).is_err());

        let mut invented = valid_strlen_inventory();
        invented["direct_call_targets"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!("invented_target"));
        assert!(std::panic::catch_unwind(|| validate_zif_strlen_inventory(&invented)).is_err());
    }

    #[test]
    fn native_return_demand_validator_requires_connected_slots_and_open_boundaries() {
        let witness = valid_return_demand_witness();
        validate_native_return_demand(&witness);

        let mut wrong_slot = witness.clone();
        wrong_slot["argument_bridge"]["parser_destination_slots"][1]["strlen_local_slot"] =
            serde_json::json!("99");
        assert!(std::panic::catch_unwind(|| validate_native_return_demand(&wrong_slot)).is_err());

        let mut false_closure = witness;
        false_closure["native_return_demand"]["coverage"]["effects"] = serde_json::json!("closed");
        assert!(
            std::panic::catch_unwind(|| validate_native_return_demand(&false_closure)).is_err()
        );

        let mut wrong_register = valid_return_demand_witness();
        wrong_register["native_return_demand"]["length_load_register"] = serde_json::json!("%999");
        assert!(
            std::panic::catch_unwind(|| validate_native_return_demand(&wrong_register)).is_err()
        );
    }

    #[test]
    fn runtime_fixture_hash_is_checked_against_the_file() {
        let repository_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("resolve repository root");
        let record = serde_json::json!({
            "path": "tools/php-catalog/experiments/comparison/runtime-strlen-weak-scalars.php",
            "sha256": "30607f7b7f81cb15aaf0ad03701cc1fcedae321f4007c3b9058acaea1362b873",
        });
        assert!(!read_witness_file(&repository_root, &record, "path", "sha256").is_empty());

        let mut incorrect_record = record;
        incorrect_record["sha256"] = serde_json::Value::String("00".repeat(32));
        let result = std::panic::catch_unwind(|| {
            read_witness_file(&repository_root, &incorrect_record, "path", "sha256")
        });
        assert!(
            result.is_err(),
            "a mismatched runtime artifact hash is rejected"
        );
    }

    #[test]
    fn runtime_output_must_match_the_declared_observation() {
        let output = br#"{"lengths":[3],"deprecations":["deprecated"]}"#;
        let expected = serde_json::json!({
            "lengths": [3],
            "deprecations": ["deprecated"],
        });
        validate_runtime_output(output, &expected, "weak");

        let incorrect = serde_json::json!({
            "lengths": [99],
            "deprecations": ["deprecated"],
        });
        let result =
            std::panic::catch_unwind(|| validate_runtime_output(output, &incorrect, "weak"));
        assert!(result.is_err(), "a mismatched observation is rejected");
    }
}
