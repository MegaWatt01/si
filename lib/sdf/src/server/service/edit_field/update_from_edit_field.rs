use axum::Json;
use dal::{
    edit_field::{EditFieldAble, EditFieldBaggage, EditFieldObjectKind},
    schema::{self, SchemaVariant},
    socket::Socket,
    Component, Prop, QualificationCheck, Schema, Visibility, WorkspaceId,
};
use serde::{Deserialize, Serialize};

use super::{EditFieldError, EditFieldResult};
use crate::server::extract::{AccessBuilder, HandlerContext};

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateFromEditFieldRequest {
    pub object_kind: EditFieldObjectKind,
    pub object_id: i64,
    pub edit_field_id: String,
    pub value: Option<serde_json::Value>,
    pub baggage: Option<EditFieldBaggage>,
    pub workspace_id: Option<WorkspaceId>,
    #[serde(flatten)]
    pub visibility: Visibility,
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateFromEditFieldResponse {
    pub success: bool,
}

/// # Panics
pub async fn update_from_edit_field(
    HandlerContext(builder, mut txns): HandlerContext,
    AccessBuilder(request_ctx): AccessBuilder,
    Json(request): Json<UpdateFromEditFieldRequest>,
) -> EditFieldResult<Json<UpdateFromEditFieldResponse>> {
    let txns = txns.start().await?;
    let ctx = builder.build(request_ctx.build(request.visibility), &txns);

    match request.object_kind {
        EditFieldObjectKind::Component => {
            Component::update_from_edit_field(
                ctx.pg_txn(),
                ctx.nats_txn(),
                ctx.veritech().clone(),
                ctx.encryption_key(),
                ctx.write_tenancy(),
                ctx.visibility(),
                ctx.history_actor(),
                request.object_id.into(),
                request.edit_field_id,
                request.value,
            )
            .await?
        }
        EditFieldObjectKind::ComponentProp => {
            // Eventually, this won't be infallible. -- Adam
            #[allow(clippy::infallible_destructuring_match)]
            let _baggage = request.baggage.ok_or(EditFieldError::MissingBaggage)?;
            todo!()
            // FIXME(nick): need to figure out prop id;
            //
            //Component::update_prop_from_edit_field(
            //    ctx.pg_txn(),
            //    ctx.nats_txn(),
            //    ctx.veritech().clone(),
            //    ctx.encryption_key(),
            //    ctx.write_tenancy(),
            //    ctx.visibility(),
            //    ctx.history_actor(),
            //    request.object_id.into(),
            //    baggage.attribute_context.prop_id(),
            //    request.edit_field_id,
            //    request.value,
            //    None, // TODO: Eventually, pass the key! -- Adam
            //)
            //.await?
        }
        EditFieldObjectKind::Prop => {
            Prop::update_from_edit_field(
                ctx.pg_txn(),
                ctx.nats_txn(),
                ctx.veritech().clone(),
                ctx.encryption_key(),
                ctx.write_tenancy(),
                ctx.visibility(),
                ctx.history_actor(),
                request.object_id.into(),
                request.edit_field_id,
                request.value,
            )
            .await?
        }
        EditFieldObjectKind::QualificationCheck => {
            QualificationCheck::update_from_edit_field(
                ctx.pg_txn(),
                ctx.nats_txn(),
                ctx.veritech().clone(),
                ctx.encryption_key(),
                ctx.write_tenancy(),
                ctx.visibility(),
                ctx.history_actor(),
                request.object_id.into(),
                request.edit_field_id,
                request.value,
            )
            .await?
        }
        EditFieldObjectKind::Schema => {
            Schema::update_from_edit_field(
                ctx.pg_txn(),
                ctx.nats_txn(),
                ctx.veritech().clone(),
                ctx.encryption_key(),
                ctx.write_tenancy(),
                ctx.visibility(),
                ctx.history_actor(),
                request.object_id.into(),
                request.edit_field_id,
                request.value,
            )
            .await?
        }
        EditFieldObjectKind::SchemaUiMenu => {
            schema::UiMenu::update_from_edit_field(
                ctx.pg_txn(),
                ctx.nats_txn(),
                ctx.veritech().clone(),
                ctx.encryption_key(),
                ctx.write_tenancy(),
                ctx.visibility(),
                ctx.history_actor(),
                request.object_id.into(),
                request.edit_field_id,
                request.value,
            )
            .await?
        }
        EditFieldObjectKind::SchemaVariant => {
            SchemaVariant::update_from_edit_field(
                ctx.pg_txn(),
                ctx.nats_txn(),
                ctx.veritech().clone(),
                ctx.encryption_key(),
                ctx.write_tenancy(),
                ctx.visibility(),
                ctx.history_actor(),
                request.object_id.into(),
                request.edit_field_id,
                request.value,
            )
            .await?
        }
        EditFieldObjectKind::Socket => {
            Socket::update_from_edit_field(
                ctx.pg_txn(),
                ctx.nats_txn(),
                ctx.veritech().clone(),
                ctx.encryption_key(),
                ctx.write_tenancy(),
                ctx.visibility(),
                ctx.history_actor(),
                request.object_id.into(),
                request.edit_field_id,
                request.value,
            )
            .await?
        }
    };

    txns.commit().await?;

    Ok(Json(UpdateFromEditFieldResponse { success: true }))
}
