use axum::extract::OriginalUri;
use axum::{response::IntoResponse, Json};
use dal::diagram::SummaryDiagramComponent;
use serde::{Deserialize, Serialize};

use dal::component::frame::Frame;
use dal::{ChangeSet, Component, ComponentId, Visibility, WsEvent};

use crate::server::extract::{AccessBuilder, HandlerContext, PosthogClient};
use crate::server::tracking::track;

use super::DiagramResult;

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FrameConnection {
    pub child_id: ComponentId,
    pub parent_id: ComponentId,
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CreateFrameConnectionRequest {
    pub connections: Vec<FrameConnection>,
    #[serde(flatten)]
    pub visibility: Visibility,
}

/// Connect a child [`Component`](dal::Component) to a parent [`Component`](dal::Component).
/// detaching any existing parents first and creating a change set if on head.
pub async fn connect_component_to_frame(
    HandlerContext(builder): HandlerContext,
    AccessBuilder(request_ctx): AccessBuilder,
    PosthogClient(posthog_client): PosthogClient,
    OriginalUri(original_uri): OriginalUri,
    Json(request): Json<CreateFrameConnectionRequest>,
) -> DiagramResult<impl IntoResponse> {
    let mut ctx = builder.build(request_ctx.build(request.visibility)).await?;
    let force_change_set_id = ChangeSet::force_new(&mut ctx).await?;

    let connections = serde_json::json!(&request.connections);

    // Connect children to parent through frame edge
    for connection in request.connections {
        Frame::upsert_parent(&ctx, connection.child_id, connection.parent_id).await?;

        let component: Component = Component::get_by_id(&ctx, connection.child_id).await?;
        let payload: SummaryDiagramComponent =
            SummaryDiagramComponent::assemble(&ctx, &component).await?;
        WsEvent::component_updated(&ctx, payload)
            .await?
            .publish_on_commit(&ctx)
            .await?;
    }

    track(
        &posthog_client,
        &ctx,
        &original_uri,
        "connect_component_to_frame",
        serde_json::json!({
            "how": "/diagram/connect_component_to_frame",
            "connections": connections,
            "change_set_id": ctx.change_set_id(),
        }),
    );

    ctx.commit().await?;

    let mut response = axum::response::Response::builder();
    if let Some(force_change_set_id) = force_change_set_id {
        response = response.header("force_change_set_id", force_change_set_id.to_string());
    }
    Ok(response
        .header("content-type", "application/json")
        .body("{}".to_owned())?)
}
