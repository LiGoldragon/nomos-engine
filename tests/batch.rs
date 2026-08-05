use batch_core_ethos::{EthosDecodeError, WholeEthosFileKind};
use nomos_engine::batch::{
    BatchComponent, BatchConfiguration, BatchGenerationError, BatchImportError,
    BatchOutcomeReporting, OfflineBatchConfiguration, OfflineBatchGeneration,
    PreparedBatchGenerator,
};
use serde_json::{Value, json};

const INTERFACE: &str = "Interface.1\n[]\n{\n  [Command.String]\n  [Event.String]\n  [Rejected.{Reason String}]\n  [String.Integer Observer.Stream.(Command Event)]\n}\n";
const NEXUS: &str = "Nexus.1\n[]\n{\n  [Decision.[Accepted Rejected.Reason]]\n  []\n}\n";
const SEMA: &str = "Sema.1\n[]\n{\n  [Stored.{Integer} Key.Integer]\n  [records.{Stored Key}]\n}\n";
const IMPORTED_SEMA: &str = "Sema.1\n[signal-domain.{Domain}]\n{\n  [Stored.{Integer} Key.Domain]\n  [records.{Stored Key}]\n}\n";
const IMPORTED_RECORD_SEMA: &str =
    "Sema.1\n[signal-domain.{Domain}]\n{\n  []\n  [records.{Domain Integer}]\n}\n";
const BUNDLE_INTERFACE: &str =
    "Interface.1\n[]\n{\n  []\n  []\n  []\n  [Key.Integer Entry.{Integer}]\n}\n";
const BUNDLE_SEMA: &str = "Sema.1\n[interface.{Entry Key}]\n{\n  []\n  [records.{Entry Key}]\n}\n";

fn generator_configuration() -> Value {
    let source_names = [
        "Integer",
        "Vector",
        "Input",
        "Output",
        "Refusal",
        "Command",
        "String",
        "Event",
        "Rejected",
        "Reason",
        "Stream",
        "Observer",
        "ObserverStreamInitiation",
        "ObserverStreamHandle",
        "ObserverStreamInitiationRefusal",
        "ObserverStreamTermination",
        "ObserverStreamTerminationRefusal",
        "Decision",
        "Accepted",
        "Stored",
        "Key",
        "records",
        "Domain",
        "Entry",
    ];
    let names: Vec<Value> = source_names
        .into_iter()
        .scan(1000_u16, |local, spelling| {
            let entry = json!({
                "spelling": spelling,
                "root": "universal",
                "chain": [*local],
            });
            *local += 1;
            Some(entry)
        })
        .collect();
    json!({
            "grammar": grammar_configuration(),
            "rust_grammar": rust_grammar_configuration(),
            "priors": {
                "integer": "Integer",
                "vector": "Vector",
                "application_heads": ["Vector"],
                "stream_transformer": "Stream",
            },
            "interface_roles": {
                "input": "Input",
                "output": "Output",
                "refusal": "Refusal",
            },
            "rust_types": [
                {
                    "spelling": "Integer",
                    "path": ["u64"],
                    "external_storage": {
                        "source": "test://integer",
                        "revision": "test-revision",
                        "fingerprint": "0101010101010101010101010101010101010101010101010101010101010101",
                    },
                },
                {
                    "spelling": "Domain",
                    "import_source": "signal-domain",
                    "path": ["signal_domain", "Domain"],
                    "external_storage": {
                        "source": "https://github.com/LiGoldragon/signal-domain",
                        "revision": "test-domain-revision",
                        "fingerprint": "0202020202020202020202020202020202020202020202020202020202020202",
                    },
                },
                {
                    "spelling": "Entry",
                    "import_source": "interface",
                    "path": ["crate", "interface", "Entry"],
                },
                {
                    "spelling": "Key",
                    "import_source": "interface",
                    "path": ["crate", "interface", "Key"],
                },
            ],
            "stream_lifecycles": [
                {
                    "stream": "Observer",
                    "initiation_input": "ObserverStreamInitiation",
                    "handle": "ObserverStreamHandle",
                    "initiation_refusal": "ObserverStreamInitiationRefusal",
                    "termination_input": "ObserverStreamTermination",
                    "termination_refusal": "ObserverStreamTerminationRefusal",
                }
            ],
            "names": names,
    })
}

fn generator() -> PreparedBatchGenerator {
    BatchConfiguration::from_json(&generator_configuration().to_string())
        .expect("configuration JSON should decode")
        .prepare()
        .expect("configuration should seat without allocating identities")
}

#[test]
fn external_storage_successor_configuration_requires_complete_abi_evidence() {
    let mut configuration = generator_configuration();
    configuration["rust_types"][1]["external_storage"]["source"] = json!("test://compiled-domain");
    configuration["rust_types"][1]["external_storage"]["revision"] = json!("compiled-revision");
    configuration["rust_types"][1]["external_storage"]["successor"] = json!({
        "physical_owner": {
            "source": "test://physical-domain",
            "revision": "physical-revision",
        },
        "compiled_owner": {
            "source": "test://compiled-domain",
            "revision": "compiled-revision",
        },
        "type_identities": ["Domain"],
        "proof_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "evidence_revision": "proof-revision",
        "archive_abi": {
            "layout": true,
            "variant_order": true,
            "discriminants": true,
            "size": true,
            "alignment": true,
            "archive_bytes": true,
        },
    });
    BatchConfiguration::from_json(&configuration.to_string())
        .expect("successor configuration syntax")
        .prepare()
        .expect("complete successor evidence is accepted");

    configuration["rust_types"][1]["external_storage"]["successor"]["archive_abi"]["archive_bytes"] =
        json!(false);
    assert!(matches!(
        BatchConfiguration::from_json(&configuration.to_string())
            .expect("incomplete successor configuration syntax")
            .prepare(),
        Err(
            nomos_engine::batch::BatchConfigurationError::StorageProvenance(
                batch_core_nomos::NexusTransformationError::ArchiveAbiCheckNotProven {
                    check: "archive bytes"
                }
            )
        )
    ));
}

#[test]
fn bundle_imports_resolve_to_pre_registered_interface_declarations() {
    let generator = generator();
    let outcomes = generator
        .generate_bundle(&[
            BatchComponent::named("interface", BUNDLE_INTERFACE),
            BatchComponent::named("sema", BUNDLE_SEMA),
        ])
        .expect("complete Interface/Sema bundle should generate");
    let [interface, sema] = outcomes.as_slice() else {
        panic!("one receipt per bundle component")
    };
    assert_eq!(interface.kind(), WholeEthosFileKind::Interface);
    assert_eq!(sema.kind(), WholeEthosFileKind::Sema);
    assert!(
        sema.rust()
            .contains("type Record = crate::interface::Entry")
    );
    assert!(sema.rust().contains("impl sema_engine::TableSpecification"));
}

#[test]
fn bundle_source_labels_refuse_duplicates_and_false_internal_declarations() {
    let generator = generator();
    match generator.generate_bundle(&[
        BatchComponent::named("interface", BUNDLE_INTERFACE),
        BatchComponent::named("interface", BUNDLE_INTERFACE),
    ]) {
        Err(BatchGenerationError::DuplicateBundleModule { module }) => {
            assert_eq!(module, "interface");
        }
        Err(error) => panic!("unexpected duplicate-module error: {error}"),
        Ok(_) => panic!("duplicate component module unexpectedly generated"),
    }

    match generator.generate_bundle(&[
        BatchComponent::named("interface", INTERFACE),
        BatchComponent::named("sema", BUNDLE_SEMA),
    ]) {
        Err(BatchGenerationError::Import(BatchImportError::BundleImportNotDeclared {
            import_source,
            spelling,
        })) => {
            assert_eq!(import_source, "interface");
            assert_eq!(spelling, "Entry");
        }
        Err(error) => panic!("unexpected false-local-import error: {error}"),
        Ok(_) => panic!("false local declaration unexpectedly generated"),
    }
}

#[test]
fn imported_types_require_exact_caller_owned_paths_and_storage_contracts() {
    let generator = generator();

    let mut generated = generator
        .generate_bundle(&[BatchComponent::standalone(IMPORTED_SEMA)])
        .expect("configured imported Domain should generate");
    let generated = generated.pop().expect("one standalone outcome");
    assert!(generated.rust().contains("signal_domain::Domain"));

    let wrong_source = IMPORTED_SEMA.replace("signal-domain", "lookalike-domain");
    match generator.generate_bundle(&[BatchComponent::standalone(&wrong_source)]) {
        Err(BatchGenerationError::Import(BatchImportError::MissingMapping {
            import_source,
            spelling,
        })) => {
            assert_eq!(import_source, "lookalike-domain");
            assert_eq!(spelling, "Domain");
        }
        Err(error) => panic!("unexpected missing-import error: {error}"),
        Ok(_) => panic!("unconfigured imported Domain unexpectedly generated"),
    }

    match generator.generate_bundle(&[BatchComponent::standalone(IMPORTED_RECORD_SEMA)]) {
        Err(BatchGenerationError::Projection(
            batch_core_nomos::NexusTransformationError::SemaTableRecordNotBundleOwned { .. },
        )) => {}
        Err(error) => panic!("unexpected imported-record Sema refusal: {error}"),
        Ok(_) => panic!("partial imported-record Sema output unexpectedly generated"),
    }
}

fn grammar_configuration() -> Value {
    json!({
        "interface_document": [1],
        "nexus_document": [2],
        "sema_document": [3],
        "header": [4],
        "imports": [5],
        "import_entry": [6],
        "interface_body": [7],
        "nexus_body": [8],
        "sema_body": [9],
        "newtype_list": [10],
        "struct_list": [11],
        "item_list": [12],
        "trait_list": [13],
        "table_list": [14],
        "newtype_declaration": [15],
        "struct_declaration": [16],
        "item": [17],
        "variant": [18],
        "type_reference": [19],
        "trait_declaration": [21],
        "table": [23],
    })
}

fn rust_grammar_configuration() -> Value {
    json!({
        "newtype_item": [1],
        "enumeration_item": [2],
        "variant": [3],
        "tuple_field": [4],
        "type_reference": [5],
        "struct_keyword": [6],
        "enum_keyword": [7],
        "public_keyword": [8],
        "comma": [9],
        "semicolon": [10],
    })
}

#[test]
fn all_current_file_kinds_return_complete_artifacts() {
    let generator = generator();

    let mut nexus = generator
        .generate_bundle(&[BatchComponent::standalone(NEXUS)])
        .expect("Nexus should generate");
    let nexus = nexus.pop().expect("one standalone outcome");
    assert_eq!(nexus.kind(), WholeEthosFileKind::Nexus);
    assert!(nexus.rust().contains("pub enum"));

    let mut interface = generator
        .generate_bundle(&[BatchComponent::standalone(INTERFACE)])
        .expect("Interface declarations should generate");
    let interface = interface.pop().expect("one standalone outcome");
    assert_eq!(interface.kind(), WholeEthosFileKind::Interface);
    assert!(interface.rust().contains("#[derive(rkyv::Archive"));
    assert!(interface.rust().contains("impl std::fmt::Display"));
    assert!(interface.rust().contains("impl std::error::Error"));
    assert!(!interface.rust().contains("impl From<"));
    assert!(interface.rust().contains("protos::Stream"));
    assert!(interface.rust().contains("UnknownStream"));
    assert!(interface.rust().contains("AlreadyClosed"));
    assert!(!interface.report().contains("deferred"));
    assert!(!interface.report().contains("membership"));
    assert!(!interface.report().contains("refusal-semantics"));

    let mut sema = generator
        .generate_bundle(&[BatchComponent::standalone(SEMA)])
        .expect("Sema record declarations should generate");
    let sema = sema.pop().expect("one standalone outcome");
    assert_eq!(sema.kind(), WholeEthosFileKind::Sema);
    assert!(sema.rust().contains("#[derive(rkyv::Archive"));
    assert!(sema.rust().contains("impl sema_engine::TableSpecification"));
    assert!(sema.rust().contains("type Record ="));
    assert!(sema.rust().contains("type Key ="));
    assert!(
        sema.rust()
            .contains("RecordKey::new(key.payload().to_string())")
    );
    assert!(!sema.report().contains("deferred"));
}

#[test]
fn header_kind_and_version_refusals_remain_typed() {
    let generator = generator();

    match generator.generate_bundle(&[BatchComponent::standalone("Unknown.1\n[]\n{ [] [] }\n")]) {
        Err(BatchGenerationError::Decode(EthosDecodeError::UnknownFileKind { .. })) => {}
        Err(error) => panic!("unexpected unknown-kind error: {error}"),
        Ok(_) => panic!("unknown file kind unexpectedly generated"),
    }
    match generator.generate_bundle(&[BatchComponent::standalone("Nexus.2\n[]\n{ [] [] }\n")]) {
        Err(BatchGenerationError::Decode(EthosDecodeError::UnsupportedVersion { .. })) => {}
        Err(error) => panic!("unexpected version error: {error}"),
        Ok(_) => panic!("unsupported version unexpectedly generated"),
    }
}
