use std::collections::BTreeMap;
use std::path::Path;

use capsule_content_identity::{CapsuleIdentity, PortableArchive};
use core_nomos::{
    NameTreeProjectionVersion, NativeAuthoredEvaluator, NativeEvaluatedLogos,
    NativeLogosPopulation, SealedNomosCapsule, SealedNomosPopulation,
};
use protos::EncodedPopulation;
use rkyv::{Archive, Deserialize, Serialize};
use sema_engine::{
    AtomicCommit, Engine, EngineOpen, EngineRecord, FamilyDirectory, FamilyName, QueryPlan,
    RecordKey, RowMaterializer, SchemaHash, SchemaVersion, TableDescriptor, TableName,
    TableReference, VersionedStoreName, VersioningPolicy,
};
use signal_nomos::{
    AdmissionSnapshot, CapsuleSelector, CommitMarker, DeployOutcome, GenerationSelection,
    NomosCapsuleArchive, NomosDeploymentArtifacts, NomosProjectionArchive, NomosSlotId,
    ProjectionOutcome, Rejection, Reply, Request, SlotExpectation, SlotGeneration,
    TransformOutcome, TransformSelector,
};

use crate::name_tree::{
    EngineEthosNameTree, EngineLogosNameTree, PopulationError, decode_ethos_population,
};

const NOMOS_TABLE: TableName = TableName::new("nomos");
const NOMOS_FAMILY: &str = "nomos-engine-state";
const NOMOS_SCHEMA: &str = "nomos-engine-state-v1";

#[derive(Archive, Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
struct CapsuleState {
    identity: CapsuleIdentity,
    capsule: NomosCapsuleArchive,
    projections: Vec<NomosProjectionArchive>,
}

impl CapsuleState {
    fn from_artifacts(identity: CapsuleIdentity, artifacts: &NomosDeploymentArtifacts) -> Self {
        Self {
            identity,
            capsule: artifacts.capsule().clone(),
            projections: vec![artifacts.projection().clone()],
        }
    }

    fn current_projection(&self) -> &NomosProjectionArchive {
        self.projections
            .last()
            .expect("validated CapsuleState has a projection")
    }

    fn sealed_population(&self) -> Result<SealedNomosPopulation> {
        SealedNomosPopulation::from_archive_parts(
            self.capsule.bytes(),
            self.current_projection().bytes(),
        )
        .map_err(|error| Error::State(error.to_string()))
    }

    fn sealed_capsule(&self) -> Result<SealedNomosCapsule> {
        self.capsule
            .restore()
            .map_err(|error| Error::State(error.to_string()))
    }

    fn validate(&self) -> Result<()> {
        let capsule = self.sealed_capsule()?;
        if capsule.content_identity() != self.identity || self.projections.is_empty() {
            return Err(Error::State(
                "Capsule row identity/projection envelope is inconsistent".into(),
            ));
        }
        let mut previous: Option<NameTreeProjectionVersion> = None;
        for projection in &self.projections {
            let population =
                SealedNomosPopulation::from_archive_parts(self.capsule.bytes(), projection.bytes())
                    .map_err(|error| Error::State(error.to_string()))?;
            let version = population.projection().version();
            match previous {
                None if version != NameTreeProjectionVersion::initial() => {
                    return Err(Error::State(
                        "Capsule projection history does not begin at version zero".into(),
                    ));
                }
                Some(prior) if prior.checked_successor() != Some(version) => {
                    return Err(Error::State(
                        "Capsule projection history is not contiguous".into(),
                    ));
                }
                _ => {}
            }
            previous = Some(version);
        }
        Ok(())
    }
}

#[derive(Archive, Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
struct SlotState {
    slot: NomosSlotId,
    seats: Vec<CapsuleIdentity>,
    live: CapsuleIdentity,
    generation: SlotGeneration,
    selection: GenerationSelection,
    binding_committed_at: CommitMarker,
}

impl SlotState {
    fn validate(&self, current: CommitMarker) -> Result<()> {
        if self.seats.is_empty()
            || self.seats.windows(2).any(|pair| pair[0] >= pair[1])
            || self.seats.binary_search(&self.live).is_err()
            || self
                .seats
                .iter()
                .any(|identity| !matches!(identity, CapsuleIdentity::Nomos(_)))
            || self.binding_committed_at.commit_sequence() == 0
            || self.binding_committed_at.snapshot() == 0
            || self.binding_committed_at > current
        {
            return Err(Error::State("stored slot row is inconsistent".into()));
        }
        Ok(())
    }
}

#[derive(Archive, Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
enum NomosRecord {
    Capsule(CapsuleState),
    Slot(SlotState),
}

impl EngineRecord for NomosRecord {
    fn record_key(&self) -> RecordKey {
        match self {
            Self::Capsule(state) => RecordKey::new(capsule_key(state.identity)),
            Self::Slot(state) => RecordKey::new(slot_key(state.slot)),
        }
    }
}

fn nomos_table() -> TableReference<NomosRecord> {
    TableReference::new(NOMOS_TABLE)
}

fn nomos_descriptor() -> TableDescriptor<NomosRecord> {
    TableDescriptor::new(
        NOMOS_TABLE,
        FamilyName::new(NOMOS_FAMILY),
        SchemaHash::for_label(NOMOS_SCHEMA),
    )
}

struct NomosDirectory;

impl FamilyDirectory for NomosDirectory {
    fn materialize(&self, row: RowMaterializer<'_>) -> sema_engine::Result<()> {
        row.apply(nomos_table())
    }
}

/// Read-only slot state exposed for administration and tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedSlot {
    pub slot: NomosSlotId,
    pub seats: Vec<CapsuleIdentity>,
    pub live: CapsuleIdentity,
    pub generation: SlotGeneration,
    pub selection: GenerationSelection,
    pub binding_committed_at: CommitMarker,
}

impl From<&SlotState> for ObservedSlot {
    fn from(state: &SlotState) -> Self {
        Self {
            slot: state.slot,
            seats: state.seats.clone(),
            live: state.live,
            generation: state.generation,
            selection: state.selection.clone(),
            binding_committed_at: state.binding_committed_at,
        }
    }
}

/// Stateful Nomos operation engine backed by its own embedded, versioned Sema
/// database.
pub struct NomosEngine {
    engine: Engine,
    records: TableReference<NomosRecord>,
    capsules: BTreeMap<CapsuleIdentity, CapsuleState>,
    slots: BTreeMap<NomosSlotId, SlotState>,
    admin_uid: u32,
    poisoned: bool,
}

impl NomosEngine {
    /// Open or create the component-owned database.
    ///
    /// Schema version and the singleton configured admin UID are
    /// `[to-be-reviewed-by-psyche]`. The database starts with no domain rows;
    /// there is no bootstrap Capsule, slot, or fixture.
    pub fn open(path: &Path, admin_uid: u32) -> Result<Self> {
        let request = EngineOpen::new(path, SchemaVersion::new(1))
            .with_versioning(VersioningPolicy::new(VersionedStoreName::new("nomos")));
        let mut engine = Engine::open_recovering(request, &NomosDirectory)
            .map_err(|error| Error::Engine(error.to_string()))?;
        let records = engine
            .register_table(nomos_descriptor())
            .map_err(|error| Error::Engine(error.to_string()))?;
        if engine
            .current_database_marker()
            .map_err(|error| Error::Engine(error.to_string()))?
            .commit_sequence()
            .value()
            > 0
        {
            engine
                .rebuild_from_log(&NomosDirectory)
                .map_err(|error| Error::Engine(error.to_string()))?;
        }
        let mut store = Self {
            engine,
            records,
            capsules: BTreeMap::new(),
            slots: BTreeMap::new(),
            admin_uid,
            poisoned: false,
        };
        store.reload_and_validate()?;
        Ok(store)
    }

    /// Execute one already-framed request as an authenticated Unix peer.
    ///
    /// Callers serialize access to this mutable engine. The daemon runtime does
    /// so with one mutex, which is also the singleton-writer premise behind
    /// predicting the next binding marker inside the same atomic commit.
    pub fn dispatch(&mut self, peer_uid: u32, request: Request) -> Result<Reply> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        if let Err(error) = request.validate() {
            return Ok(Reply::Rejected(rejection_for_invalid_request(
                &request, &error,
            )));
        }
        let result = match request {
            Request::Deploy {
                slot,
                expected,
                artifacts,
                selection,
            } => self.deploy(slot, expected, artifacts, selection),
            Request::Repoint {
                slot,
                expected_generation,
                capsule,
                selection,
            } => self.repoint(slot, expected_generation, capsule, selection),
            Request::AdvanceProjection {
                capsule,
                expected_previous_version,
                projection,
                translator_receipt,
            } => self.advance_projection(
                peer_uid,
                capsule,
                expected_previous_version,
                projection,
                translator_receipt.is_some(),
            ),
            Request::Transform { selector, ethos } => self.transform(selector, ethos),
        };
        match result {
            Err(error) if error.can_reply_storage_failed() && !self.poisoned => {
                Ok(Reply::Rejected(Rejection::StorageFailed))
            }
            other => other,
        }
    }

    pub fn current_marker(&self) -> Result<CommitMarker> {
        self.engine
            .current_database_marker()
            .map(marker_from_engine)
            .map_err(|error| Error::Engine(error.to_string()))
    }

    pub fn commit_count(&self) -> Result<usize> {
        self.engine
            .versioned_commit_log()
            .map(|entries| entries.len())
            .map_err(|error| Error::Engine(error.to_string()))
    }

    pub fn observe_slot(&self, slot: NomosSlotId) -> Option<ObservedSlot> {
        self.slots.get(&slot).map(ObservedSlot::from)
    }

    pub fn projection_versions(
        &self,
        identity: CapsuleIdentity,
    ) -> Result<Vec<NameTreeProjectionVersion>> {
        let Some(state) = self.capsules.get(&identity) else {
            return Ok(Vec::new());
        };
        state
            .projections
            .iter()
            .map(|projection| {
                projection
                    .restore()
                    .map(|value| value.version())
                    .map_err(|error| Error::State(error.to_string()))
            })
            .collect()
    }

    /// Recheck that response bytes are a canonical, semantically valid native
    /// Logos population for the seated Capsule's authored evaluator.
    pub fn validate_logos_population(
        &self,
        identity: CapsuleIdentity,
        expected: &EngineEthosNameTree,
        bytes: &[u8],
    ) -> Result<()> {
        let capsule = self
            .capsules
            .get(&identity)
            .ok_or_else(|| Error::State("unknown Nomos Capsule".into()))?
            .sealed_capsule()?;
        restore_checked_logos(&capsule, expected, bytes).map(|_| ())
    }

    fn reload_and_validate(&mut self) -> Result<()> {
        let marker = self.current_marker()?;
        let records = self
            .engine
            .match_records(QueryPlan::all(self.records))
            .map_err(|error| Error::Engine(error.to_string()))?
            .records()
            .to_vec();
        let mut capsules = BTreeMap::new();
        let mut slots = BTreeMap::new();
        for record in records {
            match record {
                NomosRecord::Capsule(state) => {
                    state.validate()?;
                    if capsules.insert(state.identity, state).is_some() {
                        return Err(Error::State("duplicate Capsule row".into()));
                    }
                }
                NomosRecord::Slot(state) => {
                    state.validate(marker)?;
                    if slots.insert(state.slot, state).is_some() {
                        return Err(Error::State("duplicate slot row".into()));
                    }
                }
            }
        }
        for state in slots.values() {
            for identity in &state.seats {
                if !capsules.contains_key(identity) {
                    return Err(Error::State(
                        "slot seat references an absent Capsule row".into(),
                    ));
                }
            }
        }
        self.capsules = capsules;
        self.slots = slots;
        Ok(())
    }

    fn deploy(
        &mut self,
        slot: NomosSlotId,
        expected: SlotExpectation,
        artifacts: NomosDeploymentArtifacts,
        selection: GenerationSelection,
    ) -> Result<Reply> {
        let population = match artifacts.validate() {
            Ok(population) => population,
            Err(_) => return Ok(Reply::Rejected(Rejection::CapsuleArchiveInvalid)),
        };
        let identity = population.capsule().content_identity();
        let existing_slot = self.slots.get(&slot).cloned();

        let new_capsule = match self.capsules.get(&identity) {
            Some(current)
                if current.capsule.bytes() == artifacts.capsule().bytes()
                    && current.current_projection().bytes() == artifacts.projection().bytes() =>
            {
                None
            }
            Some(_) => return Ok(Reply::Rejected(Rejection::ProjectionStale)),
            None => {
                if population.projection().version() != NameTreeProjectionVersion::initial() {
                    return Ok(Reply::Rejected(Rejection::ProjectionInvalid));
                }
                Some(CapsuleState::from_artifacts(identity, &artifacts))
            }
        };

        if let Some(current) = &existing_slot
            && current.live == identity
            && current.selection == selection
        {
            let observed_at = self.current_marker()?;
            return Ok(Reply::Deployed(DeployOutcome::AlreadyCurrent {
                slot,
                identity,
                generation: current.generation,
                binding_committed_at: current.binding_committed_at,
                observed_at,
            }));
        }
        if !expectation_matches(expected, existing_slot.as_ref()) {
            return Ok(Reply::Rejected(Rejection::SlotGenerationMismatch));
        }

        let predicted = self.predict_next_marker()?;
        let next_slot = match &existing_slot {
            None => SlotState {
                slot,
                seats: vec![identity],
                live: identity,
                generation: SlotGeneration::initial(),
                selection,
                binding_committed_at: predicted,
            },
            Some(current) => {
                let Some(generation) = current.generation.checked_successor() else {
                    return Ok(Reply::Rejected(Rejection::SlotGenerationMismatch));
                };
                let mut seats = current.seats.clone();
                if seats.binary_search(&identity).is_err() {
                    seats.push(identity);
                    seats.sort();
                }
                SlotState {
                    slot,
                    seats,
                    live: identity,
                    generation,
                    selection,
                    binding_committed_at: predicted,
                }
            }
        };

        let mut commit = self.engine.begin_atomic_commit();
        if let Some(state) = &new_capsule {
            commit = commit.assert(self.records, NomosRecord::Capsule(state.clone()));
        }
        commit = if existing_slot.is_some() {
            commit.mutate(self.records, NomosRecord::Slot(next_slot.clone()))
        } else {
            commit.assert(self.records, NomosRecord::Slot(next_slot.clone()))
        };
        self.commit_predicted(commit, predicted, usize::from(new_capsule.is_some()) + 1)?;

        if let Some(state) = new_capsule {
            self.capsules.insert(identity, state);
        }
        self.slots.insert(slot, next_slot.clone());
        Ok(Reply::Deployed(match existing_slot {
            None => DeployOutcome::FreshDeployed {
                slot,
                identity,
                generation: next_slot.generation,
                committed_at: predicted,
            },
            Some(previous) => DeployOutcome::Repointed {
                slot,
                previous_identity: previous.live,
                identity,
                generation: next_slot.generation,
                committed_at: predicted,
            },
        }))
    }

    fn repoint(
        &mut self,
        slot: NomosSlotId,
        expected_generation: SlotGeneration,
        selector: CapsuleSelector,
        selection: Option<GenerationSelection>,
    ) -> Result<Reply> {
        let Some(current) = self.slots.get(&slot).cloned() else {
            return Ok(Reply::Rejected(Rejection::EmptySlot));
        };
        let target = match resolve_seat(&current, selector) {
            Ok(identity) => identity,
            Err(rejection) => return Ok(Reply::Rejected(rejection)),
        };
        let selection = selection.unwrap_or_else(|| current.selection.clone());
        if target == current.live && selection == current.selection {
            let observed_at = self.current_marker()?;
            return Ok(Reply::Deployed(DeployOutcome::AlreadyCurrent {
                slot,
                identity: target,
                generation: current.generation,
                binding_committed_at: current.binding_committed_at,
                observed_at,
            }));
        }
        if current.generation != expected_generation {
            return Ok(Reply::Rejected(Rejection::SlotGenerationMismatch));
        }
        let Some(generation) = current.generation.checked_successor() else {
            return Ok(Reply::Rejected(Rejection::SlotGenerationMismatch));
        };
        let predicted = self.predict_next_marker()?;
        let next = SlotState {
            slot,
            seats: current.seats.clone(),
            live: target,
            generation,
            selection,
            binding_committed_at: predicted,
        };
        let commit = self
            .engine
            .begin_atomic_commit()
            .mutate(self.records, NomosRecord::Slot(next.clone()));
        self.commit_predicted(commit, predicted, 1)?;
        self.slots.insert(slot, next);
        Ok(Reply::Deployed(DeployOutcome::Repointed {
            slot,
            previous_identity: current.live,
            identity: target,
            generation,
            committed_at: predicted,
        }))
    }

    fn advance_projection(
        &mut self,
        peer_uid: u32,
        identity: CapsuleIdentity,
        expected_previous_version: NameTreeProjectionVersion,
        projection: NomosProjectionArchive,
        has_translator_receipt: bool,
    ) -> Result<Reply> {
        if has_translator_receipt {
            return Ok(Reply::Rejected(Rejection::TranslatorReceiptUnsupported));
        }
        if peer_uid != self.admin_uid {
            return Ok(Reply::Rejected(Rejection::Unauthorized));
        }
        let Some(current) = self.capsules.get(&identity).cloned() else {
            return Ok(Reply::Rejected(Rejection::CapsuleArchiveInvalid));
        };
        let current_projection = current
            .current_projection()
            .restore()
            .map_err(|error| Error::State(error.to_string()))?;
        if current_projection.version() != expected_previous_version {
            return Ok(Reply::Rejected(Rejection::ProjectionStale));
        }
        let next_population = match SealedNomosPopulation::from_archive_parts(
            current.capsule.bytes(),
            projection.bytes(),
        ) {
            Ok(population) => population,
            Err(_) => return Ok(Reply::Rejected(Rejection::ProjectionInvalid)),
        };
        let mut next = current.clone();
        next.projections.push(projection);
        let predicted = self.predict_next_marker()?;
        let commit = self
            .engine
            .begin_atomic_commit()
            .mutate(self.records, NomosRecord::Capsule(next.clone()));
        self.commit_predicted(commit, predicted, 1)?;
        self.capsules.insert(identity, next);
        Ok(Reply::ProjectionAdvanced(ProjectionOutcome::Advanced {
            identity,
            previous_version: expected_previous_version,
            version: next_population.projection().version(),
            committed_at: predicted,
        }))
    }

    fn transform(
        &self,
        selector: TransformSelector,
        ethos: signal_nomos::EthosPopulationArchive,
    ) -> Result<Reply> {
        let (slot_state, capsule_state) = match self.resolve_transform(selector) {
            Ok(value) => value,
            Err(rejection) => return Ok(Reply::Rejected(rejection)),
        };
        let population = match decode_ethos_population(&ethos) {
            Ok(population) => population,
            Err(_) => return Ok(Reply::Rejected(Rejection::EthosPopulationInvalid)),
        };
        let references = match population.name_tree().reference_universe() {
            Ok(references) => references,
            Err(_) => return Ok(Reply::Rejected(Rejection::ReferenceUniverseInvalid)),
        };
        let capsule = capsule_state.sealed_capsule()?;
        let evaluator = match NativeAuthoredEvaluator::try_new(capsule.transformers(), references) {
            Ok(evaluator) => evaluator,
            Err(_) => return Ok(Reply::Rejected(Rejection::LoweringFailed)),
        };
        let transformation = match evaluator.transform(&population) {
            Ok(transformation) => transformation,
            Err(_) => return Ok(Reply::Rejected(Rejection::LoweringFailed)),
        };
        let bytes = match transformation.population().to_archive_bytes() {
            Ok(bytes) => bytes,
            Err(_) => return Ok(Reply::Rejected(Rejection::LoweringFailed)),
        };
        if restore_checked_logos(&capsule, population.name_tree(), &bytes).is_err() {
            return Ok(Reply::Rejected(Rejection::LoweringFailed));
        }
        let projection_version = capsule_state.sealed_population()?.projection().version();
        let snapshot = AdmissionSnapshot::new(
            slot_state.slot,
            capsule_state.identity,
            slot_state.generation,
            projection_version,
        );
        let outcome = TransformOutcome::new(snapshot, bytes)
            .map_err(|error| Error::Archive(error.to_string()))?;
        Ok(Reply::Transformed(outcome))
    }

    fn resolve_transform(
        &self,
        selector: TransformSelector,
    ) -> std::result::Result<(&SlotState, &CapsuleState), Rejection> {
        match selector {
            TransformSelector::Live(slot) => {
                let slot = self.slots.get(&slot).ok_or(Rejection::EmptySlot)?;
                let capsule = self
                    .capsules
                    .get(&slot.live)
                    .ok_or(Rejection::CapsuleArchiveInvalid)?;
                Ok((slot, capsule))
            }
            TransformSelector::Seated { slot, capsule } => {
                let slot = self.slots.get(&slot).ok_or(Rejection::EmptySlot)?;
                let identity = resolve_seat(slot, capsule)?;
                let capsule = self
                    .capsules
                    .get(&identity)
                    .ok_or(Rejection::CapsuleArchiveInvalid)?;
                Ok((slot, capsule))
            }
        }
    }

    fn predict_next_marker(&self) -> Result<CommitMarker> {
        let current = self.current_marker()?;
        let commit_sequence = current
            .commit_sequence()
            .checked_add(1)
            .ok_or(Error::MarkerExhausted)?;
        let snapshot = current
            .snapshot()
            .checked_add(1)
            .ok_or(Error::MarkerExhausted)?;
        Ok(CommitMarker::new(commit_sequence, snapshot))
    }

    fn commit_predicted(
        &mut self,
        commit: AtomicCommit,
        predicted: CommitMarker,
        expected_operations: usize,
    ) -> Result<()> {
        let receipt = match self.engine.commit_atomic(commit) {
            Ok(receipt) => receipt,
            Err(error) => match self.current_marker() {
                Ok(durable) if durable < predicted => return Err(Error::Engine(error.to_string())),
                Ok(_) | Err(_) => {
                    self.poisoned = true;
                    return Err(Error::PostCommitFailure(error.to_string()));
                }
            },
        };
        let actual = CommitMarker::new(
            receipt.commit_sequence().value(),
            receipt.snapshot().value(),
        );
        if actual != predicted || receipt.operation_count() != expected_operations {
            self.poisoned = true;
            return Err(Error::CommitReceiptDivergence {
                predicted,
                actual,
                expected_operations,
                actual_operations: receipt.operation_count(),
            });
        }
        Ok(())
    }
}

fn expectation_matches(expected: SlotExpectation, current: Option<&SlotState>) -> bool {
    match (expected, current) {
        (SlotExpectation::Empty, None) => true,
        (SlotExpectation::Generation(expected), Some(current)) => expected == current.generation,
        _ => false,
    }
}

fn resolve_seat(
    slot: &SlotState,
    selector: CapsuleSelector,
) -> std::result::Result<CapsuleIdentity, Rejection> {
    match selector {
        CapsuleSelector::Full(identity) => {
            if slot.seats.binary_search(&identity).is_ok() {
                Ok(identity)
            } else {
                Err(Rejection::CapsuleNotSeated)
            }
        }
        CapsuleSelector::Short(display) => {
            let mut matches = slot
                .seats
                .iter()
                .copied()
                .filter(|identity| capsule_hex(*identity).starts_with(display.as_str()));
            let Some(found) = matches.next() else {
                return Err(Rejection::CapsuleNotSeated);
            };
            if matches.next().is_some() {
                return Err(Rejection::AmbiguousShortCapsuleDisplay);
            }
            Ok(found)
        }
    }
}

fn restore_checked_logos(
    capsule: &SealedNomosCapsule,
    expected: &EngineEthosNameTree,
    bytes: &[u8],
) -> Result<NativeLogosPopulation<EngineLogosNameTree>> {
    type Population = EncodedPopulation<NativeEvaluatedLogos, EngineLogosNameTree>;
    let raw = <Population as PortableArchive>::from_archive_bytes(bytes)
        .map_err(|error| Error::Archive(error.to_string()))?;
    let canonical = <Population as PortableArchive>::to_archive_bytes(&raw)
        .map_err(|error| Error::Archive(error.to_string()))?;
    if canonical.as_ref() != bytes {
        return Err(Error::Archive(
            "native Logos population archive is not canonical".into(),
        ));
    }
    if !raw.name_tree().matches_plan(expected) {
        return Err(Error::Archive(
            "native Logos NameTree is not bound to the authenticated input plan".into(),
        ));
    }
    let references = expected.reference_universe()?;
    let evaluator = NativeAuthoredEvaluator::try_new(capsule.transformers(), references)
        .map_err(|error| Error::Archive(error.to_string()))?;
    let restored =
        NativeLogosPopulation::<EngineLogosNameTree>::from_archive_bytes(bytes, &evaluator)
            .map_err(|error| Error::Archive(error.to_string()))?;
    if !restored.name_tree().matches_plan(expected) {
        return Err(Error::Archive(
            "restored native Logos NameTree is not bound to the authenticated input plan".into(),
        ));
    }
    Ok(restored)
}

fn capsule_key(identity: CapsuleIdentity) -> String {
    format!("capsule:{}", identity.content_addressed_hash())
}

fn slot_key(slot: NomosSlotId) -> String {
    format!("slot:{:020}", slot.value())
}

fn capsule_hex(identity: CapsuleIdentity) -> String {
    identity.content_addressed_hash().to_hexadecimal()
}

fn marker_from_engine(marker: sema_engine::DatabaseMarker) -> CommitMarker {
    CommitMarker::new(marker.commit_sequence().value(), marker.snapshot().value())
}

fn rejection_for_invalid_request(request: &Request, error: &signal_nomos::Error) -> Rejection {
    match request {
        Request::Deploy { artifacts, .. } => {
            if artifacts.capsule().restore().is_err() {
                Rejection::CapsuleArchiveInvalid
            } else {
                Rejection::ProjectionInvalid
            }
        }
        Request::AdvanceProjection { .. } => Rejection::ProjectionInvalid,
        Request::Transform { .. }
            if matches!(error, signal_nomos::Error::EmptyEthosPopulationArchive) =>
        {
            Rejection::EthosPopulationInvalid
        }
        Request::Transform { .. } | Request::Repoint { .. } => Rejection::CapsuleArchiveInvalid,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("embedded Sema engine: {0}")]
    Engine(String),
    #[error("portable archive: {0}")]
    Archive(String),
    #[error("persisted Nomos state: {0}")]
    State(String),
    #[error("database marker is exhausted")]
    MarkerExhausted,
    #[error(
        "predicted marker/operation count diverged: marker {predicted:?}/{actual:?}, operations {expected_operations}/{actual_operations}"
    )]
    CommitReceiptDivergence {
        predicted: CommitMarker,
        actual: CommitMarker,
        expected_operations: usize,
        actual_operations: usize,
    },
    #[error("engine is poisoned after an impossible post-commit receipt divergence")]
    Poisoned,
    #[error("embedded Sema reported failure after the predicted marker became durable: {0}")]
    PostCommitFailure(String),
    #[error(transparent)]
    Population(#[from] PopulationError),
    #[error(transparent)]
    NameTree(#[from] crate::name_tree::NameTreeError),
}

impl Error {
    /// Whether dispatch can truthfully report a typed refusal because no
    /// transaction may have become durable.
    pub const fn can_reply_storage_failed(&self) -> bool {
        matches!(self, Self::Engine(_) | Self::MarkerExhausted)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
