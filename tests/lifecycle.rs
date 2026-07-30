use capsule_content_identity::{CapsuleIdentity, PortableArchive};
use core_ethos::{
    WholeEthos, WholeEthosAttributes, WholeEthosTypeReference, WholeEthosVisibility,
    WholeEthosWrappedField,
};
use core_nomos::{
    AuthoredTransformerSet, LoadedNomosPopulation, NameTransform, NameTreeProjectionVersion,
    NativeEvaluatedLogos, NomosNameTable,
};
use nomos_engine::{
    EngineEthosNameTree, EngineLogosNameTree, EngineNameRealization, NomosEngine,
    encode_ethos_population,
};
use protos::EncodedPopulation;
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
struct ForgedWholeEthosNewtype(
    VocabularyEncodedId,
    WholeEthosVisibility,
    WholeEthosAttributes,
    WholeEthosWrappedField,
);

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
struct ForgedWholeEthosEnumeration(
    VocabularyEncodedId,
    WholeEthosVisibility,
    WholeEthosAttributes,
    Vec<ForgedWholeEthosVariant>,
);

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
struct ForgedWholeEthosVariant(
    VocabularyEncodedId,
    WholeEthosAttributes,
    ForgedWholeEthosVariantPayload,
);

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
enum ForgedWholeEthosVariantPayload {
    Unit,
    Tuple(ForgedWholeEthosTupleFields),
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
struct ForgedWholeEthosTupleFields(Vec<WholeEthosTypeReference>);

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
fn malformed_and_forged_empty_tuple_ethos_are_rejected_before_evaluation() {
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

    let enumeration_name = vocabulary_id(VocabularyRoot::Universal, &[500, 2]);
    let variant_name = vocabulary_id(VocabularyRoot::Universal, &[500, 2, 1]);
    let realized_enumeration = vocabulary_id(VocabularyRoot::Universal, &[700, 2]);
    let names = EngineEthosNameTree::try_new(
        vec![enumeration_name.clone(), variant_name.clone()],
        vec![
            EngineNameRealization::try_new(
                enumeration_name.clone(),
                NameTransform::PascalCase,
                realized_enumeration.clone(),
            )
            .expect("realization"),
        ],
        Vec::new(),
        vec![realized_enumeration, variant_name.clone()],
        Vec::new(),
    )
    .expect("authenticated plan");
    let forged = EncodedPopulation::new(
        ForgedWholeEthos(vec![ForgedWholeEthosItem::Enumeration(
            ForgedWholeEthosEnumeration(
                enumeration_name,
                WholeEthosVisibility::Public,
                WholeEthosAttributes::empty(),
                vec![ForgedWholeEthosVariant(
                    variant_name,
                    WholeEthosAttributes::empty(),
                    ForgedWholeEthosVariantPayload::Tuple(ForgedWholeEthosTupleFields(Vec::new())),
                )],
            ),
        )]),
        names,
    );
    let forged_bytes =
        <EncodedPopulation<ForgedWholeEthos, EngineEthosNameTree> as PortableArchive>::to_archive_bytes(
            &forged,
        )
        .expect("layout-compatible forged population");
    let empty_tuple = engine
        .dispatch(
            OTHER_UID,
            Request::Transform {
                selector: TransformSelector::Live(slot),
                ethos: EthosPopulationArchive::try_new(forged_bytes.as_ref().to_vec())
                    .expect("non-empty archive"),
            },
        )
        .expect("typed refusal");
    assert_eq!(
        empty_tuple,
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
fn nonempty_native_transform_uses_live_and_retained_capsules_without_writes() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database = directory.path().join("nomos.sema");
    let slot = NomosSlotId::new(9);
    let first = support::authored_artifacts(1);
    let second = support::authored_artifacts(2);
    let first_identity = identity(&first);
    let second_identity = identity(&second);
    let input = support::native_input();
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
            .dispatch(
                OTHER_UID,
                Request::Transform {
                    selector,
                    ethos: input.archive.clone(),
                },
            )
            .expect("transform dispatch");
        let Reply::Transformed(outcome) = reply else {
            panic!("native transform reply");
        };
        assert_eq!(outcome.snapshot().identity(), expected_identity);
        assert_eq!(outcome.snapshot().generation(), SlotGeneration::new(1));
        assert_eq!(
            outcome.snapshot().projection_version(),
            NameTreeProjectionVersion::initial()
        );
        engine
            .validate_logos_population(expected_identity, &input.names, outcome.logos_population())
            .expect("checked output restores against exact input plan");

        type Population = EncodedPopulation<NativeEvaluatedLogos, EngineLogosNameTree>;
        let restored =
            <Population as PortableArchive>::from_archive_bytes(outcome.logos_population())
                .expect("checked archive");
        assert_eq!(
            restored.name_tree().declarations(),
            input.names.expected_logos_declarations()
        );
        assert_eq!(
            restored.name_tree().references_used(),
            &[
                input.preserved_reference.clone(),
                input.mapped_reference.clone()
            ]
        );
        assert_eq!(restored.name_tree().references().len(), 1);
        assert_eq!(
            restored.name_tree().references()[0].source(),
            &input.mapping_source
        );
        assert_eq!(
            restored.name_tree().references()[0].target(),
            &input.mapped_reference
        );
    }

    let alternate = support::alternate_native_input();
    let alternate_reply = engine
        .dispatch(
            OTHER_UID,
            Request::Transform {
                selector: TransformSelector::Live(slot),
                ethos: alternate.archive.clone(),
            },
        )
        .expect("alternate transform");
    let Reply::Transformed(alternate_outcome) = alternate_reply else {
        panic!("alternate native transform reply");
    };
    engine
        .validate_logos_population(
            second_identity,
            &alternate.names,
            alternate_outcome.logos_population(),
        )
        .expect("alternate output matches its authenticated plan");
    assert!(
        engine
            .validate_logos_population(
                second_identity,
                &input.names,
                alternate_outcome.logos_population(),
            )
            .is_err(),
        "valid output bytes cannot be rebound to a different input plan"
    );

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
fn transform_is_direct_checked_and_mutation_free() {
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
    let names =
        EngineEthosNameTree::try_new(Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new())
            .expect("empty complete plan");
    let ethos = encode_ethos_population(WholeEthos::new(Vec::new()), names.clone())
        .expect("direct population");
    let before = engine.current_marker().expect("marker");
    let reply = engine
        .dispatch(
            OTHER_UID,
            Request::Transform {
                selector: TransformSelector::Live(slot),
                ethos,
            },
        )
        .expect("transform dispatch");
    let Reply::Transformed(outcome) = reply else {
        panic!("native transform reply");
    };
    assert_eq!(outcome.snapshot().identity(), identity);
    assert_eq!(
        outcome.snapshot().projection_version(),
        NameTreeProjectionVersion::initial()
    );
    engine
        .validate_logos_population(identity, &names, outcome.logos_population())
        .expect("reply bytes restore through native semantic boundary");
    assert_eq!(engine.current_marker().expect("marker"), before);
    assert_eq!(engine.commit_count().expect("count"), 1);

    let unexpected = signal_sema_translator::VocabularyEncodedId::new(
        signal_sema_translator::VocabularyRoot::Universal,
        vec![name_table::LocalEncodedId::new(99)],
    )
    .expect("identity");
    let wrong_plan = EngineEthosNameTree::try_new(
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![unexpected],
        Vec::new(),
    )
    .expect("internally valid but incorrect output plan");
    let wrong_ethos = encode_ethos_population(WholeEthos::new(Vec::new()), wrong_plan)
        .expect("direct population");
    let rejected = engine
        .dispatch(
            OTHER_UID,
            Request::Transform {
                selector: TransformSelector::Live(slot),
                ethos: wrong_ethos,
            },
        )
        .expect("transform dispatch");
    assert_eq!(rejected, Reply::Rejected(Rejection::LoweringFailed));
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
