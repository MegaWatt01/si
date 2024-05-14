use axum::{response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use dal::{
    diagram::SummaryDiagramComponent, AttributeValue, AttributeValueId, ChangeSet, Component,
    ComponentId, PropId, Visibility, WsEvent,
};

use crate::server::extract::{AccessBuilder, HandlerContext};

use super::ComponentResult;

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InsertPropertyEditorValueRequest {
    pub parent_attribute_value_id: AttributeValueId,
    pub prop_id: PropId,
    pub component_id: ComponentId,
    pub value: Option<serde_json::Value>,
    pub key: Option<String>,
    #[serde(flatten)]
    pub visibility: Visibility,
}

pub async fn insert_property_editor_value(
    HandlerContext(builder): HandlerContext,
    AccessBuilder(request_ctx): AccessBuilder,
    Json(request): Json<InsertPropertyEditorValueRequest>,
) -> ComponentResult<impl IntoResponse> {
    let mut ctx = builder.build(request_ctx.build(request.visibility)).await?;

    let force_change_set_id = ChangeSet::force_new(&mut ctx).await?;

    let _ = AttributeValue::insert(
        &ctx,
        request.parent_attribute_value_id,
        request.value,
        request.key,
    )
    .await?;

    let component: Component = Component::get_by_id(&ctx, request.component_id).await?;
    let payload: SummaryDiagramComponent =
        SummaryDiagramComponent::assemble(&ctx, &component).await?;
    WsEvent::component_updated(&ctx, payload)
        .await?
        .publish_on_commit(&ctx)
        .await?;

    ctx.commit().await?;

    let mut response = axum::response::Response::builder();
    if let Some(force_change_set_id) = force_change_set_id {
        response = response.header("force_change_set_id", force_change_set_id.to_string());
    }
    Ok(response.body(axum::body::Empty::new())?)
}
