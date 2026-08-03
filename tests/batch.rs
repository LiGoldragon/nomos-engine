use batch_core_ethos::{EthosDecodeError, WholeEthosFileKind};
use nomos_engine::batch::{
    BatchConfiguration, BatchGenerationError, BatchOutcomeReporting, DeferredBatchConstruct,
    OfflineBatchConfiguration, OfflineBatchGeneration, PreparedBatchGenerator,
};
use serde_json::{Value, json};

const INTERFACE: &str = "Interface.1\n[]\n{\n  [Command.Text]\n  [Event.Text]\n  [Rejected.{Reason Text}]\n  [Text.Integer Stream.Feed.{Command Event Rejected}]\n}\n";
const NEXUS: &str = "Nexus.1\n[]\n{\n  [Decision.[Accepted Rejected.Reason]]\n  []\n}\n";
const SEMA: &str = "Sema.1\n[]\n{\n  [Stored.{Integer}]\n  [records.{Stored Integer}]\n}\n";

fn generator() -> PreparedBatchGenerator {
    let source_names = [
        "Integer", "Vector", "Input", "Output", "Refusal", "Command", "Text", "Event", "Rejected",
        "Reason", "Stream", "Feed", "Decision", "Accepted", "Stored", "records",
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
                "object_application_heads": ["Stream"],
            },
            "interface_roles": {
                "input": "Input",
                "output": "Output",
                "refusal": "Refusal",
            },
            "names": names,
        })
        .to_string(),
    )
    .expect("configuration JSON should decode")
    .prepare()
    .expect("configuration should seat without allocating identities")
}

fn is_interface_operator_deferral(construct: &DeferredBatchConstruct) -> bool {
    match construct {
        DeferredBatchConstruct::InterfaceOperatorApplication { .. } => true,
        DeferredBatchConstruct::SemaTable { .. } => false,
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
        "operator_payload": [20],
        "trait_declaration": [21],
        "method": [22],
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
fn all_current_file_kinds_return_artifacts_with_explicit_deferred_receipts() {
    let generator = generator();

    let nexus = generator.generate(NEXUS).expect("Nexus should generate");
    assert_eq!(nexus.kind(), WholeEthosFileKind::Nexus);
    assert!(nexus.deferred().is_empty());
    assert!(nexus.rust().contains("pub enum"));

    let interface = generator
        .generate(INTERFACE)
        .expect("Interface declarations should generate");
    assert_eq!(interface.kind(), WholeEthosFileKind::Interface);
    assert_eq!(interface.deferred().len(), 1);
    assert!(
        interface
            .deferred()
            .iter()
            .all(is_interface_operator_deferral)
    );
    assert!(interface.rust().contains("#[derive(rkyv::Archive"));
    assert!(interface.rust().contains("impl std::fmt::Display"));
    assert!(interface.rust().contains("impl std::error::Error"));
    assert!(!interface.rust().contains("impl From<"));
    assert!(interface.report().contains("deferred 1"));
    assert!(!interface.report().contains("membership"));
    assert!(!interface.report().contains("refusal-semantics"));

    let sema = generator
        .generate(SEMA)
        .expect("Sema record declarations should generate");
    assert_eq!(sema.kind(), WholeEthosFileKind::Sema);
    assert!(sema.deferred().is_empty());
    assert!(sema.rust().contains("#[derive(rkyv::Archive"));
    assert!(sema.rust().contains("impl sema_engine::TableSpecification"));
    assert!(sema.rust().contains("type Record ="));
    assert!(sema.rust().contains("type Key ="));
    assert!(sema.report().contains("deferred 0"));
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
