//! Stateful Nomos daemon core.
//!
//! The component owns one embedded, versioned Sema database and evaluates
//! sealed authored Nomos directly over encoded Ethos populations. No central
//! storage socket, legacy package evaluator, fixture, or output-slot write is
//! part of this production surface.

pub mod batch;
mod name_tree;
mod store;

use std::path::Path;
use std::sync::Arc;

pub use name_tree::{
    EngineEthosNameTree, EngineLogosNameTree, EngineNameRealization, EngineReferenceMapping,
    NameTreeError, PopulationError, encode_ethos_population,
};
pub use store::{Error, NomosEngine, ObservedSlot, Result};
use tokio::sync::Mutex;

use signal_nomos::{Reply, Request};

/// Cloneable async facade used by the Unix daemon.
#[derive(Clone)]
pub struct Runtime {
    engine: Arc<Mutex<NomosEngine>>,
}

impl Runtime {
    pub fn open(database: &Path, admin_uid: u32) -> Result<Self> {
        Ok(Self {
            engine: Arc::new(Mutex::new(NomosEngine::open(database, admin_uid)?)),
        })
    }

    pub async fn request_as(&self, peer_uid: u32, request: Request) -> Result<Reply> {
        self.engine.lock().await.dispatch(peer_uid, request)
    }
}
