use std::collections::HashMap;

use anyhow::{Context, Result};
use tempfile::TempDir;
use uuid::Uuid;

use crate::{
    config::{NatsConfig, SetupResult},
    nsc::{create_account, create_operator, create_user, extract_account_id},
    server::generate_server_config,
};

pub mod config;
mod nsc;
mod server;

pub struct NatsForge {
    config: NatsConfig,
    store_dir: TempDir,
}

impl NatsForge {
    pub fn new(mut config: NatsConfig) -> Self {
        let store_dir = TempDir::new().expect("Failed to create temp store dir");
        let unique_operator_name = format!("{}-{}", config.operator.name, Uuid::new_v4());
        config.operator.name = unique_operator_name;

        // Generate unique names for accounts if not already set
        for server in &mut config.servers {
            for account in &mut server.accounts {
                if account.unique_name.is_empty() {
                    account.unique_name = format!("{}-{}", account.name, Uuid::new_v4());
                }
            }
        }

        NatsForge { config, store_dir }
    }

    pub fn from_config(mut config: NatsConfig) -> Result<Self> {
        let store_dir = TempDir::new().context("Failed to create temp store dir")?;
        let unique_operator_name = format!("{}-{}", config.operator.name, Uuid::new_v4());
        config.operator.name = unique_operator_name;

        for server in &mut config.servers {
            for account in &mut server.accounts {
                if account.unique_name.is_empty() {
                    account.unique_name = format!("{}-{}", account.name, Uuid::new_v4());
                }
            }
        }

        Ok(NatsForge { config, store_dir })
    }

    pub fn from_json_file(path: &str) -> Result<Self> {
        let file = std::fs::File::open(path).context("Failed to open JSON config")?;
        let mut config: NatsConfig = serde_json::from_reader(file).context("Failed to parse JSON config")?;
        let store_dir = TempDir::new().context("Failed to create temp store dir")?;
        let unique_operator_name = format!("{}-{}", config.operator.name, Uuid::new_v4());
        config.operator.name = unique_operator_name;
        for server in &mut config.servers {
            for account in &mut server.accounts {
                if account.unique_name.is_empty() {
                    account.unique_name = format!("{}-{}", account.name, Uuid::new_v4());
                }
            }
        }
        Ok(NatsForge { config, store_dir })
    }

    pub async fn initialize(&self) -> Result<SetupResult> {
        let operator_jwt = create_operator(&self.config.operator, &self.store_dir.path().to_path_buf()).await?;
        let operator_jwt_path = self.config.servers[0].output_dir.join("operator.jwt");
        std::fs::create_dir_all(operator_jwt_path.parent().unwrap())?;
        std::fs::write(&operator_jwt_path, &operator_jwt)?;

        let default_sys_jwt_path = self
            .store_dir
            .path()
            .join(&self.config.operator.name)
            .join("accounts")
            .join("SYS")
            .join("SYS.jwt");
        let default_sys_jwt = std::fs::read_to_string(&default_sys_jwt_path)?;
        let default_sys_id = extract_account_id(&default_sys_jwt)?;

        let mut account_jwt_paths = Vec::new();
        let mut user_creds_paths = Vec::new();
        let mut server_config_paths = Vec::new();
        let mut account_jwts = HashMap::new();
        let mut creds_map = HashMap::new();

        // Collect all accounts and creds across all servers
        for server in &self.config.servers {
            std::fs::create_dir_all(&server.output_dir)?;
            for account in &server.accounts {
                let account_jwt = if account.name == "SYS" && account.is_system_account {
                    default_sys_jwt.clone()
                } else {
                    create_account(
                        account,
                        &self.config.operator.name,
                        &self.store_dir.path().to_path_buf(),
                    )
                    .await?
                };
                let account_jwt_path = server.output_dir.join(format!("{}.jwt", account.name));
                std::fs::write(&account_jwt_path, &account_jwt)?;
                account_jwt_paths.push(account_jwt_path.clone());
                account_jwts.insert(account.name.clone(), account_jwt);

                for user in &account.users {
                    let creds_path =
                        create_user(account, user, &server.output_dir, self.store_dir.path()).await?;
                    let filename = creds_path.file_name().unwrap().to_string_lossy().to_string();
                    creds_map.insert(filename.clone(), (creds_path.clone(), server.output_dir.clone()));
                    user_creds_paths.push(creds_path);
                }
            }
        }

        // Distribute creds and JWTs to all servers
        for server in &self.config.servers {
            // Copy creds for remotes
            for remote in &server.leafnodes.remotes {
                if let Some((source_path, source_dir)) = creds_map.get(&remote.credentials) {
                    let abs_source = source_dir.join(source_path.file_name().unwrap());
                    let abs_dest = server.output_dir.join(&remote.credentials);
                    println!("Copying creds from {} to {}", abs_source.display(), abs_dest.display());
                    std::fs::copy(&abs_source, &abs_dest).context(format!(
                        "Failed to copy creds from {} to {}",
                        abs_source.display(),
                        abs_dest.display()
                    ))?;
                }
            }

            // Copy all account JWTs to every server
            for (account_name, account_jwt) in &account_jwts {
                let dest_jwt_path = server.output_dir.join(format!("{}.jwt", account_name));
                std::fs::write(&dest_jwt_path, account_jwt).context(format!(
                    "Failed to copy JWT for {} to {}",
                    account_name,
                    dest_jwt_path.display()
                ))?;
            }
        }

        // Generate configs with all accounts preloaded
        for server in &self.config.servers {
            let mut resolver_preload = Vec::new();
            let mut system_account_id = None;

            // Preload all accounts, not just local ones
            for (account_name, account_jwt) in &account_jwts {
                let account_id = extract_account_id(account_jwt)?;
                if account_name == "SYS" && server.accounts.iter().any(|a| a.name == "SYS" && a.is_system_account) {
                    system_account_id = Some(account_id.clone());
                }
                resolver_preload.push(format!("    {}: \"{}\"", account_id, account_jwt));
            }

            let system_account_id = system_account_id.unwrap_or(default_sys_id.clone());
            if !resolver_preload.iter().any(|entry| entry.contains(&default_sys_id)) {
                resolver_preload.push(format!("    {}: \"{}\"", default_sys_id, default_sys_jwt));
            }

            let server_config = generate_server_config(
                server,
                &operator_jwt,
                &system_account_id,
                &resolver_preload.join("\n"),
                &account_jwts,
            );
            let server_config_path = server.output_dir.join("nats.conf");
            std::fs::write(&server_config_path, &server_config)?;
            server_config_paths.push(server_config_path);
        }

        Ok(SetupResult {
            operator_jwt_path,
            account_jwt_paths,
            user_creds_paths,
            server_config_path: self.config.servers[0].output_dir.join("nats.conf"),
            server_config_paths: Some(server_config_paths),
        })
    }
}
