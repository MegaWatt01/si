use std::{env, path::Path, sync::Arc};

use color_eyre::Result;
use dal::{
    component::ComponentKind,
    func::{binding::FuncBinding, FuncId},
    job::processor::{sync_processor::SyncProcessor, JobQueueProcessor},
    jwt_key::JwtSecretKey,
    key_pair::KeyPairPk,
    node::NodeKind,
    schema,
    socket::{Socket, SocketArity, SocketEdgeKind, SocketKind},
    ChangeSet, ChangeSetPk, Component, DalContext, DiagramKind, EncryptedSecret, Func,
    FuncBackendKind, FuncBackendResponseType, KeyPair, Node, Prop, PropId, PropKind, Schema,
    SchemaId, SchemaVariantId, Secret, SecretKind, SecretObjectType, StandardModel, User,
    Visibility, Workspace, WorkspaceSignup,
};
use lazy_static::lazy_static;
use names::{Generator, Name};
use si_data_nats::{NatsClient, NatsConfig};
use si_data_pg::{PgPool, PgPoolConfig};
use uuid::Uuid;
use veritech_client::EncryptionKey;
use veritech_server::{Instance, StandardConfig};

use super::CANONICALIZE_CYCLONE_BIN_PATH_ERROR_MESSAGE;

#[derive(Debug)]
pub struct TestConfig {
    pg: PgPoolConfig,
    nats: NatsConfig,
    jwt_encrypt: JwtSecretKey,
}

impl Default for TestConfig {
    fn default() -> Self {
        let mut nats = NatsConfig::default();
        if let Ok(value) = env::var("SI_TEST_NATS_URL") {
            nats.url = value;
        }

        let mut pg = PgPoolConfig::default();
        if let Ok(value) = env::var("SI_TEST_PG_HOSTNAME") {
            pg.hostname = value;
        }
        pg.dbname = env::var("SI_TEST_PG_DBNAME").unwrap_or_else(|_| "si_test".to_string());

        Self {
            pg,
            nats,
            jwt_encrypt: JwtSecretKey::default(),
        }
    }
}

lazy_static! {
    pub static ref SETTINGS: TestConfig = TestConfig::default();
    pub static ref INIT_LOCK: Arc<tokio::sync::Mutex<bool>> =
        Arc::new(tokio::sync::Mutex::new(false));
    pub static ref INIT_PG_LOCK: Arc<tokio::sync::Mutex<bool>> =
        Arc::new(tokio::sync::Mutex::new(false));
}

pub struct TestContext {
    // we need to keep this in scope to keep the tempdir from auto-cleaning itself
    #[allow(dead_code)]
    tmp_event_log_fs_root: tempfile::TempDir,
    pub pg: PgPool,
    pub nats_conn: NatsClient,
    pub job_processor: Box<dyn JobQueueProcessor + Send + Sync>,
    pub veritech: veritech_client::Client,
    pub encryption_key: EncryptionKey,
    pub jwt_secret_key: JwtSecretKey,
    pub telemetry: telemetry::NoopClient,
    pub council_subject_prefix: String,
}

impl TestContext {
    pub async fn init() -> Self {
        Self::init_with_settings(&SETTINGS).await
    }

    pub async fn init_with_settings(settings: &TestConfig) -> Self {
        let tmp_event_log_fs_root = tempfile::tempdir().expect("could not create temp dir");
        let pg = PgPool::new(&settings.pg)
            .await
            .expect("failed to connect to postgres");
        let nats_conn = NatsClient::new(&settings.nats)
            .await
            .expect("failed to connect to NATS");
        let job_processor =
            Box::new(SyncProcessor::new()) as Box<dyn JobQueueProcessor + Send + Sync>;

        let nats_subject_prefix = nats_prefix();

        // Create a dedicated Council server with a unique subject prefix for each test
        let council_subject_prefix = format!("{nats_subject_prefix}.council");
        let council_server =
            council_server(settings.nats.clone(), council_subject_prefix.clone()).await;
        let (_shutdown_request_tx, shutdown_request_rx) = tokio::sync::watch::channel(());
        let (subscription_started_tx, mut subscription_started_rx) =
            tokio::sync::watch::channel(());
        tokio::spawn(async move {
            council_server
                .run(subscription_started_tx, shutdown_request_rx)
                .await
                .unwrap()
        });
        subscription_started_rx.changed().await.unwrap();

        // Create a dedicated Veritech server with a unique subject prefix for each test
        let veritech_subject_prefix = format!("{nats_subject_prefix}.veritech");
        let veritech_server =
            veritech_server_for_uds_cyclone(settings.nats.clone(), veritech_subject_prefix.clone())
                .await;
        tokio::spawn(veritech_server.run());
        let veritech = veritech_client::Client::with_subject_prefix(
            nats_conn.clone(),
            veritech_subject_prefix,
        );
        let encryption_key = EncryptionKey::load(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../cyclone-server/src/dev.encryption.key"),
        )
        .await
        .expect("failed to load dev encryption key");
        let secret_key = settings.jwt_encrypt.clone();
        let telemetry = telemetry::NoopClient;

        Self {
            tmp_event_log_fs_root,
            pg,
            nats_conn,
            council_subject_prefix,
            job_processor,
            veritech,
            encryption_key,
            jwt_secret_key: secret_key,
            telemetry,
        }
    }

    pub fn entries(
        &self,
    ) -> (
        &PgPool,
        &NatsClient,
        Box<dyn JobQueueProcessor + Send + Sync>,
        veritech_client::Client,
        &EncryptionKey,
        &JwtSecretKey,
        &str,
    ) {
        (
            &self.pg,
            &self.nats_conn,
            self.job_processor.clone(),
            self.veritech.clone(),
            &self.encryption_key,
            &self.jwt_secret_key,
            &self.council_subject_prefix,
        )
    }

    /// Gets a reference to the test context's telemetry.
    pub fn telemetry(&self) -> telemetry::NoopClient {
        self.telemetry
    }
}

async fn council_server(nats_config: NatsConfig, subject_prefix: String) -> council_server::Server {
    let config = council_server::server::Config::builder()
        .nats(nats_config)
        .subject_prefix(subject_prefix)
        .build()
        .expect("failed to build spec");
    council_server::Server::new_with_config(config)
        .await
        .expect("failed to create server")
}

async fn veritech_server_for_uds_cyclone(
    nats_config: NatsConfig,
    subject_prefix: String,
) -> veritech_server::Server {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cyclone_spec = veritech_server::CycloneSpec::LocalUds(
        veritech_server::LocalUdsInstance::spec()
            .try_cyclone_cmd_path(
                dir.join("../../target/debug/cyclone")
                    .canonicalize()
                    .expect(CANONICALIZE_CYCLONE_BIN_PATH_ERROR_MESSAGE)
                    .to_string_lossy()
                    .to_string(),
            )
            .expect("failed to setup cyclone_cmd_path")
            .cyclone_decryption_key_path(
                dir.join("../../lib/cyclone-server/src/dev.decryption.key")
                    .canonicalize()
                    .expect("failed to canonicalize cyclone decryption key path")
                    .to_string_lossy()
                    .to_string(),
            )
            .try_lang_server_cmd_path(
                dir.join("../../bin/lang-js/target/lang-js")
                    .canonicalize()
                    .expect("failed to canonicalize lang-js path")
                    .to_string_lossy()
                    .to_string(),
            )
            .expect("failed to setup lang_js_cmd_path")
            .all_endpoints()
            .build()
            .expect("failed to build cyclone spec"),
    );
    let config = veritech_server::Config::builder()
        .nats(nats_config)
        .subject_prefix(subject_prefix)
        .cyclone_spec(cyclone_spec)
        .build()
        .expect("failed to build spec");
    veritech_server::Server::for_cyclone_uds(config)
        .await
        .expect("failed to create server")
}

fn nats_prefix() -> String {
    Uuid::new_v4().as_simple().to_string()
}

pub async fn one_time_setup() -> Result<()> {
    crate::TestContext::global().await.map(|_| ())
}

pub fn generate_fake_name() -> String {
    Generator::with_naming(Name::Numbered).next().unwrap()
}

pub async fn create_change_set(ctx: &DalContext) -> ChangeSet {
    let name = generate_fake_name();
    ChangeSet::new(ctx, &name, None)
        .await
        .expect("cannot create change_set")
}

pub fn create_visibility_change_set(change_set: &ChangeSet) -> Visibility {
    Visibility::new(change_set.pk, None)
}

pub fn create_visibility_head() -> Visibility {
    Visibility::new(ChangeSetPk::NONE, None)
}

pub async fn create_workspace(ctx: &mut DalContext) -> Workspace {
    let name = generate_fake_name();
    Workspace::new(ctx, &name)
        .await
        .expect("cannot create workspace")
}

pub async fn create_key_pair(ctx: &DalContext) -> KeyPair {
    let name = generate_fake_name();
    KeyPair::new(ctx, &name)
        .await
        .expect("cannot create key_pair")
}

pub async fn create_user(ctx: &DalContext) -> User {
    let name = generate_fake_name();
    User::new(
        ctx,
        &name,
        &format!("{name}@test.systeminit.com"),
        "liesAreTold",
    )
    .await
    .expect("cannot create user")
}

pub async fn workspace_signup(
    ctx: &mut DalContext,
    jwt_secret_key: &JwtSecretKey,
) -> (WorkspaceSignup, String) {
    let workspace_name = generate_fake_name();
    let user_name = format!("frank {workspace_name}");
    let user_email = format!("{workspace_name}@example.com");
    let user_password = "snakes";

    let nw = Workspace::signup(
        ctx,
        &workspace_name,
        &user_name,
        &user_email,
        &user_password,
    )
    .await
    .expect("cannot signup a new workspace");
    let auth_token = nw
        .user
        .login(&*ctx, jwt_secret_key, "snakes")
        .await
        .expect("cannot log in newly created user");
    (nw, auth_token)
}

pub async fn create_schema(ctx: &DalContext) -> Schema {
    let name = generate_fake_name();
    Schema::new(ctx, &name, &ComponentKind::Standard)
        .await
        .expect("cannot create schema")
}

pub async fn create_schema_variant(ctx: &DalContext, schema_id: SchemaId) -> schema::SchemaVariant {
    create_schema_variant_with_root(ctx, schema_id).await.0
}

pub async fn create_schema_variant_with_root(
    ctx: &DalContext,
    schema_id: SchemaId,
) -> (schema::SchemaVariant, schema::RootProp) {
    let name = generate_fake_name();
    let (variant, root) = schema::SchemaVariant::new(ctx, schema_id, name)
        .await
        .expect("cannot create schema variant");

    let _input_socket = Socket::new(
        ctx,
        "input",
        SocketKind::Standalone,
        &SocketEdgeKind::ConfigurationInput,
        &SocketArity::Many,
        &DiagramKind::Configuration,
        Some(*variant.id()),
    )
    .await
    .expect("Unable to create socket");

    let _output_socket = Socket::new(
        ctx,
        "output",
        SocketKind::Standalone,
        &SocketEdgeKind::ConfigurationOutput,
        &SocketArity::Many,
        &DiagramKind::Configuration,
        Some(*variant.id()),
    )
    .await
    .expect("Unable to create socket");

    (variant, root)
}

pub async fn create_component_and_schema(ctx: &DalContext) -> Component {
    let schema = create_schema(ctx).await;
    let mut schema_variant = create_schema_variant(ctx, *schema.id()).await;
    schema_variant
        .finalize(ctx, None)
        .await
        .expect("unable to finalize schema variant");
    let name = generate_fake_name();
    let (component, _) = Component::new(ctx, &name, *schema_variant.id())
        .await
        .expect("cannot create component");
    component
}

#[allow(clippy::too_many_arguments)]
pub async fn create_component_for_schema_variant(
    ctx: &DalContext,
    schema_variant_id: &SchemaVariantId,
) -> Component {
    let name = generate_fake_name();
    let (component, _) = Component::new(ctx, &name, *schema_variant_id)
        .await
        .expect("cannot create component");
    component
}

#[allow(clippy::too_many_arguments)]
pub async fn create_component_for_schema(ctx: &DalContext, schema_id: &SchemaId) -> Component {
    let name = generate_fake_name();
    let (component, _) = Component::new_for_default_variant_from_schema(ctx, &name, *schema_id)
        .await
        .expect("cannot create component");
    component
}

pub async fn create_node(ctx: &DalContext, node_kind: &NodeKind) -> Node {
    Node::new(ctx, node_kind).await.expect("cannot create node")
}

/// Create a [`Prop`](dal::Prop) with a given [`PropKind`](dal::PropKind), name and parent
/// [`PropId`](dal::Prop).
pub async fn create_prop_and_set_parent(
    ctx: &DalContext,
    prop_kind: PropKind,
    name: impl AsRef<str>,
    parent_prop_id: PropId,
) -> Prop {
    let name = name.as_ref();
    let new_prop = Prop::new(ctx, name, prop_kind, None)
        .await
        .expect("cannot create prop");
    new_prop
        .set_parent_prop(ctx, parent_prop_id)
        .await
        .expect("cannot set parent to new prop");
    new_prop
}

pub async fn create_func(ctx: &DalContext) -> Func {
    let name = generate_fake_name();
    Func::new(
        ctx,
        name,
        FuncBackendKind::String,
        FuncBackendResponseType::String,
    )
    .await
    .expect("cannot create func")
}

#[allow(clippy::too_many_arguments)]
pub async fn create_func_binding(
    ctx: &DalContext,
    args: serde_json::Value,
    func_id: FuncId,
    backend_kind: FuncBackendKind,
) -> FuncBinding {
    FuncBinding::new(ctx, args, func_id, backend_kind)
        .await
        .expect("cannot create func")
}

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

pub async fn create_secret(ctx: &DalContext, key_pair_pk: KeyPairPk) -> Secret {
    let name = generate_fake_name();
    EncryptedSecret::new(
        ctx,
        &name,
        SecretObjectType::Credential,
        SecretKind::DockerHub,
        &encrypt_message(ctx, key_pair_pk, &serde_json::json!({ "name": name })).await,
        key_pair_pk,
        Default::default(),
        Default::default(),
    )
    .await
    .expect("cannot create secret")
}

#[allow(clippy::too_many_arguments)]
pub async fn create_secret_with_message(
    ctx: &DalContext,
    key_pair_pk: KeyPairPk,
    message: &serde_json::Value,
) -> Secret {
    let name = generate_fake_name();
    EncryptedSecret::new(
        ctx,
        &name,
        SecretObjectType::Credential,
        SecretKind::DockerHub,
        &encrypt_message(ctx, key_pair_pk, message).await,
        key_pair_pk,
        Default::default(),
        Default::default(),
    )
    .await
    .expect("cannot create secret")
}
