use async_trait::async_trait;
use futures::StreamExt;
use pinga_core::{pinga_work_queue, subject::pinga_job, REPLY_INBOX_HEADER_NAME};
use si_data_nats::{jetstream, NatsClient, Subject};
use telemetry::prelude::*;
use telemetry_nats::propagation;
use tokio::task::JoinSet;

use crate::job::{
    consumer::JobInfo,
    producer::{BlockingJobError, BlockingJobResult, JobProducer, JobProducerError},
    queue::JobQueue,
};

use super::{JobQueueProcessor, JobQueueProcessorError, JobQueueProcessorResult};

#[derive(Clone, Debug)]
pub struct NatsProcessor {
    client: NatsClient,
    context: jetstream::Context,
    prefix: Option<String>,
}

impl NatsProcessor {
    pub fn new(client: NatsClient) -> Self {
        // Take the *active* subject prefix from the connected NATS client
        let prefix = client.metadata().subject_prefix().map(|s| s.to_owned());
        let context = jetstream::new(client.clone());

        Self {
            client,
            context,
            prefix,
        }
    }

    #[instrument(
        name = "nats_processor.push_all_jobs",
        level = "debug",
        skip_all,
        fields()
    )]
    async fn push_all_jobs(&self, queue: JobQueue) -> JobQueueProcessorResult<()> {
        // Ensure the Jetstream `Stream` is created before publishing to it
        let _stream = pinga_work_queue(&self.context, self.prefix.as_deref()).await?;

        let headers = propagation::empty_injected_headers();

        while let Some(element) = queue.fetch_job().await {
            let job_info = JobInfo::new(element)?;

            let workspace_pk = job_info
                .access_builder
                .tenancy()
                .workspace_pk_opt()
                .ok_or(JobQueueProcessorError::MissingWorkspacePk)?;

            let subject = pinga_job(
                self.prefix.as_deref(),
                &String::from(workspace_pk),
                &String::from(job_info.visibility.change_set_id),
                &job_info.kind,
            );

            self.context
                .publish_with_headers(
                    subject,
                    headers.clone(),
                    serde_json::to_vec(&job_info)?.into(),
                )
                .await
                // If `Err` then message failed to publish
                .map_err(|err| JobQueueProcessorError::Transport(Box::new(err)))?
                .await
                // If `Err` then NATS server failed to ack
                .map_err(|err| JobQueueProcessorError::Transport(Box::new(err)))?;
        }
        Ok(())
    }
}

#[async_trait]
impl JobQueueProcessor for NatsProcessor {
    async fn block_on_job(&self, job: Box<dyn JobProducer + Send + Sync>) -> BlockingJobResult {
        // Ensure the Jetstream `Stream` is created before publishing to it
        let _stream = pinga_work_queue(&self.context, self.prefix.as_deref())
            .await
            .map_err(|err| BlockingJobError::JsCreateStreamError(err.to_string()))?;

        let job_info = JobInfo::new_blocking(job)
            .map_err(|e: JobProducerError| BlockingJobError::JobProducer(e.to_string()))?;

        let reply_inbox = Subject::from(self.client.new_inbox());

        let mut headers = propagation::empty_injected_headers();
        headers.insert(REPLY_INBOX_HEADER_NAME, reply_inbox.to_string());

        let mut reply_subscriber = self
            .client
            .subscribe(reply_inbox.clone())
            .await
            .map_err(|e| BlockingJobError::Nats(e.to_string()))?;

        let workspace_pk = job_info
            .access_builder
            .tenancy()
            .workspace_pk_opt()
            .ok_or(BlockingJobError::MissingWorkspacePk)?;

        let subject = pinga_job(
            self.prefix.as_deref(),
            &String::from(workspace_pk),
            &String::from(job_info.visibility.change_set_id),
            &job_info.kind,
        );

        self.context
            .publish_with_headers(
                subject,
                headers,
                serde_json::to_vec(&job_info)
                    .map_err(|e| BlockingJobError::Serde(e.to_string()))?
                    .into(),
            )
            .await
            // If `Err` then message failed to publish
            .map_err(|e| BlockingJobError::Nats(e.to_string()))?
            .await
            // If `Err` then NATS server failed to ack
            .map_err(|e| BlockingJobError::Nats(e.to_string()))?;

        // TODO(fnichol): hrm, no timeout, so we wait forever? That's probably not expected?
        match reply_subscriber.next().await {
            Some(message) => {
                propagation::associate_current_span_from_headers(message.headers());
                serde_json::from_slice::<BlockingJobResult>(message.payload())
                    .map_err(|e| BlockingJobError::Serde(e.to_string()))?
            }
            None => Err(BlockingJobError::Nats(
                "Subscriber or connection no longer valid".to_string(),
            )),
        }
    }

    async fn block_on_jobs(
        &self,
        jobs: Vec<Box<dyn JobProducer + Send + Sync>>,
    ) -> BlockingJobResult {
        let span = Span::current();

        let mut dispatched_jobs = JoinSet::new();

        // Fan out, dispatching all queued jobs to pinga over nats.
        for job in jobs {
            let job_processor = Self::new(self.client.clone());
            let parent_span = span.clone();

            dispatched_jobs.spawn(async move {
                job_processor
                    .block_on_job(job)
                    .instrument(info_span!(parent: parent_span, "job_processor.block_on_job"))
                    .await
            });
        }

        let mut results = Vec::new();
        // Wait for all queued jobs to finish (regardless of success), before exiting.
        loop {
            match dispatched_jobs.join_next().await {
                // All jobs done.
                None => break,
                Some(Ok(Ok(_))) => { /* Nothing to do. Job succeeded. */ }
                Some(Ok(Err(job_error))) => {
                    results.push(job_error);
                }
                Some(Err(join_err)) => {
                    results.push(BlockingJobError::JobExecution(join_err.to_string()));
                }
            }
        }

        if !results.is_empty() {
            Err(BlockingJobError::JobExecution(
                results
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<String>>()
                    .join("\n"),
            ))
        } else {
            Ok(())
        }
    }

    #[instrument(
        name = "nats_processor.process_queue",
        level = "info",
        skip_all,
        fields(
            queue.size = Empty,
        )
    )]
    async fn process_queue(&self, queue: JobQueue) -> JobQueueProcessorResult<()> {
        let span = current_span_for_instrument_at!("info");

        span.record("queue.size", queue.size().await);

        self.push_all_jobs(queue).await?;

        Ok(())
    }

    #[instrument(
        name = "nats_processor.blocking_process_queue",
        level = "info",
        skip_all,
        fields(
            queue.size = Empty,
        )
    )]
    async fn blocking_process_queue(&self, queue: JobQueue) -> JobQueueProcessorResult<()> {
        let span = current_span_for_instrument_at!("info");

        span.record("queue.size", queue.size().await);

        let mut jobs = Vec::with_capacity(queue.size().await);
        while let Some(element) = queue.fetch_job().await {
            jobs.push(element);
        }
        self.block_on_jobs(jobs)
            .instrument(info_span!("nats_processor.block_on_jobs"))
            .await?;

        Ok(())
    }
}
