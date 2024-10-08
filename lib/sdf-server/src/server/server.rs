use axum::routing::IntoMakeService;
use axum::Router;
use dal::context::NatsStreams;
use dal::jwt_key::JwtConfig;
use dal::pkg::PkgError;
use dal::workspace_snapshot::migrator::{SnapshotGraphMigrator, SnapshotGraphMigratorError};
use dal::{
    builtins, BuiltinsError, DalContext, JwtPublicSigningKey, TransactionsError, Workspace,
    WorkspaceError,
};
use dal::{DedicatedExecutor, ServicesContext};
use hyper::server::{accept::Accept, conn::AddrIncoming};
use module_index_client::{BuiltinsDetailsResponse, ModuleDetailsResponse, ModuleIndexClient};
use nats_multiplexer::Multiplexer;
use nats_multiplexer_client::MultiplexerClient;
use si_crypto::{
    SymmetricCryptoError, SymmetricCryptoService, SymmetricCryptoServiceConfig,
    VeritechCryptoConfig, VeritechEncryptionKey, VeritechEncryptionKeyError, VeritechKeyPairError,
};
use si_data_nats::{NatsClient, NatsConfig, NatsError};
use si_data_pg::{PgError, PgPool, PgPoolConfig, PgPoolError};
use si_pkg::{SiPkg, SiPkgError};
use si_posthog::{PosthogClient, PosthogConfig};
use std::sync::Arc;
use std::time::Duration;
use std::{io, net::SocketAddr, path::Path, path::PathBuf};
use telemetry::prelude::*;
use telemetry_http::{HttpMakeSpan, HttpOnResponse};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    signal,
    sync::{broadcast, mpsc, oneshot, RwLock},
    task::{JoinError, JoinSet},
    time,
    time::Instant,
};
use tower_http::trace::TraceLayer;
use ulid::Ulid;
use veritech_client::Client as VeritechClient;

use super::state::{AppState, ApplicationRuntimeMode};
use super::{
    routes, Config, IncomingStream, UdsIncomingStream, UdsIncomingStreamError,
    WorkspacePermissions, WorkspacePermissionsMode,
};
use crate::server::config::VeritechKeyPair;

#[remain::sorted]
#[derive(Debug, Error)]
pub enum ServerError {
    #[error("intrinsics installation error: {0}")]
    Builtins(#[from] BuiltinsError),
    #[error(transparent)]
    DalInitialization(#[from] dal::InitializationError),
    #[error("compute executor initialization error: {0}")]
    DedicatedExecutorInitialize(#[from] dal::DedicatedExecutorInitializeError),
    #[error("error when loading veritech encryption key: {0}")]
    EncryptionKey(#[from] VeritechEncryptionKeyError),
    #[error("hyper server error")]
    Hyper(#[from] hyper::Error),
    #[error("error initializing the server")]
    Init,
    #[error(transparent)]
    Join(#[from] JoinError),
    #[error("jwt secret key error")]
    JwtSecretKey(#[from] dal::jwt_key::JwtKeyError),
    #[error("layer db error: {0}")]
    LayerDb(#[from] si_layer_cache::LayerDbError),
    #[error(transparent)]
    Model(#[from] dal::ModelError),
    #[error("Module index: {0}")]
    ModuleIndex(#[from] module_index_client::ModuleIndexClientError),
    #[error("Module index url not set")]
    ModuleIndexNotSet,
    #[error(transparent)]
    Nats(#[from] NatsError),
    #[error(transparent)]
    Pg(#[from] PgError),
    #[error(transparent)]
    PgPool(#[from] Box<PgPoolError>),
    #[error("failed to install package")]
    PkgInstall,
    #[error(transparent)]
    Posthog(#[from] si_posthog::PosthogError),
    #[error("failed to setup signal handler")]
    Signal(#[source] io::Error),
    #[error(transparent)]
    SiPkg(#[from] SiPkgError),
    #[error("snapshot migrator error: {0}")]
    SnapshotGraphMigrator(#[from] SnapshotGraphMigratorError),
    #[error(transparent)]
    SymmetricCryptoService(#[from] SymmetricCryptoError),
    #[error("transactions error: {0}")]
    Transactions(#[from] TransactionsError),
    #[error(transparent)]
    Uds(#[from] UdsIncomingStreamError),
    #[error("Unable to parse URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("veritech public key already set")]
    VeritechPublicKeyAlreadySet,
    #[error("veritech public key error: {0}")]
    VeritechPublicKeyErr(#[from] VeritechKeyPairError),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    #[error("wrong incoming stream for {0} server: {1:?}")]
    WrongIncomingStream(&'static str, IncomingStream),
}

impl From<PgPoolError> for ServerError {
    fn from(value: PgPoolError) -> Self {
        Self::PgPool(Box::new(value))
    }
}

pub type ServerResult<T, E = ServerError> = std::result::Result<T, E>;

pub struct Server<I, S> {
    config: Config,
    inner: axum::Server<I, IntoMakeService<Router>>,
    socket: S,
    shutdown_rx: oneshot::Receiver<()>,
}

impl Server<(), ()> {
    #[allow(clippy::too_many_arguments)]
    pub fn http(
        config: Config,
        services_context: ServicesContext,
        jwt_public_signing_key: JwtPublicSigningKey,
        posthog_client: PosthogClient,
        ws_multiplexer: Multiplexer,
        ws_multiplexer_client: MultiplexerClient,
        crdt_multiplexer: Multiplexer,
        crdt_multiplexer_client: MultiplexerClient,
    ) -> ServerResult<(Server<AddrIncoming, SocketAddr>, broadcast::Receiver<()>)> {
        match config.incoming_stream() {
            IncomingStream::HTTPSocket(socket_addr) => {
                let (service, shutdown_rx, shutdown_broadcast_rx) = build_service(
                    services_context,
                    jwt_public_signing_key,
                    posthog_client,
                    config.auth_api_url(),
                    ws_multiplexer_client,
                    crdt_multiplexer_client,
                    *config.create_workspace_permissions(),
                    config.create_workspace_allowlist().to_vec(),
                )?;

                tokio::spawn(ws_multiplexer.run(shutdown_broadcast_rx.resubscribe()));
                tokio::spawn(crdt_multiplexer.run(shutdown_broadcast_rx.resubscribe()));

                info!("binding to HTTP socket; socket_addr={}", &socket_addr);
                let inner = axum::Server::bind(socket_addr).serve(service.into_make_service());
                let socket = inner.local_addr();

                Ok((
                    Server {
                        config,
                        inner,
                        socket,
                        shutdown_rx,
                    },
                    shutdown_broadcast_rx,
                ))
            }
            wrong @ IncomingStream::UnixDomainSocket(_) => {
                Err(ServerError::WrongIncomingStream("http", wrong.clone()))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn uds(
        config: Config,
        services_context: ServicesContext,
        jwt_public_signing_key: JwtPublicSigningKey,
        posthog_client: PosthogClient,
        ws_multiplexer: Multiplexer,
        ws_multiplexer_client: MultiplexerClient,
        crdt_multiplexer: Multiplexer,
        crdt_multiplexer_client: MultiplexerClient,
    ) -> ServerResult<(Server<UdsIncomingStream, PathBuf>, broadcast::Receiver<()>)> {
        match config.incoming_stream() {
            IncomingStream::UnixDomainSocket(path) => {
                let (service, shutdown_rx, shutdown_broadcast_rx) = build_service(
                    services_context,
                    jwt_public_signing_key,
                    posthog_client,
                    config.auth_api_url(),
                    ws_multiplexer_client,
                    crdt_multiplexer_client,
                    *config.create_workspace_permissions(),
                    config.create_workspace_allowlist().to_vec(),
                )?;

                tokio::spawn(ws_multiplexer.run(shutdown_broadcast_rx.resubscribe()));
                tokio::spawn(crdt_multiplexer.run(shutdown_broadcast_rx.resubscribe()));

                info!("binding to Unix domain socket; path={}", path.display());
                let inner = axum::Server::builder(UdsIncomingStream::create(path).await?)
                    .serve(service.into_make_service());
                let socket = path.clone();

                Ok((
                    Server {
                        config,
                        inner,
                        socket,
                        shutdown_rx,
                    },
                    shutdown_broadcast_rx,
                ))
            }
            wrong @ IncomingStream::HTTPSocket(_) => {
                Err(ServerError::WrongIncomingStream("http", wrong.clone()))
            }
        }
    }

    pub fn init() -> ServerResult<()> {
        Ok(dal::init()?)
    }

    pub async fn start_posthog(config: &PosthogConfig) -> ServerResult<PosthogClient> {
        let (posthog_client, posthog_sender) = si_posthog::from_config(config)?;

        drop(tokio::spawn(posthog_sender.run()));

        Ok(posthog_client)
    }

    #[instrument(name = "sdf.init.generate_veritech_key_pair", level = "info", skip_all)]
    pub async fn generate_veritech_key_pair(
        secret_key_path: impl AsRef<Path>,
        public_key_path: impl AsRef<Path>,
    ) -> ServerResult<()> {
        VeritechKeyPair::create_and_write_files(secret_key_path, public_key_path)
            .await
            .map_err(Into::into)
    }

    #[instrument(name = "sdf.init.generate_symmetric_key", level = "info", skip_all)]
    pub async fn generate_symmetric_key(symmetric_key_path: impl AsRef<Path>) -> ServerResult<()> {
        SymmetricCryptoService::generate_key()
            .save(symmetric_key_path.as_ref())
            .await
            .map_err(Into::into)
    }

    #[instrument(
        name = "sdf.init.load_jwt_public_signing_key",
        level = "info",
        skip_all
    )]
    pub async fn load_jwt_public_signing_key(
        config: JwtConfig,
    ) -> ServerResult<JwtPublicSigningKey> {
        Ok(JwtPublicSigningKey::from_config(config).await?)
    }

    #[instrument(
        name = "sdf.init.decode_jwt_public_signing_key",
        level = "info",
        skip_all
    )]
    pub async fn decode_jwt_public_signing_key(
        key_string: String,
    ) -> ServerResult<JwtPublicSigningKey> {
        Ok(JwtPublicSigningKey::decode(key_string).await?)
    }

    #[instrument(name = "sdf.init.load_encryption_key", level = "info", skip_all)]
    pub async fn load_encryption_key(
        crypto_config: VeritechCryptoConfig,
    ) -> ServerResult<Arc<VeritechEncryptionKey>> {
        Ok(Arc::new(
            VeritechEncryptionKey::from_config(crypto_config).await?,
        ))
    }

    pub async fn migrate_snapshots(services_context: &ServicesContext) -> ServerResult<()> {
        let dal_context = services_context.clone().into_builder(true);
        let ctx = dal_context.build_default().await?;

        let mut migrator = SnapshotGraphMigrator::new();
        migrator.migrate_all(&ctx).await?;
        ctx.commit_no_rebase().await?;

        Ok(())
    }

    #[instrument(name = "sdf.init.migrate_database", level = "info", skip_all)]
    pub async fn migrate_database(services_context: &ServicesContext) -> ServerResult<()> {
        services_context.layer_db().pg_migrate().await?;
        dal::migrate_all_with_progress(services_context).await?;

        Self::migrate_snapshots(services_context).await?;

        migrate_builtins_from_module_index(services_context).await?;
        Ok(())
    }

    #[instrument(name = "sdf.init.create_pg_pool", level = "info", skip_all)]
    pub async fn create_pg_pool(pg_pool_config: &PgPoolConfig) -> ServerResult<PgPool> {
        let pool = PgPool::new(pg_pool_config).await?;
        debug!("successfully started pg pool (note that not all connections may be healthy)");
        Ok(pool)
    }

    #[instrument(name = "sdf.init.connect_to_nats", level = "info", skip_all)]
    pub async fn connect_to_nats(nats_config: &NatsConfig) -> ServerResult<NatsClient> {
        let client = NatsClient::new(nats_config).await?;
        debug!("successfully connected nats client");
        Ok(client)
    }

    #[instrument(name = "sdf.init.get_or_create_nats_streams", level = "info", skip_all)]
    pub async fn get_or_create_nats_streams(client: &NatsClient) -> ServerResult<NatsStreams> {
        let streams = NatsStreams::get_or_create(client).await?;
        debug!("successfully connected nats streams");
        Ok(streams)
    }

    pub fn create_veritech_client(nats: NatsClient) -> VeritechClient {
        VeritechClient::new(nats)
    }

    #[instrument(
        name = "sdf.init.create_symmetric_crypto_service",
        level = "info",
        skip_all
    )]
    pub async fn create_symmetric_crypto_service(
        config: &SymmetricCryptoServiceConfig,
    ) -> ServerResult<SymmetricCryptoService> {
        SymmetricCryptoService::from_config(config)
            .await
            .map_err(Into::into)
    }

    #[instrument(name = "sdf.init.create_compute_executor", level = "info", skip_all)]
    pub fn create_compute_executor() -> ServerResult<DedicatedExecutor> {
        dal::compute_executor("sdf").map_err(Into::into)
    }
}

impl<I, IO, IE, S> Server<I, S>
where
    I: Accept<Conn = IO, Error = IE>,
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    IE: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    pub async fn run(self) -> ServerResult<()> {
        let shutdown_rx = self.shutdown_rx;

        self.inner
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
            .map_err(Into::into)
    }

    /// Gets a reference to the server's config.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Gets a reference to the server's locally bound socket.
    pub fn local_socket(&self) -> &S {
        &self.socket
    }
}

pub async fn migrate_builtins_from_module_index(
    services_context: &ServicesContext,
) -> ServerResult<()> {
    let mut interval = time::interval(Duration::from_secs(5));
    let instant = Instant::now();

    let mut dal_context = services_context.clone().into_builder(true);
    dal_context.set_no_dependent_values();
    let mut ctx = dal_context.build_default().await?;
    info!("setup builtin workspace");
    Workspace::setup_builtin(&mut ctx).await?;

    info!("migrating intrinsic functions");
    builtins::func::migrate_intrinsics(&ctx).await?;
    // info!("migrating builtin functions");
    // builtins::func::migrate(&ctx).await?;

    let module_index_url = services_context
        .module_index_url()
        .ok_or(ServerError::ModuleIndexNotSet)?;

    let module_index_client =
        ModuleIndexClient::unauthenticated_client(module_index_url.try_into()?);
    let module_list = module_index_client.list_builtins().await?;
    info!("builtins install starting");
    let install_builtins = install_builtins(ctx, module_list, module_index_client);
    tokio::pin!(install_builtins);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                info!(elapsed = instant.elapsed().as_secs_f32(), "migrating");
            }
            result = &mut install_builtins  => match result {
                Ok(_) => {
                    info!(elapsed = instant.elapsed().as_secs_f32(), "migrating completed");
                    break;
                }
                Err(err) => return Err(err),
            }
        }
    }

    Ok(())
}

async fn install_builtins(
    ctx: DalContext,
    module_list: BuiltinsDetailsResponse,
    module_index_client: ModuleIndexClient,
) -> ServerResult<()> {
    let dal = &ctx;
    let client = &module_index_client.clone();
    let modules: Vec<ModuleDetailsResponse> = module_list.modules;
    // .into_iter()
    // .filter(|module| module.name.contains("docker-image"))
    // .collect();

    let total = modules.len();

    let mut join_set = JoinSet::new();
    for module in modules {
        let module = module.clone();
        let client = client.clone();
        join_set.spawn(async move {
            (
                module.name.to_owned(),
                (module.to_owned(), fetch_builtin(&module, &client).await),
            )
        });
    }

    let mut count: usize = 0;
    while let Some(res) = join_set.join_next().await {
        let (pkg_name, (module, res)) = res?;
        match res {
            Ok(pkg) => {
                let instant = Instant::now();

                match dal::pkg::import_pkg_from_pkg(
                    &ctx,
                    &pkg,
                    Some(dal::pkg::ImportOptions {
                        is_builtin: true,
                        schema_id: module.schema_id().map(Into::into),
                        past_module_hashes: module.past_hashes,
                        ..Default::default()
                    }),
                )
                .await
                {
                    Ok(_) => {
                        count += 1;
                        let elapsed = instant.elapsed().as_secs_f32();
                        info!(
                                "pkg {pkg_name} install finished successfully and took {elapsed:.2} seconds ({count} of {total} installed)",
                            );
                    }
                    Err(PkgError::PackageAlreadyInstalled(hash)) => {
                        count += 1;
                        warn!(%hash, "pkg {pkg_name} already installed ({count} of {total} installed)");
                    }
                    Err(err) => error!(?err, "pkg {pkg_name} install failed"),
                }
            }
            Err(err) => {
                error!(?err, "pkg {pkg_name} install failed with server error");
            }
        }
    }
    dal.commit().await?;

    let mut ctx = ctx.clone();
    ctx.update_snapshot_to_visibility().await?;

    Ok(())
}

async fn fetch_builtin(
    module: &ModuleDetailsResponse,
    module_index_client: &ModuleIndexClient,
) -> ServerResult<SiPkg> {
    let module = module_index_client
        .get_builtin(Ulid::from_string(&module.id).unwrap_or_default())
        .await?;

    Ok(SiPkg::load_from_bytes(module)?)
}

#[allow(clippy::too_many_arguments)]
pub fn build_service_for_tests(
    services_context: ServicesContext,
    jwt_public_signing_key: JwtPublicSigningKey,
    posthog_client: PosthogClient,
    auth_api_url: impl AsRef<str>,
    ws_multiplexer_client: MultiplexerClient,
    crdt_multiplexer_client: MultiplexerClient,
    create_workspace_permissions: WorkspacePermissionsMode,
    create_workspace_allowlist: Vec<WorkspacePermissions>,
) -> ServerResult<(Router, oneshot::Receiver<()>, broadcast::Receiver<()>)> {
    build_service_inner(
        services_context,
        jwt_public_signing_key,
        posthog_client,
        auth_api_url,
        true,
        ws_multiplexer_client,
        crdt_multiplexer_client,
        create_workspace_permissions,
        create_workspace_allowlist,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_service(
    services_context: ServicesContext,
    jwt_public_signing_key: JwtPublicSigningKey,
    posthog_client: PosthogClient,
    auth_api_url: impl AsRef<str>,
    ws_multiplexer_client: MultiplexerClient,
    crdt_multiplexer_client: MultiplexerClient,
    create_workspace_permissions: WorkspacePermissionsMode,
    create_workspace_allowlist: Vec<WorkspacePermissions>,
) -> ServerResult<(Router, oneshot::Receiver<()>, broadcast::Receiver<()>)> {
    build_service_inner(
        services_context,
        jwt_public_signing_key,
        posthog_client,
        auth_api_url,
        false,
        ws_multiplexer_client,
        crdt_multiplexer_client,
        create_workspace_permissions,
        create_workspace_allowlist,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_service_inner(
    services_context: ServicesContext,
    jwt_public_signing_key: JwtPublicSigningKey,
    posthog_client: PosthogClient,
    auth_api_url: impl AsRef<str>,
    for_tests: bool,
    ws_multiplexer_client: MultiplexerClient,
    crdt_multiplexer_client: MultiplexerClient,
    create_workspace_permissions: WorkspacePermissionsMode,
    create_workspace_allowlist: Vec<WorkspacePermissions>,
) -> ServerResult<(Router, oneshot::Receiver<()>, broadcast::Receiver<()>)> {
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
    let (shutdown_broadcast_tx, shutdown_broadcast_rx) = broadcast::channel(1);

    let state = AppState::new(
        services_context,
        jwt_public_signing_key,
        posthog_client,
        auth_api_url,
        shutdown_broadcast_tx.clone(),
        shutdown_tx,
        for_tests,
        ws_multiplexer_client,
        crdt_multiplexer_client,
        create_workspace_permissions,
        create_workspace_allowlist,
    );

    let mode = state.application_runtime_mode.clone();

    let path_filter = Box::new(|path: &str| match path {
        "/api/" => Some(Level::TRACE),
        _ => None,
    });

    let routes = routes(state).layer(
        TraceLayer::new_for_http()
            .make_span_with(
                HttpMakeSpan::builder()
                    .level(Level::INFO)
                    .path_filter(path_filter)
                    .build(),
            )
            .on_response(HttpOnResponse::new().level(Level::DEBUG)),
    );

    let graceful_shutdown_rx = prepare_signal_handlers(shutdown_rx, shutdown_broadcast_tx, mode)?;

    Ok((routes, graceful_shutdown_rx, shutdown_broadcast_rx))
}

fn prepare_signal_handlers(
    mut shutdown_rx: mpsc::Receiver<ShutdownSource>,
    shutdown_broadcast_tx: broadcast::Sender<()>,
    mode: Arc<RwLock<ApplicationRuntimeMode>>,
) -> ServerResult<oneshot::Receiver<()>> {
    let (graceful_shutdown_tx, graceful_shutdown_rx) = oneshot::channel::<()>();

    let mut sigterm_watcher =
        signal::unix::signal(signal::unix::SignalKind::terminate()).map_err(ServerError::Signal)?;
    let mut sigusr2_watcher = signal::unix::signal(signal::unix::SignalKind::user_defined2())
        .map_err(ServerError::Signal)?;

    tokio::spawn(async move {
        fn send_graceful_shutdown(
            tx: oneshot::Sender<()>,
            shutdown_broadcast_tx: broadcast::Sender<()>,
        ) {
            // Send graceful shutdown to axum server which stops it from accepting requests
            if tx.send(()).is_err() {
                error!("the server graceful shutdown receiver has already dropped");
            }
            // Send shutdown to all long running sessions (notably, WebSocket sessions), so they
            // can cleanly terminate
            if shutdown_broadcast_tx.send(()).is_err() {
                error!("all broadcast shutdown receivers have already been dropped");
            }
        }

        loop {
            tokio::select! {
                _ = signal::ctrl_c() => {
                    info!("received SIGINT signal, performing graceful shutdown");
                    send_graceful_shutdown(graceful_shutdown_tx, shutdown_broadcast_tx);
                    break
                }
                _ = sigterm_watcher.recv() => {
                    info!("received SIGTERM signal, performing graceful shutdown");
                    send_graceful_shutdown(graceful_shutdown_tx, shutdown_broadcast_tx);
                    break
                }
                _ = sigusr2_watcher.recv() => {
                    info!("received SIGUSR2 signal, changing application runtime mode");
                    let mut mode = mode.write().await;
                    info!(?mode, "current application runtime mode (changing it...)");
                    *mode = match *mode {
                        ApplicationRuntimeMode::Maintenance => ApplicationRuntimeMode::Running,
                        ApplicationRuntimeMode::Running => ApplicationRuntimeMode::Maintenance,
                    };
                    info!(?mode, "new application runtime mode (changed!)");
                }
                source = shutdown_rx.recv() => {
                    info!(
                        "received internal shutdown, performing graceful shutdown; source={:?}",
                        source,
                    );
                    send_graceful_shutdown(graceful_shutdown_tx, shutdown_broadcast_tx);
                    break
                }
                else => {
                    // All other arms are closed, nothing left to do but return
                    trace!("returning from graceful shutdown with all select arms closed");
                    break
                }
            };
        }
    });

    Ok(graceful_shutdown_rx)
}

#[remain::sorted]
#[derive(Debug, Eq, PartialEq)]
pub enum ShutdownSource {}
