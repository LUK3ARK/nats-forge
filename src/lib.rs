use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tempfile::TempDir;
use base64::engine::general_purpose::STANDARD_NO_PAD as BASE64;
use base64::Engine;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct DeploymentConfig {
    pub name: String,
    pub operator: OperatorConfig,
    pub servers: Vec<ServerConfig>,
}

#[derive(Debug, Serialize,Deserialize)]
pub struct ServerConfig {
    pub name: String,
    pub port: u16,
    pub jetstream: Option<JetStreamConfig>,
    pub leafnodes: Option<LeafNodeConfig>,
    pub accounts: Vec<AccountConfig>,
    pub output_dir: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JetStreamConfig {
    pub enabled: bool,
    pub store_dir: String,
    pub domain: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LeafNodeConfig {
    pub port: Option<u16>,              // Hub: Listening port for leaf connections
    pub remotes: Option<Vec<RemoteConfig>>, // Leaf: Outbound connections to hub
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoteConfig {
    pub url: String,          // e.g., "nats://<validator_ip>:4248"
    pub account: String,      // e.g., "solana-arbx-searcher"
    pub credentials: String,  // Filename, e.g., "searcher-user.creds"
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OperatorConfig {
    pub name: String,
    pub reuse_existing: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountConfig {
    pub name: String,
    pub users: Vec<UserConfig>,
    pub is_system_account: bool,
    pub unique_name: String,
    pub max_connections: Option<i32>,
    pub max_payload: Option<i64>,
    pub exports: Vec<ExportConfig>,
    pub imports: Vec<ImportConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserConfig {
    pub name: String,
    pub allowed_subjects: Vec<String>,
    pub denied_subjects: Vec<String>,
    pub expiry: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportConfig {
    pub subject: String,
    pub is_service: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImportConfig {
    pub subject: String,
    pub account: String,
}

#[derive(Debug)]
pub struct SetupResult {
    pub operator_jwt_path: PathBuf,
    pub account_jwt_paths: Vec<PathBuf>,
    pub user_creds_paths: Vec<PathBuf>,
    pub server_config_paths: Vec<PathBuf>,
}

pub struct NatsDeployment {
    config: DeploymentConfig,
    store_dir: TempDir,
}

impl NatsDeployment {
    pub fn from_json_file(path: &str) -> Result<Self> {
        let file = std::fs::File::open(path).context("Failed to open JSON config")?;
        let mut config: DeploymentConfig = serde_json::from_reader(file).context("Failed to parse JSON")?;
        let store_dir = TempDir::new().context("Failed to create temp store dir")?;

        // Ensure unique names
        let unique_operator_name = format!("{}-{}", config.operator.name, Uuid::new_v4());
        config.operator.name = unique_operator_name;
        for server in &mut config.servers {
            for account in &mut server.accounts {
                if account.unique_name.is_empty() {
                    account.unique_name = format!("{}-{}", account.name, Uuid::new_v4());
                }
            }
        }

        Ok(NatsDeployment { config, store_dir })
    }

    pub async fn initialize(&self) -> Result<SetupResult> {
        let operator_jwt = self.create_operator().await?;
        let operator_jwt_path = self.config.servers[0].output_dir.join("operator.jwt"); // Shared operator
        std::fs::write(&operator_jwt_path, &operator_jwt).context("Failed to write operator JWT")?;

        let mut account_jwt_paths = Vec::new();
        let mut user_creds_paths = Vec::new();
        let mut server_config_paths = Vec::new();

        for server in &self.config.servers {
            std::fs::create_dir_all(&server.output_dir).context("Failed to create output dir")?;

            let mut resolver_preload = Vec::new();
            let mut system_account_id = None;

            for account in &server.accounts {
                let account_jwt = self.create_account(account).await?;
                let account_jwt_path = server.output_dir.join(format!("{}.jwt", account.name));
                std::fs::write(&account_jwt_path, &account_jwt)
                    .context(format!("Failed to write JWT for account {}", account.name))?;
                account_jwt_paths.push(account_jwt_path.clone());

                let account_id = Self::extract_account_id(&account_jwt)?;
                if account.is_system_account {
                    system_account_id = Some(account_id.clone());
                }
                resolver_preload.push(format!("    {}: \"{}\"", account_id, account_jwt));

                for user in &account.users {
                    let creds_path = self.create_user(account, user, server).await?;
                    user_creds_paths.push(creds_path);
                }
            }

            let system_account_id = system_account_id
                .ok_or_else(|| anyhow::anyhow!("No system account for server {}", server.name))?;
            let server_config = self.generate_server_config(server, &operator_jwt, &system_account_id, &resolver_preload.join("\n"));
            let server_config_path = server.output_dir.join("nats.conf");
            std::fs::write(&server_config_path, &server_config)
                .context(format!("Failed to write server config for {}", server.name))?;
            server_config_paths.push(server_config_path);
        }

        Ok(SetupResult {
            operator_jwt_path,
            account_jwt_paths,
            user_creds_paths,
            server_config_paths,
        })
    }

    async fn create_operator(&self) -> Result<String> {
        let store_path = self.store_dir.path().to_str().unwrap();
        let output = tokio::process::Command::new("nsc")
            .args(&["init", "--name", &self.config.operator.name, "--data-dir", store_path])
            .output()
            .await
            .context("Failed to run nsc init")?;

        if !output.status.success() {
            println!("nsc init stdout: {}", String::from_utf8_lossy(&output.stdout));
            println!("nsc init stderr: {}", String::from_utf8_lossy(&output.stderr));
            return Err(anyhow::anyhow!("nsc init failed: {}", String::from_utf8_lossy(&output.stderr)));
        }

        let operator_jwt_path = self.store_dir.path()
            .join(&self.config.operator.name)
            .join(format!("{}.jwt", &self.config.operator.name));
        std::fs::read_to_string(&operator_jwt_path)
            .context("Failed to read operator JWT")
    }

    async fn create_account(&self, account: &AccountConfig) -> Result<String> {
        let store_path = self.store_dir.path().to_str().unwrap();
        let args = vec![
            "add".to_string(),
            "account".to_string(),
            "--name".to_string(),
            account.unique_name.clone(),
            "--data-dir".to_string(),
            store_path.to_string(),
        ];

        println!("nsc add account args: {:?}", args);
        let output = tokio::process::Command::new("nsc")
            .args(&args)
            .output()
            .await
            .context(format!("Failed to run nsc add account {}", account.unique_name))?;

        if !output.status.success() {
            println!("nsc add account stdout: {}", String::from_utf8_lossy(&output.stdout));
            println!("nsc add account stderr: {}", String::from_utf8_lossy(&output.stderr));
            return Err(anyhow::anyhow!("nsc add account failed: {}", String::from_utf8_lossy(&output.stderr)));
        }

        let mut edit_args = vec![
            "edit".to_string(),
            "account".to_string(),
            "--name".to_string(),
            account.unique_name.clone(),
            "--data-dir".to_string(),
            store_path.to_string(),
        ];
        let mut should_edit = false;

        if let Some(max_conn) = account.max_connections {
            edit_args.push("--conns".to_string());
            edit_args.push(max_conn.to_string());
            should_edit = true;
        }

        if let Some(max_payload) = account.max_payload {
            edit_args.push("--data".to_string());
            edit_args.push(max_payload.to_string());
            should_edit = true;
        }

        if should_edit {
            let edit_output = tokio::process::Command::new("nsc")
                .args(&edit_args)
                .output()
                .await
                .context(format!("Failed to run nsc edit account {}", account.unique_name))?;
            if !edit_output.status.success() {
                return Err(anyhow::anyhow!("nsc edit account failed: {}", String::from_utf8_lossy(&edit_output.stderr)));
            }
        }

        for export in &account.exports {
            let mut export_args = vec![
                "add".to_string(),
                "export".to_string(),
                "--name".to_string(),
                export.subject.clone(),
                "--subject".to_string(),
                export.subject.clone(),
                "--account".to_string(),
                account.unique_name.clone(),
                "--data-dir".to_string(),
                store_path.to_string(),
            ];
            if export.is_service {
                export_args.push("--service".to_string());
            }
            let export_output = tokio::process::Command::new("nsc")
                .args(&export_args)
                .output()
                .await
                .context(format!("Failed to add export {}", export.subject))?;
            if !export_output.status.success() {
                return Err(anyhow::anyhow!("nsc add export failed: {}", String::from_utf8_lossy(&export_output.stderr)));
            }
        }

        for import in &account.imports {
            let import_args = vec![
                "add".to_string(),
                "import".to_string(),
                "--src-account".to_string(),
                import.account.clone(),
                "--subject".to_string(),
                import.subject.clone(),
                "--account".to_string(),
                account.unique_name.clone(),
                "--data-dir".to_string(),
                store_path.to_string(),
            ];
            let import_output = tokio::process::Command::new("nsc")
                .args(&import_args)
                .output()
                .await
                .context(format!("Failed to add import {}", import.subject))?;
            if !import_output.status.success() {
                return Err(anyhow::anyhow!("nsc add import failed: {}", String::from_utf8_lossy(&import_output.stderr)));
            }
        }

        let account_jwt_path = self.store_dir.path()
            .join(&self.config.operator.name)
            .join("accounts")
            .join(&account.unique_name)
            .join(format!("{}.jwt", account.unique_name));
        std::fs::read_to_string(&account_jwt_path)
            .context(format!("Failed to read JWT for account {}", account.unique_name))
    }

    async fn create_user(&self, account: &AccountConfig, user: &UserConfig, server: &ServerConfig) -> Result<PathBuf> {
        let store_path = self.store_dir.path().to_str().unwrap();
        let creds_path = server.output_dir.join(format!("{}-{}.creds", account.name, user.name));

        let mut args = vec![
            "add".to_string(),
            "user".to_string(),
            "--name".to_string(),
            user.name.clone(),
            "--account".to_string(),
            account.unique_name.clone(),
            "--data-dir".to_string(),
            store_path.to_string(),
        ];

        if !user.allowed_subjects.is_empty() {
            args.push("--allow-pubsub".to_string());
            args.push(user.allowed_subjects.join(","));
        }

        if !user.denied_subjects.is_empty() {
            args.push("--deny-pubsub".to_string());
            args.push(user.denied_subjects.join(","));
        }

        if let Some(expiry) = &user.expiry {
            let nsc_expiry = if expiry.contains('T') {
                expiry.split('T').next().unwrap_or(expiry).to_string()
            } else {
                expiry.clone()
            };
            args.push("--expiry".to_string());
            args.push(nsc_expiry);
        }

        let output = tokio::process::Command::new("nsc")
            .args(&args)
            .output()
            .await
            .context(format!("Failed to run nsc add user {}", user.name))?;

        if !output.status.success() {
            println!("nsc add user stderr: {}", String::from_utf8_lossy(&output.stderr));
            return Err(anyhow::anyhow!("nsc add user failed: {}", String::from_utf8_lossy(&output.stderr)));
        }

        let _ = std::fs::remove_file(&creds_path);
        let output = tokio::process::Command::new("nsc")
            .args(&[
                "generate".to_string(),
                "creds".to_string(),
                "--account".to_string(),
                account.unique_name.clone(),
                "--name".to_string(),
                user.name.clone(),
                "--output-file".to_string(),
                creds_path.to_str().unwrap().to_string(),
                "--data-dir".to_string(),
                store_path.to_string(),
            ])
            .output()
            .await
            .context(format!("Failed to generate creds for user {}", user.name))?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("nsc generate creds failed: {}", String::from_utf8_lossy(&output.stderr)));
        }

        Ok(creds_path)
    }

    fn generate_server_config(&self, server: &ServerConfig, operator_jwt: &str, system_account: &str, resolver_preload: &str) -> String {
        let mut config = format!(
            "port: {}\nserver_name: \"{}\"\n\n",
            server.port, server.name
        );

        if let Some(js) = &server.jetstream {
            if js.enabled {
                config.push_str(&format!(
                    "jetstream {{\n    store_dir: \"{}\"\n    domain: \"{}\"\n}}\n\n",
                    js.store_dir, js.domain
                ));
            }
        }

        if let Some(leaf) = &server.leafnodes {
            config.push_str("leafnodes {\n");
            if let Some(port) = leaf.port {
                config.push_str(&format!("    port: {}\n", port));
            }
            if let Some(remotes) = &leaf.remotes {
                config.push_str("    remotes = [\n");
                for remote in remotes {
                    config.push_str(&format!(
                        "        {{ url: \"{}\", account: \"{}\", credentials: \"{}\" }}\n",
                        remote.url, remote.account, remote.credentials
                    ));
                }
                config.push_str("    ]\n");
            }
            config.push_str("}\n\n");
        }

        config.push_str(&format!("operator: \"{}\"\n", operator_jwt));
        config.push_str(&format!("system_account: \"{}\"\n", system_account));
        config.push_str("resolver: MEMORY\n");
        if !resolver_preload.is_empty() {
            config.push_str("resolver_preload: {\n");
            config.push_str(resolver_preload);
            config.push_str("\n}\n");
        }

        config
    }

    fn extract_account_id(jwt: &str) -> Result<String> {
        let parts: Vec<&str> = jwt.split('.').collect();
        if parts.len() != 3 {
            return Err(anyhow::anyhow!("Invalid JWT format: {} parts", parts.len()));
        }
        let payload = BASE64.decode(parts[1]).context("Failed to decode JWT payload")?;
        let payload_str = String::from_utf8(payload).context("JWT payload is not UTF-8")?;
        let json: serde_json::Value = serde_json::from_str(&payload_str).context("Failed to parse JWT JSON")?;
        json["sub"].as_str().map(String::from)
            .ok_or_else(|| anyhow::anyhow!("No 'sub' field in JWT"))
    }
}

impl Drop for NatsDeployment {
    fn drop(&mut self) {
        // TempDir cleans up automatically
    }
}
