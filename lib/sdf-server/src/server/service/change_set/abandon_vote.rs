use crate::server::extract::{AccessBuilder, HandlerContext, PosthogClient};
use crate::server::tracking::track;
use crate::service::change_set::{ChangeSetError, ChangeSetResult};
use axum::extract::{Host, OriginalUri};
use axum::Json;
use dal::{ChangeSet, Visibility};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AbandonVoteRequest {
    pub vote: String,
    #[serde(flatten)]
    pub visibility: Visibility,
}

pub async fn abandon_vote(
    OriginalUri(original_uri): OriginalUri,
    Host(host_name): Host,
    PosthogClient(posthog_client): PosthogClient,
    HandlerContext(builder): HandlerContext,
    AccessBuilder(request_ctx): AccessBuilder,
    Json(request): Json<AbandonVoteRequest>,
) -> ChangeSetResult<Json<()>> {
    let ctx = builder.build(request_ctx.build(request.visibility)).await?;

    let mut change_set = ChangeSet::find(&ctx, ctx.change_set_id())
        .await?
        .ok_or(ChangeSetError::ChangeSetNotFound)?;
    change_set.abandon_vote(&ctx, request.vote.clone()).await?;

    track(
        &posthog_client,
        &ctx,
        &original_uri,
        &host_name,
        "abandon_vote",
        serde_json::json!({
            "how": "/change_set/abandon_vote",
            "change_set_id": ctx.visibility().change_set_id,
            "vote": request.vote,
        }),
    );

    ctx.commit_no_rebase().await?;

    Ok(Json(()))
}
