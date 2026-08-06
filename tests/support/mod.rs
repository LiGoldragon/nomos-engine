use std::collections::{BTreeMap, BTreeSet};

use core_ethos::bootstrap::{
    BootstrapCatalog, BootstrapGrammarIdentities, BootstrapPriorIdentities,
    BootstrapPriorVocabulary, BootstrapVersionPolicy, CanonicalIdentityOrder, EthosKind,
    EthosVersion, IdentitySchema, IdentitySchemaCatalog, InterfaceRole, NomosSchema, SchemaRole,
    TextualMetadataRecord, TextualMetadataSnapshot, TextualProjectionAddress,
};
use core_logos::{LogosLanguage, LogosLanguageTypeIds, LogosLanguageWords};
use core_nomos::{
    AuthoredBindingIdentity, AuthoredInputParameter, AuthoredInputSignature,
    AuthoredTransformerDeclaration, AuthoredTransformerIdentity, AuthoredTransformerSet,
    LoadedNomosPopulation, MacroKind, MetaType, NameTransform, NameTreeProjectionVersion,
    NomosNameTable, SealedNomosPopulation, SectionDefault, TemplateFieldValue, TemplateFuture,
    TemplateFutureOutput, TemplateLandingShape, TemplateLanguage, TemplateTerm, TemplateValue,
};
use name_table::LocalEncodedId;
use sema_translator::bootstrap::{
    AuthorizedBootstrapTransition, BootstrapAuthorityIdentity, BootstrapAuthorityRevision,
    BootstrapTransactionAssembler, VerifiedBootstrapAssembly,
};
use signal_nomos::NomosDeploymentArtifacts;
use signal_sema_translator::{VocabularyEncodedId, VocabularyRoot};
use structural_codec::{LandingShape, LeafCodec, ScalarValue, StableRoleId};

const NAME_FIELD: usize = 2;
const NEWTYPE_WRAPPED_FIELD: usize = 4;
const ENUMERATION_VARIANTS_FIELD: usize = 4;

pub fn authored_population(seed: u16, version: NameTreeProjectionVersion) -> SealedNomosPopulation {
    let logos = logos();
    let newtype = TemplateLanguage::derive(logos.grammar(), logos.landing(), logos.newtype_type())
        .expect("newtype Template(Logos)");
    let enumeration =
        TemplateLanguage::derive(logos.grammar(), logos.landing(), logos.enumeration_type())
            .expect("enumeration Template(Logos)");
    let base = 1_000_u16.checked_add(seed).expect("fixture seed");

    let newtype_name = binding(&[base, 3, 1]);
    let wrapped = binding(&[base, 3, 2]);
    let newtype_result = root_value(&newtype, |role, _shape| {
        if role == field_role(&newtype, NAME_FIELD) {
            Some(TemplateTerm::Future(TemplateFuture::Realize {
                binding: newtype_name.clone(),
                transform: NameTransform::PascalCase,
            }))
        } else if role == field_role(&newtype, NEWTYPE_WRAPPED_FIELD) {
            Some(TemplateTerm::Future(TemplateFuture::Realize {
                binding: wrapped.clone(),
                transform: NameTransform::Identity,
            }))
        } else {
            None
        }
    });
    let newtype_declaration = AuthoredTransformerDeclaration::try_new(
        transformer(&[base, 3]),
        MacroKind::Structural(SectionDefault::Newtype),
        AuthoredInputSignature::try_new(vec![
            parameter(newtype_name, MetaType::Name, &newtype, NAME_FIELD),
            parameter(wrapped, MetaType::Type, &newtype, NEWTYPE_WRAPPED_FIELD),
        ])
        .expect("distinct newtype bindings"),
        newtype_result,
        &newtype,
    )
    .expect("valid authored newtype");

    let enumeration_name = binding(&[base, 5, 1]);
    let variants = binding(&[base, 5, 2]);
    let enumeration_result = root_value(&enumeration, |role, _shape| {
        if role == field_role(&enumeration, NAME_FIELD) {
            Some(TemplateTerm::Future(TemplateFuture::Realize {
                binding: enumeration_name.clone(),
                transform: NameTransform::PascalCase,
            }))
        } else if role == field_role(&enumeration, ENUMERATION_VARIANTS_FIELD) {
            Some(TemplateTerm::Sequence(vec![TemplateTerm::Future(
                TemplateFuture::Splice {
                    binding: variants.clone(),
                },
            )]))
        } else {
            None
        }
    });
    let enumeration_declaration = AuthoredTransformerDeclaration::try_new(
        transformer(&[base, 5]),
        MacroKind::Structural(SectionDefault::Enumeration),
        AuthoredInputSignature::try_new(vec![
            parameter(enumeration_name, MetaType::Name, &enumeration, NAME_FIELD),
            parameter(
                variants,
                MetaType::Variants,
                &enumeration,
                ENUMERATION_VARIANTS_FIELD,
            ),
        ])
        .expect("distinct enumeration bindings"),
        enumeration_result,
        &enumeration,
    )
    .expect("valid authored enumeration");

    let transformers =
        AuthoredTransformerSet::try_new(vec![newtype_declaration, enumeration_declaration])
            .expect("distinct structural transformers");
    let names = reachable_names(&transformers);
    LoadedNomosPopulation::from_typed(transformers, names)
        .seal(version)
        .expect("publicly constructed authored population seals")
}

pub fn authored_artifacts(seed: u16) -> NomosDeploymentArtifacts {
    NomosDeploymentArtifacts::from_population(&authored_population(
        seed,
        NameTreeProjectionVersion::initial(),
    ))
    .expect("deployment artifacts")
}

pub struct BootstrapInput {
    pub assembly: VerifiedBootstrapAssembly,
    pub wrapped: VocabularyEncodedId,
    pub choice: VocabularyEncodedId,
    pub none: VocabularyEncodedId,
    pub some: VocabularyEncodedId,
    pub pair: VocabularyEncodedId,
}

pub fn bootstrap_input() -> BootstrapInput {
    let (catalog, before) = bootstrap_catalog();
    let wrapped = encoded(VocabularyRoot::Universal, &[100]);
    let choice = encoded(VocabularyRoot::Universal, &[101]);
    let none = encoded(VocabularyRoot::Universal, &[102]);
    let some = encoded(VocabularyRoot::Universal, &[103]);
    let pair = encoded(VocabularyRoot::Universal, &[104]);
    let mut records = before.records().to_vec();
    records.extend([
        metadata_record(&["app"], None, "Wrapped", wrapped.clone()),
        metadata_record(&["app"], None, "Choice", choice.clone()),
        metadata_record(&["app"], Some(choice.clone()), "None", none.clone()),
        metadata_record(&["app"], Some(choice.clone()), "Some", some.clone()),
        metadata_record(&["app"], Some(choice.clone()), "Pair", pair.clone()),
    ]);
    let canonical_bytes = [
        wrapped.clone(),
        choice.clone(),
        none.clone(),
        some.clone(),
        pair.clone(),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, identity)| (identity, vec![0x90, index as u8]))
    .collect();
    let approval = AuthorizedBootstrapTransition::new(
        TextualMetadataSnapshot::new(records).expect("complete approved metadata"),
        canonical_bytes,
        BTreeMap::new(),
    );
    let assembler = BootstrapTransactionAssembler::new(
        BootstrapAuthorityIdentity::new([0x42; 32]),
        BootstrapAuthorityRevision::new(1),
        BootstrapGrammarIdentities {
            document: encoded(VocabularyRoot::Universal, &[900]),
            syntax: encoded(VocabularyRoot::Universal, &[901]),
        },
        catalog,
    );
    let source = "Nexus.{1 0 0}\n[]\n{[] [Wrapped.Vector<Option<String>> Choice.[None Some.String Pair.{Map<String Integer> Boolean}]]}";
    let assembly = assembler
        .assemble(source, approval)
        .expect("authority-sealed bootstrap transaction");
    BootstrapInput {
        assembly,
        wrapped,
        choice,
        none,
        some,
        pair,
    }
}

fn bootstrap_catalog() -> (BootstrapCatalog, TextualMetadataSnapshot) {
    let prior_specs = [
        (
            1,
            "Interface",
            vec![SchemaRole::FileKind(EthosKind::Interface)],
        ),
        (2, "Nexus", vec![SchemaRole::FileKind(EthosKind::Nexus)]),
        (3, "Sema", vec![SchemaRole::FileKind(EthosKind::Sema)]),
        (
            4,
            "Input",
            vec![SchemaRole::InterfaceRole(InterfaceRole::Input)],
        ),
        (
            5,
            "Output",
            vec![SchemaRole::InterfaceRole(InterfaceRole::Output)],
        ),
        (
            6,
            "Refusal",
            vec![SchemaRole::InterfaceRole(InterfaceRole::Refusal)],
        ),
        (7, "String", vec![SchemaRole::Nominal { persistent: true }]),
        (8, "Integer", vec![SchemaRole::Nominal { persistent: true }]),
        (9, "Boolean", vec![SchemaRole::Nominal { persistent: true }]),
        (10, "Unit", vec![SchemaRole::Nominal { persistent: true }]),
        (11, "Vector", vec![SchemaRole::Shape { arity: 1 }]),
        (12, "Option", vec![SchemaRole::Shape { arity: 1 }]),
        (13, "Map", vec![SchemaRole::Shape { arity: 2 }]),
        (14, "Result", vec![SchemaRole::Shape { arity: 2 }]),
        (
            15,
            "Stream",
            vec![
                SchemaRole::Shape { arity: 1 },
                SchemaRole::Nomos(NomosSchema::StreamInitiation { arity: 2 }),
            ],
        ),
        (16, "StreamIdentity", vec![SchemaRole::Shape { arity: 1 }]),
    ];
    let mut records = Vec::new();
    let mut schemas = Vec::new();
    let mut order = Vec::new();
    for (local, name, roles) in prior_specs {
        let identity = encoded(VocabularyRoot::Universal, &[local]);
        records.push(metadata_record(&["builtin"], None, name, identity.clone()));
        schemas.push(IdentitySchema::new(identity.clone(), roles).expect("valid prior schema"));
        order.push((identity, vec![0x80, local as u8]));
    }
    let before = TextualMetadataSnapshot::new(records).expect("unique prior metadata");
    let schemas = IdentitySchemaCatalog::new(schemas).expect("unique prior schemas");
    let canonical = CanonicalIdentityOrder::new(order).expect("unique prior ordering");
    let priors = BootstrapPriorVocabulary::new(
        BootstrapPriorIdentities {
            interface_kind: encoded(VocabularyRoot::Universal, &[1]),
            nexus_kind: encoded(VocabularyRoot::Universal, &[2]),
            sema_kind: encoded(VocabularyRoot::Universal, &[3]),
            input_role: encoded(VocabularyRoot::Universal, &[4]),
            output_role: encoded(VocabularyRoot::Universal, &[5]),
            refusal_role: encoded(VocabularyRoot::Universal, &[6]),
            string_type: encoded(VocabularyRoot::Universal, &[7]),
            integer_type: encoded(VocabularyRoot::Universal, &[8]),
            boolean_type: encoded(VocabularyRoot::Universal, &[9]),
            unit_type: encoded(VocabularyRoot::Universal, &[10]),
            vector_shape: encoded(VocabularyRoot::Universal, &[11]),
            option_shape: encoded(VocabularyRoot::Universal, &[12]),
            map_shape: encoded(VocabularyRoot::Universal, &[13]),
            result_shape: encoded(VocabularyRoot::Universal, &[14]),
            stream_nomos: encoded(VocabularyRoot::Universal, &[15]),
            stream_shape: encoded(VocabularyRoot::Universal, &[15]),
            stream_identity_shape: encoded(VocabularyRoot::Universal, &[16]),
        },
        &schemas,
        &before,
    )
    .expect("valid bootstrap prior vocabulary");
    let catalog = BootstrapCatalog::new(
        vec!["app".to_owned()],
        before.clone(),
        schemas,
        priors,
        BootstrapVersionPolicy::exact(EthosVersion::new(1, 0, 0)),
        canonical,
    )
    .expect("valid bootstrap catalog");
    (catalog, before)
}

fn metadata_record(
    module: &[&str],
    owner: Option<VocabularyEncodedId>,
    name: &str,
    identity: VocabularyEncodedId,
) -> TextualMetadataRecord {
    TextualMetadataRecord {
        address: TextualProjectionAddress {
            module_path: module.iter().map(|part| (*part).to_owned()).collect(),
            lexical_owner: owner,
            visible_name: name.to_owned(),
        },
        encoded_name: identity,
    }
}

fn logos() -> LogosLanguage {
    LogosLanguage::seal(
        LogosLanguageTypeIds {
            newtype: encoded(VocabularyRoot::Universal, &[1]),
            structure: encoded(VocabularyRoot::Universal, &[13]),
            enumeration: encoded(VocabularyRoot::Universal, &[2]),
            visibility: encoded(VocabularyRoot::Universal, &[3]),
            attributes: encoded(VocabularyRoot::Universal, &[4]),
            attribute: encoded(VocabularyRoot::Universal, &[5]),
            path: encoded(VocabularyRoot::Universal, &[6]),
            configuration_predicate: encoded(VocabularyRoot::Universal, &[7]),
            derive_group: encoded(VocabularyRoot::Universal, &[8]),
            generics: encoded(VocabularyRoot::Universal, &[9]),
            generic_parameter: encoded(VocabularyRoot::Universal, &[10]),
            type_reference: encoded(VocabularyRoot::Universal, &[11]),
            field: encoded(VocabularyRoot::Universal, &[14]),
            variant: encoded(VocabularyRoot::Universal, &[12]),
        },
        LogosLanguageWords {
            public: encoded(VocabularyRoot::Universal, &[20]),
            private: encoded(VocabularyRoot::Universal, &[21]),
        },
    )
    .expect("canonical Logos language")
}

fn transformer(chain: &[u16]) -> AuthoredTransformerIdentity {
    AuthoredTransformerIdentity::try_new(encoded(VocabularyRoot::Universal, chain))
        .expect("Universal transformer")
}

fn binding(chain: &[u16]) -> AuthoredBindingIdentity {
    AuthoredBindingIdentity::try_new(encoded(VocabularyRoot::Universal, chain))
        .expect("Universal binding")
}

fn encoded(root: VocabularyRoot, chain: &[u16]) -> VocabularyEncodedId {
    VocabularyEncodedId::new(
        root,
        chain.iter().copied().map(LocalEncodedId::new).collect(),
    )
    .expect("non-empty identity")
}

fn root_constructor(
    language: &TemplateLanguage<VocabularyRoot>,
) -> &core_nomos::TemplateConstructorDeclaration<VocabularyRoot> {
    language
        .type_declaration(language.root())
        .and_then(|declaration| declaration.constructors().first())
        .expect("root constructor")
}

fn field_shape(
    language: &TemplateLanguage<VocabularyRoot>,
    index: usize,
) -> &TemplateLandingShape<VocabularyRoot> {
    root_constructor(language)
        .landing_fields()
        .get(index)
        .map(core_nomos::TemplateLandingField::shape)
        .expect("root fixture field")
}

fn field_role(language: &TemplateLanguage<VocabularyRoot>, index: usize) -> StableRoleId {
    root_constructor(language)
        .landing_fields()
        .get(index)
        .map(core_nomos::TemplateLandingField::role)
        .expect("root fixture field")
}

fn literal_landing(shape: &TemplateLandingShape<VocabularyRoot>) -> LandingShape<VocabularyRoot> {
    match shape {
        TemplateLandingShape::Fixed(landing)
        | TemplateLandingShape::ValueOrFuture { value: landing, .. } => landing.clone(),
        TemplateLandingShape::Nested(target) => LandingShape::Type(target.clone()),
        TemplateLandingShape::Sequence {
            minimum,
            maximum,
            element,
            ..
        } => LandingShape::sequence(*minimum, *maximum, literal_landing(element)),
    }
}

fn parameter(
    binding: AuthoredBindingIdentity,
    meta: MetaType,
    language: &TemplateLanguage<VocabularyRoot>,
    field: usize,
) -> AuthoredInputParameter {
    AuthoredInputParameter::new(
        binding,
        meta,
        TemplateFutureOutput::new(literal_landing(field_shape(language, field))),
    )
}

fn literal_value(
    constructor: &structural_codec::EncodedConstructorId<VocabularyRoot>,
    language: &TemplateLanguage<VocabularyRoot>,
) -> TemplateValue<VocabularyRoot> {
    let declaration = language
        .constructor(constructor)
        .expect("addressed constructor");
    let fields = declaration
        .landing_fields()
        .iter()
        .map(|field| TemplateFieldValue::new(field.role(), literal_term(field.shape(), language)))
        .collect();
    TemplateValue::try_new(constructor.clone(), fields).expect("unique landing roles")
}

fn literal_term(
    shape: &TemplateLandingShape<VocabularyRoot>,
    language: &TemplateLanguage<VocabularyRoot>,
) -> TemplateTerm<VocabularyRoot> {
    match shape {
        TemplateLandingShape::Fixed(LandingShape::Literal(value)) => {
            TemplateTerm::Literal(value.clone())
        }
        TemplateLandingShape::Fixed(LandingShape::Scalar(codec)) => {
            let scalar = match codec {
                LeafCodec::Integer => ScalarValue::Integer(0),
                LeafCodec::Float => ScalarValue::Float(0.0),
                LeafCodec::Boolean => ScalarValue::Boolean(false),
                LeafCodec::Text | LeafCodec::PipeText | LeafCodec::Foreign(_) => {
                    panic!("native authored fixtures cannot contain text scalars")
                }
            };
            TemplateTerm::Scalar(scalar)
        }
        TemplateLandingShape::ValueOrFuture { value, .. } => match value {
            LandingShape::Declaration => {
                TemplateTerm::Declaration(encoded(VocabularyRoot::Universal, &[900, 1]))
            }
            LandingShape::Reference => {
                TemplateTerm::Reference(encoded(VocabularyRoot::Universal, &[900, 2]))
            }
            LandingShape::Type(target) => {
                let constructor = language
                    .type_declaration(target)
                    .and_then(|declaration| declaration.constructors().first())
                    .expect("nested constructor");
                TemplateTerm::Nested(Box::new(literal_value(constructor.constructor(), language)))
            }
            LandingShape::Literal(_) | LandingShape::Scalar(_) | LandingShape::Sequence { .. } => {
                panic!("single value position")
            }
        },
        TemplateLandingShape::Nested(target) => {
            let constructor = language
                .type_declaration(target)
                .and_then(|declaration| declaration.constructors().first())
                .expect("nested constructor");
            TemplateTerm::Nested(Box::new(literal_value(constructor.constructor(), language)))
        }
        TemplateLandingShape::Sequence { .. } => TemplateTerm::Sequence(Vec::new()),
        TemplateLandingShape::Fixed(
            LandingShape::Declaration
            | LandingShape::Reference
            | LandingShape::Type(_)
            | LandingShape::Sequence { .. },
        ) => panic!("term-producing landing cannot be fixed"),
    }
}

fn root_value(
    language: &TemplateLanguage<VocabularyRoot>,
    mut replacement: impl FnMut(
        StableRoleId,
        &TemplateLandingShape<VocabularyRoot>,
    ) -> Option<TemplateTerm<VocabularyRoot>>,
) -> TemplateValue<VocabularyRoot> {
    let constructor = root_constructor(language);
    let fields = constructor
        .landing_fields()
        .iter()
        .map(|field| {
            TemplateFieldValue::new(
                field.role(),
                replacement(field.role(), field.shape())
                    .unwrap_or_else(|| literal_term(field.shape(), language)),
            )
        })
        .collect();
    TemplateValue::try_new(constructor.constructor().clone(), fields).expect("unique landing roles")
}

fn reachable_names(transformers: &AuthoredTransformerSet) -> NomosNameTable {
    let mut identities = BTreeSet::new();
    for declaration in transformers.declarations() {
        include_ancestors(declaration.name().encoded_id(), &mut identities);
        for parameter in declaration.input().parameters() {
            include_ancestors(parameter.binding().encoded_id(), &mut identities);
        }
        collect_value(declaration.result(), &mut identities);
    }
    let entries = identities
        .into_iter()
        .enumerate()
        .map(|(index, identity)| (identity, format!("fixture_name_{index}")))
        .collect();
    NomosNameTable::try_from_entries(entries).expect("complete unique fixture names")
}

fn collect_value(
    value: &TemplateValue<VocabularyRoot>,
    identities: &mut BTreeSet<VocabularyEncodedId>,
) {
    for field in value.fields() {
        collect_term(field.term(), identities);
    }
}

fn collect_term(
    term: &TemplateTerm<VocabularyRoot>,
    identities: &mut BTreeSet<VocabularyEncodedId>,
) {
    match term {
        TemplateTerm::Declaration(identity)
        | TemplateTerm::Reference(identity)
        | TemplateTerm::Literal(identity) => include_ancestors(identity, identities),
        TemplateTerm::Nested(value) => collect_value(value, identities),
        TemplateTerm::Sequence(values) => {
            for value in values {
                collect_term(value, identities);
            }
        }
        TemplateTerm::Future(TemplateFuture::Realize { binding, .. })
        | TemplateTerm::Future(TemplateFuture::Splice { binding }) => {
            include_ancestors(binding.encoded_id(), identities);
        }
        TemplateTerm::Future(TemplateFuture::Invoke(transformer)) => {
            include_ancestors(transformer.encoded_id(), identities);
        }
        TemplateTerm::Future(TemplateFuture::RecursiveInvoke { payload }) => {
            include_ancestors(payload.target().encoded_id(), identities);
            include_ancestors(payload.subject_binding().encoded_id(), identities);
            include_ancestors(payload.constructor_binding().encoded_id(), identities);
            include_ancestors(payload.children_binding().encoded_id(), identities);
        }
        TemplateTerm::Future(TemplateFuture::InsertAt { payload }) => {
            include_ancestors(payload.target().encoded_id(), identities);
        }
        TemplateTerm::Scalar(_) => {}
    }
}

fn include_ancestors(
    identity: &VocabularyEncodedId,
    identities: &mut BTreeSet<VocabularyEncodedId>,
) {
    for length in 1..=identity.chain().len() {
        identities.insert(
            VocabularyEncodedId::new(
                *identity.root_variant(),
                identity.chain()[..length].to_vec(),
            )
            .expect("non-empty ancestor"),
        );
    }
}
