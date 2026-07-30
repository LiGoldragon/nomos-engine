use core_ethos::{
    WholeEthos, WholeEthosAttributes, WholeEthosItem, WholeEthosNewtype, WholeEthosTypeReference,
    WholeEthosVisibility, WholeEthosWrappedField,
};
use core_nomos::NameTransform;
use name_table::LocalEncodedId;
use nomos_engine::{
    EngineEthosNameTree, EngineNameRealization, EngineReferenceMapping, Error, NameTreeError,
    encode_ethos_population,
};
use signal_nomos::CommitMarker;
use signal_sema_translator::{VocabularyEncodedId, VocabularyRoot};

fn identity(root: VocabularyRoot, chain: &[u16]) -> VocabularyEncodedId {
    VocabularyEncodedId::new(
        root,
        chain.iter().copied().map(LocalEncodedId::new).collect(),
    )
    .expect("non-empty identity")
}

#[test]
fn reference_mapping_is_exact_across_same_leaf_different_ancestors() {
    let mapped_source = identity(VocabularyRoot::Universal, &[4, 1]);
    let homonym = identity(VocabularyRoot::Universal, &[5, 1]);
    let target = identity(VocabularyRoot::Rust, &[8, 2]);
    let tree = EngineEthosNameTree::try_new(
        vec![mapped_source.clone(), homonym.clone()],
        Vec::new(),
        vec![
            EngineReferenceMapping::try_new(mapped_source.clone(), target.clone())
                .expect("mapping"),
        ],
        Vec::new(),
        Vec::new(),
    )
    .expect("complete plan");
    assert_eq!(tree.mapped_reference(&mapped_source), target);
    assert_eq!(tree.mapped_reference(&homonym), homonym);
}

#[test]
fn colliding_realization_targets_are_refused_before_evaluation() {
    let first = identity(VocabularyRoot::Universal, &[1]);
    let second = identity(VocabularyRoot::Universal, &[2]);
    let target = identity(VocabularyRoot::Universal, &[9]);
    let result = EngineEthosNameTree::try_new(
        vec![first.clone(), second.clone()],
        vec![
            EngineNameRealization::try_new(first, NameTransform::PascalCase, target.clone())
                .expect("realization"),
            EngineNameRealization::try_new(second, NameTransform::PascalCase, target.clone())
                .expect("realization"),
        ],
        Vec::new(),
        vec![target],
        Vec::new(),
    );
    assert!(matches!(
        result,
        Err(NameTreeError::DuplicateRealizationTarget)
    ));
}

#[test]
fn ethos_population_refuses_missing_declaration_closure_and_wrong_roots() {
    let name = identity(VocabularyRoot::Universal, &[30, 1]);
    let reference = identity(VocabularyRoot::Universal, &[40, 1]);
    let ethos = WholeEthos::new(vec![WholeEthosItem::Newtype(WholeEthosNewtype::new(
        name,
        WholeEthosVisibility::Public,
        WholeEthosAttributes::empty(),
        WholeEthosWrappedField::new(
            WholeEthosVisibility::Private,
            WholeEthosTypeReference::Identity(reference),
        ),
    ))]);
    let empty_names =
        EngineEthosNameTree::try_new(Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new())
            .expect("empty plan");
    assert!(encode_ethos_population(ethos, empty_names).is_err());

    let wrong_root = WholeEthos::new(vec![WholeEthosItem::Newtype(WholeEthosNewtype::new(
        identity(VocabularyRoot::Rust, &[30, 1]),
        WholeEthosVisibility::Public,
        WholeEthosAttributes::empty(),
        WholeEthosWrappedField::new(
            WholeEthosVisibility::Private,
            WholeEthosTypeReference::Identity(identity(VocabularyRoot::Universal, &[40, 1])),
        ),
    ))]);
    let wrong_root_names = EngineEthosNameTree::try_new(
        vec![identity(VocabularyRoot::Rust, &[30, 1])],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    assert!(wrong_root_names.is_err());
    let empty_names =
        EngineEthosNameTree::try_new(Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new())
            .expect("empty plan");
    assert!(encode_ethos_population(wrong_root, empty_names).is_err());
}

#[test]
fn production_sources_exclude_legacy_and_central_storage_paths() {
    let library = include_str!("../src/lib.rs");
    let store = include_str!("../src/store.rs");
    let manifest = include_str!("../Cargo.toml");
    for forbidden in [
        "MacroPackage",
        "apply_enriched",
        "enriched_fixture",
        "wire_fixture",
        "signal-sema-storage",
        "signal_sema_storage",
        "SemaPlane",
        "output_slot",
        "kameo",
    ] {
        assert!(
            !library.contains(forbidden)
                && !store.contains(forbidden)
                && !manifest.contains(forbidden),
            "production reachability contains {forbidden}"
        );
    }
    assert!(store.contains("EngineOpen::new"));
    assert!(store.contains("NativeAuthoredEvaluator"));
    assert!(store.contains("NativeLogosPopulation::<EngineLogosNameTree>::from_archive_bytes"));
}

#[test]
fn daemon_rejects_oversized_frames_before_allocating_payload() {
    let source = include_str!("../src/bin/nomos-engine.rs");
    let guard = source
        .find("if length > MAX_REQUEST_FRAME_BYTES")
        .expect("frame bound guard");
    let allocation = source.find("vec![0; length]").expect("payload allocation");
    assert!(guard < allocation);
    assert!(source.contains("[to-be-reviewed-by-psyche]"));
}

#[test]
fn only_proven_precommit_storage_errors_can_become_typed_refusals() {
    assert!(Error::Engine("precommit".into()).can_reply_storage_failed());
    assert!(Error::MarkerExhausted.can_reply_storage_failed());
    assert!(!Error::PostCommitFailure("unknown".into()).can_reply_storage_failed());
    assert!(
        !Error::CommitReceiptDivergence {
            predicted: CommitMarker::new(1, 1),
            actual: CommitMarker::new(2, 2),
            expected_operations: 1,
            actual_operations: 2,
        }
        .can_reply_storage_failed()
    );
    assert!(!Error::Poisoned.can_reply_storage_failed());
    assert!(!Error::State("corrupt".into()).can_reply_storage_failed());

    let store = include_str!("../src/store.rs");
    assert!(store.contains("Err(error) if error.can_reply_storage_failed() && !self.poisoned"));
    assert!(store.contains("Ok(_) | Err(_) =>"));
    assert!(store.contains("self.poisoned = true;"));
}
