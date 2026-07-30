use std::collections::BTreeSet;

use capsule_content_identity::PortableArchive;
use core_ethos::{WholeEthos, WholeEthosItem, WholeEthosVariantPayload};
use core_logos::{WholeLogosTypeReference, WholeLogosVariantPayload};
use core_nomos::{
    NameTransform, NativeEvaluatedLogos, NativeEvaluatedTerm, NativeLogosNameTree,
    NativeNameTreeBoundary, NativeNameTreePlan, NativeNameTreeRefusal, NativeReferenceMapping,
    NativeReferenceUniverse,
};
use protos::EncodedPopulation;
use signal_nomos::EthosPopulationArchive;
use signal_sema_translator::{VocabularyEncodedId, VocabularyRoot};

/// One exact non-identity declaration-name realization supplied by the
/// authenticated input NameTree.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct EngineNameRealization {
    source: VocabularyEncodedId,
    transform: NameTransform,
    target: VocabularyEncodedId,
}

impl EngineNameRealization {
    pub fn try_new(
        source: VocabularyEncodedId,
        transform: NameTransform,
        target: VocabularyEncodedId,
    ) -> Result<Self, NameTreeError> {
        require_nonempty_universal(&source)?;
        require_nonempty_universal(&target)?;
        if transform == NameTransform::Identity {
            return Err(NameTreeError::IdentityRealization);
        }
        Ok(Self {
            source,
            transform,
            target,
        })
    }

    pub const fn source(&self) -> &VocabularyEncodedId {
        &self.source
    }

    pub const fn transform(&self) -> NameTransform {
        self.transform
    }

    pub const fn target(&self) -> &VocabularyEncodedId {
        &self.target
    }
}

/// One exact Universal-to-Rust reference mapping.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct EngineReferenceMapping {
    source: VocabularyEncodedId,
    target: VocabularyEncodedId,
}

impl EngineReferenceMapping {
    pub fn try_new(
        source: VocabularyEncodedId,
        target: VocabularyEncodedId,
    ) -> Result<Self, NameTreeError> {
        require_nonempty(&source)?;
        require_nonempty(&target)?;
        if source.root_variant() != &VocabularyRoot::Universal {
            return Err(NameTreeError::ReferenceSourceRoot);
        }
        if target.root_variant() != &VocabularyRoot::Rust {
            return Err(NameTreeError::ReferenceTargetRoot);
        }
        Ok(Self { source, target })
    }

    pub const fn source(&self) -> &VocabularyEncodedId {
        &self.source
    }

    pub const fn target(&self) -> &VocabularyEncodedId {
        &self.target
    }
}

/// Complete declaration and exact transformation data paired with one Ethos
/// encoded form.
///
/// The exact ancestor/spelling reachability closure remains po2.4 work and is
/// deliberately not claimed here. This tree authenticates complete identities,
/// realization results, and reference mappings only.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct EngineEthosNameTree {
    declarations: Vec<VocabularyEncodedId>,
    realizations: Vec<EngineNameRealization>,
    references: Vec<EngineReferenceMapping>,
    expected_logos_declarations: Vec<VocabularyEncodedId>,
    expected_logos_references: Vec<VocabularyEncodedId>,
}

impl EngineEthosNameTree {
    pub fn try_new(
        mut declarations: Vec<VocabularyEncodedId>,
        mut realizations: Vec<EngineNameRealization>,
        mut references: Vec<EngineReferenceMapping>,
        mut expected_logos_declarations: Vec<VocabularyEncodedId>,
        mut expected_logos_references: Vec<VocabularyEncodedId>,
    ) -> Result<Self, NameTreeError> {
        declarations.sort();
        realizations.sort_by(realization_order);
        references.sort_by(|left, right| left.source.cmp(&right.source));
        expected_logos_declarations.sort();
        expected_logos_references.sort();
        let tree = Self {
            declarations,
            realizations,
            references,
            expected_logos_declarations,
            expected_logos_references,
        };
        tree.validate_intrinsic()?;
        Ok(tree)
    }

    pub fn declarations(&self) -> &[VocabularyEncodedId] {
        &self.declarations
    }

    pub fn realizations(&self) -> &[EngineNameRealization] {
        &self.realizations
    }

    pub fn references(&self) -> &[EngineReferenceMapping] {
        &self.references
    }

    pub fn expected_logos_declarations(&self) -> &[VocabularyEncodedId] {
        &self.expected_logos_declarations
    }

    pub fn expected_logos_references(&self) -> &[VocabularyEncodedId] {
        &self.expected_logos_references
    }

    /// Resolve one reference exactly. An unlisted full identity is preserved.
    pub fn mapped_reference(&self, source: &VocabularyEncodedId) -> VocabularyEncodedId {
        self.references
            .binary_search_by(|mapping| mapping.source.cmp(source))
            .map(|index| self.references[index].target.clone())
            .unwrap_or_else(|_| source.clone())
    }

    pub(crate) fn validate_for_ethos(&self, ethos: &WholeEthos) -> Result<(), NameTreeError> {
        self.validate_intrinsic()?;
        let expected = ethos_declaration_closure(ethos)?;
        if self.declarations != expected {
            return Err(NameTreeError::EthosDeclarationClosure);
        }
        Ok(())
    }

    pub(crate) fn reference_universe(&self) -> Result<NativeReferenceUniverse, NameTreeError> {
        reference_universe(&self.references)
    }

    fn validate_intrinsic(&self) -> Result<(), NameTreeError> {
        validate_declarations(&self.declarations)?;
        validate_declarations(&self.expected_logos_declarations)?;
        validate_output_references(&self.expected_logos_references)?;
        if self
            .realizations
            .windows(2)
            .any(|pair| realization_order(&pair[0], &pair[1]).is_ge())
        {
            return Err(NameTreeError::NonCanonicalRealizations);
        }
        let mut realization_targets = BTreeSet::new();
        for realization in &self.realizations {
            require_nonempty_universal(realization.source())?;
            require_nonempty_universal(realization.target())?;
            if realization.transform() == NameTransform::Identity {
                return Err(NameTreeError::IdentityRealization);
            }
            if self
                .declarations
                .binary_search(realization.source())
                .is_err()
            {
                return Err(NameTreeError::UnknownRealizationSource);
            }
            if self
                .expected_logos_declarations
                .binary_search(realization.target())
                .is_err()
            {
                return Err(NameTreeError::UnplannedRealizationTarget);
            }
            if !realization_targets.insert(realization.target()) {
                return Err(NameTreeError::DuplicateRealizationTarget);
            }
        }
        if self
            .references
            .windows(2)
            .any(|pair| pair[0].source >= pair[1].source)
        {
            return Err(NameTreeError::NonCanonicalReferences);
        }
        for mapping in &self.references {
            EngineReferenceMapping::try_new(mapping.source.clone(), mapping.target.clone())?;
        }
        Ok(())
    }
}

fn reference_universe(
    references: &[EngineReferenceMapping],
) -> Result<NativeReferenceUniverse, NameTreeError> {
    let mappings = references
        .iter()
        .map(|mapping| {
            NativeReferenceMapping::try_new(mapping.source.clone(), mapping.target.clone())
                .map_err(|error| NameTreeError::ReferenceUniverse(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    NativeReferenceUniverse::try_new(mappings)
        .map_err(|error| NameTreeError::ReferenceUniverse(error.to_string()))
}

/// The exact declaration closure and reference universe returned with a native
/// Logos population.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct EngineLogosNameTree {
    declarations: Vec<VocabularyEncodedId>,
    references_used: Vec<VocabularyEncodedId>,
    references: Vec<EngineReferenceMapping>,
}

impl EngineLogosNameTree {
    pub fn declarations(&self) -> &[VocabularyEncodedId] {
        &self.declarations
    }

    pub fn references_used(&self) -> &[VocabularyEncodedId] {
        &self.references_used
    }

    pub fn references(&self) -> &[EngineReferenceMapping] {
        &self.references
    }

    pub(crate) fn matches_plan(&self, plan: &EngineEthosNameTree) -> bool {
        self.declarations == plan.expected_logos_declarations
            && self.references_used == plan.expected_logos_references
            && self.references == plan.references
    }

    fn from_evaluated(
        evaluated: &NativeEvaluatedLogos,
        expected_declarations: Vec<VocabularyEncodedId>,
        expected_references: Vec<VocabularyEncodedId>,
        references: Vec<EngineReferenceMapping>,
    ) -> Result<Self, NativeNameTreeRefusal> {
        if logos_declaration_closure(evaluated).as_ref() != Some(&expected_declarations)
            || logos_reference_closure(evaluated) != expected_references
        {
            return Err(NativeNameTreeRefusal::CannotConstructLogosTree);
        }
        let tree = Self {
            declarations: expected_declarations,
            references_used: expected_references,
            references,
        };
        tree.validate_for(evaluated)?;
        Ok(tree)
    }
}

impl NativeLogosNameTree for EngineLogosNameTree {
    fn validate_for(&self, evaluated: &NativeEvaluatedLogos) -> Result<(), NativeNameTreeRefusal> {
        let Some(expected) = logos_declaration_closure(evaluated) else {
            return Err(NativeNameTreeRefusal::CannotConstructLogosTree);
        };
        let expected_references = logos_reference_closure(evaluated);
        if self.declarations != expected
            || self.references_used != expected_references
            || self.declarations.windows(2).any(|pair| pair[0] >= pair[1])
            || self
                .references_used
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self
                .references
                .windows(2)
                .any(|pair| pair[0].source >= pair[1].source)
            || self.references.iter().any(|mapping| {
                EngineReferenceMapping::try_new(mapping.source.clone(), mapping.target.clone())
                    .is_err()
            })
        {
            return Err(NativeNameTreeRefusal::CannotConstructLogosTree);
        }
        Ok(())
    }
}

/// Authenticated, mutation-free plan handed to the native evaluator.
#[doc(hidden)]
pub struct EngineNameTreePlan {
    realizations: Vec<EngineNameRealization>,
    references: Vec<EngineReferenceMapping>,
    expected_declarations: Vec<VocabularyEncodedId>,
    expected_references: Vec<VocabularyEncodedId>,
}

impl NativeNameTreeBoundary for EngineEthosNameTree {
    type LogosNameTree = EngineLogosNameTree;
    type Plan = EngineNameTreePlan;

    fn authenticated_plan(&self) -> Result<Self::Plan, NativeNameTreeRefusal> {
        self.validate_intrinsic()
            .map_err(|_| NativeNameTreeRefusal::CannotConstructLogosTree)?;
        Ok(EngineNameTreePlan {
            realizations: self.realizations.clone(),
            references: self.references.clone(),
            expected_declarations: self.expected_logos_declarations.clone(),
            expected_references: self.expected_logos_references.clone(),
        })
    }
}

impl NativeNameTreePlan for EngineNameTreePlan {
    type LogosNameTree = EngineLogosNameTree;

    fn realize_declaration(
        &self,
        source: &VocabularyEncodedId,
        transform: NameTransform,
    ) -> Result<VocabularyEncodedId, NativeNameTreeRefusal> {
        self.realizations
            .binary_search_by(|candidate| {
                candidate
                    .source
                    .cmp(source)
                    .then_with(|| transform_tag(candidate.transform).cmp(&transform_tag(transform)))
            })
            .map(|index| self.realizations[index].target.clone())
            .map_err(|_| NativeNameTreeRefusal::CannotRealize {
                identity: source.clone(),
                transform,
            })
    }

    fn logos_name_tree(
        &self,
        evaluated: &NativeEvaluatedLogos,
    ) -> Result<Self::LogosNameTree, NativeNameTreeRefusal> {
        EngineLogosNameTree::from_evaluated(
            evaluated,
            self.expected_declarations.clone(),
            self.expected_references.clone(),
            self.references.clone(),
        )
    }
}

/// Construct the canonical direct Ethos population archive accepted by this
/// engine.
pub fn encode_ethos_population(
    ethos: WholeEthos,
    names: EngineEthosNameTree,
) -> Result<EthosPopulationArchive, PopulationError> {
    validate_whole_ethos(&ethos)?;
    names.validate_for_ethos(&ethos)?;
    let population = EncodedPopulation::new(ethos, names);
    let bytes =
        <EncodedPopulation<WholeEthos, EngineEthosNameTree> as PortableArchive>::to_archive_bytes(
            &population,
        )
        .map_err(|error| PopulationError::Archive(error.to_string()))?;
    EthosPopulationArchive::try_new(bytes.as_ref().to_vec())
        .map_err(|error| PopulationError::Archive(error.to_string()))
}

pub(crate) fn decode_ethos_population(
    archive: &EthosPopulationArchive,
) -> Result<EncodedPopulation<WholeEthos, EngineEthosNameTree>, PopulationError> {
    type Population = EncodedPopulation<WholeEthos, EngineEthosNameTree>;
    let population = <Population as PortableArchive>::from_archive_bytes(archive.bytes())
        .map_err(|error| PopulationError::Archive(error.to_string()))?;
    let canonical = <Population as PortableArchive>::to_archive_bytes(&population)
        .map_err(|error| PopulationError::Archive(error.to_string()))?;
    if canonical.as_ref() != archive.bytes() {
        return Err(PopulationError::NonCanonical);
    }
    validate_whole_ethos(population.encoded_form())?;
    population
        .name_tree()
        .validate_for_ethos(population.encoded_form())?;
    Ok(population)
}

fn validate_whole_ethos(ethos: &WholeEthos) -> Result<(), PopulationError> {
    let bytes = ethos
        .to_archive_bytes()
        .map_err(|error| PopulationError::Ethos(error.to_string()))?;
    let restored = WholeEthos::from_archive_bytes(&bytes)
        .map_err(|error| PopulationError::Ethos(error.to_string()))?;
    if restored != *ethos {
        return Err(PopulationError::EthosRoundTrip);
    }
    for item in ethos.items() {
        if let WholeEthosItem::Enumeration(enumeration) = item {
            for variant in enumeration.variants() {
                if matches!(
                    variant.payload(),
                    WholeEthosVariantPayload::Tuple(fields) if fields.fields().is_empty()
                ) {
                    return Err(PopulationError::EmptyTuple);
                }
            }
        }
    }
    Ok(())
}

fn ethos_declaration_closure(
    ethos: &WholeEthos,
) -> Result<Vec<VocabularyEncodedId>, NameTreeError> {
    let mut declarations = Vec::new();
    for item in ethos.items() {
        match item {
            WholeEthosItem::Newtype(newtype) => declarations.push(newtype.name().clone()),
            WholeEthosItem::Enumeration(enumeration) => {
                declarations.push(enumeration.name().clone());
                declarations.extend(
                    enumeration
                        .variants()
                        .iter()
                        .map(|variant| variant.name().clone()),
                );
            }
        }
    }
    declarations.sort();
    validate_declarations(&declarations)?;
    Ok(declarations)
}

fn validate_declarations(declarations: &[VocabularyEncodedId]) -> Result<(), NameTreeError> {
    for identity in declarations {
        require_nonempty_universal(identity)?;
    }
    if declarations.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(NameTreeError::NonCanonicalDeclarations);
    }
    Ok(())
}

fn validate_output_references(references: &[VocabularyEncodedId]) -> Result<(), NameTreeError> {
    for identity in references {
        require_nonempty(identity)?;
        if !matches!(
            identity.root_variant(),
            VocabularyRoot::Universal | VocabularyRoot::Rust
        ) {
            return Err(NameTreeError::OutputReferenceRoot);
        }
    }
    if references.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(NameTreeError::NonCanonicalOutputReferences);
    }
    Ok(())
}

fn logos_declaration_closure(evaluated: &NativeEvaluatedLogos) -> Option<Vec<VocabularyEncodedId>> {
    let mut declarations = Vec::new();
    for value in evaluated.values() {
        for field in value.fields() {
            collect_logos_declarations(field.term(), &mut declarations);
        }
    }
    declarations.sort();
    if declarations.windows(2).any(|pair| pair[0] == pair[1]) {
        None
    } else {
        Some(declarations)
    }
}

fn collect_logos_declarations(
    term: &NativeEvaluatedTerm,
    declarations: &mut Vec<VocabularyEncodedId>,
) {
    match term {
        NativeEvaluatedTerm::Declaration(identity) => {
            declarations.push(identity.clone());
        }
        NativeEvaluatedTerm::Nested(value) => {
            for field in value.fields() {
                collect_logos_declarations(field.term(), declarations);
            }
        }
        NativeEvaluatedTerm::Sequence(terms) => {
            for term in terms {
                collect_logos_declarations(term, declarations);
            }
        }
        NativeEvaluatedTerm::Variant(variant) => {
            declarations.push(variant.name().clone());
        }
        NativeEvaluatedTerm::Reference(_)
        | NativeEvaluatedTerm::Literal(_)
        | NativeEvaluatedTerm::Scalar(_)
        | NativeEvaluatedTerm::TypeReference(_) => {}
    }
}

fn logos_reference_closure(evaluated: &NativeEvaluatedLogos) -> Vec<VocabularyEncodedId> {
    let mut references = BTreeSet::new();
    for value in evaluated.values() {
        for field in value.fields() {
            collect_logos_references(field.term(), &mut references);
        }
    }
    references.into_iter().collect()
}

fn collect_logos_references(
    term: &NativeEvaluatedTerm,
    references: &mut BTreeSet<VocabularyEncodedId>,
) {
    match term {
        NativeEvaluatedTerm::Reference(identity) => {
            references.insert(identity.clone());
        }
        NativeEvaluatedTerm::Nested(value) => {
            for field in value.fields() {
                collect_logos_references(field.term(), references);
            }
        }
        NativeEvaluatedTerm::Sequence(terms) => {
            for term in terms {
                collect_logos_references(term, references);
            }
        }
        NativeEvaluatedTerm::TypeReference(reference) => {
            collect_type_reference(reference, references);
        }
        NativeEvaluatedTerm::Variant(variant) => {
            if let WholeLogosVariantPayload::Tuple(fields) = variant.payload() {
                for reference in fields.fields() {
                    collect_type_reference(reference, references);
                }
            }
        }
        NativeEvaluatedTerm::Declaration(_)
        | NativeEvaluatedTerm::Literal(_)
        | NativeEvaluatedTerm::Scalar(_) => {}
    }
}

fn collect_type_reference(
    reference: &WholeLogosTypeReference,
    references: &mut BTreeSet<VocabularyEncodedId>,
) {
    match reference {
        WholeLogosTypeReference::Identity(identity) => {
            references.insert(identity.clone());
        }
        WholeLogosTypeReference::Application(application) => {
            references.insert(application.head().clone());
            collect_type_reference(application.payload(), references);
        }
    }
}

fn realization_order(
    left: &EngineNameRealization,
    right: &EngineNameRealization,
) -> std::cmp::Ordering {
    left.source
        .cmp(&right.source)
        .then_with(|| transform_tag(left.transform).cmp(&transform_tag(right.transform)))
}

const fn transform_tag(transform: NameTransform) -> u8 {
    match transform {
        NameTransform::Identity => 0,
        NameTransform::FieldName => 1,
        NameTransform::Screaming => 2,
        NameTransform::PascalCase => 3,
    }
}

fn require_nonempty(identity: &VocabularyEncodedId) -> Result<(), NameTreeError> {
    if identity.chain().is_empty() {
        Err(NameTreeError::EmptyIdentity)
    } else {
        Ok(())
    }
}

fn require_nonempty_universal(identity: &VocabularyEncodedId) -> Result<(), NameTreeError> {
    require_nonempty(identity)?;
    if identity.root_variant() != &VocabularyRoot::Universal {
        return Err(NameTreeError::DeclarationRoot);
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum NameTreeError {
    #[error("a complete encoded identity has an empty chain")]
    EmptyIdentity,
    #[error("declaration identities must remain Universal")]
    DeclarationRoot,
    #[error("reference mapping source must be Universal")]
    ReferenceSourceRoot,
    #[error("reference mapping target must be Rust")]
    ReferenceTargetRoot,
    #[error("Identity is implicit and must not be stored as a realization")]
    IdentityRealization,
    #[error("declaration identities are not sorted and unique")]
    NonCanonicalDeclarations,
    #[error("realizations are not sorted and unique")]
    NonCanonicalRealizations,
    #[error("reference mappings are not sorted and unique")]
    NonCanonicalReferences,
    #[error("a realization source is absent from the complete Ethos declarations")]
    UnknownRealizationSource,
    #[error("a realized declaration target is absent from the authenticated Logos plan")]
    UnplannedRealizationTarget,
    #[error("two declaration realizations collide on one output identity")]
    DuplicateRealizationTarget,
    #[error("the NameTree declaration closure differs from the Ethos population")]
    EthosDeclarationClosure,
    #[error("native reference universe refused the mapping: {0}")]
    ReferenceUniverse(String),
    #[error("an expected Logos reference has an unsupported vocabulary root")]
    OutputReferenceRoot,
    #[error("expected Logos references are not sorted and unique")]
    NonCanonicalOutputReferences,
}

#[derive(Debug, thiserror::Error)]
pub enum PopulationError {
    #[error("direct population archive: {0}")]
    Archive(String),
    #[error("WholeEthos archive: {0}")]
    Ethos(String),
    #[error("WholeEthos checked round trip changed the value")]
    EthosRoundTrip,
    #[error("WholeEthos tuple variant payload is empty")]
    EmptyTuple,
    #[error("direct population archive is not canonical")]
    NonCanonical,
    #[error(transparent)]
    NameTree(#[from] NameTreeError),
}
