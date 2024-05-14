//! This module contains helpers for use when authoring dal integration tests.

use async_recursion::async_recursion;
use dal::key_pair::KeyPairPk;
use dal::property_editor::schema::{
    PropertyEditorProp, PropertyEditorPropKind, PropertyEditorSchema,
};
use dal::property_editor::values::{PropertyEditorValue, PropertyEditorValues};
use dal::property_editor::{PropertyEditorPropId, PropertyEditorValueId};
use dal::{
    AttributeValue, Component, ComponentId, DalContext, InputSocket, KeyPair, OutputSocket, Prop,
    Schema, SchemaVariant, SchemaVariantId, User, UserClaim, UserPk,
};
use itertools::enumerate;
use jwt_simple::algorithms::RSAKeyPairLike;
use jwt_simple::{claims::Claims, reexports::coarsetime::Duration};
use names::{Generator, Name};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::jwt_private_signing_key;

mod change_set;

pub use change_set::ChangeSetTestHelpers;
pub use change_set::ChangeSetTestHelpersError;

/// Generates a fake name.
pub fn generate_fake_name() -> String {
    Generator::with_naming(Name::Numbered)
        .next()
        .expect("could not generate fake name")
}

/// Creates an auth token.
pub async fn create_auth_token(claim: UserClaim) -> String {
    let key_pair = jwt_private_signing_key()
        .await
        .expect("failed to load jwt private signing key");
    let claim = Claims::with_custom_claims(claim, Duration::from_days(1))
        .with_audience("https://app.systeminit.com")
        .with_issuer("https://app.systeminit.com")
        .with_subject(claim.user_pk);

    key_pair.sign(claim).expect("unable to sign jwt")
}

/// Creates a connection annotation string.
#[macro_export]
macro_rules! connection_annotation_string {
    ($str:expr) => {
        serde_json::to_string(&vec![$str]).expect("Unable to parse annotation string")
    };
}

/// Creates a dummy key pair.
pub async fn create_key_pair(ctx: &DalContext) -> KeyPair {
    let name = generate_fake_name();
    KeyPair::new(ctx, &name)
        .await
        .expect("cannot create key pair")
}

/// Creates a dummy user.
pub async fn create_user(ctx: &DalContext) -> User {
    let name = generate_fake_name();
    User::new(
        ctx,
        UserPk::generate(),
        &name,
        &format!("{name}@test.systeminit.com"),
        None::<&str>,
    )
    .await
    .expect("cannot create user")
}

/// Creates a dummy schema.
pub async fn create_schema(ctx: &DalContext) -> Schema {
    let name = generate_fake_name();
    Schema::new(ctx, &name).await.expect("cannot create schema")
}

/// Creates a [`Component`] from the default [`SchemaVariant`] corresponding to a provided
/// [`Schema`] name.
pub async fn create_component_for_schema_name(
    ctx: &DalContext,
    schema_name: impl AsRef<str>,
    name: impl AsRef<str>,
) -> Component {
    let schema = Schema::find_by_name(ctx, schema_name)
        .await
        .expect("could not find schema")
        .expect("schema not found");
    let schema_variant = SchemaVariant::list_for_schema(ctx, schema.id())
        .await
        .expect("failed listing schema variants")
        .pop()
        .expect("no schema variant found");

    Component::new(ctx, name.as_ref().to_string(), schema_variant.id())
        .await
        .expect("could not create component")
}

/// Creates a [`Component`] for a given [`SchemaVariantId`](SchemaVariant).
pub async fn create_component_for_schema_variant(
    ctx: &DalContext,
    schema_variant_id: SchemaVariantId,
) -> Component {
    let name = generate_fake_name();
    Component::new(ctx, &name, schema_variant_id)
        .await
        .expect("cannot create component")
}

/// Connects two [`Components`](Component) for a given set of socket names.
pub async fn connect_components_with_socket_names(
    ctx: &DalContext,
    source_component_id: ComponentId,
    output_socket_name: impl AsRef<str>,
    destination_component_id: ComponentId,
    input_socket_name: impl AsRef<str>,
) {
    let from_socket_id = {
        let sv_id = Component::schema_variant_id(ctx, source_component_id)
            .await
            .expect("find schema variant for source component");

        OutputSocket::find_with_name(ctx, output_socket_name, sv_id)
            .await
            .expect("perform find output socket")
            .expect("find output socket")
            .id()
    };

    let to_socket_id = {
        let sv_id = Component::schema_variant_id(ctx, destination_component_id)
            .await
            .expect("find schema variant for destination component");

        InputSocket::find_with_name(ctx, input_socket_name, sv_id)
            .await
            .expect("perform find input socket")
            .expect("find input socket")
            .id()
    };

    Component::connect(
        ctx,
        source_component_id,
        from_socket_id,
        destination_component_id,
        to_socket_id,
    )
    .await
    .expect("could not connect components");
}

/// Gets the [`Value`] for a specific [`Component`]'s [`InputSocket`] by the [`InputSocket`] name
pub async fn get_component_input_socket_value(
    ctx: &DalContext,
    component_id: ComponentId,
    input_socket_name: impl AsRef<str>,
) -> Option<serde_json::Value> {
    let schema_variant_id = Component::schema_variant_id(ctx, component_id)
        .await
        .expect("got schema variant id");
    let component = Component::get_by_id(ctx, component_id)
        .await
        .expect("got component");
    let component_input_sockets = component
        .input_socket_attribute_values(ctx)
        .await
        .expect("got attribute values");
    let input_socket = InputSocket::find_with_name(ctx, input_socket_name, schema_variant_id)
        .await
        .expect("got input socket")
        .expect("has value");
    let input_socket_match = component_input_sockets
        .get(&input_socket.id())
        .expect("got input socket av");
    let input_socket_av = AttributeValue::get_by_id(ctx, input_socket_match.attribute_value_id)
        .await
        .expect("got input av");
    input_socket_av.view(ctx).await.expect("got view")
}

/// Gets the [`Value`] for a specific [`Component`]'s [`OutputSocket`] by the [`OutputSocket`] name
pub async fn get_component_output_socket_value(
    ctx: &DalContext,
    component_id: ComponentId,
    output_socket_name: impl AsRef<str>,
) -> Option<serde_json::Value> {
    let schema_variant_id = Component::schema_variant_id(ctx, component_id)
        .await
        .expect("got schema variant id");
    let component = Component::get_by_id(ctx, component_id)
        .await
        .expect("got component");
    let component_output_sockets = component
        .output_socket_attribute_values(ctx)
        .await
        .expect("got attribute values");
    let output_socket = OutputSocket::find_with_name(ctx, output_socket_name, schema_variant_id)
        .await
        .expect("got output socket")
        .expect("has value");
    let output_socket_match = component_output_sockets
        .get(&output_socket.id())
        .expect("got output socket av");
    let output_socket_av = AttributeValue::get_by_id(ctx, output_socket_match.attribute_value_id)
        .await
        .expect("got output av");
    output_socket_av.view(ctx).await.expect("got view")
}
/// Update the [`Value`] for a specific [`AttributeValue`] for the given [`Component`](ComponentId) by the [`PropPath`]
pub async fn update_attribute_value_for_component(
    ctx: &DalContext,
    component_id: ComponentId,
    prop_path: &[&str],
    value: serde_json::Value,
) {
    let component = Component::get_by_id(ctx, component_id)
        .await
        .expect("got component");
    let attribute_value_id = component
        .attribute_values_for_prop(ctx, prop_path)
        .await
        .expect("found attribute values for prop")
        .into_iter()
        .next()
        .expect("got attribute value id");
    AttributeValue::update(ctx, attribute_value_id, Some(value))
        .await
        .expect("updated attribute value");
}

/// Encrypts a message with a given [`KeyPairPk`](KeyPair).
pub async fn encrypt_message(
    ctx: &DalContext,
    key_pair_pk: KeyPairPk,
    message: &serde_json::Value,
) -> Vec<u8> {
    let public_key = KeyPair::get_by_pk(ctx, key_pair_pk)
        .await
        .expect("failed to fetch key pair");

    let crypted = sodiumoxide::crypto::sealedbox::seal(
        &serde_json::to_vec(message).expect("failed to serialize message"),
        public_key.public_key(),
    );
    crypted
}

#[allow(missing_docs)]
#[derive(Serialize, Deserialize, Debug)]
pub struct PropEditorTestView {
    pub prop: PropertyEditorProp,
    pub value: PropertyEditorValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<HashMap<String, PropEditorTestView>>,
}

impl PropEditorTestView {
    fn get_view(&self, prop_path: &[&str]) -> Value {
        let mut value = serde_json::to_value(self).expect("convert UnifiedViewItem to json Value");

        // "root" is necessary for compatibility with other prop apis, but we skip it here
        for &prop_name in prop_path.iter().skip(1) {
            value = value
                .get("children")
                .expect("get children entry of PropEditorView")
                .get(prop_name)
                .expect("get child entry of PropEditorView")
                .clone();
        }

        value
    }

    /// Gets the "value" for a given [`Prop`](dal::Prop) path.
    pub fn get_value(&self, prop_path: &[&str]) -> Value {
        let view = self.get_view(prop_path);
        view.get("value").expect("get prop field of view").clone()
    }

    /// Generates a [`PropEditorTestView`] for a given [`ComponentId`](Component).
    pub async fn for_component_id(ctx: &DalContext, component_id: ComponentId) -> Self {
        let sv_id = Component::schema_variant_id(ctx, component_id)
            .await
            .expect("get schema variant from component");

        let PropertyEditorValues {
            root_value_id,
            values,
            child_values,
        } = PropertyEditorValues::assemble(ctx, component_id)
            .await
            .expect("assemble property editor values");

        let PropertyEditorSchema { props, .. } = PropertyEditorSchema::assemble(ctx, sv_id)
            .await
            .expect("assemble property editor schema");

        let root_view = {
            let value = values
                .get(&root_value_id)
                .expect("get property editor root value")
                .clone();

            let prop = props.get(&value.prop_id).expect("get property editor prop");

            Self {
                prop: prop.clone(),
                value,
                children: Self::property_editor_compile_children(
                    ctx,
                    root_value_id,
                    &prop.kind,
                    &values,
                    &child_values,
                    &props,
                )
                .await,
            }
        };

        root_view
    }

    #[async_recursion]
    async fn property_editor_compile_children(
        ctx: &DalContext,
        parent_value_id: PropertyEditorValueId,
        parent_prop_kind: &PropertyEditorPropKind,
        values: &HashMap<PropertyEditorValueId, PropertyEditorValue>,
        child_values: &HashMap<PropertyEditorValueId, Vec<PropertyEditorValueId>>,
        props: &HashMap<PropertyEditorPropId, PropertyEditorProp>,
    ) -> Option<HashMap<String, PropEditorTestView>> {
        let mut children = HashMap::new();

        for (index, child_id) in enumerate(
            child_values
                .get(&parent_value_id)
                .expect("get prop editor value children"),
        ) {
            let value = values
                .get(child_id)
                .expect("get property editor root value")
                .clone();
            let real_prop = Prop::get_by_id_or_error(ctx, value.prop_id.into_inner().into())
                .await
                .expect("prop should exist");
            if real_prop.hidden {
                continue;
            }

            let prop = props.get(&value.prop_id).expect("get property editor prop");

            let key = match parent_prop_kind {
                PropertyEditorPropKind::Array => index.to_string(),
                PropertyEditorPropKind::Map => value.key.clone().unwrap_or("ERROR".to_string()),
                _ => prop.name.clone(),
            };

            let child = PropEditorTestView {
                prop: prop.clone(),
                value,
                children: Self::property_editor_compile_children(
                    ctx,
                    *child_id,
                    &prop.kind,
                    values,
                    child_values,
                    props,
                )
                .await,
            };

            children.insert(key, child);
        }

        if children.is_empty() {
            None
        } else {
            Some(children)
        }
    }
}
