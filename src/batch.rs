//! Offline, socket-free Ethos-to-Rust batch generation.
//!
//! Callers supply every translator-issued identity and current spelling. The
//! batch path never allocates an identity and never enters the stateful daemon.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use batch_core_ethos::{
    EthosCodec, EthosCodecBuildError, EthosDecodeError, EthosGrammarError, EthosGrammarIdentities,
    EthosGrammarIds, WholeEthosBuiltinPriorError, WholeEthosBuiltinPriors, WholeEthosFileKind,
    WholeEthosImports,
};
use batch_core_logos::WholeLogos;
use batch_core_nomos::{
    InterfaceRoleIdentities, InterfaceStructuralTransformation, NexusStructuralTransformation,
    NexusTransformation, NexusTransformationError, SemaStorageTypeFingerprintMapping,
    SemaStructuralTransformation, StreamLifecycleIdentities,
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
    /// Decode the source, project every declaration, and emit complete Rust.
    fn generate(&self, source: &str) -> Result<BatchGenerationOutcome, BatchGenerationError>;
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
    fn generate(&self, source: &str) -> Result<BatchGenerationOutcome, BatchGenerationError> {
        let decoded = self.ethos.decode(source, &self.names)?;
        let rust_types = self.rust_types.activate(decoded.imports())?;
        let transformation = NexusTransformation::new()
            .with_storage_fingerprints(rust_types.storage_fingerprints().to_vec())?
            .with_stream_lifecycle_identities(self.stream_lifecycles.clone())?;
        let kind = decoded.ethos().header().kind();
        let logos = match kind {
            WholeEthosFileKind::Interface => transformation
                .lower_interface(decoded.ethos(), &self.interface_roles)?
                .logos()
                .clone(),
            WholeEthosFileKind::Nexus => transformation.lower(decoded.ethos())?,
            WholeEthosFileKind::Sema => {
                let outcome = transformation.lower_sema(decoded.ethos())?;
                if !outcome.deferred_tables().is_empty() {
                    return Err(BatchGenerationError::SemaTablesRequireGeneratedOwner {
                        count: outcome.deferred_tables().len(),
                    });
                }
                outcome.logos().clone()
            }
        };
        let rust = if kind == WholeEthosFileKind::Interface {
            self.rust.emit_interface_with_type_paths(
                &logos,
                &self.names,
                &self.interface_rust_roles,
                &rust_types,
            )?
        } else {
            self.rust
                .emit_with_type_paths(&logos, &self.names, &rust_types)?
        };
        Ok(BatchGenerationOutcome {
            kind,
            version: decoded.ethos().header().version(),
            logos,
            rust,
        })
    }
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
    /// Header, body, name, or structural source decoding failed.
    #[error("Ethos batch decode failed: {0}")]
    Decode(#[from] EthosDecodeError),
    /// Current typed Nomos projection refused the decoded document.
    #[error("Nomos batch projection failed: {0}")]
    Projection(#[from] NexusTransformationError),
    /// An imported Sema record table requires another generated owner and this
    /// complete-document generator refuses to emit a partial artifact.
    #[error(
        "Sema batch generation requires an owning generated record for {count} imported table(s)"
    )]
    SemaTablesRequireGeneratedOwner { count: usize },
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

/// One caller-owned binding from a Universal vocabulary identity to both its
/// canonical Rust path and its complete external storage contract.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RustTypeConfiguration {
    spelling: String,
    #[serde(default)]
    import_source: Option<String>,
    path: Vec<String>,
    storage_fingerprint: String,
}

struct ConfiguredRustTypeBinding {
    identity: VocabularyEncodedId,
    path: RustTypePath,
    storage: SemaStorageTypeFingerprintMapping,
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
        for entry in entries {
            if let Some(import_source) = &entry.import_source {
                let key = (import_source.clone(), entry.spelling.clone());
                if imported.contains_key(&key) {
                    return Err(BatchConfigurationError::DuplicateRustImport {
                        import_source: key.0,
                        spelling: key.1,
                    });
                }
            }
            let identity = names.require_universal(&entry.spelling)?;
            if !identities.insert(identity.clone()) {
                return Err(BatchConfigurationError::DuplicateRustTypeIdentity {
                    spelling: entry.spelling,
                });
            }
            let path = RustTypePath::try_new(entry.path)?;
            let fingerprint =
                parse_storage_fingerprint(&entry.spelling, &entry.storage_fingerprint)?;
            let storage = SemaStorageTypeFingerprintMapping::new(identity.clone(), fingerprint)?;
            let binding = ConfiguredRustTypeBinding {
                identity: identity.clone(),
                path,
                storage,
            };
            if let Some(source) = entry.import_source {
                let key = (source, entry.spelling);
                imported.insert(key, binding);
            } else {
                unowned.insert(identity, binding);
            }
        }
        Ok(Self { unowned, imported })
    }

    fn activate(
        &self,
        imports: &WholeEthosImports,
    ) -> Result<ActiveRustTypeBindings, BatchImportError> {
        let mut active = BTreeMap::new();
        for binding in self.unowned.values() {
            active.insert(binding.identity.clone(), binding);
        }
        for import in imports.entries() {
            for spelling in import.names() {
                let key = (import.source().to_owned(), spelling.clone());
                let binding =
                    self.imported
                        .get(&key)
                        .ok_or_else(|| BatchImportError::MissingMapping {
                            import_source: import.source().to_owned(),
                            spelling: spelling.clone(),
                        })?;
                active.insert(binding.identity.clone(), binding);
            }
        }
        Ok(ActiveRustTypeBindings {
            paths: active
                .iter()
                .map(|(identity, binding)| (identity.clone(), binding.path.clone()))
                .collect(),
            storage_fingerprints: active
                .into_values()
                .map(|binding| binding.storage.clone())
                .collect(),
        })
    }
}

struct ActiveRustTypeBindings {
    paths: BTreeMap<VocabularyEncodedId, RustTypePath>,
    storage_fingerprints: Vec<SemaStorageTypeFingerprintMapping>,
}

impl ActiveRustTypeBindings {
    fn storage_fingerprints(&self) -> &[SemaStorageTypeFingerprintMapping] {
        &self.storage_fingerprints
    }
}

impl RustTypePathResolver for ActiveRustTypeBindings {
    fn resolve_type_path(&self, encoded_id: &VocabularyEncodedId) -> Option<&RustTypePath> {
        self.paths.get(encoded_id)
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
