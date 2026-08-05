use batch_core_ethos::{EthosDecodeError, WholeEthosFileKind};
use nomos_engine::batch::{
    BatchConfiguration, BatchGenerationError, BatchImportError, BatchOutcomeReporting,
    OfflineBatchConfiguration, OfflineBatchGeneration, PreparedBatchGenerator,
};
use serde_json::{Value, json};

const INTERFACE: &str = "Interface.1\n[]\n{\n  [Command.String]\n  [Event.String]\n  [Rejected.{Reason String}]\n  [String.Integer Observer.Stream.(Command Event)]\n}\n";
const NEXUS: &str = "Nexus.1\n[]\n{\n  [Decision.[Accepted Rejected.Reason]]\n  []\n}\n";
const SEMA: &str = "Sema.1\n[]\n{\n  [Stored.{Integer}]\n  [records.{Stored Integer}]\n}\n";
const IMPORTED_SEMA: &str =
    "Sema.1\n[signal-domain.{Domain}]\n{\n  [Stored.{Integer}]\n  [records.{Stored Domain}]\n}\n";
const IMPORTED_RECORD_SEMA: &str =
    "Sema.1\n[signal-domain.{Domain}]\n{\n  []\n  [records.{Domain Integer}]\n}\n";

fn generator() -> PreparedBatchGenerator {
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
        "records",
        "Domain",
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
    BatchConfiguration::from_json(
        &json!({
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
                    "storage_fingerprint": "0101010101010101010101010101010101010101010101010101010101010101",
                },
                {
                    "spelling": "Domain",
                    "import_source": "signal-domain",
                    "path": ["signal_domain", "Domain"],
                    "storage_fingerprint": "0202020202020202020202020202020202020202020202020202020202020202",
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
        .to_string(),
    )
    .expect("configuration JSON should decode")
    .prepare()
    .expect("configuration should seat without allocating identities")
}

#[test]
fn imported_types_require_exact_caller_owned_paths_and_storage_contracts() {
    let generator = generator();

    let generated = generator
        .generate(IMPORTED_SEMA)
        .expect("configured imported Domain should generate");
    assert!(
        generated
            .rust()
            .contains("type Key = signal_domain::Domain")
    );

    let wrong_source = IMPORTED_SEMA.replace("signal-domain", "lookalike-domain");
    match generator.generate(&wrong_source) {
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

    match generator.generate(IMPORTED_RECORD_SEMA) {
        Err(BatchGenerationError::SemaTablesRequireGeneratedOwner { count }) => {
            assert_eq!(count, 1);
        }
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

    let nexus = generator.generate(NEXUS).expect("Nexus should generate");
    assert_eq!(nexus.kind(), WholeEthosFileKind::Nexus);
    assert!(nexus.rust().contains("pub enum"));

    let interface = generator
        .generate(INTERFACE)
        .expect("Interface declarations should generate");
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

    let sema = generator
        .generate(SEMA)
        .expect("Sema record declarations should generate");
    assert_eq!(sema.kind(), WholeEthosFileKind::Sema);
    assert!(sema.rust().contains("#[derive(rkyv::Archive"));
    assert!(sema.rust().contains("impl sema_engine::TableSpecification"));
    assert!(sema.rust().contains("type Record ="));
    assert!(sema.rust().contains("type Key ="));
    assert!(!sema.report().contains("deferred"));
}

#[test]
fn header_kind_and_version_refusals_remain_typed() {
    let generator = generator();

    match generator.generate("Unknown.1\n[]\n{ [] [] }\n") {
        Err(BatchGenerationError::Decode(EthosDecodeError::UnknownFileKind { .. })) => {}
        Err(error) => panic!("unexpected unknown-kind error: {error}"),
        Ok(_) => panic!("unknown file kind unexpectedly generated"),
    }
    match generator.generate("Nexus.2\n[]\n{ [] [] }\n") {
        Err(BatchGenerationError::Decode(EthosDecodeError::UnsupportedVersion { .. })) => {}
        Err(error) => panic!("unexpected version error: {error}"),
        Ok(_) => panic!("unsupported version unexpectedly generated"),
    }
}
