use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use tracing::debug;

use ::std::path::Path;
use rand::distributions::Alphanumeric;
use rand::thread_rng;
use rand::Rng;
use std::{io, path::PathBuf, result, time::Duration};

use bollard::container::{
    Config, CreateContainerOptions, RemoveContainerOptions, StartContainerOptions,
};
use bollard::models::{HostConfig, Mount, MountTypeEnum};
use bollard::{errors::Error, Docker};

use async_trait::async_trait;
use cyclone_client::{
    Client, ClientConfig, ClientError, Connection, CycloneClient, Execution, LivenessStatus,
    PingExecution, ReadinessStatus, UdsClient, UnixStream, Watch, WatchError, WatchStarted,
};
use cyclone_core::{
    process::{self, ShutdownError},
    ActionRunRequest, ActionRunResultSuccess, CanonicalCommand, ReconciliationRequest,
    ReconciliationResultSuccess, ResolverFunctionRequest, ResolverFunctionResultSuccess,
    SchemaVariantDefinitionRequest, SchemaVariantDefinitionResultSuccess, ValidationRequest,
    ValidationResultSuccess,
};
use derive_builder::Builder;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tempfile::{NamedTempFile, TempPath};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    process::{Child, Command},
    sync::oneshot,
    time,
};
use tracing::trace;

use crate::instance::{Instance, Spec, SpecBuilder};

/// Error type for [`LocalUdsInstance`].
#[remain::sorted]
#[derive(Debug, Error)]
pub enum LocalUdsInstanceError {
    /// Spec builder error.
    #[error(transparent)]
    Builder(#[from] LocalUdsInstanceSpecBuilderError),
    /// Error when waiting for child process to shutdown.
    #[error(transparent)]
    ChildShutdown(#[from] ShutdownError),
    /// Failed to spawn a child process.
    #[error("failed to spawn cyclone child process")]
    ChildSpawn(#[source] io::Error),
    /// Cyclone client error.
    #[error(transparent)]
    Client(#[from] ClientError),
    /// Failed to build a container.
    #[error("failed to build a cyclone container")]
    ContainerBuild(#[source] Error),
    /// Failed to run a container.
    #[error("failed to spawn cyclone container")]
    ContainerRun(#[source] Error),
    /// Error when shutting down a container.
    #[error(transparent)]
    ContainerShutdown(#[from] Error),
    /// Docker api not found
    #[error("no docker api")]
    DockerAPINotFound,
    /// Failed to firecracker jail.
    #[error("failed in working with a jail: {0}")]
    Firecracker(#[from] std::io::Error),
    /// Failed to create firecracker-setup file.
    #[error("failed to create firecracker-setup file")]
    FirecrackerSetupCreate(#[source] io::Error),
    /// Failed to set permissions on the firecracker-setup file.
    #[error("failed to set permissions on the firecracker-setup file")]
    FirecrackerSetupPermissions(#[source] io::Error),
    /// Failed to run firecracker-setup file.
    #[error("failed to run firecracker-setup file: {0}")]
    FirecrackerSetupRun(String),
    /// Failed to write to firecracker-setup file.
    #[error("failed to write to firecracker-setup file")]
    FirecrackerSetupWrite(#[source] io::Error),
    /// Instance has exhausted its predefined request count.
    #[error("no remaining requests, cyclone server is considered unhealthy")]
    NoRemainingRequests,
    /// Failed to setup the host correctly.
    #[error("failed to setup host")]
    SetupFailed,
    /// Failed to create socket from temporary file.
    #[error("failed to create temp socket")]
    TempSocket(#[source] io::Error),
    /// Cyclone client `watch` endpoint error.
    #[error(transparent)]
    Watch(#[from] WatchError),
    /// Cyclone client `watch` session ended earlier than expected.
    #[error("server closed watch session before expected")]
    WatchClosed,
    /// Cyclone client initial `watch` session connection with retries timed out.
    #[error("timeout while retrying to start a client watch session")]
    WatchInitTimeout,
    /// Cyclone client `watch` session shut down earlier than expected.
    #[error("watch session is shut down, cyclone server is considered unhealthy")]
    WatchShutDown,
}

type Result<T> = result::Result<T, LocalUdsInstanceError>;

/// A local Cyclone [`Instance`], managed as a spawned child process, communicating over a Unix
/// domain socket ("Uds").
pub struct LocalUdsInstance {
    // The `TempPath` type is kept around as an [RAII
    // guard](https://rust-unofficial.github.io/patterns/patterns/behavioural/RAII.html), that is,
    // when `LocalUdsInstance` is dropped, the temp file is marked for deletion.
    _temp_path: Option<TempPath>,
    client: UdsClient,
    limit_requests: Option<u32>,
    runtime: Box<dyn LocalInstanceRuntime>,
    watch_shutdown_tx: oneshot::Sender<()>,
}

#[async_trait]
impl Instance for LocalUdsInstance {
    type SpecBuilder = LocalUdsInstanceSpecBuilder;
    type Error = LocalUdsInstanceError;

    async fn terminate(&mut self) -> result::Result<(), Self::Error> {
        self.runtime.terminate().await
    }

    async fn ensure_healthy(&mut self) -> result::Result<(), Self::Error> {
        self.ensure_healthy_client().await?;

        Ok(())
    }
    fn id(&self) -> u32 {
        self.runtime.id()
    }
}

#[async_trait]
impl CycloneClient<UnixStream> for LocalUdsInstance {
    async fn watch(&mut self) -> result::Result<Watch<UnixStream>, ClientError> {
        self.ensure_healthy_client()
            .await
            .map_err(ClientError::unhealthy)?;
        self.client.watch().await
    }

    async fn liveness(&mut self) -> result::Result<LivenessStatus, ClientError> {
        self.ensure_healthy_client()
            .await
            .map_err(ClientError::unhealthy)?;
        self.client.liveness().await
    }

    async fn readiness(&mut self) -> result::Result<ReadinessStatus, ClientError> {
        self.ensure_healthy_client()
            .await
            .map_err(ClientError::unhealthy)?;
        self.client.readiness().await
    }

    async fn execute_ping(&mut self) -> result::Result<PingExecution<UnixStream>, ClientError> {
        self.ensure_healthy_client()
            .await
            .map_err(ClientError::unhealthy)?;
        let result = self.client.execute_ping().await;
        self.count_request();

        result
    }

    async fn execute_resolver(
        &mut self,
        request: ResolverFunctionRequest,
    ) -> result::Result<
        Execution<UnixStream, ResolverFunctionRequest, ResolverFunctionResultSuccess>,
        ClientError,
    > {
        self.ensure_healthy_client()
            .await
            .map_err(ClientError::unhealthy)?;
        let result = self.client.execute_resolver(request).await;
        self.count_request();
        result
    }

    async fn execute_validation(
        &mut self,
        request: ValidationRequest,
    ) -> result::Result<
        Execution<UnixStream, ValidationRequest, ValidationResultSuccess>,
        ClientError,
    > {
        self.ensure_healthy_client()
            .await
            .map_err(ClientError::unhealthy)?;
        let result = self.client.execute_validation(request).await;
        self.count_request();

        result
    }

    // The request argument is the same as that in the "impl FuncDispatch for
    // FuncBackendJsAction" in the dal.
    async fn execute_action_run(
        &mut self,
        request: ActionRunRequest,
    ) -> result::Result<Execution<UnixStream, ActionRunRequest, ActionRunResultSuccess>, ClientError>
    {
        self.ensure_healthy_client()
            .await
            .map_err(ClientError::unhealthy)?;
        // Use the websocket client for cyclone to execute command run.
        let result = self.client.execute_action_run(request).await;
        self.count_request();

        result
    }

    async fn execute_reconciliation(
        &mut self,
        request: ReconciliationRequest,
    ) -> result::Result<
        Execution<UnixStream, ReconciliationRequest, ReconciliationResultSuccess>,
        ClientError,
    > {
        self.ensure_healthy_client()
            .await
            .map_err(ClientError::unhealthy)?;
        // Use the websocket client for cyclone to execute reconciliation.
        let result = self.client.execute_reconciliation(request).await;
        self.count_request();

        result
    }

    async fn execute_schema_variant_definition(
        &mut self,
        request: SchemaVariantDefinitionRequest,
    ) -> result::Result<
        Execution<UnixStream, SchemaVariantDefinitionRequest, SchemaVariantDefinitionResultSuccess>,
        ClientError,
    > {
        self.ensure_healthy_client()
            .await
            .map_err(ClientError::unhealthy)?;
        // Use the websocket client for cyclone to execute reconciliation.
        let result = self.client.execute_schema_variant_definition(request).await;
        self.count_request();

        result
    }
}

impl LocalUdsInstance {
    async fn ensure_healthy_client(&mut self) -> Result<()> {
        if !self.is_watch_shutdown_open() {
            return Err(LocalUdsInstanceError::WatchShutDown);
        }
        if !self.has_remaining_requests() {
            return Err(LocalUdsInstanceError::NoRemainingRequests);
        }

        Ok(())
    }

    fn has_remaining_requests(&self) -> bool {
        match self.limit_requests {
            Some(0) => false,
            Some(_) | None => true,
        }
    }

    fn is_watch_shutdown_open(&self) -> bool {
        !self.watch_shutdown_tx.is_closed()
    }

    fn count_request(&mut self) {
        if let Some(limit_requests) = self.limit_requests.as_mut() {
            *limit_requests = limit_requests.saturating_sub(1);
        }
    }
}

/// The [`Spec`] for [`LocalUdsInstance`]
#[derive(Builder, Clone, Debug)]
pub struct LocalUdsInstanceSpec {
    /// Canonical path to the `cyclone` program.
    #[builder(try_setter, setter(into), default)]
    cyclone_cmd_path: CanonicalCommand,

    /// Canonical path to Cyclone's secret key file.
    #[builder(setter(into), default)]
    cyclone_decryption_key_path: String,

    /// Canonical path to the language server program.
    #[builder(try_setter, setter(into), default)]
    lang_server_cmd_path: CanonicalCommand,

    /// Socket strategy for a spawned Cyclone server.
    #[builder(default)]
    socket_strategy: LocalUdsSocketStrategy,

    /// Runtime strategy for a spawned Cyclone server.
    #[builder(default)]
    runtime_strategy: LocalUdsRuntimeStrategy,

    /// Sets the watch timeout value for a spawned Cyclone server.
    #[builder(setter(into, strip_option), default)]
    watch_timeout: Option<Duration>,

    /// Sets the limit requests strategy for a spawned Cyclone server.
    #[builder(setter(into), default = "Some(1)")]
    limit_requests: Option<u32>,

    /// Enables the `ping` execution endpoint for a spawned Cyclone server.
    #[builder(private, setter(name = "_ping"), default = "false")]
    ping: bool,

    /// Enables the `resolver` execution endpoint for a spawned Cyclone server.
    #[builder(private, setter(name = "_resolver"), default = "false")]
    resolver: bool,

    /// Enables the `action` execution endpoint for a spawned Cyclone server.
    #[builder(private, setter(name = "_action"), default = "false")]
    action: bool,

    /// Size of the pool to configure for the spec.
    #[builder(setter(into), default = "10")]
    pub pool_size: u16,

    /// Sets the timeout for connecting to firecracker
    #[builder(setter(into), default = "10")]
    connect_timeout: u64,
}

#[async_trait]
impl Spec for LocalUdsInstanceSpec {
    type Instance = LocalUdsInstance;
    type Error = LocalUdsInstanceError;

    async fn clean(&self, id: u32) -> result::Result<(), Self::Error> {
        match self.runtime_strategy {
            LocalUdsRuntimeStrategy::LocalDocker => Ok(()),
            LocalUdsRuntimeStrategy::LocalProcess => Ok(()),
            LocalUdsRuntimeStrategy::LocalFirecracker => LocalFirecrackerRuntime::clean(id).await,
        }
    }

    async fn prepare(&self, id: u32) -> result::Result<(), Self::Error> {
        match self.runtime_strategy {
            LocalUdsRuntimeStrategy::LocalDocker => Ok(()),
            LocalUdsRuntimeStrategy::LocalProcess => Ok(()),
            LocalUdsRuntimeStrategy::LocalFirecracker => LocalFirecrackerRuntime::prepare(id).await,
        }
    }
    async fn setup(&mut self) -> result::Result<(), Self::Error> {
        match self.runtime_strategy {
            LocalUdsRuntimeStrategy::LocalDocker => Ok(()),
            LocalUdsRuntimeStrategy::LocalProcess => Ok(()),
            LocalUdsRuntimeStrategy::LocalFirecracker => {
                LocalFirecrackerRuntime::setup_firecracker(self).await
            }
        }
    }

    async fn spawn(&self, id: u32) -> result::Result<Self::Instance, Self::Error> {
        let (temp_path, socket) = temp_path_and_socket_from(&self.socket_strategy)?;
        let mut runtime = runtime_instance_from_spec(self, &socket, id).await?;

        runtime.spawn().await?;
        //TODO(scott): Firecracker requires the client to add a special connection detail. We
        //should find a better way to handle this.
        let firecracker_connect = matches!(
            self.runtime_strategy,
            LocalUdsRuntimeStrategy::LocalFirecracker
        );

        let config = ClientConfig {
            connect_timeout: Duration::from_millis(self.connect_timeout),
            firecracker_connect,
            ..Default::default()
        };
        let mut client = Client::uds(runtime.socket(), Arc::new(config))?;

        // Establish the client watch session. As the process may be booting, we will retry for a
        // period before giving up and assuming that the server instance has failed.
        let watch = {
            let mut retries = 30;
            loop {
                match client.watch().await {
                    Ok(watch) => {
                        break watch;
                    }
                    Err(err) => err,
                };
                if retries < 1 {
                    return Err(Self::Error::WatchInitTimeout);
                }
                retries -= 1;
                time::sleep(Duration::from_millis(64)).await;
            }
        };

        let mut watch_progress = watch.start().await?;

        // Establish that we have received our first watch ping, which should happen immediately
        // after establishing a watch session
        watch_progress
            .next()
            .await
            .ok_or(Self::Error::WatchClosed)??;

        let (watch_shutdown_tx, watch_shutdown_rx) = oneshot::channel();
        // Spawn a task to keep the watch session open until we shut it down
        tokio::spawn(watch_task(watch_progress, watch_shutdown_rx));

        Ok(Self::Instance {
            _temp_path: temp_path,
            client,
            limit_requests: self.limit_requests,
            runtime,
            watch_shutdown_tx,
        })
    }
}

impl SpecBuilder for LocalUdsInstanceSpecBuilder {
    type Spec = LocalUdsInstanceSpec;
    type Error = LocalUdsInstanceError;

    fn build(&self) -> result::Result<Self::Spec, Self::Error> {
        self.build().map_err(Into::into)
    }
}

impl LocalUdsInstanceSpecBuilder {
    /// Sets the limit requests strategy to `1` for a spawned Cyclone server.
    pub fn oneshot(&mut self) -> &mut Self {
        self.limit_requests(Some(1))
    }

    /// Enables the `ping` execution endpoint for a spawned Cyclone server.
    pub fn ping(&mut self) -> &mut Self {
        self._ping(true)
    }

    /// Enables the `resolver` execution endpoint for a spawned Cyclone server.
    pub fn resolver(&mut self) -> &mut Self {
        self._resolver(true)
    }

    /// Enables the `action` execution endpoint for a spawned Cyclone server.
    pub fn action(&mut self) -> &mut Self {
        self._action(true)
    }

    /// Enables all available endpoints for a spawned Cyclone server
    pub fn all_endpoints(&mut self) -> &mut Self {
        self.action().resolver()
    }
}

/// Socket strategy when spawning [`Instance`]s using a local Unix domain socket.
#[remain::sorted]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum LocalUdsSocketStrategy {
    /// Use the given path as the socket location.
    Custom(PathBuf),
    /// Randomly assign a socket from a temp file.
    Random,
    /// Randomly assign a socket from a temp file in the given parent directory.
    RandomIn(PathBuf),
}

impl Default for LocalUdsSocketStrategy {
    fn default() -> Self {
        Self::Random
    }
}

impl LocalUdsSocketStrategy {
    /// Creates a random socket strategy.
    #[must_use]
    pub fn random() -> Self {
        Self::Random
    }

    /// Creates a random socket strategy in the given parent directory.
    pub fn random_in(path: impl Into<PathBuf>) -> Self {
        Self::RandomIn(path.into())
    }

    /// Creates a custom socket strategy for the given socket location.
    pub fn custom(path: impl Into<PathBuf>) -> Self {
        Self::Custom(path.into())
    }
}

fn temp_path_and_socket_from(
    socket_strategy: &LocalUdsSocketStrategy,
) -> Result<(Option<TempPath>, PathBuf)> {
    match socket_strategy {
        LocalUdsSocketStrategy::Random => {
            let temp_path = NamedTempFile::new()
                .map_err(LocalUdsInstanceError::TempSocket)?
                .into_temp_path();
            let socket = PathBuf::from(&temp_path);

            Ok((Some(temp_path), socket))
        }
        LocalUdsSocketStrategy::RandomIn(parent_path) => {
            let temp_path = NamedTempFile::new_in(parent_path)
                .map_err(LocalUdsInstanceError::TempSocket)?
                .into_temp_path();
            let socket = PathBuf::from(&temp_path);

            Ok((Some(temp_path), socket))
        }
        LocalUdsSocketStrategy::Custom(socket) => Ok((None, socket.clone())),
    }
}

#[remain::sorted]
/// Runtime strategy when spawning [`Instance`]s.
#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub enum LocalUdsRuntimeStrategy {
    /// Run Docker containers on the local machine
    LocalDocker,
    /// Run processes on firecracker
    LocalFirecracker,
    /// Run processes on the local machine
    LocalProcess,
}

impl Default for LocalUdsRuntimeStrategy {
    fn default() -> Self {
        // firecracker-setup: change LocalProcess to LocalFirecracker
        Self::LocalProcess
    }
}

#[async_trait]
pub trait LocalInstanceRuntime: Send + Sync {
    fn id(&self) -> u32;
    fn socket(&mut self) -> PathBuf;
    async fn spawn(&mut self) -> result::Result<(), LocalUdsInstanceError>;
    async fn terminate(&mut self) -> result::Result<(), LocalUdsInstanceError>;
}

#[derive(Debug)]
struct LocalProcessRuntime {
    cmd: Command,
    child: Option<Child>,
    socket: PathBuf,
}

impl LocalProcessRuntime {
    async fn build(
        socket: &PathBuf,
        spec: LocalUdsInstanceSpec,
    ) -> Result<Box<dyn LocalInstanceRuntime>> {
        let mut cmd = Command::new(&spec.cyclone_cmd_path);
        cmd.arg("--bind-uds")
            .arg(socket)
            .arg("--decryption-key")
            .arg(&spec.cyclone_decryption_key_path)
            .arg("--lang-server")
            .arg(&spec.lang_server_cmd_path)
            .arg("--enable-watch");
        if let Some(limit_requests) = spec.limit_requests {
            cmd.arg("--limit-requests").arg(limit_requests.to_string());
        }
        if let Some(timeout) = spec.watch_timeout {
            cmd.arg("--watch-timeout")
                .arg(timeout.as_secs().to_string());
        }
        if spec.ping {
            cmd.arg("--enable-ping");
        }
        if spec.resolver {
            cmd.arg("--enable-resolver");
        }
        if spec.action {
            cmd.arg("--enable-action-run");
        }

        Ok(Box::new(LocalProcessRuntime {
            cmd,
            child: None,
            socket: socket.to_path_buf(),
        }))
    }
}

#[async_trait]
impl LocalInstanceRuntime for LocalProcessRuntime {
    fn id(&self) -> u32 {
        0
    }
    fn socket(&mut self) -> PathBuf {
        self.socket.to_path_buf()
    }

    async fn spawn(&mut self) -> result::Result<(), LocalUdsInstanceError> {
        self.child = Some(
            self.cmd
                .spawn()
                .map_err(LocalUdsInstanceError::ChildSpawn)?,
        );
        Ok(())
    }
    async fn terminate(&mut self) -> result::Result<(), LocalUdsInstanceError> {
        match self.child.as_mut() {
            Some(c) => {
                process::child_shutdown(c, Some(process::Signal::SIGTERM), None).await?;
                Ok(())
            }
            None => Ok(()),
        }
    }
}

#[derive(Debug)]
struct LocalDockerRuntime {
    container_id: String,
    docker: Docker,
    socket: PathBuf,
}

impl LocalDockerRuntime {
    async fn build(
        socket: &Path,
        spec: LocalUdsInstanceSpec,
    ) -> Result<Box<dyn LocalInstanceRuntime>> {
        let mut cmd = vec![
            String::from("--bind-uds"),
            socket.to_string_lossy().to_string(),
            String::from("--decryption-key"),
            String::from("/tmp/key"),
            String::from("--lang-server"),
            String::from("/usr/local/bin/lang-js"),
            String::from("--enable-watch"),
        ];
        if let Some(limit_requests) = spec.limit_requests {
            cmd.push(String::from("--limit-requests"));
            cmd.push(limit_requests.to_string())
        }
        if let Some(timeout) = spec.watch_timeout {
            cmd.push(String::from("--watch-timeout"));
            cmd.push(timeout.as_secs().to_string());
        }
        if spec.ping {
            cmd.push(String::from("--enable-ping"));
        }
        if spec.resolver {
            cmd.push(String::from("--enable-resolver"));
        }
        if spec.action {
            cmd.push(String::from("--enable-action-run"));
        }

        let docker = Docker::connect_with_local_defaults()?;

        let rand_string: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();

        let socket_dir = socket
            .parent()
            .expect("socket path not available")
            .to_str()
            .expect("unable to unpack path");
        let mounts = vec![
            Mount {
                source: Some(String::from(socket_dir)),
                target: Some(String::from(socket_dir)),
                typ: Some(MountTypeEnum::BIND),
                ..Default::default()
            },
            Mount {
                source: Some(spec.cyclone_decryption_key_path),
                target: Some(String::from("/tmp/key")),
                typ: Some(MountTypeEnum::BIND),
                ..Default::default()
            },
        ];

        let container_id = docker
            .create_container(
                Some(CreateContainerOptions {
                    name: format!("cyclone-container-{rand_string}"),
                    platform: Some(String::from("linux/amd64")),
                }),
                Config {
                    image: Some(String::from("systeminit/cyclone:stable")),
                    cmd: Some(cmd),
                    host_config: Some(HostConfig {
                        mounts: Some(mounts),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .await?
            .id;

        Ok(Box::new(LocalDockerRuntime {
            container_id,
            docker,
            socket: socket.to_path_buf(),
        }))
    }
}

#[async_trait]
impl LocalInstanceRuntime for LocalDockerRuntime {
    fn id(&self) -> u32 {
        0
    }
    fn socket(&mut self) -> PathBuf {
        self.socket.to_path_buf()
    }

    async fn spawn(&mut self) -> result::Result<(), LocalUdsInstanceError> {
        self.docker
            .start_container(
                &self.container_id.clone(),
                None::<StartContainerOptions<String>>,
            )
            .await?;
        Ok(())
    }

    async fn terminate(&mut self) -> result::Result<(), LocalUdsInstanceError> {
        self.docker
            .remove_container(
                &self.container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await?;
        Ok(())
    }
}

#[derive(Debug)]
struct LocalFirecrackerRuntime {
    cmd: Command,
    child: Option<Child>,
    vm_id: u32,
    socket: PathBuf,
}

impl LocalFirecrackerRuntime {
    async fn build(_spec: LocalUdsInstanceSpec, id: u32) -> Result<Box<dyn LocalInstanceRuntime>> {
        let mut cmd = Command::new("/usr/bin/jailer");
        cmd.arg("--cgroup-version")
            .arg("2")
            .arg("--id")
            .arg(id.to_string())
            .arg("--exec-file")
            .arg("/usr/bin/firecracker")
            .arg("--uid")
            .arg(format!("500{}", id))
            .arg("--gid")
            .arg("10000")
            .arg("--netns")
            .arg(format!("/var/run/netns/jailer-{}", id))
            .arg("--")
            .arg("--config-file")
            .arg("./firecracker.conf");

        let socket = PathBuf::from(&format!("/srv/jailer/firecracker/{}/root/v.sock", id));
        Ok(Box::new(LocalFirecrackerRuntime {
            cmd,
            child: None,
            vm_id: id,
            socket,
        }))
    }
}

#[async_trait]
impl LocalInstanceRuntime for LocalFirecrackerRuntime {
    fn id(&self) -> u32 {
        self.vm_id
    }
    fn socket(&mut self) -> PathBuf {
        self.socket.to_path_buf()
    }

    async fn spawn(&mut self) -> result::Result<(), LocalUdsInstanceError> {
        self.child = Some(
            self.cmd
                .spawn()
                .map_err(LocalUdsInstanceError::ChildSpawn)?,
        );
        Ok(())
    }

    async fn terminate(&mut self) -> result::Result<(), LocalUdsInstanceError> {
        match self.child.as_mut() {
            Some(c) => {
                process::child_shutdown(c, Some(process::Signal::SIGTERM), None).await?;
                Ok(())
            }
            None => Ok(()),
        }
    }
}

impl LocalFirecrackerRuntime {
    async fn clean(id: u32) -> Result<()> {
        let command = String::from("/firecracker-data/stop.sh");
        Command::new("sudo")
            .arg(command)
            .arg(id.to_string())
            .output()
            .await?;
        Ok(())
    }

    async fn prepare(id: u32) -> Result<()> {
        let command = String::from("/firecracker-data/prepare_jailer.sh");
        let _output = Command::new("sudo")
            .arg(command)
            .arg(id.to_string())
            .output()
            .await?;
        Ok(())
    }
    async fn setup_firecracker(spec: &LocalUdsInstanceSpec) -> Result<()> {
        let script_bytes = include_bytes!("firecracker-setup.sh");
        let command = Path::new("/firecracker-data/firecracker-setup.sh");

        // we need to ensure the file is in the correct location with the correct permissions
        std::fs::create_dir_all(
            command
                .parent()
                .expect("This should never happen. Did you remove the path from the string above?"),
        )
        .map_err(LocalUdsInstanceError::FirecrackerSetupCreate)?;
        std::fs::write(command, script_bytes)
            .map_err(LocalUdsInstanceError::FirecrackerSetupWrite)?;
        std::fs::set_permissions(command, std::fs::Permissions::from_mode(0o755))
            .map_err(LocalUdsInstanceError::FirecrackerSetupPermissions)?;

        // Spawn the shell process
        let output = Command::new("sudo")
            .arg(command)
            .arg("-j")
            .arg(&spec.pool_size.to_string())
            .arg("-rk")
            .spawn()
            .map_err(|e| LocalUdsInstanceError::FirecrackerSetupRun(e.to_string()))
            .expect("Failed to start firecracker-setup")
            .wait_with_output()
            .await
            .map_err(|e| LocalUdsInstanceError::FirecrackerSetupRun(e.to_string()))
            .expect("Failed to run firecracker-setup");

        if !output.status.success() {
            return Err(LocalUdsInstanceError::FirecrackerSetupRun(
                String::from_utf8(output.stderr).expect("This should not be empty"),
            ));
        }

        Ok(())
    }
}

async fn runtime_instance_from_spec(
    spec: &LocalUdsInstanceSpec,
    socket: &PathBuf,
    id: u32,
) -> Result<Box<dyn LocalInstanceRuntime>> {
    match spec.runtime_strategy {
        LocalUdsRuntimeStrategy::LocalProcess => {
            LocalProcessRuntime::build(socket, spec.clone()).await
        }
        LocalUdsRuntimeStrategy::LocalDocker => {
            LocalDockerRuntime::build(socket, spec.clone()).await
        }
        LocalUdsRuntimeStrategy::LocalFirecracker => {
            LocalFirecrackerRuntime::build(spec.clone(), id).await
        }
    }
}

async fn watch_task<Strm>(
    mut watch_progress: WatchStarted<Strm>,
    mut shutdown_rx: oneshot::Receiver<()>,
) where
    Strm: AsyncRead + AsyncWrite + Connection + Unpin + Send + Sync + 'static,
{
    loop {
        tokio::select! {
            // Got a shutdown message
            _ = Pin::new(&mut shutdown_rx) => {
                trace!("watch task received shutdown");
                if let Err(err) = watch_progress.stop().await {
                    trace!(error = ?err, "failed to cleanly close the watch session");
                }
                break;
            }
            // Got progress on the watch session
            result = watch_progress.next() => {
                match result {
                    // Got a ping, good news, proceed
                    Some(Ok(())) => {

                    },
                    // An error occurred on the stream. We are going to treat this as catastrophic
                    // and end the watch.
                    Some(Err(err)) => {
                        debug!(error = ?err, "error on watch stream");
                        if let Err(err) = watch_progress.stop().await {
                            debug!(error = ?err, "failed to cleanly close the watch session");
                        }
                        break
                    }
                    // Stream is closed
                    None => {
                        trace!("watch stream has closed");
                        break
                    }
                }
            }
            // All other arms are closed, nothing left to do but return
            else => {
                trace!("returning from watch task with all select arms closed");
                break
            }
        }
    }
}
