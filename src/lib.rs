use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tempfile::TempDir;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct NatsConfig {
    pub operator: OperatorConfig,
    pub accounts: Vec<AccountConfig>,
    pub output_dir: PathBuf,
    pub server_options: ServerOptions,
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
    pub expiry: Option<String>, // ISO 8601 timestamp
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

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerOptions {
    pub port: u16,
    pub jetstream: bool,
    pub resolver: ResolverType,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ResolverType {
    Memory,
    Url(String),
}

pub struct NatsSetup {
    config: NatsConfig,
    temp_dir: Option<TempDir>,
    store_dir: TempDir,
}

#[derive(Debug)]
pub struct SetupResult {
    pub operator_jwt_path: PathBuf,
    pub account_jwt_paths: Vec<PathBuf>,
    pub user_creds_paths: Vec<PathBuf>,
    pub server_config_path: PathBuf,
}

impl NatsSetup {
    pub fn from_json_file(path: &str) -> Result<Self> {
        let file = std::fs::File::open(path).context("Failed to open JSON config")?;
        let mut config: NatsConfig = serde_json::from_reader(file).context("Failed to parse JSON config")?;
        let store_dir = TempDir::new().expect("Failed to create temp store dir");
        let unique_operator_name = format!("{}-{}", config.operator.name, Uuid::new_v4());
        config.operator.name = unique_operator_name.clone();
        for account in &mut config.accounts {
            if account.unique_name.is_empty() {
                account.unique_name = format!("{}-{}", account.name, Uuid::new_v4());
            }
        }
        Ok(NatsSetup {
            config,
            temp_dir: None,
            store_dir,
        })
    }

    pub fn new(mut config: NatsConfig) -> Self {
        let store_dir = TempDir::new().expect("Failed to create temp store dir");
        let unique_operator_name = format!("{}-{}", config.operator.name, Uuid::new_v4());
        config.operator.name = unique_operator_name.clone();
        for account in &mut config.accounts {
            if account.unique_name.is_empty() {
                account.unique_name = format!("{}-{}", account.name, Uuid::new_v4());
            }
        }
        NatsSetup {
            config,
            temp_dir: None,
            store_dir,
        }
    }

    pub fn for_test(mut config: NatsConfig) -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let store_dir = TempDir::new().expect("Failed to create temp store dir");
        let unique_operator_name = format!("{}-{}", config.operator.name, Uuid::new_v4());
        config.operator.name = unique_operator_name;
        for account in &mut config.accounts {
            if account.unique_name.is_empty() {
                account.unique_name = format!("{}-{}", account.name, Uuid::new_v4());
            }
        }
        config.output_dir = temp_dir.path().to_path_buf();
        NatsSetup {
            config,
            temp_dir: Some(temp_dir),
            store_dir,
        }
    }

    pub async fn initialize(&self) -> Result<SetupResult> {
        std::fs::create_dir_all(&self.config.output_dir)
            .context("Failed to create output directory")?;

        let store_path = self.store_dir.path().to_str().unwrap();

        let operator_jwt = if self.config.operator.reuse_existing {
            unimplemented!("Reusing existing operator not yet implemented");
        } else {
            self.create_operator().await?
        };
        let operator_jwt_path = self.config.output_dir.join("operator.jwt");
        std::fs::write(&operator_jwt_path, &operator_jwt)
            .context("Failed to write operator JWT")?;

        let delete_output = tokio::process::Command::new("nsc")
            .args(&["delete", "account", "--name", "SYS", "--data-dir", store_path])
            .output()
            .await
            .context("Failed to delete default SYS account")?;
        if !delete_output.status.success() && !String::from_utf8_lossy(&delete_output.stderr).contains("account not found") {
            return Err(anyhow::anyhow!(
                "nsc delete account SYS failed: {}",
                String::from_utf8_lossy(&delete_output.stderr)
            ));
        }

        let mut account_jwt_paths = Vec::new();
        let mut user_creds_paths = Vec::new();
        let mut resolver_preload = Vec::new();
        let mut system_account_id = None;

        for account in &self.config.accounts {
            let account_jwt = self.create_account(account).await?;
            let account_jwt_path = self.config.output_dir.join(format!("{}.jwt", account.name));
            std::fs::write(&account_jwt_path, &account_jwt)
                .context(format!("Failed to write JWT for account {}", account.name))?;
            account_jwt_paths.push(account_jwt_path);

            let account_id = Self::extract_account_id(&account_jwt)?;
            if account.is_system_account {
                system_account_id = Some(account_id.clone());
            }
            resolver_preload.push(format!("    {}: \"{}\"", account_id, account_jwt));

            for user in &account.users {
                let creds_path = self.create_user(account, user).await?;
                user_creds_paths.push(creds_path);
            }
        }

        let system_account_id = system_account_id
            .ok_or_else(|| anyhow::anyhow!("No system account specified"))?;

        let server_config = self.generate_server_config(
            &operator_jwt,
            &system_account_id,
            &resolver_preload.join("\n"),
        );
        let server_config_path = self.config.output_dir.join("nats.conf");
        std::fs::write(&server_config_path, &server_config)
            .context("Failed to write server config")?;

        Ok(SetupResult {
            operator_jwt_path,
            account_jwt_paths,
            user_creds_paths,
            server_config_path,
        })
    }

    async fn create_operator(&self) -> Result<String> {
        let operator_name = &self.config.operator.name;
        let store_path = self.store_dir.path().to_str().unwrap();

        let output = tokio::process::Command::new("nsc")
            .args(&["init", "--name", operator_name, "--dir", store_path, "--data-dir", store_path])
            .output()
            .await
            .context("Failed to run nsc init")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "nsc init failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let operator_jwt_path = self
            .store_dir
            .path()
            .join(operator_name)
            .join(format!("{}.jwt", operator_name));
        let operator_jwt = std::fs::read_to_string(&operator_jwt_path)
            .context("Failed to read operator JWT")?;

        Ok(operator_jwt)
    }
    async fn create_account(&self, account: &AccountConfig) -> Result<String> {
        let store_path = self.store_dir.path().to_str().unwrap();
        let mut args = vec![
            "add".to_string(),
            "account".to_string(),
            "--name".to_string(),
            account.unique_name.clone(),
            "--data-dir".to_string(),
            store_path.to_string(),
        ];

        if let Some(max_conn) = account.max_connections {
            args.push("--max-conns".to_string());
            args.push(max_conn.to_string());
        }

        if let Some(max_payload) = account.max_payload {
            args.push("--max-payload".to_string());
            args.push(max_payload.to_string());
        }

        let output = tokio::process::Command::new("nsc")
            .args(&args)
            .output()
            .await
            .context(format!("Failed to run nsc add account {}", account.unique_name))?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "nsc add account failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Handle exports
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
                return Err(anyhow::anyhow!(
                    "nsc add export failed: {}",
                    String::from_utf8_lossy(&export_output.stderr)
                ));
            }
        }

        // Handle imports
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
                return Err(anyhow::anyhow!(
                    "nsc add import failed: {}",
                    String::from_utf8_lossy(&import_output.stderr)
                ));
            }
        }

        let account_jwt_path = self
            .store_dir
            .path()
            .join(&self.config.operator.name)
            .join("accounts")
            .join(&account.unique_name)
            .join(format!("{}.jwt", account.unique_name));
        let account_jwt = std::fs::read_to_string(&account_jwt_path)
            .context(format!("Failed to read JWT for account {}", account.unique_name))?;

        Ok(account_jwt)
    }

    async fn create_user(&self, account: &AccountConfig, user: &UserConfig) -> Result<PathBuf> {
        let store_path = self.store_dir.path().to_str().unwrap();
        let creds_path = self
            .config
            .output_dir
            .join(format!("{}-{}.creds", account.name, user.name));

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
            args.push("--expiry".to_string());
            args.push(expiry.clone());
        }

        let output = tokio::process::Command::new("nsc")
            .args(&args)
            .output()
            .await
            .context(format!("Failed to run nsc add user {}", user.name))?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "nsc add user failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

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
            return Err(anyhow::anyhow!(
                "nsc generate creds failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(creds_path)
    }

    fn generate_server_config(&self, operator_jwt: &str, system_account: &str, resolver_preload: &str) -> String {
        let mut config = format!(
            "port: {}\nserver_name: \"natsforge_server\"\n\n",
            self.config.server_options.port
        );

        if self.config.server_options.jetstream {
            config.push_str(&format!(
                "jetstream {{\n    store_dir: \"{}/jetstream\"\n    domain: \"core\"\n}}\n\n",
                self.config.output_dir.display()
            ));
        }

        config.push_str(&format!("operator: \"{}\"\n", operator_jwt));
        config.push_str(&format!("system_account: \"{}\"\n", system_account));
        match &self.config.server_options.resolver {
            ResolverType::Memory => {
                config.push_str("resolver: MEMORY\n");
                if !resolver_preload.is_empty() {
                    config.push_str("resolver_preload: {\n");
                    config.push_str(resolver_preload);
                    config.push_str("\n}\n");
                }
            }
            ResolverType::Url(url) => {
                config.push_str(&format!("resolver: URL({})\n", url));
            }
        }

        config
    }

    fn extract_account_id(jwt: &str) -> Result<String> {
        let parts: Vec<&str> = jwt.split('.').collect();
        if parts.len() != 3 {
            return Err(anyhow::anyhow!("Invalid JWT format"));
        }
        let payload = BASE64
            .decode(parts[1])
            .context("Failed to decode JWT payload")?;
        let payload_str = String::from_utf8(payload).context("JWT payload is not UTF-8")?;
        let json: serde_json::Value = serde_json::from_str(&payload_str).context("Failed to parse JWT JSON")?;
        json["sub"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| anyhow::anyhow!("No 'sub' field in JWT"))
    }
}

impl Drop for NatsSetup {
    fn drop(&mut self) {
        if let Some(temp_dir) = self.temp_dir.take() {
            drop(temp_dir);
        }
    }
}
