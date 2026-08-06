use std::mem::{align_of, size_of};

use capsule_content_identity::CapsuleIdentity;
use core_ethos::{
    WholeEthosAttributes, WholeEthosQuality, WholeEthosTypeApplication, WholeEthosTypeReference,
    WholeEthosVisibility, WholeEthosWrappedField,
};
use core_logos::{WholeLogos, WholeLogosItem, WholeLogosVariantPayload};
use core_nomos::{
    AuthoredTransformerSet, LoadedNomosPopulation, NameTreeProjectionVersion, NomosNameTable,
};
use nomos_engine::NomosEngine;
use signal_nomos::{
    CapsuleSelector, DeployOutcome, EthosPopulationArchive, GenerationSelection,
    NomosDeploymentArtifacts, NomosProjectionArchive, NomosSlotId, ProjectionOutcome, Rejection,
    Reply, Request, ShortCapsuleDisplay, SlotExpectation, SlotGeneration, TransformSelector,
    TranslatorRenameReceiptArchive, encode_request,
};
use signal_sema_translator::{VocabularyEncodedId, VocabularyRoot};

mod support;

const ADMIN_UID: u32 = 4100;
const OTHER_UID: u32 = 4200;

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
struct ForgedWholeEthos(Vec<ForgedWholeEthosItem>);

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
enum ForgedWholeEthosItem {
    Newtype(ForgedWholeEthosNewtype),
    Enumeration(ForgedWholeEthosEnumeration),
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
struct ForgedWholeEthosNewtype {
    name: VocabularyEncodedId,
    visibility: WholeEthosVisibility,
    attributes: WholeEthosAttributes,
    wrapped_field: WholeEthosWrappedField,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
struct ForgedWholeEthosEnumeration {
    name: VocabularyEncodedId,
    visibility: WholeEthosVisibility,
    attributes: WholeEthosAttributes,
    variants: Vec<ForgedWholeEthosVariant>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
struct ForgedWholeEthosVariant {
    name: VocabularyEncodedId,
    attributes: WholeEthosAttributes,
    payload: ForgedWholeEthosVariantPayload,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
enum ForgedWholeEthosVariantPayload {
    Unit,
    Tuple(ForgedWholeEthosTupleFields),
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
struct ForgedWholeEthosTupleFields(Vec<WholeEthosTypeReference>);

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
struct NamedForgedWholeEthos {
    items: Vec<NamedForgedWholeEthosItem>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
enum NamedForgedWholeEthosItem {
    Newtype(NamedForgedWholeEthosNewtype),
    Enumeration(NamedForgedWholeEthosEnumeration),
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
struct NamedForgedWholeEthosNewtype {
    name: VocabularyEncodedId,
    visibility: WholeEthosVisibility,
    attributes: WholeEthosAttributes,
    wrapped_field: WholeEthosWrappedField,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
struct NamedForgedWholeEthosEnumeration {
    name: VocabularyEncodedId,
    visibility: WholeEthosVisibility,
    attributes: WholeEthosAttributes,
    variants: Vec<ForgedWholeEthosVariant>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
struct NamedForgedWholeEthosVariant {
    name: VocabularyEncodedId,
    attributes: WholeEthosAttributes,
    payload: ForgedWholeEthosVariantPayload,
}

trait AssertTypeReferenceFieldsEqual {
    fn assert_type_reference_fields_equal(&self, right: &Self);
}

impl AssertTypeReferenceFieldsEqual for WholeEthosTypeReference {
    fn assert_type_reference_fields_equal(&self, right: &Self) {
        match self {
            WholeEthosTypeReference::Identity(left_identity) => {
                let WholeEthosTypeReference::Identity(right_identity) = right else {
                    panic!("type-reference kinds must match")
                };
                assert_eq!(
                    left_identity, right_identity,
                    "type-reference identities must match",
                );
            }
            WholeEthosTypeReference::Application(left_application) => {
                let WholeEthosTypeReference::Application(right_application) = right else {
                    panic!("type-reference kinds must match")
                };
                assert_eq!(
                    left_application.head(),
                    right_application.head(),
                    "type-application head identities must match",
                );
                assert_eq!(left_application.arguments(), right_application.arguments());
            }
            WholeEthosTypeReference::Parameter(left_parameter) => {
                let WholeEthosTypeReference::Parameter(right_parameter) = right else {
                    panic!("type-reference kinds must match")
                };
                assert_eq!(left_parameter, right_parameter);
            }
        }
    }
}

macro_rules! assert_forged_payload_fields_equal {
    ($left:expr, $right:expr) => {{
        match $left {
            ForgedWholeEthosVariantPayload::Unit => {
                let ForgedWholeEthosVariantPayload::Unit = $right else {
                    panic!("variant payload kinds must match")
                };
            }
            ForgedWholeEthosVariantPayload::Tuple(left) => {
                let ForgedWholeEthosVariantPayload::Tuple(right) = $right else {
                    panic!("variant payload kinds must match")
                };
                assert_eq!(
                    left.0.len(),
                    right.0.len(),
                    "tuple payload field counts must match",
                );
                for index in 0..left.0.len() {
                    left.0[index].assert_type_reference_fields_equal(&right.0[index]);
                }
            }
        }
    }};
}

macro_rules! assert_forged_newtype_fields_equal {
    ($positional:expr, $named:expr) => {{
        assert_eq!($positional.name, $named.name, "newtype names must match");
        assert_eq!(
            $positional.visibility, $named.visibility,
            "newtype visibility must match",
        );
        assert_eq!(
            $positional.attributes, $named.attributes,
            "newtype attributes must match",
        );
        assert_eq!(
            $positional.wrapped_field.visibility(),
            $named.wrapped_field.visibility(),
            "wrapped-field visibility must match",
        );
        assert_eq!(
            $positional.wrapped_field.reference(),
            $named.wrapped_field.reference(),
            "wrapped-field type reference must match",
        );
        $positional
            .wrapped_field
            .reference()
            .assert_type_reference_fields_equal($named.wrapped_field.reference());
    }};
}

macro_rules! assert_forged_positional_variant_fields_equal {
    ($left:expr, $right:expr) => {{
        assert_eq!($left.name, $right.name, "variant names must match");
        assert_eq!(
            $left.attributes, $right.attributes,
            "variant attributes must match"
        );
        assert_forged_payload_fields_equal!(&$left.payload, &$right.payload);
    }};
}

macro_rules! assert_forged_variant_fields_equal {
    ($positional:expr, $named:expr) => {{
        assert_eq!($positional.name, $named.name, "variant names must match");
        assert_eq!(
            $positional.attributes, $named.attributes,
            "variant attributes must match",
        );
        assert_forged_payload_fields_equal!(&$positional.payload, &$named.payload);
    }};
}

macro_rules! assert_forged_enumeration_fields_equal {
    ($positional:expr, $named:expr) => {{
        assert_eq!(
            $positional.name, $named.name,
            "enumeration names must match"
        );
        assert_eq!(
            $positional.visibility, $named.visibility,
            "enumeration visibility must match",
        );
        assert_eq!(
            $positional.attributes, $named.attributes,
            "enumeration attributes must match",
        );
        assert_eq!(
            $positional.variants.len(),
            $named.variants.len(),
            "enumeration variant counts must match",
        );
        for index in 0..$positional.variants.len() {
            assert_forged_positional_variant_fields_equal!(
                &$positional.variants[index],
                &$named.variants[index]
            );
        }
    }};
}

macro_rules! assert_forged_whole_fields_equal {
    ($positional:expr, $named:expr) => {{
        assert_eq!(
            $positional.0.len(),
            $named.items.len(),
            "WholeEthos item counts must match",
        );
        for index in 0..$positional.0.len() {
            match &$positional.0[index] {
                ForgedWholeEthosItem::Newtype(positional_newtype) => {
                    let NamedForgedWholeEthosItem::Newtype(named_newtype) = &$named.items[index]
                    else {
                        panic!("WholeEthos item kinds must match")
                    };
                    assert_forged_newtype_fields_equal!(positional_newtype, named_newtype);
                }
                ForgedWholeEthosItem::Enumeration(positional_enumeration) => {
                    let NamedForgedWholeEthosItem::Enumeration(named_enumeration) =
                        &$named.items[index]
                    else {
                        panic!("WholeEthos item kinds must match")
                    };
                    assert_forged_enumeration_fields_equal!(
                        positional_enumeration,
                        named_enumeration
                    );
                }
            }
        }
    }};
}

macro_rules! assert_forged_archive_compatible {
    (
        $positional_type:ty,
        $named_type:ty,
        $positional:expr,
        $named:expr,
        |$positional_from_named:ident, $named_source:ident| $positional_assertions:block,
        |$named_from_positional:ident, $positional_source:ident| $named_assertions:block
    ) => {{
        let positional = $positional;
        let named = $named;
        let positional_bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&positional)
            .expect("archive positional forged carrier");
        let named_bytes =
            rkyv::to_bytes::<rkyv::rancor::Error>(&named).expect("archive named forged carrier");

        assert_eq!(
            positional_bytes.as_slice(),
            named_bytes.as_slice(),
            "positional and named forged carriers must emit identical bytes",
        );
        assert_eq!(
            size_of::<rkyv::Archived<$positional_type>>(),
            size_of::<rkyv::Archived<$named_type>>(),
            "archived sizes must match",
        );
        assert_eq!(
            align_of::<rkyv::Archived<$positional_type>>(),
            align_of::<rkyv::Archived<$named_type>>(),
            "archived alignments must match",
        );

        let _: &rkyv::Archived<$positional_type> =
            rkyv::access::<rkyv::Archived<$positional_type>, rkyv::rancor::Error>(&named_bytes)
                .expect("access named bytes through positional archived layout");
        let _: &rkyv::Archived<$named_type> =
            rkyv::access::<rkyv::Archived<$named_type>, rkyv::rancor::Error>(&positional_bytes)
                .expect("access positional bytes through named archived layout");

        let positional_from_named =
            rkyv::from_bytes::<$positional_type, rkyv::rancor::Error>(&named_bytes)
                .expect("restore positional forged carrier from named bytes");
        let named_from_positional =
            rkyv::from_bytes::<$named_type, rkyv::rancor::Error>(&positional_bytes)
                .expect("restore named forged carrier from positional bytes");
        {
            let $positional_from_named = &positional_from_named;
            let $named_source = &named;
            $positional_assertions
        }
        {
            let $named_from_positional = &named_from_positional;
            let $positional_source = &positional;
            $named_assertions
        }
        assert_eq!(
            rkyv::to_bytes::<rkyv::rancor::Error>(&positional_from_named)
                .expect("reserialize positional carrier restored from named bytes")
                .as_slice(),
            named_bytes.as_slice(),
        );
        assert_eq!(
            rkyv::to_bytes::<rkyv::rancor::Error>(&named_from_positional)
                .expect("reserialize named carrier restored from positional bytes")
                .as_slice(),
            positional_bytes.as_slice(),
        );

        named_bytes
    }};
}

fn population(version: NameTreeProjectionVersion) -> core_nomos::SealedNomosPopulation {
    LoadedNomosPopulation::from_typed(
        AuthoredTransformerSet::try_new(Vec::new()).expect("empty authored set"),
        NomosNameTable::empty(),
    )
    .seal(version)
    .expect("empty population seals")
}

fn artifacts() -> NomosDeploymentArtifacts {
    NomosDeploymentArtifacts::from_population(&population(NameTreeProjectionVersion::initial()))
        .expect("deployment artifacts")
}

fn identity(artifacts: &NomosDeploymentArtifacts) -> CapsuleIdentity {
    artifacts
        .validate()
        .expect("artifacts validate")
        .capsule()
        .content_identity()
}

fn tamper_request_bytes(request: &Request, embedded: &[u8]) -> Request {
    let mut bytes = encode_request(request).expect("wire request");
    let positions = bytes
        .windows(embedded.len())
        .enumerate()
        .filter_map(|(index, candidate)| (candidate == embedded).then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(positions.len(), 1, "embedded archive occurs exactly once");
    bytes[positions[0]] ^= 0x01;
    rkyv::from_bytes::<Request, rkyv::rancor::Error>(&bytes)
        .expect("outer request archive remains structurally valid")
}

fn vocabulary_id(root: VocabularyRoot, chain: &[u16]) -> VocabularyEncodedId {
    VocabularyEncodedId::new(
        root,
        chain
            .iter()
            .copied()
            .map(name_table::LocalEncodedId::new)
            .collect(),
    )
    .expect("non-empty identity")
}

// Trait exception — the proper trait cannot be determined: this entry point's
// contract is supplied by Rust's test harness.
#[test]
fn historical_named_fields_preserve_positional_archive_layout() {
    let newtype_name = vocabulary_id(VocabularyRoot::Universal, &[500, 1]);
    let application_head = vocabulary_id(VocabularyRoot::Universal, &[600, 3]);
    let application_payload = vocabulary_id(VocabularyRoot::Universal, &[600, 5, 7]);
    let wrapped_field = WholeEthosWrappedField::new(
        WholeEthosVisibility::Private,
        WholeEthosTypeReference::Application(
            WholeEthosTypeApplication::new(
                WholeEthosQuality::Shape(application_head.clone()),
                vec![WholeEthosTypeReference::Identity(
                    application_payload.clone(),
                )],
            )
            .expect("non-empty historical application"),
        ),
    );
    let positional_newtype = ForgedWholeEthosNewtype {
        name: newtype_name.clone(),
        visibility: WholeEthosVisibility::Public,
        attributes: WholeEthosAttributes::empty(),
        wrapped_field: wrapped_field.clone(),
    };
    let named_newtype = NamedForgedWholeEthosNewtype {
        name: newtype_name.clone(),
        visibility: WholeEthosVisibility::Public,
        attributes: WholeEthosAttributes::empty(),
        wrapped_field: wrapped_field.clone(),
    };
    let named_newtype_bytes = assert_forged_archive_compatible!(
        ForgedWholeEthosNewtype,
        NamedForgedWholeEthosNewtype,
        positional_newtype,
        named_newtype.clone(),
        |positional_from_named, named_source| {
            assert_forged_newtype_fields_equal!(positional_from_named, named_source);
        },
        |named_from_positional, positional_source| {
            assert_forged_newtype_fields_equal!(positional_source, named_from_positional);
        }
    );
    assert!(!named_newtype_bytes.is_empty());

    let variant_name = vocabulary_id(VocabularyRoot::Universal, &[500, 2, 11]);
    let variant_references = vec![
        WholeEthosTypeReference::Identity(vocabulary_id(VocabularyRoot::Universal, &[700, 13])),
        WholeEthosTypeReference::Application(
            WholeEthosTypeApplication::new(
                WholeEthosQuality::Shape(vocabulary_id(VocabularyRoot::Universal, &[700, 17])),
                vec![WholeEthosTypeReference::Identity(vocabulary_id(
                    VocabularyRoot::Universal,
                    &[700, 19, 23],
                ))],
            )
            .expect("non-empty historical application"),
        ),
    ];
    let variant_payload = ForgedWholeEthosVariantPayload::Tuple(ForgedWholeEthosTupleFields(
        variant_references.clone(),
    ));
    let positional_variant = ForgedWholeEthosVariant {
        name: variant_name.clone(),
        attributes: WholeEthosAttributes::empty(),
        payload: variant_payload.clone(),
    };
    let named_variant = NamedForgedWholeEthosVariant {
        name: variant_name.clone(),
        attributes: WholeEthosAttributes::empty(),
        payload: variant_payload.clone(),
    };
    let named_variant_bytes = assert_forged_archive_compatible!(
        ForgedWholeEthosVariant,
        NamedForgedWholeEthosVariant,
        positional_variant.clone(),
        named_variant,
        |positional_from_named, named_source| {
            assert_forged_variant_fields_equal!(positional_from_named, named_source);
        },
        |named_from_positional, positional_source| {
            assert_forged_variant_fields_equal!(positional_source, named_from_positional);
        }
    );
    assert!(!named_variant_bytes.is_empty());

    let enumeration_name = vocabulary_id(VocabularyRoot::Universal, &[500, 2]);
    let positional_variants = vec![
        ForgedWholeEthosVariant {
            name: vocabulary_id(VocabularyRoot::Universal, &[500, 2, 3]),
            attributes: WholeEthosAttributes::empty(),
            payload: ForgedWholeEthosVariantPayload::Unit,
        },
        positional_variant,
    ];
    let positional_enumeration = ForgedWholeEthosEnumeration {
        name: enumeration_name.clone(),
        visibility: WholeEthosVisibility::Private,
        attributes: WholeEthosAttributes::empty(),
        variants: positional_variants.clone(),
    };
    let named_enumeration = NamedForgedWholeEthosEnumeration {
        name: enumeration_name.clone(),
        visibility: WholeEthosVisibility::Private,
        attributes: WholeEthosAttributes::empty(),
        variants: positional_variants,
    };
    let named_enumeration_bytes = assert_forged_archive_compatible!(
        ForgedWholeEthosEnumeration,
        NamedForgedWholeEthosEnumeration,
        positional_enumeration.clone(),
        named_enumeration.clone(),
        |positional_from_named, named_source| {
            assert_forged_enumeration_fields_equal!(positional_from_named, named_source);
        },
        |named_from_positional, positional_source| {
            assert_forged_enumeration_fields_equal!(positional_source, named_from_positional);
        }
    );
    assert!(!named_enumeration_bytes.is_empty());

    let positional_whole = ForgedWholeEthos(vec![
        ForgedWholeEthosItem::Newtype(ForgedWholeEthosNewtype {
            name: newtype_name,
            visibility: WholeEthosVisibility::Public,
            attributes: WholeEthosAttributes::empty(),
            wrapped_field,
        }),
        ForgedWholeEthosItem::Enumeration(positional_enumeration),
    ]);
    let named_whole = NamedForgedWholeEthos {
        items: vec![
            NamedForgedWholeEthosItem::Newtype(named_newtype),
            NamedForgedWholeEthosItem::Enumeration(named_enumeration),
        ],
    };
    let named_whole_bytes = assert_forged_archive_compatible!(
        ForgedWholeEthos,
        NamedForgedWholeEthos,
        positional_whole,
        named_whole,
        |positional_from_named, named_source| {
            assert_forged_whole_fields_equal!(positional_from_named, named_source);
        },
        |named_from_positional, positional_source| {
            assert_forged_whole_fields_equal!(positional_source, named_from_positional);
        }
    );
    assert!(!named_whole_bytes.is_empty());
}

#[test]
fn public_authored_constructor_seals_distinct_packages() {
    let first = support::authored_artifacts(1);
    let second = support::authored_artifacts(2);
    assert_ne!(identity(&first), identity(&second));
}

#[test]
fn forged_capsule_and_projection_archives_have_distinct_refusals() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("nomos.sema");
    let artifacts = support::authored_artifacts(1);
    let request = Request::Deploy {
        slot: NomosSlotId::new(5),
        expected: SlotExpectation::Empty,
        artifacts: artifacts.clone(),
        selection: GenerationSelection::enriched(),
    };
    let forged_capsule = tamper_request_bytes(&request, artifacts.capsule().bytes());
    let forged_projection = tamper_request_bytes(&request, artifacts.projection().bytes());
    let mut engine = NomosEngine::open(&database, ADMIN_UID).expect("engine opens");

    assert_eq!(
        engine
            .dispatch(OTHER_UID, forged_capsule)
            .expect("typed refusal"),
        Reply::Rejected(Rejection::CapsuleArchiveInvalid)
    );
    assert_eq!(
        engine
            .dispatch(OTHER_UID, forged_projection)
            .expect("typed refusal"),
        Reply::Rejected(Rejection::ProjectionInvalid)
    );
    assert_eq!(engine.commit_count().expect("count"), 0);
    assert_eq!(
        engine.current_marker().expect("marker"),
        signal_nomos::CommitMarker::new(0, 0)
    );
}

#[test]
fn unarchived_bootstrap_wire_is_typed_refused_without_evaluation() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("nomos.sema");
    let slot = NomosSlotId::new(4);
    let artifacts = support::authored_artifacts(1);
    let mut engine = NomosEngine::open(&database, ADMIN_UID).expect("engine opens");
    let _ = deploy(
        &mut engine,
        slot,
        SlotExpectation::Empty,
        artifacts,
        GenerationSelection::enriched(),
    );
    let before = engine.current_marker().expect("marker");

    let malformed = engine
        .dispatch(
            OTHER_UID,
            Request::Transform {
                selector: TransformSelector::Live(slot),
                ethos: EthosPopulationArchive::try_new(vec![1, 2, 3])
                    .expect("non-empty opaque archive"),
            },
        )
        .expect("typed refusal");
    assert_eq!(
        malformed,
        Reply::Rejected(Rejection::EthosPopulationInvalid)
    );

    let nonempty_opaque = engine
        .dispatch(
            OTHER_UID,
            Request::Transform {
                selector: TransformSelector::Live(slot),
                ethos: EthosPopulationArchive::try_new(vec![0x42])
                    .expect("non-empty opaque archive"),
            },
        )
        .expect("typed refusal");
    assert_eq!(
        nonempty_opaque,
        Reply::Rejected(Rejection::EthosPopulationInvalid)
    );
    assert_eq!(engine.current_marker().expect("marker"), before);
    assert_eq!(engine.commit_count().expect("count"), 1);
}

#[test]
fn deploying_a_second_capsule_retains_the_first_for_rollback() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("nomos.sema");
    let slot = NomosSlotId::new(6);
    let first = support::authored_artifacts(1);
    let second = support::authored_artifacts(2);
    let first_identity = identity(&first);
    let second_identity = identity(&second);
    let mut engine = NomosEngine::open(&database, ADMIN_UID).expect("engine opens");

    let first_reply = deploy(
        &mut engine,
        slot,
        SlotExpectation::Empty,
        first,
        GenerationSelection::enriched(),
    );
    assert!(matches!(
        first_reply,
        Reply::Deployed(DeployOutcome::FreshDeployed {
            identity,
            generation,
            committed_at,
            ..
        }) if identity == first_identity
            && generation == SlotGeneration::initial()
            && committed_at.commit_sequence() == 1
    ));

    let second_reply = deploy(
        &mut engine,
        slot,
        SlotExpectation::Generation(SlotGeneration::initial()),
        second,
        GenerationSelection::enriched(),
    );
    assert!(matches!(
        second_reply,
        Reply::Deployed(DeployOutcome::Repointed {
            previous_identity,
            identity,
            generation,
            committed_at,
            ..
        }) if previous_identity == first_identity
            && identity == second_identity
            && generation == SlotGeneration::new(1)
            && committed_at.commit_sequence() == 2
    ));
    let seated = engine.observe_slot(slot).expect("slot");
    assert_eq!(seated.live, second_identity);
    assert_eq!(seated.seats.len(), 2);
    assert!(seated.seats.contains(&first_identity));
    assert!(seated.seats.contains(&second_identity));
    assert_eq!(engine.commit_count().expect("commit count"), 2);

    let rollback = engine
        .dispatch(
            OTHER_UID,
            Request::Repoint {
                slot,
                expected_generation: SlotGeneration::new(1),
                capsule: CapsuleSelector::Full(first_identity),
                selection: None,
            },
        )
        .expect("rollback dispatch");
    assert!(matches!(
        rollback,
        Reply::Deployed(DeployOutcome::Repointed {
            previous_identity,
            identity,
            generation,
            committed_at,
            ..
        }) if previous_identity == second_identity
            && identity == first_identity
            && generation == SlotGeneration::new(2)
            && committed_at.commit_sequence() == 3
    ));
    let rolled_back = engine.observe_slot(slot).expect("slot");
    assert_eq!(rolled_back.live, first_identity);
    assert_eq!(rolled_back.seats, seated.seats);
    assert_eq!(engine.commit_count().expect("commit count"), 3);

    drop(engine);
    let recovered = NomosEngine::open(&database, ADMIN_UID).expect("engine recovers");
    let recovered_slot = recovered.observe_slot(slot).expect("slot recovers");
    assert_eq!(recovered_slot.live, first_identity);
    assert_eq!(recovered_slot.seats, rolled_back.seats);
    assert_eq!(recovered_slot.generation, SlotGeneration::new(2));
    assert_eq!(recovered.commit_count().expect("commit count"), 3);
}

#[test]
fn sealed_bootstrap_transform_uses_live_and_retained_capsules_without_writes() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("nomos.sema");
    let slot = NomosSlotId::new(9);
    let first = support::authored_artifacts(1);
    let second = support::authored_artifacts(2);
    let first_identity = identity(&first);
    let second_identity = identity(&second);
    let input = support::bootstrap_input();
    let mut engine = NomosEngine::open(&database, ADMIN_UID).expect("engine opens");
    let _ = deploy(
        &mut engine,
        slot,
        SlotExpectation::Empty,
        first,
        GenerationSelection::enriched(),
    );
    let _ = deploy(
        &mut engine,
        slot,
        SlotExpectation::Generation(SlotGeneration::initial()),
        second,
        GenerationSelection::enriched(),
    );
    let before = engine.current_marker().expect("marker");

    for (selector, expected_identity) in [
        (TransformSelector::Live(slot), second_identity),
        (
            TransformSelector::Seated {
                slot,
                capsule: CapsuleSelector::Full(first_identity),
            },
            first_identity,
        ),
    ] {
        let reply = engine
            .transform_bootstrap(selector, &input.assembly)
            .expect("bootstrap transform");
        let Reply::Transformed(outcome) = reply else {
            panic!("native transform reply");
        };
        assert_eq!(outcome.snapshot().identity(), expected_identity);
        assert_eq!(outcome.snapshot().generation(), SlotGeneration::new(1));
        assert_eq!(
            outcome.snapshot().projection_version(),
            NameTreeProjectionVersion::initial()
        );
        let restored = WholeLogos::from_archive_bytes(outcome.logos_population())
            .expect("canonical whole Logos archive");
        let [
            WholeLogosItem::Newtype(wrapped),
            WholeLogosItem::Enumeration(choice),
        ] = restored.items()
        else {
            panic!("bootstrap lowering preserves the exact two declarations")
        };
        assert_eq!(wrapped.name(), &input.wrapped);
        assert_eq!(choice.name(), &input.choice);
        assert_eq!(
            choice
                .variants()
                .iter()
                .map(|variant| variant.name())
                .collect::<Vec<_>>(),
            [&input.none, &input.some, &input.pair]
        );
        let WholeLogosVariantPayload::Tuple(fields) = choice.variants()[2].payload() else {
            panic!("Pair remains a product variant")
        };
        assert_eq!(fields.fields().len(), 2);
    }

    assert_eq!(engine.current_marker().expect("marker"), before);
    assert_eq!(engine.commit_count().expect("commit count"), 2);
}

fn deploy(
    engine: &mut NomosEngine,
    slot: NomosSlotId,
    expected: SlotExpectation,
    artifacts: NomosDeploymentArtifacts,
    selection: GenerationSelection,
) -> Reply {
    engine
        .dispatch(
            OTHER_UID,
            Request::Deploy {
                slot,
                expected,
                artifacts,
                selection,
            },
        )
        .expect("dispatch succeeds")
}

#[test]
fn lifecycle_is_atomic_noop_aware_and_recoverable() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("nomos.sema");
    let slot = NomosSlotId::new(7);
    let second_slot = NomosSlotId::new(8);
    let artifacts = artifacts();
    let identity = identity(&artifacts);
    let mut engine = NomosEngine::open(&database, ADMIN_UID).expect("fresh engine opens");
    assert_eq!(engine.commit_count().expect("commit count"), 0);
    assert!(engine.observe_slot(slot).is_none());

    let fresh = deploy(
        &mut engine,
        slot,
        SlotExpectation::Empty,
        artifacts.clone(),
        GenerationSelection::enriched(),
    );
    let Reply::Deployed(DeployOutcome::FreshDeployed {
        generation,
        committed_at,
        ..
    }) = fresh
    else {
        panic!("fresh deployment reply");
    };
    assert_eq!(generation, SlotGeneration::initial());
    assert_eq!(committed_at.commit_sequence(), 1);
    assert_eq!(committed_at.snapshot(), 1);
    assert_eq!(engine.commit_count().expect("commit count"), 1);

    let before_noop = engine.current_marker().expect("marker");
    let noop = deploy(
        &mut engine,
        slot,
        SlotExpectation::Generation(SlotGeneration::new(999)),
        artifacts.clone(),
        GenerationSelection::enriched(),
    );
    assert!(matches!(
        noop,
        Reply::Deployed(DeployOutcome::AlreadyCurrent {
            observed_at,
            generation,
            ..
        }) if observed_at == before_noop && generation == SlotGeneration::initial()
    ));
    assert_eq!(engine.current_marker().expect("marker"), before_noop);
    assert_eq!(engine.commit_count().expect("commit count"), 1);

    let stale_change = deploy(
        &mut engine,
        slot,
        SlotExpectation::Generation(SlotGeneration::new(999)),
        artifacts.clone(),
        GenerationSelection::new(Vec::new()),
    );
    assert_eq!(
        stale_change,
        Reply::Rejected(Rejection::SlotGenerationMismatch)
    );
    assert_eq!(engine.current_marker().expect("marker"), before_noop);

    let changed_selection = deploy(
        &mut engine,
        slot,
        SlotExpectation::Generation(SlotGeneration::initial()),
        artifacts.clone(),
        GenerationSelection::new(Vec::new()),
    );
    assert!(matches!(
        changed_selection,
        Reply::Deployed(DeployOutcome::Repointed {
            generation,
            committed_at,
            ..
        }) if committed_at.commit_sequence() == 2 && generation == SlotGeneration::new(1)
    ));
    assert_eq!(engine.commit_count().expect("commit count"), 2);

    let existing_capsule_new_slot = deploy(
        &mut engine,
        second_slot,
        SlotExpectation::Empty,
        artifacts.clone(),
        GenerationSelection::enriched(),
    );
    assert!(matches!(
        existing_capsule_new_slot,
        Reply::Deployed(DeployOutcome::FreshDeployed {
            generation,
            committed_at,
            ..
        }) if committed_at.commit_sequence() == 3
            && generation == SlotGeneration::initial()
    ));
    assert_eq!(engine.commit_count().expect("commit count"), 3);

    let short = identity.content_addressed_hash().to_hexadecimal()[..4].to_owned();
    let short_noop = engine
        .dispatch(
            OTHER_UID,
            Request::Repoint {
                slot: second_slot,
                expected_generation: SlotGeneration::new(999),
                capsule: CapsuleSelector::Short(
                    ShortCapsuleDisplay::try_new(short).expect("short display"),
                ),
                selection: None,
            },
        )
        .expect("short repoint dispatch");
    assert!(matches!(
        short_noop,
        Reply::Deployed(DeployOutcome::AlreadyCurrent { .. })
    ));
    assert_eq!(engine.commit_count().expect("commit count"), 3);

    drop(engine);
    let recovered = NomosEngine::open(&database, ADMIN_UID).expect("recovery opens");
    assert_eq!(recovered.commit_count().expect("commit count"), 3);
    assert_eq!(
        recovered
            .observe_slot(slot)
            .expect("slot survives")
            .generation,
        SlotGeneration::new(1)
    );
    assert_eq!(
        recovered
            .observe_slot(second_slot)
            .expect("slot survives")
            .live,
        identity
    );
}

#[test]
fn projection_admin_path_is_separate_and_receipts_are_not_trusted() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("nomos.sema");
    let slot = NomosSlotId::new(11);
    let artifacts = artifacts();
    let initial_artifacts = artifacts.clone();
    let identity = identity(&artifacts);
    let mut engine = NomosEngine::open(&database, ADMIN_UID).expect("engine opens");
    let _ = deploy(
        &mut engine,
        slot,
        SlotExpectation::Empty,
        artifacts,
        GenerationSelection::enriched(),
    );
    let binding_before = engine.observe_slot(slot).expect("slot");

    let successor = population(NameTreeProjectionVersion::new(1));
    let projection =
        NomosProjectionArchive::from_projection(successor.projection()).expect("successor archive");
    let unauthorized = engine
        .dispatch(
            OTHER_UID,
            Request::AdvanceProjection {
                capsule: identity,
                expected_previous_version: NameTreeProjectionVersion::initial(),
                projection: projection.clone(),
                translator_receipt: None,
            },
        )
        .expect("dispatch");
    assert_eq!(unauthorized, Reply::Rejected(Rejection::Unauthorized));
    assert_eq!(engine.commit_count().expect("count"), 1);

    let unsupported = engine
        .dispatch(
            ADMIN_UID,
            Request::AdvanceProjection {
                capsule: identity,
                expected_previous_version: NameTreeProjectionVersion::initial(),
                projection: projection.clone(),
                translator_receipt: Some(
                    TranslatorRenameReceiptArchive::try_new(vec![1]).expect("opaque receipt"),
                ),
            },
        )
        .expect("dispatch");
    assert_eq!(
        unsupported,
        Reply::Rejected(Rejection::TranslatorReceiptUnsupported)
    );
    assert_eq!(engine.commit_count().expect("count"), 1);

    let advanced = engine
        .dispatch(
            ADMIN_UID,
            Request::AdvanceProjection {
                capsule: identity,
                expected_previous_version: NameTreeProjectionVersion::initial(),
                projection,
                translator_receipt: None,
            },
        )
        .expect("dispatch");
    assert!(matches!(
        advanced,
        Reply::ProjectionAdvanced(ProjectionOutcome::Advanced {
            previous_version,
            version,
            committed_at,
            ..
        }) if previous_version == NameTreeProjectionVersion::initial()
            && version == NameTreeProjectionVersion::new(1)
            && committed_at.commit_sequence() == 2
    ));
    let binding_after = engine.observe_slot(slot).expect("slot");
    assert_eq!(binding_after.generation, binding_before.generation);
    assert_eq!(
        binding_after.binding_committed_at,
        binding_before.binding_committed_at
    );
    assert_eq!(
        engine
            .projection_versions(identity)
            .expect("projection versions"),
        vec![
            NameTreeProjectionVersion::initial(),
            NameTreeProjectionVersion::new(1)
        ]
    );
    let stale_projection = deploy(
        &mut engine,
        slot,
        SlotExpectation::Generation(SlotGeneration::new(999)),
        initial_artifacts,
        GenerationSelection::enriched(),
    );
    assert_eq!(
        stale_projection,
        Reply::Rejected(Rejection::ProjectionStale)
    );
    assert_eq!(engine.commit_count().expect("count"), 2);

    drop(engine);
    let recovered = NomosEngine::open(&database, ADMIN_UID).expect("engine recovers");
    assert_eq!(
        recovered
            .projection_versions(identity)
            .expect("projection history recovers"),
        vec![
            NameTreeProjectionVersion::initial(),
            NameTreeProjectionVersion::new(1)
        ]
    );
    let recovered_binding = recovered.observe_slot(slot).expect("binding recovers");
    assert_eq!(recovered_binding.generation, binding_before.generation);
    assert_eq!(
        recovered_binding.binding_committed_at,
        binding_before.binding_committed_at
    );
    assert_eq!(recovered.commit_count().expect("count"), 2);
}

#[test]
fn bootstrap_transform_is_direct_checked_and_mutation_free() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("nomos.sema");
    let slot = NomosSlotId::new(13);
    let artifacts = artifacts();
    let identity = identity(&artifacts);
    let mut engine = NomosEngine::open(&database, ADMIN_UID).expect("engine opens");
    let _ = deploy(
        &mut engine,
        slot,
        SlotExpectation::Empty,
        artifacts,
        GenerationSelection::enriched(),
    );
    let input = support::bootstrap_input();
    let before = engine.current_marker().expect("marker");
    let reply = engine
        .transform_bootstrap(TransformSelector::Live(slot), &input.assembly)
        .expect("bootstrap transform");
    let Reply::Transformed(outcome) = reply else {
        panic!("native transform reply");
    };
    assert_eq!(outcome.snapshot().identity(), identity);
    assert_eq!(
        outcome.snapshot().projection_version(),
        NameTreeProjectionVersion::initial()
    );
    let restored = WholeLogos::from_archive_bytes(outcome.logos_population())
        .expect("reply bytes restore through Whole Logos archive truth");
    assert_eq!(restored.items().len(), 2);
    assert_eq!(engine.current_marker().expect("marker"), before);
    assert_eq!(engine.commit_count().expect("count"), 1);

    let wire_refusal = engine
        .dispatch(
            OTHER_UID,
            Request::Transform {
                selector: TransformSelector::Live(slot),
                ethos: EthosPopulationArchive::try_new(vec![0x42])
                    .expect("non-empty opaque archive"),
            },
        )
        .expect("typed wire refusal");
    assert_eq!(
        wire_refusal,
        Reply::Rejected(Rejection::EthosPopulationInvalid)
    );
    assert_eq!(engine.current_marker().expect("marker"), before);
}

#[test]
fn short_capsule_displays_are_slot_scoped_ambiguous_and_lengthenable() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("nomos.sema");
    let slot = NomosSlotId::new(10);
    let single_seat_slot = NomosSlotId::new(12);
    let first = support::authored_artifacts(106);
    let second = support::authored_artifacts(301);
    let first_identity = identity(&first);
    let second_identity = identity(&second);
    let first_hex = first_identity.content_addressed_hash().to_hexadecimal();
    let second_hex = second_identity.content_addressed_hash().to_hexadecimal();
    assert_eq!(&first_hex[..4], "d525");
    assert_eq!(&second_hex[..4], "d525");
    assert_ne!(&first_hex[..5], &second_hex[..5]);
    assert!(ShortCapsuleDisplay::try_new("d52").is_err());

    let mut engine = NomosEngine::open(&database, ADMIN_UID).expect("engine opens");
    let _ = deploy(
        &mut engine,
        slot,
        SlotExpectation::Empty,
        first.clone(),
        GenerationSelection::enriched(),
    );
    let _ = deploy(
        &mut engine,
        slot,
        SlotExpectation::Generation(SlotGeneration::initial()),
        second,
        GenerationSelection::enriched(),
    );
    let _ = deploy(
        &mut engine,
        single_seat_slot,
        SlotExpectation::Empty,
        first,
        GenerationSelection::enriched(),
    );
    let before = engine.current_marker().expect("marker");
    let count_before = engine.commit_count().expect("count");

    let scoped = engine
        .dispatch(
            OTHER_UID,
            Request::Repoint {
                slot: single_seat_slot,
                expected_generation: SlotGeneration::new(999),
                capsule: CapsuleSelector::Short(
                    ShortCapsuleDisplay::try_new("d525").expect("four digits"),
                ),
                selection: None,
            },
        )
        .expect("dispatch");
    assert!(matches!(
        scoped,
        Reply::Deployed(DeployOutcome::AlreadyCurrent {
            identity,
            generation,
            ..
        }) if identity == first_identity && generation == SlotGeneration::initial()
    ));

    let ambiguous = engine
        .dispatch(
            OTHER_UID,
            Request::Repoint {
                slot,
                expected_generation: SlotGeneration::new(1),
                capsule: CapsuleSelector::Short(
                    ShortCapsuleDisplay::try_new("d525").expect("four digits"),
                ),
                selection: None,
            },
        )
        .expect("dispatch");
    assert_eq!(
        ambiguous,
        Reply::Rejected(Rejection::AmbiguousShortCapsuleDisplay)
    );

    let unknown = engine
        .dispatch(
            OTHER_UID,
            Request::Repoint {
                slot,
                expected_generation: SlotGeneration::new(1),
                capsule: CapsuleSelector::Short(
                    ShortCapsuleDisplay::try_new("ffff").expect("four digits"),
                ),
                selection: None,
            },
        )
        .expect("dispatch");
    assert_eq!(unknown, Reply::Rejected(Rejection::CapsuleNotSeated));
    assert_eq!(engine.current_marker().expect("marker"), before);
    assert_eq!(engine.commit_count().expect("count"), count_before);

    let lengthened = engine
        .dispatch(
            OTHER_UID,
            Request::Repoint {
                slot,
                expected_generation: SlotGeneration::new(1),
                capsule: CapsuleSelector::Short(
                    ShortCapsuleDisplay::try_new(first_hex[..5].to_owned())
                        .expect("unique lengthened display"),
                ),
                selection: None,
            },
        )
        .expect("dispatch");
    assert!(matches!(
        lengthened,
        Reply::Deployed(DeployOutcome::Repointed {
            previous_identity,
            identity,
            generation,
            ..
        }) if previous_identity == second_identity
            && identity == first_identity
            && generation == SlotGeneration::new(2)
    ));
}
