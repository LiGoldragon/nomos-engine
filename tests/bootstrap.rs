use std::collections::BTreeMap;

use core_ethos::bootstrap::{
    BootstrapCatalog, BootstrapGrammarIdentities, BootstrapPriorIdentities,
    BootstrapPriorVocabulary, BootstrapVersionPolicy, CanonicalIdentityOrder, EthosKind,
    EthosVersion, IdentitySchema, IdentitySchemaCatalog, InterfaceRole, NomosSchema, SchemaRole,
    TextualMetadataRecord, TextualMetadataSnapshot, TextualProjectionAddress,
};
use core_logos::{WholeLogos, WholeLogosItem};
use core_nomos::{ExternalStorageProvenance, StorageProvenanceOwner};
use encoded_name_table::LocalEncodedId;
use nomos_engine::{AuthoritySealedBootstrapTransformation, BootstrapTransformationError};
use sema_translator::bootstrap::{
    AuthorizedBootstrapTransition, BootstrapAuthorityIdentity, BootstrapAuthorityRevision,
    BootstrapTransactionAssembler, VerifiedBootstrapAssembly,
};
use signal_sema_translator::{VocabularyEncodedId, VocabularyRoot};

fn encoded(local: u16) -> VocabularyEncodedId {
    VocabularyEncodedId::new(VocabularyRoot::Universal, vec![LocalEncodedId::new(local)])
        .expect("nonempty Universal identity")
}

fn metadata_record(
    owner: Option<VocabularyEncodedId>,
    name: &str,
    identity: VocabularyEncodedId,
) -> TextualMetadataRecord {
    TextualMetadataRecord {
        address: TextualProjectionAddress {
            module_path: vec!["app".to_owned()],
            lexical_owner: owner,
            visible_name: name.to_owned(),
        },
        encoded_name: identity,
    }
}

fn bootstrap_catalog() -> (BootstrapCatalog, TextualMetadataSnapshot) {
    let prior_specs = [
        (
            1,
            "Interface",
            vec![SchemaRole::FileKind(EthosKind::Interface)],
        ),
        (2, "Nexus", vec![SchemaRole::FileKind(EthosKind::Nexus)]),
        (3, "Sema", vec![SchemaRole::FileKind(EthosKind::Sema)]),
        (
            4,
            "Input",
            vec![SchemaRole::InterfaceRole(InterfaceRole::Input)],
        ),
        (
            5,
            "Output",
            vec![SchemaRole::InterfaceRole(InterfaceRole::Output)],
        ),
        (
            6,
            "Refusal",
            vec![SchemaRole::InterfaceRole(InterfaceRole::Refusal)],
        ),
        (7, "String", vec![SchemaRole::Nominal { persistent: true }]),
        (8, "Integer", vec![SchemaRole::Nominal { persistent: true }]),
        (9, "Boolean", vec![SchemaRole::Nominal { persistent: true }]),
        (10, "Unit", vec![SchemaRole::Nominal { persistent: true }]),
        (11, "Vector", vec![SchemaRole::Shape { arity: 1 }]),
        (12, "Option", vec![SchemaRole::Shape { arity: 1 }]),
        (13, "Map", vec![SchemaRole::Shape { arity: 2 }]),
        (14, "Result", vec![SchemaRole::Shape { arity: 2 }]),
        (
            15,
            "Stream",
            vec![
                SchemaRole::Shape { arity: 1 },
                SchemaRole::Nomos(NomosSchema::StreamInitiation { arity: 2 }),
            ],
        ),
        (16, "StreamIdentity", vec![SchemaRole::Shape { arity: 1 }]),
    ];
    let mut records = Vec::new();
    let mut schemas = Vec::new();
    let mut order = Vec::new();
    for (local, name, roles) in prior_specs {
        let identity = encoded(local);
        records.push(TextualMetadataRecord {
            address: TextualProjectionAddress {
                module_path: vec!["builtin".to_owned()],
                lexical_owner: None,
                visible_name: name.to_owned(),
            },
            encoded_name: identity.clone(),
        });
        schemas.push(IdentitySchema::new(identity.clone(), roles).expect("valid prior schema"));
        order.push((identity, vec![0x80, local as u8]));
    }
    let before = TextualMetadataSnapshot::new(records).expect("unique prior metadata");
    let schemas = IdentitySchemaCatalog::new(schemas).expect("unique prior schemas");
    let priors = BootstrapPriorVocabulary::new(
        BootstrapPriorIdentities {
            interface_kind: encoded(1),
            nexus_kind: encoded(2),
            sema_kind: encoded(3),
            input_role: encoded(4),
            output_role: encoded(5),
            refusal_role: encoded(6),
            string_type: encoded(7),
            integer_type: encoded(8),
            boolean_type: encoded(9),
            unit_type: encoded(10),
            vector_shape: encoded(11),
            option_shape: encoded(12),
            map_shape: encoded(13),
            result_shape: encoded(14),
            stream_nomos: encoded(15),
            stream_shape: encoded(15),
            stream_identity_shape: encoded(16),
        },
        &schemas,
        &before,
    )
    .expect("valid bootstrap priors");
    let catalog = BootstrapCatalog::new(
        vec!["app".to_owned()],
        before.clone(),
        schemas,
        priors,
        BootstrapVersionPolicy::exact(EthosVersion::new(1, 0, 0)),
        CanonicalIdentityOrder::new(order).expect("unique prior order"),
    )
    .expect("valid bootstrap catalog");
    (catalog, before)
}

fn assembly(
    source: &str,
    declarations: Vec<(&str, VocabularyEncodedId, Option<VocabularyEncodedId>)>,
) -> VerifiedBootstrapAssembly {
    let (catalog, before) = bootstrap_catalog();
    let mut records = before.records().to_vec();
    let mut canonical_bytes = BTreeMap::new();
    for (index, (name, identity, owner)) in declarations.into_iter().enumerate() {
        records.push(metadata_record(owner, name, identity.clone()));
        canonical_bytes.insert(identity, vec![0x90, index as u8]);
    }
    let approval = AuthorizedBootstrapTransition::new(
        TextualMetadataSnapshot::new(records).expect("complete approved metadata"),
        canonical_bytes,
        BTreeMap::new(),
    );
    BootstrapTransactionAssembler::new(
        BootstrapAuthorityIdentity::new([0x42; 32]),
        BootstrapAuthorityRevision::new(1),
        BootstrapGrammarIdentities {
            document: encoded(900),
            syntax: encoded(901),
        },
        catalog,
    )
    .assemble(source, approval)
    .expect("authority-sealed bootstrap transaction")
}

fn external(local: u16, byte: u8) -> ExternalStorageProvenance {
    ExternalStorageProvenance::new(
        encoded(local),
        [byte; 32],
        StorageProvenanceOwner::new(
            "https://github.com/LiGoldragon/bootstrap-priors".to_owned(),
            "published-v1".to_owned(),
        )
        .expect("complete provenance owner"),
    )
    .expect("Universal external storage identity")
}

#[test]
fn authority_sealed_nexus_lowers_and_restores_the_exact_logos_archive() {
    let wrapped = encoded(100);
    let assembly = assembly(
        "Nexus.{1 0 0}\n[]\n{[] [Wrapped.String]}",
        vec![("Wrapped", wrapped.clone(), None)],
    );
    let outcome = assembly
        .lower_bootstrap(&[])
        .expect("strict bootstrap transformation");
    let [WholeLogosItem::Newtype(item)] = outcome.logos().items() else {
        panic!("one Nexus newtype lowers to one Logos newtype")
    };
    assert_eq!(item.name(), &wrapped);
    assert_eq!(
        WholeLogos::from_archive_bytes(outcome.archive()).expect("validated Logos archive"),
        *outcome.logos()
    );
}

#[test]
fn sema_consumes_explicit_storage_evidence_and_other_kinds_refuse_it() {
    let domain = encoded(100);
    let record = encoded(101);
    let table = encoded(102);
    let sema = assembly(
        "Sema.{1 0 0}\n[]\n{[Domain.String Record.{Domain Integer}] [records.{Record Domain}]}",
        vec![
            ("Domain", domain, None),
            ("Record", record, None),
            ("records", table, None),
        ],
    );
    let outcome = sema
        .lower_bootstrap(&[external(7, 0x17), external(8, 0x18)])
        .expect("strict storage-aware Sema transformation");
    assert!(matches!(
        outcome.logos().items(),
        [_, _, WholeLogosItem::Table(_)]
    ));

    let nexus = assembly(
        "Nexus.{1 0 0}\n[]\n{[] [Wrapped.String]}",
        vec![("Wrapped", encoded(110), None)],
    );
    assert!(matches!(
        nexus.lower_bootstrap(&[external(7, 0x17)]),
        Err(
            BootstrapTransformationError::UnexpectedExternalStorageProvenance {
                kind: EthosKind::Nexus,
                count: 1,
            }
        )
    ));
}
