//! Offline, socket-free Ethos-to-Rust batch generation.
//!
//! Callers supply every translator-issued identity and current spelling. The
//! batch path never allocates an identity and never enters the stateful daemon.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use batch_core_ethos::{
    DecodedEthos, EthosCodec, EthosCodecBuildError, EthosDecodeError, EthosGrammarError,
    EthosGrammarIdentities, EthosGrammarIds, WholeEthos, WholeEthosBody,
    WholeEthosBuiltinPriorError, WholeEthosBuiltinPriors, WholeEthosFileKind, WholeEthosItem,
};
use batch_core_logos::WholeLogos;
use batch_core_nomos::{
    BundleStorageProvenance, ExternalStorageProvenance, InterfaceRoleIdentities,
    InterfaceStructuralTransformation, NexusStructuralTransformation, NexusTransformation,
    NexusTransformationError, SemaStructuralTransformation, StorageProvenanceOwner,
    StreamLifecycleIdentities,
};
use batch_structural_codec::{
    DeclarationAssignment, DecodeNameBindings, EncodedNameResolver, NameOccurrence,
    ResolvedReference,
};
use name_table::{LocalEncodedId, Name};
use rust_logos::{
    FixtureRustVocabulary, FixtureRustVocabularyIds, InterfaceRustRoleIds, RustLogos, RustTypePath,
    RustTypePathResolver,
};
use serde::Deserialize;
use signal_sema_translator::{VocabularyEncodedId, VocabularyRoot};

/// Execute one complete offline batch generation request.
pub trait OfflineBatchGeneration {
    /// Decode every named component in one capsule, pre-register all typed
    /// declarations, then project and emit each complete Rust artifact.
    fn generate_bundle(
        &self,
        components: &[BatchComponent<'_>],
    ) -> Result<Vec<BatchGenerationOutcome>, BatchGenerationError>;
}

/// One source component in a strict offline capsule. A named component may be
/// imported by that exact source spelling from another component. A standalone
/// component deliberately has no importable module identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchComponent<'source> {
    module: Option<&'source str>,
    source: &'source str,
}

impl<'source> BatchComponent<'source> {
    /// Make one named, importable component of a complete capsule.
    pub const fn named(module: &'source str, source: &'source str) -> Self {
        Self {
            module: Some(module),
            source,
        }
    }

    /// Make one self-contained capsule component with no importable module.
    pub const fn standalone(source: &'source str) -> Self {
        Self {
            module: None,
            source,
        }
    }

    /// Optional source spelling through which this component may be imported.
    pub const fn module(&self) -> Option<&'source str> {
        self.module
    }

    /// Authored Ethos source text.
    pub const fn source(&self) -> &'source str {
        self.source
    }
}

/// Stable human-readable projection of a typed batch receipt.
pub trait BatchOutcomeReporting {
    /// Render the source kind, version, and complete artifact breadth.
    fn report(&self) -> String;
}

/// Validate caller-authored identity configuration into an offline generator.
pub trait OfflineBatchConfiguration {
    /// Seat the Ethos and Rust structural vocabularies without allocating names.
    fn prepare(self) -> Result<PreparedBatchGenerator, BatchConfigurationError>;
}

/// A validated socket-free generator and its caller-supplied name view.
pub struct PreparedBatchGenerator {
    ethos: EthosCodec,
    rust: RustLogos,
    names: BatchNameBindings,
    rust_types: BatchRustTypeBindings,
    interface_roles: InterfaceRoleIdentities,
    interface_rust_roles: InterfaceRustRoleIds,
    stream_lifecycles: Vec<StreamLifecycleIdentities>,
}

impl OfflineBatchGeneration for PreparedBatchGenerator {
    fn generate_bundle(
        &self,
        components: &[BatchComponent<'_>],
    ) -> Result<Vec<BatchGenerationOutcome>, BatchGenerationError> {
        if components.is_empty() {
            return Err(BatchGenerationError::EmptyBundle);
        }
        let mut modules = BTreeSet::new();
        let mut decoded = Vec::with_capacity(components.len());
        for component in components {
            if let Some(module) = component.module() {
                if module.is_empty() {
                    return Err(BatchGenerationError::EmptyBundleModule);
                }
                if !modules.insert(module) {
                    return Err(BatchGenerationError::DuplicateBundleModule {
                        module: module.to_owned(),
                    });
                }
            }
            decoded.push(DecodedBatchComponent {
                module: component.module(),
                decoded: self.ethos.decode(component.source(), &self.names)?,
            });
        }
        let rust_types = self.rust_types.activate(&decoded, &self.names)?;
        let provenance = BundleStorageProvenance::from_documents(
            decoded
                .iter()
                .map(|component| component.decoded.ethos().clone()),
            rust_types.external_storage().to_vec(),
        )?;
        let transformation = NexusTransformation::new()
            .with_stream_lifecycle_identities(self.stream_lifecycles.clone())?;
        decoded
            .into_iter()
            .map(|component| {
                let kind = component.decoded.ethos().header().kind();
                let logos = match kind {
                    WholeEthosFileKind::Interface => transformation
                        .lower_interface(component.decoded.ethos(), &self.interface_roles)?
                        .logos()
                        .clone(),
                    WholeEthosFileKind::Nexus => transformation.lower(component.decoded.ethos())?,
                    WholeEthosFileKind::Sema => transformation
                        .lower_sema(component.decoded.ethos(), &provenance)?
                        .logos()
                        .clone(),
                };
                let rust = if kind == WholeEthosFileKind::Interface {
                    let component_rust_types = rust_types.for_document(component.decoded.ethos());
                    self.rust.emit_interface_with_type_paths(
                        &logos,
                        &self.names,
                        &self.interface_rust_roles,
                        &component_rust_types,
                    )?
                } else {
                    let component_rust_types = rust_types.for_document(component.decoded.ethos());
                    self.rust
                        .emit_with_type_paths(&logos, &self.names, &component_rust_types)?
                };
                Ok(BatchGenerationOutcome {
                    kind,
                    version: component.decoded.ethos().header().version(),
                    logos,
                    rust,
                })
            })
            .collect()
    }
}

struct DecodedBatchComponent<'source> {
    module: Option<&'source str>,
    decoded: DecodedEthos,
}

/// Successful partial or complete generation receipt.
pub struct BatchGenerationOutcome {
    kind: WholeEthosFileKind,
    version: u64,
    logos: WholeLogos,
    rust: String,
}

// Trait exception — too trivial: read-only receipt accessors.
impl BatchGenerationOutcome {
    /// Decoded file kind.
    pub const fn kind(&self) -> WholeEthosFileKind {
        self.kind
    }

    /// Decoded header version.
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// Canonical projected Logos content.
    pub const fn logos(&self) -> &WholeLogos {
        &self.logos
    }

    /// Complete emitted Rust artifact.
    pub fn rust(&self) -> &str {
        &self.rust
    }
}

impl BatchOutcomeReporting for BatchGenerationOutcome {
    fn report(&self) -> String {
        let mut report = String::new();
        writeln!(report, "kind {}", self.kind.spelling()).expect("String writes cannot fail");
        writeln!(report, "version {}", self.version).expect("String writes cannot fail");
        writeln!(report, "emitted-items {}", self.logos.items().len())
            .expect("String writes cannot fail");
        report
    }
}

/// Typed failure before any Rust artifact is returned.
#[derive(Debug, thiserror::Error)]
pub enum BatchGenerationError {
    /// A bundle must contain at least one decoded component.
    #[error("batch capsule contains no components")]
    EmptyBundle,
    /// A named component must expose a non-empty module spelling.
    #[error("batch component module spelling must be non-empty")]
    EmptyBundleModule,
    /// More than one component exposed the same importable module spelling.
    #[error("batch capsule repeats component module {module:?}")]
    DuplicateBundleModule { module: String },
    /// Header, body, name, or structural source decoding failed.
    #[error("Ethos batch decode failed: {0}")]
    Decode(#[from] EthosDecodeError),
    /// Current typed Nomos projection refused the decoded document.
    #[error("Nomos batch projection failed: {0}")]
    Projection(#[from] NexusTransformationError),
    /// Rust projection failed without returning partial source.
    #[error("Rust batch emission failed: {0}")]
    Rust(#[from] rust_logos::Error),
    /// An authored import has no exact caller-owned Rust path/storage mapping.
    #[error("Ethos import resolution failed: {0}")]
    Import(#[from] BatchImportError),
}

/// Typed refusal when authored imports do not have exact assembly-owned type
/// and storage bindings.
#[derive(Debug, thiserror::Error)]
pub enum BatchImportError {
    /// No binding matched both the authored source module and imported name.
    #[error("no Rust type/storage mapping for import {import_source}.{spelling}")]
    MissingMapping {
        import_source: String,
        spelling: String,
    },
    /// An import names a source component but the selected declaration does
    /// not exist in that component.
    #[error("bundle component {import_source}.{spelling} has no matching declaration")]
    BundleImportNotDeclared {
        import_source: String,
        spelling: String,
    },
    /// A bundle-owned import was configured as externally archived, which
    /// would bypass its complete bundle-local structural fingerprint.
    #[error("bundle import {import_source}.{spelling} must not carry external storage provenance")]
    BundleImportHasExternalProvenance {
        import_source: String,
        spelling: String,
    },
    /// An import outside the complete capsule lacked published producer
    /// evidence for its storage shape.
    #[error("external import {import_source}.{spelling} needs storage provenance")]
    ExternalImportNeedsProvenance {
        import_source: String,
        spelling: String,
    },
    /// An import spelling decoded without an exact configured identity.
    #[error("batch import spelling {spelling:?} has no configured identity")]
    UnknownConfiguredIdentity { spelling: String },
}

/// JSON configuration for the CLI and build-script entry points.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchConfiguration {
    grammar: EthosGrammarConfiguration,
    rust_grammar: RustGrammarConfiguration,
    priors: PriorConfiguration,
    interface_roles: InterfaceRoleConfiguration,
    #[serde(default)]
    rust_types: Vec<RustTypeConfiguration>,
    #[serde(default)]
    stream_lifecycles: Vec<StreamLifecycleConfiguration>,
    names: Vec<NameConfiguration>,
}

/// Caller-authored generated identities for one strict stream lifecycle.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamLifecycleConfiguration {
    stream: String,
    initiation_input: String,
    handle: String,
    initiation_refusal: String,
    termination_input: String,
    termination_refusal: String,
}

/// One caller-owned Rust path binding. Bundle-owned imports carry a path only;
/// genuinely external identities additionally carry owner/revision/archive
/// provenance.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RustTypeConfiguration {
    spelling: String,
    #[serde(default)]
    import_source: Option<String>,
    path: Vec<String>,
    #[serde(default)]
    external_storage: Option<ExternalStorageConfiguration>,
}

/// Published provenance for an external archived type.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalStorageConfiguration {
    source: String,
    revision: String,
    fingerprint: String,
}

struct ConfiguredRustTypeBinding {
    identity: VocabularyEncodedId,
    path: RustTypePath,
    external_storage: Option<ExternalStorageProvenance>,
}

struct BatchRustTypeBindings {
    unowned: BTreeMap<VocabularyEncodedId, ConfiguredRustTypeBinding>,
    imported: BTreeMap<(String, String), ConfiguredRustTypeBinding>,
}

impl BatchRustTypeBindings {
    fn try_new(
        entries: Vec<RustTypeConfiguration>,
        names: &BatchNameBindings,
    ) -> Result<Self, BatchConfigurationError> {
        let mut unowned = BTreeMap::new();
        let mut imported = BTreeMap::new();
        let mut identities = BTreeSet::new();
        for RustTypeConfiguration {
            spelling,
            import_source,
            path,
            external_storage,
        } in entries
        {
            if let Some(import_source) = &import_source {
                let key = (import_source.clone(), spelling.clone());
                if imported.contains_key(&key) {
                    return Err(BatchConfigurationError::DuplicateRustImport {
                        import_source: key.0,
                        spelling: key.1,
                    });
                }
            }
            let identity = names.require_universal(&spelling)?;
            if !identities.insert(identity.clone()) {
                return Err(BatchConfigurationError::DuplicateRustTypeIdentity { spelling });
            }
            let path = RustTypePath::try_new(path)?;
            let external_storage = external_storage
                .map(|storage| {
                    let fingerprint = parse_storage_fingerprint(&spelling, &storage.fingerprint)?;
                    let owner = StorageProvenanceOwner::new(storage.source, storage.revision)
                        .map_err(BatchConfigurationError::StorageProvenance)?;
                    ExternalStorageProvenance::new(identity.clone(), fingerprint, owner)
                        .map_err(BatchConfigurationError::StorageProvenance)
                })
                .transpose()?;
            if import_source.is_none() && external_storage.is_none() {
                return Err(BatchConfigurationError::UnownedRustTypeNeedsProvenance { spelling });
            }
            let binding = ConfiguredRustTypeBinding {
                identity: identity.clone(),
                path,
                external_storage,
            };
            if let Some(source) = import_source {
                let key = (source, spelling);
                imported.insert(key, binding);
            } else {
                unowned.insert(identity, binding);
            }
        }
        Ok(Self { unowned, imported })
    }

    fn activate(
        &self,
        components: &[DecodedBatchComponent<'_>],
        names: &BatchNameBindings,
    ) -> Result<ActiveRustTypeBindings, BatchImportError> {
        let mut declarations = BTreeMap::new();
        for component in components {
            if let Some(module) = component.module {
                declarations.insert(
                    module,
                    component_declaration_identities(component.decoded.ethos()),
                );
            }
        }
        let mut active = BTreeMap::new();
        for binding in self.unowned.values() {
            active.insert(binding.identity.clone(), binding);
        }
        for component in components {
            for import in component.decoded.imports().entries() {
                for spelling in import.names() {
                    let key = (import.source().to_owned(), spelling.clone());
                    let binding = self.imported.get(&key).ok_or_else(|| {
                        BatchImportError::MissingMapping {
                            import_source: import.source().to_owned(),
                            spelling: spelling.clone(),
                        }
                    })?;
                    if let Some(declarations) = declarations.get(import.source()) {
                        let identity = names.identity(spelling).ok_or_else(|| {
                            BatchImportError::UnknownConfiguredIdentity {
                                spelling: spelling.clone(),
                            }
                        })?;
                        if !declarations.contains(identity) {
                            return Err(BatchImportError::BundleImportNotDeclared {
                                import_source: import.source().to_owned(),
                                spelling: spelling.clone(),
                            });
                        }
                        if binding.external_storage.is_some() {
                            return Err(BatchImportError::BundleImportHasExternalProvenance {
                                import_source: import.source().to_owned(),
                                spelling: spelling.clone(),
                            });
                        }
                    } else if binding.external_storage.is_none() {
                        return Err(BatchImportError::ExternalImportNeedsProvenance {
                            import_source: import.source().to_owned(),
                            spelling: spelling.clone(),
                        });
                    }
                    active.insert(binding.identity.clone(), binding);
                }
            }
        }
        Ok(ActiveRustTypeBindings {
            paths: active
                .iter()
                .map(|(identity, binding)| (identity.clone(), binding.path.clone()))
                .collect(),
            external_storage: active
                .into_values()
                .filter_map(|binding| binding.external_storage.clone())
                .collect(),
        })
    }
}

struct ActiveRustTypeBindings {
    paths: BTreeMap<VocabularyEncodedId, RustTypePath>,
    external_storage: Vec<ExternalStorageProvenance>,
}

impl ActiveRustTypeBindings {
    fn external_storage(&self) -> &[ExternalStorageProvenance] {
        &self.external_storage
    }

    fn for_document(&self, document: &WholeEthos) -> ComponentRustTypeBindings<'_> {
        ComponentRustTypeBindings {
            paths: &self.paths,
            declarations: component_declaration_identities(document),
        }
    }
}

fn component_declaration_identities(document: &WholeEthos) -> BTreeSet<VocabularyEncodedId> {
    let mut declarations = BTreeSet::new();
    match document.body() {
        WholeEthosBody::Interface(body) => {
            for input in body.inputs() {
                declarations.insert(input.name().clone());
            }
            for output in body.outputs() {
                declarations.insert(output.name().clone());
            }
            for refusal in body.refusals() {
                declarations.insert(refusal.name().clone());
            }
            for item in body.types() {
                if let Some(identity) = item_declaration_identity(item) {
                    declarations.insert(identity.clone());
                }
            }
        }
        WholeEthosBody::Nexus(body) => {
            for item in body.types() {
                if let Some(identity) = item_declaration_identity(item) {
                    declarations.insert(identity.clone());
                }
            }
        }
        WholeEthosBody::Sema(body) => {
            for item in body.record_types() {
                if let Some(identity) = item_declaration_identity(item) {
                    declarations.insert(identity.clone());
                }
            }
        }
    }
    declarations
}

fn item_declaration_identity(item: &WholeEthosItem) -> Option<&VocabularyEncodedId> {
    match item {
        WholeEthosItem::Newtype(newtype) => Some(newtype.name()),
        WholeEthosItem::Struct(structure) => Some(structure.name()),
        WholeEthosItem::Enumeration(enumeration) => Some(enumeration.name()),
        WholeEthosItem::StreamInitiation(_) => None,
    }
}

struct ComponentRustTypeBindings<'bindings> {
    paths: &'bindings BTreeMap<VocabularyEncodedId, RustTypePath>,
    declarations: BTreeSet<VocabularyEncodedId>,
}

impl RustTypePathResolver for ComponentRustTypeBindings<'_> {
    fn resolve_type_path(&self, encoded_id: &VocabularyEncodedId) -> Option<&RustTypePath> {
        if self.declarations.contains(encoded_id) {
            None
        } else {
            self.paths.get(encoded_id)
        }
    }
}

fn parse_storage_fingerprint(
    spelling: &str,
    encoded: &str,
) -> Result<[u8; 32], BatchConfigurationError> {
    if encoded.len() != 64 {
        return Err(BatchConfigurationError::InvalidStorageFingerprintLength {
            spelling: spelling.to_owned(),
            found: encoded.len(),
        });
    }
    let mut fingerprint = [0_u8; 32];
    for (index, byte) in fingerprint.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(&encoded[start..start + 2], 16).map_err(|_| {
            BatchConfigurationError::InvalidStorageFingerprintHex {
                spelling: spelling.to_owned(),
                offset: start,
            }
        })?;
    }
    Ok(fingerprint)
}

// Trait exception — too trivial: serde parsing convenience only; validation
// and construction live under OfflineBatchConfiguration.
impl BatchConfiguration {
    /// Parse one caller-authored JSON configuration without seating it.
    pub fn from_json(source: &str) -> Result<Self, BatchConfigurationError> {
        Ok(serde_json::from_str(source)?)
    }
}

impl OfflineBatchConfiguration for BatchConfiguration {
    fn prepare(self) -> Result<PreparedBatchGenerator, BatchConfigurationError> {
        let mut names = BatchNameBindings::try_new(self.names)?;
        let grammar = EthosGrammarIds::new(self.grammar.into_identities()?)?;
        let integer = names.require_universal(&self.priors.integer)?;
        let vector = names.require_universal(&self.priors.vector)?;
        let mut priors = WholeEthosBuiltinPriors::new(integer, vector)?;
        for identity in names.universal_identities() {
            priors = priors.with_identity(identity)?;
        }
        for spelling in self.priors.application_heads {
            priors = priors.with_application_head(names.require_universal(&spelling)?)?;
        }
        if let Some(spelling) = self.priors.stream_transformer {
            priors = priors.with_stream_transformer(names.require_universal(&spelling)?)?;
        }
        let input_role = names.require_universal(&self.interface_roles.input)?;
        let output_role = names.require_universal(&self.interface_roles.output)?;
        let refusal_role = names.require_universal(&self.interface_roles.refusal)?;
        let interface_roles = InterfaceRoleIdentities::new(
            input_role.clone(),
            output_role.clone(),
            refusal_role.clone(),
        )?;
        let interface_rust_roles =
            InterfaceRustRoleIds::new(input_role, output_role, refusal_role)?;
        let rust_types = BatchRustTypeBindings::try_new(self.rust_types, &names)?;
        let stream_lifecycles = self
            .stream_lifecycles
            .into_iter()
            .map(|entry| {
                StreamLifecycleIdentities::new(
                    names.require_universal(&entry.stream)?,
                    names.require_universal(&entry.initiation_input)?,
                    names.require_universal(&entry.handle)?,
                    names.require_universal(&entry.initiation_refusal)?,
                    names.require_universal(&entry.termination_input)?,
                    names.require_universal(&entry.termination_refusal)?,
                )
                .map_err(BatchConfigurationError::InterfaceProjection)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let rust_ids = self.rust_grammar.seat(&mut names)?;
        let rust_vocabulary = FixtureRustVocabulary::seal(rust_ids, &names)?;
        Ok(PreparedBatchGenerator {
            ethos: EthosCodec::build(grammar, priors)?,
            rust: RustLogos::new(rust_vocabulary),
            names,
            rust_types,
            interface_roles,
            interface_rust_roles,
            stream_lifecycles,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NameConfiguration {
    spelling: String,
    root: ConfiguredRoot,
    chain: Vec<u16>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ConfiguredRoot {
    Universal,
    Rust,
}

impl From<ConfiguredRoot> for VocabularyRoot {
    fn from(root: ConfiguredRoot) -> Self {
        match root {
            ConfiguredRoot::Universal => Self::Universal,
            ConfiguredRoot::Rust => Self::Rust,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PriorConfiguration {
    integer: String,
    vector: String,
    #[serde(default)]
    application_heads: Vec<String>,
    #[serde(default)]
    stream_transformer: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InterfaceRoleConfiguration {
    input: String,
    output: String,
    refusal: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EthosGrammarConfiguration {
    interface_document: Vec<u16>,
    nexus_document: Vec<u16>,
    sema_document: Vec<u16>,
    header: Vec<u16>,
    imports: Vec<u16>,
    import_entry: Vec<u16>,
    interface_body: Vec<u16>,
    nexus_body: Vec<u16>,
    sema_body: Vec<u16>,
    newtype_list: Vec<u16>,
    struct_list: Vec<u16>,
    item_list: Vec<u16>,
    trait_list: Vec<u16>,
    table_list: Vec<u16>,
    newtype_declaration: Vec<u16>,
    struct_declaration: Vec<u16>,
    item: Vec<u16>,
    variant: Vec<u16>,
    type_reference: Vec<u16>,
    trait_declaration: Vec<u16>,
    table: Vec<u16>,
}

impl EthosGrammarConfiguration {
    fn into_identities(self) -> Result<EthosGrammarIdentities, BatchConfigurationError> {
        Ok(EthosGrammarIdentities {
            interface_document: universal_id(
                "grammar.interface_document",
                self.interface_document,
            )?,
            nexus_document: universal_id("grammar.nexus_document", self.nexus_document)?,
            sema_document: universal_id("grammar.sema_document", self.sema_document)?,
            header: universal_id("grammar.header", self.header)?,
            imports: universal_id("grammar.imports", self.imports)?,
            import_entry: universal_id("grammar.import_entry", self.import_entry)?,
            interface_body: universal_id("grammar.interface_body", self.interface_body)?,
            nexus_body: universal_id("grammar.nexus_body", self.nexus_body)?,
            sema_body: universal_id("grammar.sema_body", self.sema_body)?,
            newtype_list: universal_id("grammar.newtype_list", self.newtype_list)?,
            struct_list: universal_id("grammar.struct_list", self.struct_list)?,
            item_list: universal_id("grammar.item_list", self.item_list)?,
            trait_list: universal_id("grammar.trait_list", self.trait_list)?,
            table_list: universal_id("grammar.table_list", self.table_list)?,
            newtype_declaration: universal_id(
                "grammar.newtype_declaration",
                self.newtype_declaration,
            )?,
            struct_declaration: universal_id(
                "grammar.struct_declaration",
                self.struct_declaration,
            )?,
            item: universal_id("grammar.item", self.item)?,
            variant: universal_id("grammar.variant", self.variant)?,
            type_reference: universal_id("grammar.type_reference", self.type_reference)?,
            trait_declaration: universal_id("grammar.trait_declaration", self.trait_declaration)?,
            table: universal_id("grammar.table", self.table)?,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RustGrammarConfiguration {
    newtype_item: Vec<u16>,
    enumeration_item: Vec<u16>,
    variant: Vec<u16>,
    tuple_field: Vec<u16>,
    type_reference: Vec<u16>,
    struct_keyword: Vec<u16>,
    enum_keyword: Vec<u16>,
    public_keyword: Vec<u16>,
    comma: Vec<u16>,
    semicolon: Vec<u16>,
}

impl RustGrammarConfiguration {
    fn seat(
        self,
        names: &mut BatchNameBindings,
    ) -> Result<FixtureRustVocabularyIds, BatchConfigurationError> {
        let newtype_item = names.insert_rust(
            "rust_grammar.newtype_item",
            self.newtype_item,
            "NewtypeItemRecord",
        )?;
        let enumeration_item = names.insert_rust(
            "rust_grammar.enumeration_item",
            self.enumeration_item,
            "EnumerationItemRecord",
        )?;
        let variant = names.insert_rust("rust_grammar.variant", self.variant, "VariantRecord")?;
        let tuple_field = names.insert_rust(
            "rust_grammar.tuple_field",
            self.tuple_field,
            "TupleFieldRecord",
        )?;
        let type_reference = names.insert_rust(
            "rust_grammar.type_reference",
            self.type_reference,
            "TypeReferenceRecord",
        )?;
        let struct_keyword =
            names.insert_rust("rust_grammar.struct_keyword", self.struct_keyword, "struct")?;
        let enum_keyword =
            names.insert_rust("rust_grammar.enum_keyword", self.enum_keyword, "enum")?;
        let public_keyword =
            names.insert_rust("rust_grammar.public_keyword", self.public_keyword, "pub")?;
        let comma = names.insert_rust("rust_grammar.comma", self.comma, ",")?;
        let semicolon = names.insert_rust("rust_grammar.semicolon", self.semicolon, ";")?;
        Ok(FixtureRustVocabularyIds::new(
            newtype_item,
            enumeration_item,
            variant,
            tuple_field,
            type_reference,
            struct_keyword,
            enum_keyword,
            public_keyword,
            comma,
            semicolon,
        ))
    }
}

struct BatchNameBindings {
    by_spelling: BTreeMap<String, VocabularyEncodedId>,
    by_identity: BTreeMap<VocabularyEncodedId, Name>,
}

impl BatchNameBindings {
    fn try_new(entries: Vec<NameConfiguration>) -> Result<Self, BatchConfigurationError> {
        let mut bindings = Self {
            by_spelling: BTreeMap::new(),
            by_identity: BTreeMap::new(),
        };
        for entry in entries {
            let identity = configured_id("names", entry.root.into(), entry.chain)?;
            if bindings.by_spelling.contains_key(&entry.spelling) {
                return Err(BatchConfigurationError::DuplicateSpelling {
                    spelling: entry.spelling,
                });
            }
            if let Some(existing) = bindings.by_identity.get(&identity) {
                return Err(BatchConfigurationError::DuplicateIdentity {
                    first: existing.as_str().to_owned(),
                    second: entry.spelling,
                });
            }
            bindings
                .by_identity
                .insert(identity.clone(), Name::new(&entry.spelling));
            bindings.by_spelling.insert(entry.spelling, identity);
        }
        Ok(bindings)
    }

    fn require_universal(
        &self,
        spelling: &str,
    ) -> Result<VocabularyEncodedId, BatchConfigurationError> {
        let identity =
            self.by_spelling
                .get(spelling)
                .ok_or_else(|| BatchConfigurationError::MissingName {
                    spelling: spelling.to_owned(),
                })?;
        if identity.root_variant() != &VocabularyRoot::Universal {
            return Err(BatchConfigurationError::ExpectedUniversal {
                spelling: spelling.to_owned(),
            });
        }
        Ok(identity.clone())
    }

    fn universal_identities(&self) -> Vec<VocabularyEncodedId> {
        self.by_identity
            .keys()
            .filter(|identity| identity.root_variant() == &VocabularyRoot::Universal)
            .cloned()
            .collect()
    }

    fn identity(&self, spelling: &str) -> Option<&VocabularyEncodedId> {
        self.by_spelling.get(spelling)
    }

    fn insert_rust(
        &mut self,
        position: &'static str,
        chain: Vec<u16>,
        spelling: &'static str,
    ) -> Result<VocabularyEncodedId, BatchConfigurationError> {
        let identity = configured_id(position, VocabularyRoot::Rust, chain)?;
        if let Some(existing) = self.by_identity.get(&identity) {
            return Err(BatchConfigurationError::DuplicateIdentity {
                first: existing.as_str().to_owned(),
                second: spelling.to_owned(),
            });
        }
        self.by_identity
            .insert(identity.clone(), Name::new(spelling));
        Ok(identity)
    }
}

impl EncodedNameResolver<VocabularyRoot> for BatchNameBindings {
    fn resolve(&self, encoded_id: &VocabularyEncodedId) -> Option<&Name> {
        self.by_identity.get(encoded_id)
    }
}

impl DecodeNameBindings<VocabularyRoot> for BatchNameBindings {
    fn declaration_assignment(
        &self,
        occurrence: NameOccurrence<'_>,
    ) -> Option<DeclarationAssignment<VocabularyRoot>> {
        self.by_spelling
            .get(occurrence.spelling())
            .cloned()
            .map(DeclarationAssignment::new)
    }

    fn reference_resolution(
        &self,
        occurrence: NameOccurrence<'_>,
    ) -> Option<ResolvedReference<VocabularyRoot>> {
        self.by_spelling
            .get(occurrence.spelling())
            .cloned()
            .map(ResolvedReference::new)
    }
}

fn universal_id(
    position: &'static str,
    chain: Vec<u16>,
) -> Result<VocabularyEncodedId, BatchConfigurationError> {
    configured_id(position, VocabularyRoot::Universal, chain)
}

fn configured_id(
    position: &'static str,
    root: VocabularyRoot,
    chain: Vec<u16>,
) -> Result<VocabularyEncodedId, BatchConfigurationError> {
    VocabularyEncodedId::new(root, chain.into_iter().map(LocalEncodedId::new).collect())
        .map_err(|_| BatchConfigurationError::EmptyIdentity { position })
}

/// Typed refusal while loading caller-supplied batch configuration.
#[derive(Debug, thiserror::Error)]
pub enum BatchConfigurationError {
    /// JSON syntax or shape is invalid.
    #[error("batch configuration JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    /// A configured identity chain is empty.
    #[error("batch configuration identity {position} has an empty chain")]
    EmptyIdentity { position: &'static str },
    /// One source spelling was configured more than once.
    #[error("batch configuration repeats spelling {spelling:?}")]
    DuplicateSpelling { spelling: String },
    /// One complete identity was assigned to two spellings.
    #[error("batch configuration identity is assigned to both {first:?} and {second:?}")]
    DuplicateIdentity { first: String, second: String },
    /// One Universal identity was assigned more than one Rust type contract.
    #[error("batch configuration repeats Rust type identity for {spelling:?}")]
    DuplicateRustTypeIdentity { spelling: String },
    /// One exact import source/name pair was configured more than once.
    #[error("batch configuration repeats Rust import mapping {import_source}.{spelling}")]
    DuplicateRustImport {
        import_source: String,
        spelling: String,
    },
    /// A type not imported from a source module is external to this capsule
    /// and must carry its published storage provenance.
    #[error("unowned Rust type {spelling:?} needs external storage provenance")]
    UnownedRustTypeNeedsProvenance { spelling: String },
    /// An external storage fingerprint was not exactly 32 encoded bytes.
    #[error(
        "batch configuration storage fingerprint for {spelling:?} must contain 64 hexadecimal characters, found {found}"
    )]
    InvalidStorageFingerprintLength { spelling: String, found: usize },
    /// An external storage fingerprint contained a non-hexadecimal byte pair.
    #[error(
        "batch configuration storage fingerprint for {spelling:?} is not hexadecimal at byte offset {offset}"
    )]
    InvalidStorageFingerprintHex { spelling: String, offset: usize },
    /// Typed Nomos provenance validation rejected a configured external type.
    #[error("batch external storage provenance failed: {0}")]
    StorageProvenance(NexusTransformationError),
    /// A prior names a spelling absent from the supplied identity view.
    #[error("batch configuration has no identity for required name {spelling:?}")]
    MissingName { spelling: String },
    /// An Ethos prior selected a Rust-root name.
    #[error("batch configuration prior {spelling:?} must be Universal")]
    ExpectedUniversal { spelling: String },
    /// Interface role validation at the structural Nomos boundary failed.
    #[error("Interface role configuration failed: {0}")]
    InterfaceProjection(#[from] NexusTransformationError),
    /// Ethos grammar identity validation failed.
    #[error(transparent)]
    EthosGrammar(#[from] EthosGrammarError),
    /// Builtin-prior validation failed.
    #[error(transparent)]
    Prior(#[from] WholeEthosBuiltinPriorError),
    /// Ethos structural table construction failed.
    #[error(transparent)]
    EthosCodec(#[from] EthosCodecBuildError),
    /// Rust structural vocabulary construction failed.
    #[error("Rust batch vocabulary failed: {0}")]
    Rust(#[from] rust_logos::Error),
}
