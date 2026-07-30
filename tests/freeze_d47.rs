use std::fs;
use std::path::{Path, PathBuf};

use nomos_engine::NomosEngine;
use signal_nomos::{
    GenerationSelection, NomosSlotId, Reply, Request, SlotExpectation, TransformSelector,
    encode_request,
};

#[expect(
    dead_code,
    reason = "the one-shot freezer uses only the authored fixture half of shared support"
)]
mod support;

const ADMIN_UID: u32 = 4_100;
const CALLER_UID: u32 = 4_200;
const FIXTURE_SEED: u16 = 19;
const FIXTURE_SLOT: NomosSlotId = NomosSlotId::new(19);

fn workspace_root() -> PathBuf {
    PathBuf::from(
        std::env::var_os("FREEZE_D47_WORKSPACE")
            .expect("FREEZE_D47_WORKSPACE names the LiGoldragon checkout root"),
    )
}

fn write_bytes(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().expect("fixture has a parent"))
        .expect("create golden directory");
    fs::write(path, bytes).expect("write frozen bytes");
}

#[test]
#[ignore = "one-shot d47 compatibility fixture generator"]
fn freeze_d47_compatibility_artifacts() {
    let root = workspace_root();
    let artifacts = support::authored_artifacts(FIXTURE_SEED);
    write_bytes(
        &root.join("core-nomos/tests/goldens/d47_nomos_capsule.bin"),
        artifacts.capsule().bytes(),
    );
    write_bytes(
        &root.join("core-nomos/tests/goldens/d47_nomos_projection.bin"),
        artifacts.projection().bytes(),
    );

    let deploy = Request::Deploy {
        slot: FIXTURE_SLOT,
        expected: SlotExpectation::Empty,
        artifacts: artifacts.clone(),
        selection: GenerationSelection::enriched(),
    };
    let deploy_bytes = encode_request(&deploy).expect("encode old Deploy request");
    write_bytes(
        &root.join("signal-nomos/tests/goldens/d47_deploy_request.bin"),
        &deploy_bytes,
    );

    let state_root = root.join("nomos-engine/tests/goldens/d47_state");
    if state_root.exists() {
        fs::remove_dir_all(&state_root).expect("replace prior generated state fixture");
    }
    fs::create_dir_all(&state_root).expect("create state fixture root");
    let database = state_root.join("nomos.sema");
    let mut engine = NomosEngine::open(&database, ADMIN_UID).expect("open old engine state");
    let deployed = engine
        .dispatch(ADMIN_UID, deploy)
        .expect("dispatch old Deploy request");
    assert!(matches!(deployed, Reply::Deployed(_)));

    let input = support::native_input();
    let transformed = engine
        .dispatch(
            CALLER_UID,
            Request::Transform {
                selector: TransformSelector::Live(FIXTURE_SLOT),
                ethos: input.archive,
            },
        )
        .expect("dispatch old transform");
    assert!(matches!(transformed, Reply::Transformed(_)));
    assert_eq!(engine.commit_count().expect("old commit count"), 1);
}
