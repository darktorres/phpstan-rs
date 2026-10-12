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
    let opcode_witness_path = repository_root.join(
        "tools/php-catalog/experiments/comparison/results/evaluation/strlen-opcode-witness-8.5.4.json",
    );
    let opcode_witness_bytes =
        fs::read(&opcode_witness_path).expect("read direct ZEND_STRLEN opcode witness");
    let opcode_witness: serde_json::Value =
        serde_json::from_slice(&opcode_witness_bytes).expect("opcode witness is JSON");
    let (opcode_compiler_evidence, opcode_vm_evidence) = validate_strlen_opcode_witness(
        &source_path,
        &repository_root,
        &runtime,
        &callback_runtime,
        &profiles,
        &opcode_witness,
    );
    let opcode_compiler_evidence_id = opcode_compiler_evidence
        .id()
        .expect("opcode compiler source evidence encodes");
    let opcode_vm_evidence_id = opcode_vm_evidence
        .id()
        .expect("opcode VM source evidence encodes");
    let mut opcode_observation_dependencies = vec![
        opcode_compiler_evidence_id,
        opcode_vm_evidence_id,
        callback_observation_id,
    ];
    opcode_observation_dependencies.sort();
    let opcode_observation = EvidenceRecord {
        kind: EvidenceKind::Observation,
        source_digest: Digest(Sha256::digest(&opcode_witness_bytes).into()),
        site: Some(Bytes(
            b"PHP 8.5.4 TAILCALL/CALL OPcache dumps: direct ZEND_STRLEN compile and VM path"
                .to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-strlen-direct-opcode-observation-v1".to_vec()),
        assumptions: vec![],
        dependencies: opcode_observation_dependencies,
    };
    let opcode_observation_id = opcode_observation
        .id()
        .expect("direct opcode observation evidence encodes");
    let parser_outcomes_path = repository_root.join(
        "tools/php-catalog/experiments/comparison/results/evaluation/strlen-parser-outcomes-witness-8.5.4.json",
    );
    let parser_outcomes_bytes =
        fs::read(&parser_outcomes_path).expect("read scalar parser outcomes witness");
    let parser_outcomes_witness: serde_json::Value = serde_json::from_slice(&parser_outcomes_bytes)
        .expect("scalar parser outcomes witness is JSON");
    let parser_outcomes_observation = validate_strlen_parser_outcomes_witness(
        &repository_root,
        &profiles,
        &parser_outcomes_witness,
        &parser_outcomes_bytes,
        &[parser_evidence_id, opcode_observation_id],
    );
    let parser_outcomes_observation_id = parser_outcomes_observation
        .id()
        .expect("parser outcomes observation evidence encodes");
    let release_witness_path = repository_root.join(
        "tools/php-catalog/experiments/comparison/results/evaluation/strlen-release-witness-8.5.4.json",
    );
    let release_witness_bytes =
        fs::read(&release_witness_path).expect("read strlen release/destructor witness");
    let release_witness: serde_json::Value =
        serde_json::from_slice(&release_witness_bytes).expect("release witness is JSON");
    let (parser_release_evidence, cast_release_evidence, vm_release_evidence) =
        validate_strlen_release_witness(
            &source_path,
            &repository_root,
            &profiles,
            &release_witness,
        );
    let parser_release_evidence_id = parser_release_evidence
        .id()
        .expect("parser release evidence encodes");
    let cast_release_evidence_id = cast_release_evidence
        .id()
        .expect("cast release evidence encodes");
    let vm_release_evidence_id = vm_release_evidence
        .id()
        .expect("VM release evidence encodes");
    let mut release_observation_dependencies = vec![
        parser_release_evidence_id,
        cast_release_evidence_id,
        vm_release_evidence_id,
        opcode_observation_id,
        callback_observation_id,
    ];
    release_observation_dependencies.sort();
    let release_observation = EvidenceRecord {
        kind: EvidenceKind::Observation,
        source_digest: Digest(Sha256::digest(&release_witness_bytes).into()),
        site: Some(Bytes(
            b"PHP 8.5.4 TAILCALL/CALL strlen Stringable release and destructor ordering".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-strlen-release-observation-v1".to_vec()),
        assumptions: vec![],
        dependencies: release_observation_dependencies,
    };
    let release_observation_id = release_observation
        .id()
        .expect("release observation evidence encodes");
    let abrupt_witness_path = repository_root.join(
        "tools/php-catalog/experiments/comparison/results/evaluation/strlen-abrupt-outcomes-witness-8.5.4.json",
    );
    let abrupt_witness_bytes =
        fs::read(&abrupt_witness_path).expect("read strlen abrupt-outcomes witness");
    let abrupt_witness: serde_json::Value =
        serde_json::from_slice(&abrupt_witness_bytes).expect("abrupt-outcomes witness is JSON");
    let abrupt_observation = validate_strlen_abrupt_outcomes_witness(
        &source_path,
        &repository_root,
        &profiles,
        &abrupt_witness,
        &abrupt_witness_bytes,
        &[
            parser_outcomes_observation_id,
            release_observation_id,
            opcode_observation_id,
        ],
    );
    let abrupt_observation_id = abrupt_observation
        .id()
        .expect("abrupt-outcomes observation evidence encodes");
    let mut diagnostic_dependencies = vec![
        parser_evidence_id,
        opcode_observation_id,
        parser_outcomes_observation_id,
    ];
    diagnostic_dependencies.sort();
    let diagnostic_evidence = EvidenceRecord {
        kind: EvidenceKind::Observation,
        source_digest: Digest(runtime_digest),
        site: Some(Bytes(
            b"PHP 8.5.4 TAILCALL/CALL weak strlen(null) deprecation witness".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-strlen-null-deprecation-observation-v1".to_vec()),
        assumptions: vec![],
        dependencies: diagnostic_dependencies,
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
            0x3c, 0x51, 0xa5, 0x31, 0xcd, 0x14, 0xbe, 0x7c, 0x0f, 0x78, 0x95, 0x9e, 0xbd, 0xab,
            0xa1, 0xba, 0xed, 0x05, 0x6b, 0x0c, 0xf2, 0xae, 0xaa, 0xb3, 0x66, 0x2c, 0xe4, 0x78,
            0xcf, 0xe8, 0xaa, 0xb7,
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
        opcode_compiler_evidence_id,
        opcode_vm_evidence_id,
        opcode_observation_id,
    ];
    return_evidence.sort();
    let mut diagnostic_coverage_evidence = vec![parser_evidence_id, diagnostic_evidence_id];
    diagnostic_coverage_evidence.extend([
        opcode_compiler_evidence_id,
        opcode_vm_evidence_id,
        opcode_observation_id,
        parser_outcomes_observation_id,
    ]);
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
                        b"weak scalar parsing/coercion, strict call-site mode, custom error handlers, object conversion, temporary release, and parser failure exits are only partially evidenced".to_vec(),
                    ),
                }],
                Terminator::Branch {
                    predicate: Predicate::Host(Bytes(b"strlen-string-parser-accepts-under-call-strictness".to_vec())),
                    yes: 1,
                    no: 5,
                },
            ),
            Block::new(
                vec![],
                Terminator::Branch {
                    predicate: Predicate::Host(Bytes(b"strlen-weak-null-deprecation".to_vec())),
                    yes: 2,
                    no: 3,
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
                    yes: 4,
                    no: 7,
                },
            ),
            {
                let mut block = Block::new(
                    vec![
                        Event::Invoke {
                            callable: Operand::Literal(Bytes(b"__toString".to_vec())),
                            arguments: vec![Operand::Argument(0)],
                            result: 0,
                        },
                    ],
                    Terminator::Goto(6),
                );
                block.exceptional.php_throw = Some(8);
                block
            },
            Block::new(
                vec![],
                Terminator::Throw(Operand::Unknown),
            ),
            Block::new(
                vec![
                    Event::Release {
                        value: Operand::Argument(0),
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
                        reason: Bytes(b"destructor output and re-entry effects remain conservatively open".to_vec()),
                    },
                ],
                Terminator::Return(Some(Operand::StringByteLength(0))),
            ),
            // Keep the invoked and non-invoked paths separate. Invoke defines
            // result slot 0 only on the Stringable path; merging it with the
            // non-Stringable path makes otherwise valid mixed calls fail with
            // ResultDefinedOnOnlySomePaths even though the return uses arg 0.
            Block::new(
                vec![],
                Terminator::Return(Some(Operand::StringByteLength(0))),
            ),
            // A callback throw still releases the cast argument before the
            // pending PHP exception resumes into the caller.
            Block::new(
                vec![
                    Event::Release {
                        value: Operand::Argument(0),
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
                        reason: Bytes(b"destructor output and re-entry effects during PHP-throw cleanup remain conservatively open".to_vec()),
                    },
                ],
                Terminator::Resume,
            ),
        ],
        // The closed Return fact is the nonnegative byte-length envelope of
        // the parsed string. Its expression does not consume the __toString Invoke
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
                                Domain::Invocation => b"normal, throwing, fatal, and suspending Stringable callback paths are observed; callback body behavior remains open".to_vec(),
                                Domain::Release => b"normal and PHP-throw cleanup release events are ordered; refcount aliases and destructor body effects remain conservative".to_vec(),
                                Domain::PhpThrow => b"parser rejection, callback throw, and temporary-destructor throw are represented; exact exception values and non-PHP completion replacement remain open".to_vec(),
                                Domain::NativeUnwind => b"native unwind is distinct from PHP throw and is not modeled by this graph".to_vec(),
                                Domain::Fatal => b"callback fatal/bailout is observed but its state and cleanup semantics remain open".to_vec(),
                                Domain::Suspension => b"Fiber suspension during callback is observed but its continuation semantics remain open".to_vec(),
                                _ => b"callback effects and non-normal behavior are not modeled in this slice".to_vec(),
                            })],
                    }
                },
                precision: Precision::Exact,
                dependencies: vec![],
                evidence: if matches!(domain, Domain::Invocation | Domain::Release | Domain::PhpThrow | Domain::UserWrite | Domain::Output | Domain::Fatal | Domain::Suspension) {
                    let mut evidence = return_evidence.clone();
                    evidence.extend([object_cast_evidence_id, callback_observation_id]);
                    if matches!(domain, Domain::Release | Domain::PhpThrow | Domain::UserWrite | Domain::Output) {
                        evidence.extend([
                            parser_release_evidence_id,
                            cast_release_evidence_id,
                            vm_release_evidence_id,
                            release_observation_id,
                        ]);
                    }
                    if domain == Domain::PhpThrow {
                        evidence.push(parser_outcomes_observation_id);
                    }
                    if matches!(domain, Domain::PhpThrow | Domain::Fatal | Domain::Suspension) {
                        evidence.push(abrupt_observation_id);
                    }
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
        os: Some(Bytes(b"linux".to_vec())),
        arch: Some(Bytes(b"x86_64".to_vec())),
        sapi: Some(Bytes(b"cli".to_vec())),
        zts: Some(false),
        debug: Some(false),
        ..TargetPredicate::any()
    };
    let mut evidence = vec![
        (source_evidence_id, source_evidence),
        (layout_evidence_id, layout_evidence),
        (parser_evidence_id, parser_evidence),
        (object_cast_evidence_id, object_cast_evidence),
        (callback_observation_id, callback_observation),
        (opcode_compiler_evidence_id, opcode_compiler_evidence),
        (opcode_vm_evidence_id, opcode_vm_evidence),
        (opcode_observation_id, opcode_observation),
        (parser_outcomes_observation_id, parser_outcomes_observation),
        (parser_release_evidence_id, parser_release_evidence),
        (cast_release_evidence_id, cast_release_evidence),
        (vm_release_evidence_id, vm_release_evidence),
        (release_observation_id, release_observation),
        (abrupt_observation_id, abrupt_observation),
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

fn validate_strlen_opcode_witness(
    source_path: &std::path::Path,
    repository_root: &std::path::Path,
    runtime: &serde_json::Value,
    callback_runtime: &serde_json::Value,
    profiles: &serde_json::Value,
    witness: &serde_json::Value,
) -> (EvidenceRecord, EvidenceRecord) {
    assert_eq!(witness["schema"].as_u64(), Some(1));
    assert_eq!(
        witness["source_commit"].as_str(),
        Some("e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03")
    );
    assert_eq!(
        witness["capture"]["command_template"].as_str(),
        Some("php -n -d opcache.enable_cli=1 -d opcache.opt_debug_level=0x20000 <fixture>")
    );
    assert_eq!(
        witness["capture"]["dump_stage"].as_str(),
        Some("OPcache optimizer dump after optimization")
    );
    assert_eq!(
        witness["capture"]["required_opcode"].as_str(),
        Some("STRLEN")
    );

    validate_opcode_capture_artifacts(
        repository_root,
        runtime,
        callback_runtime,
        profiles,
        witness,
    );

    let zend_dir = source_path
        .parent()
        .expect("locked Zend source has a parent directory");
    let compiler_path = zend_dir.join("zend_compile.c");
    let compiler_bytes = fs::read(&compiler_path).expect("read locked zend_compile.c");
    let compiler_digest: [u8; 32] = Sha256::digest(&compiler_bytes).into();
    assert_eq!(
        compiler_digest,
        decode_digest("b4cf524933f89b566a916cabceaa2969c45764be51c67c900d89d538074fc2d5"),
        "compiler source differs from locked PHP 8.5.4"
    );
    assert_eq!(
        witness["source"]["compiler"]["path"].as_str(),
        Some("Zend/zend_compile.c")
    );
    assert_eq!(
        witness["source"]["compiler"]["sha256"].as_str(),
        Some("b4cf524933f89b566a916cabceaa2969c45764be51c67c900d89d538074fc2d5")
    );
    let compiler_text = std::str::from_utf8(&compiler_bytes).expect("zend_compile.c is UTF-8");
    for (fact, expected) in [
        (
            "single_argument_only",
            "if (args->children != 1) {\n\t\treturn FAILURE;",
        ),
        (
            "constant_string_fold",
            "if (arg_node.op_type == IS_CONST && Z_TYPE(arg_node.u.constant) == IS_STRING) {\n\t\tresult->op_type = IS_CONST;\n\t\tZVAL_LONG(&result->u.constant, Z_STRLEN(arg_node.u.constant));",
        ),
        (
            "dynamic_opcode_emit",
            "zend_emit_op_tmp(result, ZEND_STRLEN, &arg_node, NULL);",
        ),
    ] {
        assert_eq!(
            witness["source"]["compiler"]["facts"][fact].as_str(),
            Some(expected)
        );
        assert!(
            compiler_text.contains(expected),
            "compiler source lost {fact}"
        );
    }
    assert!(compiler_text.contains(
        "if (zend_string_equals_literal(lcname, \"strlen\")) {\n\t\treturn zend_compile_func_strlen(result, args);"
    ));
    assert_eq!(
        witness["source"]["compiler"]["facts"]["special_dispatch"].as_str(),
        Some(
            "if (zend_string_equals_literal(lcname, \"strlen\")) {\n\t\treturn zend_compile_func_strlen(result, args);"
        )
    );

    let vm_path = zend_dir.join("zend_vm_def.h");
    let vm_bytes = fs::read(&vm_path).expect("read locked zend_vm_def.h");
    let vm_digest: [u8; 32] = Sha256::digest(&vm_bytes).into();
    assert_eq!(
        vm_digest,
        decode_digest("6403411fe62a39176fed58060e1c13a934951cd90f187b9d37b6b85532996423"),
        "VM handler source differs from locked PHP 8.5.4"
    );
    assert_eq!(
        witness["source"]["vm_handler"]["path"].as_str(),
        Some("Zend/zend_vm_def.h")
    );
    assert_eq!(
        witness["source"]["vm_handler"]["sha256"].as_str(),
        Some("6403411fe62a39176fed58060e1c13a934951cd90f187b9d37b6b85532996423")
    );
    assert_eq!(
        witness["source"]["vm_handler"]["opcode_number"].as_u64(),
        Some(121)
    );
    let vm_text = std::str::from_utf8(&vm_bytes).expect("zend_vm_def.h is UTF-8");
    let handler_start = vm_text
        .find("ZEND_VM_COLD_CONST_HANDLER(121, ZEND_STRLEN, CONST|TMPVAR|CV, ANY)")
        .expect("locked VM source contains the ZEND_STRLEN handler");
    let handler_end = vm_text[handler_start..]
        .find("ZEND_VM_HOT_NOCONST_HANDLER(123, ZEND_TYPE_CHECK")
        .map(|offset| handler_start + offset)
        .expect("ZEND_STRLEN handler has a following opcode handler");
    let handler = &vm_text[handler_start..handler_end];
    assert_eq!(
        witness["source"]["vm_handler"]["handler"].as_str(),
        Some("ZEND_VM_COLD_CONST_HANDLER(121, ZEND_STRLEN, CONST|TMPVAR|CV, ANY)")
    );
    for (fact, expected) in [
        (
            "string_fast_path",
            "if (EXPECTED(Z_TYPE_P(value) == IS_STRING)) {\n\t\tZVAL_LONG(EX_VAR(opline->result.var), Z_STRLEN_P(value));",
        ),
        (
            "weak_parser_result",
            "if (zend_parse_arg_str_weak(&tmp, &str, 1)) {\n\t\t\t\t\tZVAL_LONG(EX_VAR(opline->result.var), ZSTR_LEN(str));",
        ),
        ("null_result", "ZVAL_LONG(EX_VAR(opline->result.var), 0);"),
        (
            "failed_result_is_undefined",
            "ZVAL_UNDEF(EX_VAR(opline->result.var));",
        ),
        (
            "strictness_gate",
            "strict = EX_USES_STRICT_TYPES();\n\t\tdo {\n\t\t\tif (EXPECTED(!strict)) {",
        ),
        ("weak_parser", "zend_parse_arg_str_weak(&tmp, &str, 1)"),
        (
            "null_diagnostic",
            "zend_error(E_DEPRECATED,\n\t\t\t\t\t\t\"strlen(): Passing null to parameter #1 ($string) of type string is deprecated\");",
        ),
        (
            "type_error",
            "zend_type_error(\"strlen(): Argument #1 ($string) must be of type string, %s given\", zend_zval_value_name(value));",
        ),
        (
            "exception_aware_dispatch",
            "ZEND_VM_NEXT_OPCODE_CHECK_EXCEPTION();",
        ),
    ] {
        assert_eq!(
            witness["source"]["vm_handler"]["facts"][fact].as_str(),
            Some(expected)
        );
        assert!(handler.contains(expected), "VM handler source lost {fact}");
    }

    let compiler_evidence = EvidenceRecord {
        kind: EvidenceKind::SourceTransfer,
        source_digest: Digest(compiler_digest),
        site: Some(Bytes(
            b"Zend/zend_compile.c:zend_try_compile_special_func_ex:strlen-to-ZEND_STRLEN".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-zend-strlen-compiler-opcode-v1".to_vec()),
        assumptions: vec![],
        dependencies: vec![],
    };
    let compiler_id = compiler_evidence.id().expect("compiler evidence encodes");
    let vm_evidence = EvidenceRecord {
        kind: EvidenceKind::SourceTransfer,
        source_digest: Digest(vm_digest),
        site: Some(Bytes(
            b"Zend/zend_vm_def.h:ZEND_STRLEN:121:strict-weak-diagnostic-error".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-zend-strlen-vm-handler-v1".to_vec()),
        assumptions: vec![],
        dependencies: vec![compiler_id],
    };
    (compiler_evidence, vm_evidence)
}

fn validate_opcode_capture_artifacts(
    repository_root: &std::path::Path,
    runtime: &serde_json::Value,
    callback_runtime: &serde_json::Value,
    profiles: &serde_json::Value,
    witness: &serde_json::Value,
) {
    assert_eq!(
        witness["profile_records"]["normal"]["binary_sha256"],
        profiles["normal"]["binary_sha256"]
    );
    assert_eq!(
        witness["profile_records"]["call"]["binary_sha256"],
        profiles["call"]["binary_sha256"]
    );
    let cases = [
        ("weak_scalars", &runtime["weak_fixture"], 5),
        ("strict_scalars", &runtime["strict_fixture"], 1),
        ("stringable_callbacks", &callback_runtime["fixture"], 2),
    ];
    for (case_name, fixture, expected_opcodes) in cases {
        let case = &witness["cases"][case_name];
        assert_eq!(case["fixture"], fixture["path"]);
        assert_eq!(case["fixture_sha256"], fixture["sha256"]);
        let fixture_path = repository_root.join(
            fixture["path"]
                .as_str()
                .expect("opcode fixture path is recorded"),
        );
        let fixture_bytes = fs::read(fixture_path).expect("read opcode runtime fixture");
        assert_eq!(
            Sha256::digest(&fixture_bytes).as_slice(),
            decode_digest(
                fixture["sha256"]
                    .as_str()
                    .expect("opcode fixture digest is recorded")
            )
        );
        for profile in ["normal", "call"] {
            let capture = &case["profiles"][profile];
            assert_eq!(capture["vm_kind"], profiles[profile]["vm_kind"]);
            assert_eq!(capture["binary_sha256"], profiles[profile]["binary_sha256"]);
            let dump = capture["dump"]
                .as_str()
                .expect("opcode dump text is recorded");
            assert_eq!(
                Sha256::digest(dump.as_bytes()).as_slice(),
                decode_digest(
                    capture["dump_sha256"]
                        .as_str()
                        .expect("opcode dump digest is recorded")
                ),
                "opcode dump bytes changed for {case_name}/{profile}"
            );
            assert!(dump.contains("<repo>/tools/php-catalog/experiments/comparison/"));
            let actual_opcode_lines = dump
                .lines()
                .filter(|line| line.contains(" = STRLEN "))
                .count();
            assert_eq!(
                actual_opcode_lines, expected_opcodes,
                "{case_name}/{profile}"
            );
        }
        assert_eq!(
            case["profiles"]["normal"]["dump"], case["profiles"]["call"]["dump"],
            "TAILCALL/CALL compiler dumps disagree for {case_name}"
        );
        assert_eq!(
            case["profiles"]["normal"]["stdout_sha256"], case["profiles"]["call"]["stdout_sha256"],
            "TAILCALL/CALL outputs disagree for {case_name}"
        );
        let expected_stdout = if case_name == "stringable_callbacks" {
            callback_runtime["result"]["output_files_identical"]
                .as_bool()
                .filter(|same| *same)
                .expect("callback TAILCALL/CALL output identity is observed");
            callback_runtime["fixture"]["output_sha256"].as_str()
        } else {
            fixture["output_sha256"].as_str()
        }
        .expect("expected runtime output digest");
        assert_eq!(
            case["profiles"]["normal"]["stdout_sha256"].as_str(),
            Some(expected_stdout)
        );
        for (profile, output_key) in [("normal", "output"), ("call", "call_output")] {
            let output_key = if case_name == "stringable_callbacks" {
                if profile == "normal" {
                    "tailcall_output"
                } else {
                    "call_output"
                }
            } else {
                output_key
            };
            let path = repository_root.join(
                fixture[output_key]
                    .as_str()
                    .expect("runtime output artifact path is recorded"),
            );
            let output = fs::read(path).expect("read paired runtime output artifact");
            assert_eq!(
                Sha256::digest(&output).as_slice(),
                decode_digest(
                    witness["cases"][case_name]["profiles"][profile]["stdout_sha256"]
                        .as_str()
                        .expect("runtime stdout digest is recorded")
                )
                .as_slice(),
                "runtime output changed for {case_name}/{profile}"
            );
        }
    }
}

fn validate_strlen_release_witness(
    source_path: &std::path::Path,
    repository_root: &std::path::Path,
    profiles: &serde_json::Value,
    witness: &serde_json::Value,
) -> (EvidenceRecord, EvidenceRecord, EvidenceRecord) {
    assert_eq!(witness["schema"].as_u64(), Some(1));
    assert_eq!(
        witness["source_commit"].as_str(),
        Some("e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03")
    );
    validate_strlen_release_artifacts(repository_root, profiles, witness);

    let zend_dir = source_path
        .parent()
        .expect("locked Zend source has a parent directory");
    let parser_path = zend_dir.join("zend_API.c");
    let parser_bytes = fs::read(&parser_path).expect("read locked Zend parser source");
    let parser_digest: [u8; 32] = Sha256::digest(&parser_bytes).into();
    assert_eq!(
        parser_digest,
        decode_digest("ba6d163b75905a3fc122186452a4cc08ab06b25e7fc51b22d830b38d5daca23b"),
        "Zend parser source differs from locked PHP 8.5.4"
    );
    assert_eq!(witness["sources"]["parser"]["path"], "Zend/zend_API.c");
    assert_eq!(
        witness["sources"]["parser"]["sha256"],
        "ba6d163b75905a3fc122186452a4cc08ab06b25e7fc51b22d830b38d5daca23b"
    );
    let parser_text = std::str::from_utf8(&parser_bytes).expect("Zend parser source is UTF-8");
    let parser_start = parser_text
        .find("zend_parse_arg_str_weak(zval *arg")
        .expect("locked weak string parser exists");
    let parser_end = parser_text[parser_start..]
        .find("/* }}} */")
        .map(|offset| parser_start + offset)
        .expect("weak parser has a section terminator");
    for fact in witness["sources"]["parser"]["facts"]
        .as_array()
        .expect("parser source facts are recorded")
    {
        let fact = fact.as_str().expect("parser source fact is text");
        assert!(
            parser_text[parser_start..parser_end].contains(fact),
            "weak parser source lost {fact}"
        );
    }

    let cast_path = zend_dir.join("zend_object_handlers.c");
    let cast_bytes = fs::read(&cast_path).expect("read locked object-cast source");
    let cast_digest: [u8; 32] = Sha256::digest(&cast_bytes).into();
    assert_eq!(
        cast_digest,
        decode_digest("418536503bfde765c5dff821f689643e0f6ed1d38365f354cbb164633fce10b4"),
        "Zend object-cast source differs from locked PHP 8.5.4"
    );
    assert_eq!(
        witness["sources"]["object_cast"]["path"],
        "Zend/zend_object_handlers.c"
    );
    assert_eq!(
        witness["sources"]["object_cast"]["sha256"],
        "418536503bfde765c5dff821f689643e0f6ed1d38365f354cbb164633fce10b4"
    );
    let cast_text = std::str::from_utf8(&cast_bytes).expect("object-cast source is UTF-8");
    let cast_start = cast_text
        .find("ZEND_API zend_result zend_std_cast_object_tostring")
        .expect("locked Stringable cast handler exists");
    let cast_end = cast_text[cast_start..]
        .find("/* }}} */")
        .map(|offset| cast_start + offset)
        .expect("Stringable cast handler has a section terminator");
    for fact in witness["sources"]["object_cast"]["facts"]
        .as_array()
        .expect("object-cast source facts are recorded")
    {
        let fact = fact.as_str().expect("object-cast source fact is text");
        assert!(
            cast_text[cast_start..cast_end].contains(fact),
            "Stringable cast source lost {fact}"
        );
    }

    let vm_path = zend_dir.join("zend_vm_def.h");
    let vm_bytes = fs::read(&vm_path).expect("read locked ZEND_STRLEN VM handler");
    let vm_digest: [u8; 32] = Sha256::digest(&vm_bytes).into();
    assert_eq!(
        vm_digest,
        decode_digest("6403411fe62a39176fed58060e1c13a934951cd90f187b9d37b6b85532996423"),
        "Zend VM source differs from locked PHP 8.5.4"
    );
    assert_eq!(
        witness["sources"]["vm_handler"]["path"],
        "Zend/zend_vm_def.h"
    );
    assert_eq!(
        witness["sources"]["vm_handler"]["sha256"],
        "6403411fe62a39176fed58060e1c13a934951cd90f187b9d37b6b85532996423"
    );
    let vm_text = std::str::from_utf8(&vm_bytes).expect("Zend VM source is UTF-8");
    let vm_start = vm_text
        .find("ZEND_VM_COLD_CONST_HANDLER(121, ZEND_STRLEN, CONST|TMPVAR|CV, ANY)")
        .expect("locked ZEND_STRLEN handler exists");
    let vm_end = vm_text[vm_start..]
        .find("ZEND_VM_HOT_NOCONST_HANDLER(123, ZEND_TYPE_CHECK")
        .map(|offset| vm_start + offset)
        .expect("ZEND_STRLEN handler has a section terminator");
    for fact in witness["sources"]["vm_handler"]["facts"]
        .as_array()
        .expect("VM release facts are recorded")
    {
        let fact = fact.as_str().expect("VM release fact is text");
        assert!(
            vm_text[vm_start..vm_end].contains(fact),
            "ZEND_STRLEN handler lost {fact}"
        );
    }

    let destructor_path = zend_dir.join("zend_objects.c");
    let destructor_bytes = fs::read(&destructor_path).expect("read locked destructor source");
    let destructor_digest: [u8; 32] = Sha256::digest(&destructor_bytes).into();
    assert_eq!(
        destructor_digest,
        decode_digest("d8ce93d530d740d0cc5063950cc9309057278df50accdbef429d8ec63e27f818"),
        "Zend destructor source differs from locked PHP 8.5.4"
    );
    assert_eq!(
        witness["sources"]["destructor"]["path"],
        "Zend/zend_objects.c"
    );
    assert_eq!(
        witness["sources"]["destructor"]["sha256"],
        "d8ce93d530d740d0cc5063950cc9309057278df50accdbef429d8ec63e27f818"
    );
    let destructor_text =
        std::str::from_utf8(&destructor_bytes).expect("Zend destructor source is UTF-8");
    let destructor_start = destructor_text
        .find("ZEND_API void zend_objects_destroy_object")
        .expect("locked object destructor transfer exists");
    let destructor_end = destructor_text[destructor_start..]
        .find("ZEND_API zend_object* ZEND_FASTCALL zend_objects_new")
        .map(|offset| destructor_start + offset)
        .expect("object destructor transfer has a section terminator");
    for fact in witness["sources"]["destructor"]["facts"]
        .as_array()
        .expect("destructor source facts are recorded")
    {
        let fact = fact.as_str().expect("destructor source fact is text");
        assert!(
            destructor_text[destructor_start..destructor_end].contains(fact),
            "object destructor source lost {fact}"
        );
    }

    let parser_evidence = EvidenceRecord {
        kind: EvidenceKind::SourceTransfer,
        source_digest: Digest(parser_digest),
        site: Some(Bytes(
            b"Zend/zend_API.c:zend_parse_arg_str_weak:OBJ_RELEASE:temporary-string-replacement"
                .to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-zend-strlen-parser-release-v1".to_vec()),
        assumptions: vec![],
        dependencies: vec![],
    };
    let parser_id = parser_evidence
        .id()
        .expect("parser release evidence encodes");
    let cast_evidence = EvidenceRecord {
        kind: EvidenceKind::SourceTransfer,
        source_digest: Digest(cast_digest),
        site: Some(Bytes(
            b"Zend/zend_object_handlers.c:zend_std_cast_object_tostring:callback-release-and-result"
                .to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-zend-strlen-object-cast-release-v1".to_vec()),
        assumptions: vec![],
        dependencies: vec![parser_id],
    };
    let cast_id = cast_evidence.id().expect("cast release evidence encodes");
    let mut vm_release_dependencies = vec![parser_id, cast_id];
    vm_release_dependencies.sort();
    let vm_evidence = EvidenceRecord {
        kind: EvidenceKind::SourceTransfer,
        source_digest: Digest(vm_digest),
        site: Some(Bytes(
            b"Zend/zend_vm_def.h:ZEND_STRLEN:ZVAL_COPY-and-zval_ptr_dtor-temporary-lifetime"
                .to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-zend-strlen-vm-release-v1".to_vec()),
        assumptions: vec![],
        dependencies: vm_release_dependencies,
    };
    (parser_evidence, cast_evidence, vm_evidence)
}

fn validate_strlen_parser_outcomes_witness(
    repository_root: &std::path::Path,
    profiles: &serde_json::Value,
    witness: &serde_json::Value,
    witness_bytes: &[u8],
    evidence_dependencies: &[Digest],
) -> EvidenceRecord {
    assert_eq!(witness["schema"].as_u64(), Some(1));
    assert_eq!(
        witness["source_commit"].as_str(),
        Some("e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03")
    );
    assert_eq!(witness["result"]["paired_outputs_identical"], true);
    assert_eq!(witness["result"]["paired_dumps_identical"], true);

    for (profile, profile_record) in [("tailcall", "normal"), ("call", "call")] {
        assert_eq!(
            witness["profiles"][profile]["vm_kind"], profiles[profile_record]["vm_kind"],
            "parser oracle uses a different VM profile"
        );
        assert_eq!(
            witness["profiles"][profile]["binary_sha256"],
            profiles[profile_record]["binary_sha256"],
            "parser oracle uses an unlocked PHP binary"
        );
    }

    let expected_events = [
        (
            "weak",
            serde_json::json!([
                "handler:E_DEPRECATED",
                "direct-null:caught:handler-throw",
                "handler:E_DEPRECATED",
                "dynamic-null:caught:handler-throw",
                "direct-array:TypeError",
                "dynamic-array:TypeError"
            ]),
        ),
        (
            "strict",
            serde_json::json!([
                "direct-null:TypeError",
                "dynamic-null:TypeError",
                "direct-array:TypeError",
                "dynamic-array:TypeError"
            ]),
        ),
    ];
    for (case, events) in expected_events {
        let mut reference_dump: Option<&str> = None;
        let mut reference_stdout: Option<&str> = None;
        for profile in ["tailcall", "call"] {
            let record = &witness["cases"][case][profile];
            let fixture = record["fixture"]
                .as_str()
                .expect("parser oracle fixture path is recorded");
            assert_eq!(
                fixture,
                format!(
                    "tools/php-catalog/experiments/comparison/runtime-strlen-parser-outcomes-{case}.php"
                )
            );
            let fixture_bytes = fs::read(repository_root.join(fixture))
                .unwrap_or_else(|error| panic!("read parser oracle fixture {fixture}: {error}"));
            assert_eq!(
                hex_digest(Sha256::digest(&fixture_bytes).as_slice()),
                record["fixture_sha256"]
                    .as_str()
                    .expect("fixture digest is text"),
                "parser oracle fixture changed"
            );
            let expected_stdout = record["expected_stdout"]
                .as_str()
                .expect("expected oracle stdout is text");
            let expected_json: serde_json::Value =
                serde_json::from_str(expected_stdout).expect("expected oracle stdout is JSON");
            assert_eq!(expected_json["events"], events);
            let stdout = format!("{expected_stdout}\n").into_bytes();
            let output_path = record["output"]
                .as_str()
                .expect("parser oracle output path is recorded");
            let actual_output = fs::read(repository_root.join(output_path))
                .unwrap_or_else(|error| panic!("read parser oracle output {output_path}: {error}"));
            assert_eq!(actual_output, stdout, "parser oracle output differs");
            assert_eq!(
                hex_digest(Sha256::digest(&actual_output).as_slice()),
                record["stdout_sha256"]
                    .as_str()
                    .expect("stdout digest is text")
            );
            if let Some(reference) = reference_stdout {
                assert_eq!(expected_stdout, reference, "paired PHP outputs differ");
            } else {
                reference_stdout = Some(expected_stdout);
            }

            assert_eq!(record["vm_kind"], witness["profiles"][profile]["vm_kind"]);
            assert_eq!(
                record["binary_sha256"],
                witness["profiles"][profile]["binary_sha256"]
            );
            let dump = record["dump"]
                .as_str()
                .expect("normalized opcode dump is text");
            let fixture_in_dump = format!("<repo>/{fixture}");
            assert!(dump.contains(&fixture_in_dump));
            assert_eq!(dump.matches(" = STRLEN ").count(), 2);
            assert_eq!(dump.matches("INIT_DYNAMIC_CALL").count(), 2);
            assert_eq!(
                hex_digest(Sha256::digest(dump.as_bytes()).as_slice()),
                record["dump_sha256"].as_str().expect("dump digest is text")
            );
            if let Some(reference) = reference_dump {
                assert_eq!(dump, reference, "paired PHP opcode dumps differ");
            } else {
                reference_dump = Some(dump);
            }
        }
    }

    let mut dependencies = evidence_dependencies.to_vec();
    dependencies.sort();
    dependencies.dedup();
    EvidenceRecord {
        kind: EvidenceKind::Observation,
        source_digest: Digest(Sha256::digest(witness_bytes).into()),
        site: Some(Bytes(
            b"PHP 8.5.4 TAILCALL/CALL parser outcomes: weak handler throw, strict rejection, and array TypeError across direct opcode and dynamic native calls".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-strlen-parser-outcomes-observation-v1".to_vec()),
        assumptions: vec![],
        dependencies,
    }
}

fn validate_strlen_abrupt_outcomes_witness(
    source_path: &std::path::Path,
    repository_root: &std::path::Path,
    profiles: &serde_json::Value,
    witness: &serde_json::Value,
    witness_bytes: &[u8],
    evidence_dependencies: &[Digest],
) -> EvidenceRecord {
    assert_eq!(
        witness["runtime_profile"],
        serde_json::json!({
            "os": "linux",
            "arch": "x86_64",
            "sapi": "cli",
            "zts": false,
            "debug": false
        }),
        "abrupt-outcome binaries only establish this runtime profile"
    );
    assert_eq!(witness["schema"].as_u64(), Some(1));
    assert_eq!(
        witness["source_commit"].as_str(),
        Some("e5e09d06c13e2d280a4c5066ac3d62d88dbf5b03")
    );
    assert_eq!(witness["result"]["paired_outputs_identical"], true);
    assert_eq!(witness["result"]["paired_dumps_identical"], true);
    assert_eq!(
        witness["scope"]["not_modeled"],
        "NativeUnwind has no source/runtime witness for this PHP C boundary and remains open"
    );

    let source_root = source_path
        .parent()
        .and_then(std::path::Path::parent)
        .expect("resolve locked PHP source root");
    for (name, path, digest, facts) in [
        (
            "trigger_error",
            "Zend/zend_builtin_functions.c",
            "0d4477170a78540131893a8cfca2b9d1033c79eab754f9ca3d85e05ae6919505",
            vec![
                "ZEND_FUNCTION(trigger_error)",
                "case E_USER_ERROR:",
                "zend_error_zstr_at(error_type, zend_get_executed_filename_ex(), zend_get_executed_lineno(), message);",
            ],
        ),
        (
            "fatal_dispatch",
            "Zend/zend.c",
            "13e0fb4e816324e1a5ebd3b9636515476ba45196999aa46370a7d7c1e022fef2",
            vec![
                "ZEND_API ZEND_COLD void zend_error_zstr_at(",
                "case E_USER_ERROR:",
                "zend_error_cb(orig_type, error_filename, error_lineno, message);",
            ],
        ),
        (
            "fatal_bailout",
            "main/main.c",
            "865b79a5dbc107e64a09559873818a4ed67a0e98366b6e44d6b06c52f6549bcb",
            vec![
                "case E_USER_ERROR:",
                "EG(exit_status) = 255;",
                "zend_bailout();",
            ],
        ),
        (
            "fiber_suspend",
            "Zend/zend_fibers.c",
            "84ff0df942a2bdb295c647f80dd8fae41da65ca588927c0b8a417a68672177af",
            vec![
                "ZEND_METHOD(Fiber, suspend)",
                "zend_fiber_transfer transfer = zend_fiber_suspend_internal(fiber, value);",
                "zend_fiber_delegate_transfer_result(&transfer, EG(current_execute_data), return_value);",
            ],
        ),
        (
            "stringable_cast",
            "Zend/zend_object_handlers.c",
            "418536503bfde765c5dff821f689643e0f6ed1d38365f354cbb164633fce10b4",
            vec![
                "zend_call_known_instance_method_with_0_params(ce->__tostring, readobj, &retval);",
                "if (EXPECTED(Z_TYPE(retval) == IS_STRING)) {",
            ],
        ),
    ] {
        let source = fs::read(source_root.join(path))
            .unwrap_or_else(|error| panic!("read locked abrupt source {path}: {error}"));
        assert_eq!(hex_digest(Sha256::digest(&source).as_slice()), digest);
        assert_eq!(witness["sources"][name]["path"], path);
        assert_eq!(witness["sources"][name]["sha256"], digest);
        assert_eq!(witness["sources"][name]["facts"], serde_json::json!(facts));
        let source_text = std::str::from_utf8(&source).expect("locked PHP source is UTF-8");
        for fact in facts {
            assert!(
                source_text.contains(fact),
                "missing Zend source fact: {fact}"
            );
        }
    }

    let fixture_path = witness["fixture"]["path"]
        .as_str()
        .expect("abrupt fixture path is text");
    assert_eq!(
        fixture_path,
        "tools/php-catalog/experiments/comparison/runtime-strlen-abrupt-outcomes.php"
    );
    let fixture = fs::read(repository_root.join(fixture_path)).expect("read abrupt fixture");
    assert_eq!(
        hex_digest(Sha256::digest(&fixture).as_slice()),
        witness["fixture"]["sha256"]
    );

    let expected = serde_json::json!({
        "direct_suspension": [
            "fiber:before:direct", "direct:cast-before-suspend",
            "outside:suspended:direct:suspended", "outside:is-suspended:yes",
            "direct:cast-after-resume:resume-value", "fiber:after:direct:1",
            "outside:is-terminated:yes", "outside:return:1"
        ],
        "dynamic_suspension": [
            "fiber:before:dynamic", "dynamic:cast-before-suspend",
            "outside:suspended:dynamic:suspended", "outside:is-suspended:yes",
            "dynamic:cast-after-resume:resume-value", "fiber:after:dynamic:1",
            "outside:is-terminated:yes", "outside:return:1"
        ],
        "direct_throw": [
            "throw:cast", "throw:destruct",
            "throw:caught:strlen-throw-probe", "throw:finally"
        ],
        "dynamic_throw": [
            "throw:cast", "throw:caught:strlen-throw-probe",
            "throw:finally", "throw:destruct"
        ],
        "fatal_child_status": 255,
        "fatal_reported": true,
        "fatal_shutdown": true,
        "fatal_continued": false,
        "fatal_finally": false
    });
    let mut reference_stdout: Option<&str> = None;
    let mut reference_dump: Option<&str> = None;
    for (profile, profile_record) in [("tailcall", "normal"), ("call", "call")] {
        assert_eq!(
            witness["profiles"][profile]["vm_kind"],
            profiles[profile_record]["vm_kind"]
        );
        assert_eq!(
            witness["profiles"][profile]["binary_sha256"],
            profiles[profile_record]["binary_sha256"]
        );
        let record = &witness["cases"][profile];
        assert_eq!(record["vm_kind"], witness["profiles"][profile]["vm_kind"]);
        assert_eq!(
            record["binary_sha256"],
            witness["profiles"][profile]["binary_sha256"]
        );
        let stdout = record["expected_stdout"]
            .as_str()
            .expect("abrupt stdout is text");
        let value: serde_json::Value = serde_json::from_str(stdout).expect("abrupt output JSON");
        assert_eq!(value, expected);
        if let Some(reference) = reference_stdout {
            assert_eq!(stdout, reference, "paired abrupt outputs differ");
        } else {
            reference_stdout = Some(stdout);
        }
        let output_path = record["output"]
            .as_str()
            .expect("abrupt output path is text");
        let output = fs::read(repository_root.join(output_path))
            .unwrap_or_else(|error| panic!("read abrupt output {output_path}: {error}"));
        assert_eq!(output, format!("{stdout}\n").as_bytes());
        assert_eq!(
            hex_digest(Sha256::digest(&output).as_slice()),
            record["stdout_sha256"]
        );
        let dump = record["dump"].as_str().expect("abrupt dump is text");
        assert!(dump.contains(&format!("<repo>/{fixture_path}")));
        assert_eq!(dump.matches(" = STRLEN ").count(), 3);
        assert_eq!(dump.matches("INIT_DYNAMIC_CALL").count(), 2);
        assert_eq!(
            hex_digest(Sha256::digest(dump.as_bytes()).as_slice()),
            record["dump_sha256"]
        );
        if let Some(reference) = reference_dump {
            assert_eq!(dump, reference, "paired abrupt opcode dumps differ");
        } else {
            reference_dump = Some(dump);
        }
    }

    let mut dependencies = evidence_dependencies.to_vec();
    dependencies.sort();
    dependencies.dedup();
    EvidenceRecord {
        kind: EvidenceKind::Observation,
        source_digest: Digest(Sha256::digest(witness_bytes).into()),
        site: Some(Bytes(
            b"PHP 8.5.4 Stringable callback PHP throw, fatal bailout, and Fiber suspension across direct and dynamic strlen calls".to_vec(),
        )),
        rule_version: Bytes(b"php-8.5.4-strlen-abrupt-outcomes-observation-v1".to_vec()),
        assumptions: vec![],
        dependencies,
    }
}

fn validate_strlen_release_artifacts(
    repository_root: &std::path::Path,
    profiles: &serde_json::Value,
    witness: &serde_json::Value,
) {
    let opcode = &witness["opcode_capture"];
    assert_eq!(
        opcode["command_template"].as_str(),
        Some("php -n -d opcache.enable_cli=1 -d opcache.opt_debug_level=0x20000 <fixture>")
    );
    assert_eq!(
        opcode["stage"].as_str(),
        Some("OPcache optimizer dump after optimization")
    );
    assert_eq!(opcode["opcode"].as_str(), Some("STRLEN"));
    assert_eq!(opcode["count"].as_u64(), Some(5));
    assert_eq!(opcode["outputs_identical"].as_bool(), Some(true));
    let dump = opcode["dump"]
        .as_str()
        .expect("release opcode dump is recorded");
    assert_eq!(
        Sha256::digest(dump.as_bytes()).as_slice(),
        decode_digest(
            opcode["dump_sha256"]
                .as_str()
                .expect("release opcode dump hash is recorded")
        ),
        "release opcode dump changed"
    );
    assert!(
        dump.contains("<repo>/tools/php-catalog/experiments/comparison/runtime-strlen-release.php")
    );
    assert_eq!(
        dump.lines()
            .filter(|line| line.contains(" = STRLEN "))
            .count(),
        5
    );
    assert!(dump.contains("INIT_DYNAMIC_CALL 1 CV11($callable)"));
    let fixture = &witness["fixture"];
    assert_eq!(
        fixture["path"].as_str(),
        Some("tools/php-catalog/experiments/comparison/runtime-strlen-release.php")
    );
    let fixture_bytes = fs::read(
        repository_root.join(
            fixture["path"]
                .as_str()
                .expect("release fixture path is recorded"),
        ),
    )
    .expect("read release runtime fixture");
    assert_eq!(
        Sha256::digest(&fixture_bytes).as_slice(),
        decode_digest(
            fixture["sha256"]
                .as_str()
                .expect("release fixture digest is recorded")
        ),
        "release runtime fixture changed"
    );
    for profile in ["normal", "call"] {
        let capture = &witness["profiles"][profile];
        assert_eq!(capture["vm_kind"], profiles[profile]["vm_kind"]);
        assert_eq!(capture["binary_sha256"], profiles[profile]["binary_sha256"]);
        let output_path = capture["output_path"]
            .as_str()
            .expect("release runtime output path is recorded");
        let output = fs::read(repository_root.join(output_path))
            .expect("read paired release runtime output");
        assert_eq!(
            Sha256::digest(&output).as_slice(),
            decode_digest(
                capture["stdout_sha256"]
                    .as_str()
                    .expect("release runtime output digest is recorded")
            ),
            "release runtime output changed for {profile}"
        );
    }
    let normal = &witness["profiles"]["normal"]["stdout_sha256"];
    let call = &witness["profiles"]["call"]["stdout_sha256"];
    assert_eq!(normal, call, "TAILCALL/CALL release observations disagree");
    assert_eq!(normal, &witness["stdout_sha256"]);
    assert_eq!(witness["outputs_identical"].as_bool(), Some(true));
    assert_eq!(
        witness["output"],
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(
                repository_root.join(
                    witness["profiles"]["normal"]["output_path"]
                        .as_str()
                        .unwrap()
                )
            )
            .expect("read normalized release output"),
        )
        .expect("release output is JSON")
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

    #[test]
    fn direct_opcode_witness_rejects_dump_and_runtime_drift() {
        let repository_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("resolve repository root");
        let read_json = |path: &str| {
            serde_json::from_slice(
                &fs::read(repository_root.join(path)).expect("read locked opcode witness input"),
            )
            .expect("locked opcode witness input is JSON")
        };
        let runtime = read_json(
            "tools/php-catalog/experiments/comparison/results/runtime/strlen-weak-scalars-8.5.4.json",
        );
        let callback = read_json(
            "tools/php-catalog/experiments/comparison/results/runtime/tailcall-call-reference-8.5.4.json",
        );
        let profiles =
            read_json("tools/php-catalog/experiments/comparison/results/runtime/profiles.json");
        let witness_path = "tools/php-catalog/experiments/comparison/results/evaluation/strlen-opcode-witness-8.5.4.json";
        let witness = read_json(witness_path);
        validate_opcode_capture_artifacts(
            &repository_root,
            &runtime,
            &callback,
            &profiles,
            &witness,
        );

        let mut changed_dump = witness.clone();
        changed_dump["cases"]["weak_scalars"]["profiles"]["normal"]["dump"] =
            serde_json::json!("no STRLEN opcode");
        assert!(
            std::panic::catch_unwind(|| {
                validate_opcode_capture_artifacts(
                    &repository_root,
                    &runtime,
                    &callback,
                    &profiles,
                    &changed_dump,
                )
            })
            .is_err()
        );

        let mut changed_mapping = witness.clone();
        changed_mapping["source"]["vm_handler"]["opcode_number"] = serde_json::json!(122);
        assert!(
            std::panic::catch_unwind(|| {
                assert_eq!(
                    changed_mapping["source"]["vm_handler"]["opcode_number"],
                    121
                );
            })
            .is_err()
        );

        let mut changed_output = runtime.clone();
        changed_output["weak_fixture"]["output_sha256"] = serde_json::json!("00".repeat(32));
        assert!(
            std::panic::catch_unwind(|| {
                validate_opcode_capture_artifacts(
                    &repository_root,
                    &changed_output,
                    &callback,
                    &profiles,
                    &witness,
                )
            })
            .is_err()
        );
    }

    #[test]
    fn release_witness_rejects_fixture_and_paired_output_drift() {
        let repository_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("resolve repository root");
        let read_json = |path: &str| {
            serde_json::from_slice(
                &fs::read(repository_root.join(path)).expect("read locked release witness input"),
            )
            .expect("locked release witness input is JSON")
        };
        let profiles =
            read_json("tools/php-catalog/experiments/comparison/results/runtime/profiles.json");
        let witness = read_json(
            "tools/php-catalog/experiments/comparison/results/evaluation/strlen-release-witness-8.5.4.json",
        );
        validate_strlen_release_artifacts(&repository_root, &profiles, &witness);

        let mut changed_fixture = witness.clone();
        changed_fixture["fixture"]["sha256"] = serde_json::json!("00".repeat(32));
        assert!(
            std::panic::catch_unwind(|| {
                validate_strlen_release_artifacts(&repository_root, &profiles, &changed_fixture)
            })
            .is_err()
        );

        let mut changed_output = witness.clone();
        changed_output["profiles"]["call"]["stdout_sha256"] = serde_json::json!("00".repeat(32));
        assert!(
            std::panic::catch_unwind(|| {
                validate_strlen_release_artifacts(&repository_root, &profiles, &changed_output)
            })
            .is_err()
        );

        let mut changed_opcode = witness.clone();
        changed_opcode["opcode_capture"]["dump"] = serde_json::json!("no STRLEN opcode");
        assert!(
            std::panic::catch_unwind(|| {
                validate_strlen_release_artifacts(&repository_root, &profiles, &changed_opcode)
            })
            .is_err()
        );
    }

    #[test]
    fn parser_outcome_witness_rejects_fixture_output_and_opcode_drift() {
        let repository_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("resolve repository root");
        let profiles: serde_json::Value = serde_json::from_slice(
            &fs::read(
                repository_root
                    .join("tools/php-catalog/experiments/comparison/results/runtime/profiles.json"),
            )
            .expect("read PHP profiles"),
        )
        .expect("PHP profiles are JSON");
        let path = repository_root.join(
            "tools/php-catalog/experiments/comparison/results/evaluation/strlen-parser-outcomes-witness-8.5.4.json",
        );
        let bytes = fs::read(path).expect("read parser outcome witness");
        let witness: serde_json::Value =
            serde_json::from_slice(&bytes).expect("parser outcome witness is JSON");
        validate_strlen_parser_outcomes_witness(&repository_root, &profiles, &witness, &bytes, &[]);

        let mut changed_fixture = witness.clone();
        changed_fixture["cases"]["weak"]["tailcall"]["fixture_sha256"] =
            serde_json::json!("00".repeat(32));
        assert!(
            std::panic::catch_unwind(|| {
                validate_strlen_parser_outcomes_witness(
                    &repository_root,
                    &profiles,
                    &changed_fixture,
                    &bytes,
                    &[],
                )
            })
            .is_err()
        );

        let mut changed_output = witness.clone();
        changed_output["cases"]["strict"]["call"]["stdout_sha256"] =
            serde_json::json!("00".repeat(32));
        assert!(
            std::panic::catch_unwind(|| {
                validate_strlen_parser_outcomes_witness(
                    &repository_root,
                    &profiles,
                    &changed_output,
                    &bytes,
                    &[],
                )
            })
            .is_err()
        );

        let mut changed_dump = witness;
        changed_dump["cases"]["weak"]["tailcall"]["dump"] =
            serde_json::json!("no direct opcode or dynamic native call");
        assert!(
            std::panic::catch_unwind(|| {
                validate_strlen_parser_outcomes_witness(
                    &repository_root,
                    &profiles,
                    &changed_dump,
                    &bytes,
                    &[],
                )
            })
            .is_err()
        );
    }

    #[test]
    fn abrupt_outcome_witness_rejects_source_output_dump_and_scope_drift() {
        let repository_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("resolve repository root");
        let profiles: serde_json::Value = serde_json::from_slice(
            &fs::read(
                repository_root
                    .join("tools/php-catalog/experiments/comparison/results/runtime/profiles.json"),
            )
            .expect("read PHP profiles"),
        )
        .expect("PHP profiles are JSON");
        let source_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data/locked-sources/php-8.5.4/Zend/zend_builtin_functions.c");
        let path = repository_root.join(
            "tools/php-catalog/experiments/comparison/results/evaluation/strlen-abrupt-outcomes-witness-8.5.4.json",
        );
        let bytes = fs::read(path).expect("read abrupt-outcomes witness");
        let witness: serde_json::Value =
            serde_json::from_slice(&bytes).expect("abrupt-outcomes witness is JSON");
        validate_strlen_abrupt_outcomes_witness(
            &source_path,
            &repository_root,
            &profiles,
            &witness,
            &bytes,
            &[],
        );

        let mut changed_source = witness.clone();
        changed_source["sources"]["fatal_bailout"]["sha256"] = serde_json::json!("00".repeat(32));
        assert!(
            std::panic::catch_unwind(|| {
                validate_strlen_abrupt_outcomes_witness(
                    &source_path,
                    &repository_root,
                    &profiles,
                    &changed_source,
                    &bytes,
                    &[],
                )
            })
            .is_err()
        );

        let mut changed_output = witness.clone();
        changed_output["cases"]["tailcall"]["stdout_sha256"] = serde_json::json!("00".repeat(32));
        assert!(
            std::panic::catch_unwind(|| {
                validate_strlen_abrupt_outcomes_witness(
                    &source_path,
                    &repository_root,
                    &profiles,
                    &changed_output,
                    &bytes,
                    &[],
                )
            })
            .is_err()
        );

        let mut changed_dump = witness.clone();
        changed_dump["cases"]["call"]["dump"] = serde_json::json!("missing outcome opcodes");
        assert!(
            std::panic::catch_unwind(|| {
                validate_strlen_abrupt_outcomes_witness(
                    &source_path,
                    &repository_root,
                    &profiles,
                    &changed_dump,
                    &bytes,
                    &[],
                )
            })
            .is_err()
        );

        let mut falsely_closed = witness;
        falsely_closed["scope"]["not_modeled"] = serde_json::json!("native unwind is closed");
        assert!(
            std::panic::catch_unwind(|| {
                validate_strlen_abrupt_outcomes_witness(
                    &source_path,
                    &repository_root,
                    &profiles,
                    &falsely_closed,
                    &bytes,
                    &[],
                )
            })
            .is_err()
        );

        let wrong_profile = fs::read(
            repository_root.join(
                "tools/php-catalog/experiments/comparison/results/evaluation/strlen-abrupt-outcomes-witness-8.5.4.json",
            ),
        )
        .expect("read abrupt-outcomes witness");
        let mut wrong_profile: serde_json::Value =
            serde_json::from_slice(&wrong_profile).expect("abrupt witness JSON");
        wrong_profile["runtime_profile"]["arch"] = serde_json::json!("aarch64");
        assert!(
            std::panic::catch_unwind(|| {
                validate_strlen_abrupt_outcomes_witness(
                    &source_path,
                    &repository_root,
                    &profiles,
                    &wrong_profile,
                    &bytes,
                    &[],
                )
            })
            .is_err()
        );
    }
}
