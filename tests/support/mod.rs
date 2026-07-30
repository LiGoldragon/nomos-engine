use std::collections::BTreeSet;

use core_ethos::{
    WholeEthos, WholeEthosAttributes, WholeEthosEnumeration, WholeEthosItem, WholeEthosNewtype,
    WholeEthosTupleFields, WholeEthosTypeReference, WholeEthosVariant, WholeEthosVariantPayload,
    WholeEthosVisibility, WholeEthosWrappedField,
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
use nomos_engine::{
    EngineEthosNameTree, EngineNameRealization, EngineReferenceMapping, encode_ethos_population,
};
use signal_nomos::{EthosPopulationArchive, NomosDeploymentArtifacts};
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

pub struct NativeInput {
    pub archive: EthosPopulationArchive,
    pub names: EngineEthosNameTree,
    pub mapping_source: VocabularyEncodedId,
    pub mapped_reference: VocabularyEncodedId,
    pub preserved_reference: VocabularyEncodedId,
}

pub fn native_input() -> NativeInput {
    native_input_with_realization_parent(700)
}

pub fn alternate_native_input() -> NativeInput {
    native_input_with_realization_parent(701)
}

fn native_input_with_realization_parent(realization_parent: u16) -> NativeInput {
    let newtype_name = encoded(VocabularyRoot::Universal, &[500, 1]);
    let enumeration_name = encoded(VocabularyRoot::Universal, &[500, 2]);
    let variant_name = encoded(VocabularyRoot::Universal, &[500, 2, 1]);
    let realized_newtype = encoded(VocabularyRoot::Universal, &[realization_parent, 1]);
    let realized_enumeration = encoded(VocabularyRoot::Universal, &[realization_parent, 2]);
    let mapped_source = encoded(VocabularyRoot::Universal, &[600, 4, 1]);
    let preserved_source = encoded(VocabularyRoot::Universal, &[601, 4, 1]);
    let mapped_target = encoded(VocabularyRoot::Rust, &[800, 1]);

    let ethos = WholeEthos::new(vec![
        WholeEthosItem::Newtype(WholeEthosNewtype::new(
            newtype_name.clone(),
            WholeEthosVisibility::Public,
            WholeEthosAttributes::empty(),
            WholeEthosWrappedField::new(
                WholeEthosVisibility::Private,
                WholeEthosTypeReference::Identity(mapped_source.clone()),
            ),
        )),
        WholeEthosItem::Enumeration(WholeEthosEnumeration::new(
            enumeration_name.clone(),
            WholeEthosVisibility::Public,
            WholeEthosAttributes::empty(),
            vec![WholeEthosVariant::new(
                variant_name.clone(),
                WholeEthosAttributes::empty(),
                WholeEthosVariantPayload::Tuple(
                    WholeEthosTupleFields::new(vec![WholeEthosTypeReference::Identity(
                        preserved_source.clone(),
                    )])
                    .expect("non-empty tuple"),
                ),
            )],
        )),
    ]);
    let names = EngineEthosNameTree::try_new(
        vec![
            newtype_name.clone(),
            enumeration_name.clone(),
            variant_name.clone(),
        ],
        vec![
            EngineNameRealization::try_new(
                newtype_name,
                NameTransform::PascalCase,
                realized_newtype.clone(),
            )
            .expect("newtype realization"),
            EngineNameRealization::try_new(
                enumeration_name,
                NameTransform::PascalCase,
                realized_enumeration.clone(),
            )
            .expect("enumeration realization"),
        ],
        vec![
            EngineReferenceMapping::try_new(mapped_source.clone(), mapped_target.clone())
                .expect("exact reference mapping"),
        ],
        vec![realized_newtype, realized_enumeration, variant_name],
        vec![mapped_target.clone(), preserved_source.clone()],
    )
    .expect("complete authenticated native plan");
    let archive = encode_ethos_population(ethos, names.clone()).expect("valid Ethos population");
    NativeInput {
        archive,
        names,
        mapping_source: mapped_source,
        mapped_reference: mapped_target,
        preserved_reference: preserved_source,
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
