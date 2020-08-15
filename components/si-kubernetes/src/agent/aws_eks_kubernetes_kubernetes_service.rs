use crate::{
    agent::agent_apply,
    gen::agent::{
        AwsEksKubernetesKubernetesServiceDispatchFunctions,
        AwsEksKubernetesKubernetesServiceDispatcherBuilder,
    },
    yaml_bytes,
};
use si_agent::prelude::*;
use si_cea::agent::prelude::*;

pub struct AwsEksKubernetesKubernetesServiceDispatchFunctionsImpl;

#[async_trait]
impl AwsEksKubernetesKubernetesServiceDispatchFunctions
    for AwsEksKubernetesKubernetesServiceDispatchFunctionsImpl
{
    async fn apply(
        transport: &si_agent::Transport,
        stream_header: si_agent::Header,
        entity_event: &mut crate::protobuf::KubernetesServiceEntityEvent,
    ) -> si_agent::AgentResult<()> {
        async {
            agent_apply(
                transport,
                stream_header,
                entity_event,
                yaml_bytes!(entity_event),
            )
            .await
        }
        .instrument(debug_span!("apply"))
        .await
    }

    async fn create(
        _transport: &si_agent::Transport,
        _stream_header: si_agent::Header,
        _entity_event: &mut crate::protobuf::KubernetesServiceEntityEvent,
    ) -> si_agent::AgentResult<()> {
        async { Ok(()) }.instrument(debug_span!("create")).await
    }

    async fn sync(
        _transport: &si_agent::Transport,
        _stream_header: si_agent::Header,
        _entity_event: &mut crate::protobuf::KubernetesServiceEntityEvent,
    ) -> si_agent::AgentResult<()> {
        async { Ok(()) }.instrument(debug_span!("sync")).await
    }
}

pub fn dispatcher_builder() -> AwsEksKubernetesKubernetesServiceDispatcherBuilder<
    AwsEksKubernetesKubernetesServiceDispatchFunctionsImpl,
> {
    AwsEksKubernetesKubernetesServiceDispatcherBuilder::new()
}
