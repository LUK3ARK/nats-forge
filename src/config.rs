use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct NatsConfig {
    pub name: Option<String>,
    pub operator: OperatorConfig,
    #[serde(default)]
    pub servers: Vec<ServerConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerConfig {
    pub name: String,
    pub port: u16,
    #[serde(default)]
    pub jetstream: JetStreamConfig,
    #[serde(default)]
    pub leafnodes: LeafNodeConfig,
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
    pub output_dir: PathBuf,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct JetStreamConfig {
    pub enabled: bool,
    pub store_dir: Option<String>,
    pub domain: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LeafNodeConfig {
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub remotes: Vec<RemoteConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoteConfig {
    pub url: String,
    pub account: String,
    pub credentials: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OperatorConfig {
    pub name: String,
    #[serde(default)]
    pub reuse_existing: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountConfig {
    pub name: String,
    #[serde(default)]
    pub users: Vec<UserConfig>,
    #[serde(default)]
    pub is_system_account: bool,
    #[serde(default)]
    pub unique_name: String,
    #[serde(default)]
    pub max_connections: Option<i32>,
    #[serde(default)]
    pub max_payload: Option<i64>,
    #[serde(default)]
    pub exports: Vec<ExportConfig>,
    #[serde(default)]
    pub imports: Vec<ImportConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserConfig {
    pub name: String,
    #[serde(default)]
    pub allowed_subjects: Vec<String>,
    #[serde(default)]
    pub denied_subjects: Vec<String>,
    #[serde(default)]
    pub expiry: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExportConfig {
    pub subject: String,
    #[serde(default)]
    pub is_service: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImportConfig {
    pub subject: String,
    pub account: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ResolverType {
    Memory,
    Url(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerOptions {
    pub port: u16,
    #[serde(default)]
    pub jetstream: bool,
    pub resolver: ResolverType,
}

#[derive(Debug)]
pub struct SetupResult {
    pub operator_jwt_path: PathBuf,
    pub account_jwt_paths: Vec<PathBuf>,
    pub user_creds_paths: Vec<PathBuf>,
    pub server_config_path: PathBuf,
    pub server_config_paths: Option<Vec<PathBuf>>,
}
