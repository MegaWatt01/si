#![recursion_limit = "256"]

use std::future::IntoFuture;
use std::path::PathBuf;

use color_eyre::Result;
use nats_multiplexer::Multiplexer;
use sdf_server::server::{LayerDb, CRDT_MULTIPLEXER_SUBJECT, WS_MULTIPLEXER_SUBJECT};
use sdf_server::{
    Config, FeatureFlagService, IncomingStream, JobProcessorClientCloser, JobProcessorConnector,
    MigrationMode, Server, ServicesContext,
};
use si_service::startup;
use telemetry_application::prelude::*;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

mod args;

type JobProcessor = sdf_server::NatsProcessor;

const RT_DEFAULT_THREAD_STACK_SIZE: usize = 2 * 1024 * 1024 * 10;

fn main() -> Result<()> {
    let thread_builder = ::std::thread::Builder::new().stack_size(RT_DEFAULT_THREAD_STACK_SIZE);
    let thread_handler = thread_builder.spawn(|| {
        tokio::runtime::Builder::new_multi_thread()
            .thread_stack_size(RT_DEFAULT_THREAD_STACK_SIZE)
            .thread_name("bin/sdf-tokio::runtime")
            .enable_all()
            .build()?
            .block_on(async_main())
    })?;
    thread_handler.join().unwrap()
}

async fn async_main() -> Result<()> {
    let layer_db_tracker = TaskTracker::new();
    let layer_db_token = CancellationToken::new();
    let billing_events_server_tracker = TaskTracker::new();
    let billing_events_server_token = CancellationToken::new();
    let telemetry_tracker = TaskTracker::new();
    let telemetry_token = CancellationToken::new();

    color_eyre::install()?;
    let args = args::parse();
    let (mut telemetry, telemetry_shutdown) = {
        let config = TelemetryConfig::builder()
            .force_color(args.force_color.then_some(true))
            .no_color(args.no_color.then_some(true))
            .console_log_format(
                args.log_json
                    .then_some(ConsoleLogFormat::Json)
                    .unwrap_or_default(),
            )
            .service_name("sdf")
            .service_namespace("si")
            .log_env_var_prefix("SI")
            .app_modules(vec!["sdf", "sdf_server"])
            .interesting_modules(vec!["dal", "si_data_nats", "si_data_pg", "si_layer_cache"])
            .build()?;

        telemetry_application::init(config, &telemetry_tracker, telemetry_token.clone())?
    };

    startup::startup("sdf").await?;

    if args.verbose > 0 {
        telemetry
            .set_verbosity_and_wait(args.verbose.into())
            .await?;
    }
    debug!(arguments =?args, "parsed cli arguments");

    Server::init()?;

    if let (Some(secret_key_path), Some(public_key_path)) = (
        &args.generate_veritech_secret_key_path,
        &args.generate_veritech_public_key_path,
    ) {
        info!(
            "Generating Veritech key pair at: (secret = {}, public = {})",
            secret_key_path.display(),
            public_key_path.display()
        );
        Server::generate_veritech_key_pair(secret_key_path, public_key_path).await?;
        return Ok(());
    }

    if let Some(symmetric_key_path) = &args.generate_symmetric_key_path {
        info!(
            "Generating Symmetric key at: {}",
            symmetric_key_path.display()
        );
        Server::generate_symmetric_key(symmetric_key_path).await?;
        return Ok(());
    }

    let config = Config::try_from(args)?;

    debug!(config =?config, "entire startup config");

    let encryption_key = Server::load_encryption_key(config.crypto().clone()).await?;
    let jwt_public_signing_key =
        Server::load_jwt_public_signing_key(config.jwt_signing_public_key().clone()).await?;

    let nats_conn = Server::connect_to_nats(config.nats()).await?;

    let nats_streams = Server::get_or_create_nats_streams(&nats_conn).await?;

    let (job_client, job_processor) = JobProcessor::connect(&config).await?;

    let pg_pool = Server::create_pg_pool(config.pg_pool()).await?;

    let veritech = Server::create_veritech_client(nats_conn.clone());

    let symmetric_crypto_service =
        Server::create_symmetric_crypto_service(config.symmetric_crypto_service()).await?;

    let pkgs_path: PathBuf = config.pkgs_path().into();

    let module_index_url = config.module_index_url().to_string();

    let (ws_multiplexer, ws_multiplexer_client) =
        Multiplexer::new(&nats_conn, WS_MULTIPLEXER_SUBJECT).await?;
    let (crdt_multiplexer, crdt_multiplexer_client) =
        Multiplexer::new(&nats_conn, CRDT_MULTIPLEXER_SUBJECT).await?;

    let compute_executor = Server::create_compute_executor()?;

    let (layer_db, layer_db_graceful_shutdown) = LayerDb::from_config(
        config.layer_db_config().clone(),
        compute_executor.clone(),
        layer_db_token.clone(),
    )
    .await?;
    layer_db_tracker.spawn(layer_db_graceful_shutdown.into_future());

    // TODO(nick): allow the ability to configure the delivery mechanism.
    let billing_events_server_future =
        billing_events_server::new(nats_conn.clone(), None, billing_events_server_token.clone())
            .await?;
    billing_events_server_tracker.spawn(billing_events_server_future);

    let feature_flags_service = FeatureFlagService::new(config.boot_feature_flags().clone());

    let services_context = ServicesContext::new(
        pg_pool,
        nats_conn,
        nats_streams,
        job_processor,
        veritech,
        encryption_key,
        Some(pkgs_path),
        Some(module_index_url),
        symmetric_crypto_service,
        layer_db,
        feature_flags_service,
        compute_executor,
    );

    if let MigrationMode::Run | MigrationMode::RunAndQuit = config.migration_mode() {
        Server::migrate_database(&services_context).await?;
        if let MigrationMode::RunAndQuit = config.migration_mode() {
            info!(
                "migration mode is {}, shutting down",
                config.migration_mode()
            );

            // TODO(fnichol): ensure that layer-db and telemetry are gracefully shut down
            for (tracker, token) in [
                (layer_db_tracker, layer_db_token),
                (billing_events_server_tracker, billing_events_server_token),
                (telemetry_tracker, telemetry_token),
            ] {
                info!("performing graceful shutdown for task group");
                tracker.close();
                token.cancel();
                tracker.wait().await;
            }

            // TODO(nick): we need to handle telemetry shutdown properly as well.
            telemetry_shutdown.wait().await?;

            info!("graceful shutdown complete.");
            return Ok(());
        }
    } else {
        trace!("migration mode is skip, not running migrations");
    }

    let posthog_client = Server::start_posthog(config.posthog()).await?;

    layer_db_tracker.close();
    billing_events_server_tracker.close();
    telemetry_tracker.close();

    match config.incoming_stream() {
        IncomingStream::HTTPSocket(_) => {
            let (server, initial_shutdown_broadcast_rx) = Server::http(
                config,
                services_context.clone(),
                jwt_public_signing_key,
                posthog_client,
                ws_multiplexer,
                ws_multiplexer_client,
                crdt_multiplexer,
                crdt_multiplexer_client,
            )?;
            let _second_shutdown_broadcast_rx = initial_shutdown_broadcast_rx.resubscribe();

            server.run().await?;
        }
        IncomingStream::UnixDomainSocket(_) => {
            let (server, initial_shutdown_broadcast_rx) = Server::uds(
                config,
                services_context.clone(),
                jwt_public_signing_key,
                posthog_client,
                ws_multiplexer,
                ws_multiplexer_client,
                crdt_multiplexer,
                crdt_multiplexer_client,
            )
            .await?;
            let _second_shutdown_broadcast_rx = initial_shutdown_broadcast_rx.resubscribe();

            server.run().await?;
        }
    }

    // TODO(fnichol): this will eventually go into the signal handler code but at the moment in
    // sdf's case, this is embedded in server library code which is incorrect. At this moment in
    // the program however, axum has shut down so it's an appropriate time to cancel other
    // remaining tasks and wait on their graceful shutdowns
    {
        // TODO(nick): Fletcher's comment above still stands, but now we shutdown for multiple task groups.
        for (tracker, token) in [
            (layer_db_tracker, layer_db_token),
            (billing_events_server_tracker, billing_events_server_token),
            (telemetry_tracker, telemetry_token),
        ] {
            info!("performing graceful shutdown for task group");
            tracker.close();
            token.cancel();
            tracker.wait().await;
        }

        // TODO(nick): we need to handle telemetry shutdown properly as well.
        telemetry_shutdown.wait().await?;
    }

    if let Err(err) = (&job_client as &dyn JobProcessorClientCloser).close().await {
        error!("Failed to close job client: {err}");
    }

    info!("graceful shutdown complete.");
    Ok(())
}
