use core_logos::{WholeLogosItem, WholeLogosVariantPayload};
use core_nomos::BootstrapSliceOneLowering;
use nomos_engine::Error;
use signal_nomos::CommitMarker;

#[expect(
    dead_code,
    reason = "the shared fixture surface also carries authored Capsule construction"
)]
mod support;

#[test]
fn sealed_bootstrap_transaction_revalidates_and_lowers_directly() {
    let input = support::bootstrap_input();
    input
        .assembly
        .reader()
        .validate_transaction(input.assembly.transaction())
        .expect("matching reader revalidates authority receipt and prepared model");
    let logos = BootstrapSliceOneLowering::new()
        .lower(input.assembly.reader(), input.assembly.transaction())
        .expect("Core Nomos lowers the sealed transaction directly");

    let [
        WholeLogosItem::Newtype(wrapped),
        WholeLogosItem::Enumeration(choice),
    ] = logos.items()
    else {
        panic!("canonical transaction order contains Wrapped then Choice")
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
    let WholeLogosVariantPayload::Tuple(product) = choice.variants()[2].payload() else {
        panic!("Pair remains a product variant")
    };
    assert_eq!(product.fields().len(), 2);
    assert_eq!(
        input.assembly.canonical_source(),
        "Nexus.{1 0 0}\n[]\n{\n  []\n  [Wrapped.Vector<Option<String>> Choice.[None Some.String Pair.{Map<String Integer> Boolean}]]\n}\n"
    );
}

#[test]
fn production_surface_is_bootstrap_sealed_and_exactly_pinned() {
    let library = include_str!("../src/lib.rs");
    let store = include_str!("../src/store.rs");
    let manifest = include_str!("../Cargo.toml");
    let lockfile = include_str!("../Cargo.lock");
    for forbidden in [
        "MacroPackage",
        "NativeAuthoredEvaluator",
        "EngineEthosNameTree",
        "EncodedPopulation<WholeEthos",
        "encode_ethos_population",
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
    for required in [
        "VerifiedBootstrapAssembly",
        "BootstrapSliceOneLowering",
        "transform_bootstrap",
        "EthosPopulationInvalid",
    ] {
        assert!(store.contains(required), "missing live boundary {required}");
    }
    assert!(manifest.contains("version = \"0.16.0\""));
    for revision in [
        "db5a97a573113202e4de6c97e71b33491bd9666f",
        "abee4036fbeb58c767ef7dc3489804e2afd5c6e1",
        "250e728fa9e5a02e3c9a6d4f0cfee0683863df83",
        "22de53fced0eff372930f5b7baec0c667f1a16d5",
        "bdcf54021e880f75ab693d00e3707478ca7de487",
        "1da83e03cdc5cea10e529d081d5a10437bd6628e",
        "3a26cb43f8ce7f9fe85da64d19aa55aa662943ce",
        "413e3744569ca237e837a1fd57d9ba6ad6adc3de",
    ] {
        assert!(manifest.contains(revision), "missing exact pin {revision}");
        assert!(
            lockfile.contains(revision),
            "lockfile does not resolve exact pin {revision}"
        );
    }
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
