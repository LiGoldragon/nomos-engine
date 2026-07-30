use std::fs;

use nomos_engine::NomosEngine;
use signal_nomos::{NomosSlotId, Reply, Request, TransformSelector};

mod support;

const ADMIN_UID: u32 = 4_100;
const CALLER_UID: u32 = 4_200;
const FIXTURE_SLOT: NomosSlotId = NomosSlotId::new(19);
const STATE: &[u8] = include_bytes!("goldens/d47_state/nomos.sema");

#[test]
fn d47_engine_state_restarts_transforms_and_remains_byte_exact() {
    let directory = tempfile::tempdir().expect("temporary d47 engine directory");
    let database = directory.path().join("nomos.sema");
    fs::write(&database, STATE).expect("copy frozen d47 engine state");

    let mut engine = NomosEngine::open(&database, ADMIN_UID).expect("restart d47 engine state");
    let slot = engine
        .observe_slot(FIXTURE_SLOT)
        .expect("d47 live slot restores");
    assert_eq!(engine.commit_count().expect("d47 commit count"), 1);

    let input = support::native_input();
    let names = input.names.clone();
    let transformed = engine
        .dispatch(
            CALLER_UID,
            Request::Transform {
                selector: TransformSelector::Live(FIXTURE_SLOT),
                ethos: input.archive,
            },
        )
        .expect("transform from d47 state");
    let Reply::Transformed(outcome) = transformed else {
        panic!("d47 state produces a native transform");
    };
    assert_eq!(outcome.snapshot().identity(), slot.live);
    engine
        .validate_logos_population(slot.live, &names, outcome.logos_population())
        .expect("d47 transform output remains semantically valid");
    assert_eq!(engine.commit_count().expect("mutation-free transform"), 1);
    let marker = engine.current_marker().expect("recovered marker");
    drop(engine);

    let reopened = NomosEngine::open(&database, ADMIN_UID).expect("reopen recovered d47 state");
    assert_eq!(reopened.current_marker().expect("stable marker"), marker);
    assert_eq!(reopened.commit_count().expect("stable commit count"), 1);
    assert_eq!(
        reopened.observe_slot(FIXTURE_SLOT),
        Some(slot),
        "the recovered domain record is unchanged"
    );
}
