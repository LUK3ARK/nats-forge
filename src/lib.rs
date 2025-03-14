use std::collections::{HashMap, HashSet};
use crate::config::AccountConfig;
use anyhow::{Context, Result};
use tempfile::TempDir;
use uuid::Uuid;
use std::path::PathBuf;
use tokio::process::Command;

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

        let default_sys_jwt_path = self.store_dir.path()
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
        let mut creds_map: HashMap<String, Vec<(PathBuf, PathBuf)>> = HashMap::new();
        let mut name_to_unique: HashMap<String, String> = HashMap::new();

        let mut all_accounts: Vec<(usize, usize, &AccountConfig)> = Vec::new();
        for (server_idx, server) in self.config.servers.iter().enumerate() {
            std::fs::create_dir_all(&server.output_dir)?;
            for (account_idx, account) in server.accounts.iter().enumerate() {
                all_accounts.push((server_idx, account_idx, account));
                name_to_unique.insert(account.name.clone(), account.unique_name.clone());
            }
        }

        let mut dependencies: HashMap<String, HashSet<String>> = HashMap::new();
        for (_, _, account) in &all_accounts {
            let account_unique_name = &account.unique_name;
            dependencies.entry(account_unique_name.clone()).or_default();
            for import in &account.imports {
                let src_unique_name = name_to_unique.get(&import.account)
                    .ok_or_else(|| anyhow::anyhow!("Unknown import account: {}", import.account))?;
                dependencies.entry(src_unique_name.clone()).or_default().insert(account_unique_name.clone());
            }
        }

        let sorted_accounts = topological_sort(&dependencies)?;

        for account_unique_name in &sorted_accounts {
            if let Some((server_idx, _, account)) = all_accounts.iter().find(|(_, _, a)| a.unique_name == *account_unique_name) {
                let server = &self.config.servers[*server_idx];
                let abs_output_dir = std::fs::canonicalize(&server.output_dir)?;
                std::fs::create_dir_all(&abs_output_dir)?;

                let account_jwt = if account.name == "SYS" && account.is_system_account {
                    default_sys_jwt.clone()
                } else {
                    create_account(account, &self.config.operator.name, self.store_dir.path()).await?
                };
                let account_jwt_path = abs_output_dir.join(format!("{}.jwt", account.name));
                std::fs::write(&account_jwt_path, &account_jwt)?;
                account_jwt_paths.push(account_jwt_path.clone());
                account_jwts.insert(account.name.clone(), account_jwt);

                for user in &account.users {
                    let creds_path = create_user(account, user, &abs_output_dir, self.store_dir.path()).await?;
                    let filename = creds_path.file_name().unwrap().to_string_lossy().to_string();
                    creds_map.entry(filename.clone()).or_insert_with(Vec::new).push((creds_path.clone(), server.output_dir.clone()));
                    user_creds_paths.push(creds_path);
                }
            }
        }

        for (_, _, account) in &all_accounts {
            for (i, import) in account.imports.iter().enumerate() {
                let import_name = format!("import-{}", i);
                let src_unique_name = name_to_unique.get(&import.account)
                    .ok_or_else(|| anyhow::anyhow!("Unknown import account: {}", import.account))?;
                let mut import_args = vec![
                    "add".to_string(), "import".to_string(),
                    "--name".to_string(), import_name,
                    "--src-account".to_string(), src_unique_name.clone(),
                    "--remote-subject".to_string(), import.subject.clone(),
                    "--account".to_string(), account.unique_name.clone(),
                    "--data-dir".to_string(), self.store_dir.path().to_str().unwrap().to_string(),
                ];
                if let Some(local_subject) = &import.local_subject {
                    import_args.push("--local-subject".to_string());
                    import_args.push(local_subject.clone());
                }
                if import.service {
                    import_args.push("--service".to_string());
                }
                let import_output = Command::new("nsc").args(&import_args).output().await
                    .context(format!("Failed to add import {}", import.subject))?;
                if !import_output.status.success() {
                    return Err(anyhow::anyhow!("nsc add import failed: {}", String::from_utf8_lossy(&import_output.stderr)));
                }
            }
        }

        for server in &self.config.servers {
            let abs_output_dir = std::fs::canonicalize(&server.output_dir)?;
            for remote in &server.leafnodes.remotes {
                if let Some(creds_entries) = creds_map.get(&remote.credentials) {
                    let (source_path, _) = creds_entries.iter().find(|(path, _)| path.exists())
                        .ok_or_else(|| anyhow::anyhow!("No existing creds file for {}", remote.credentials))?;

                    let source_content = std::fs::read_to_string(source_path)?;
                    let abs_dest = abs_output_dir.join(&remote.credentials);
                    std::fs::write(&abs_dest, &source_content)?;
                } else {
                    return Err(anyhow::anyhow!("No creds entry found for {}", remote.credentials));
                }
            }

            for (account_name, account_jwt) in &account_jwts {
                let dest_jwt_path = abs_output_dir.join(format!("{}.jwt", account_name));
                std::fs::write(&dest_jwt_path, account_jwt)?;
            }
        }

        for server in &self.config.servers {
            let abs_output_dir = std::fs::canonicalize(&server.output_dir)?;
            let mut resolver_preload = Vec::new();
            let mut system_account_id = None;

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
            let server_config_path = abs_output_dir.join("nats.conf");
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

fn topological_sort(deps: &HashMap<String, HashSet<String>>) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut temp = HashSet::new();

    fn visit(
        node: &str,
        deps: &HashMap<String, HashSet<String>>,
        visited: &mut HashSet<String>,
        temp: &mut HashSet<String>,
        result: &mut Vec<String>,
    ) -> Result<()> {
        if temp.contains(node) {
            return Err(anyhow::anyhow!("Circular dependency detected at {}", node));
        }
        if visited.contains(node) {
            return Ok(());
        }
        temp.insert(node.to_string());
        if let Some(children) = deps.get(node) {
            for child in children {
                visit(child, deps, visited, temp, result)?;
            }
        }
        temp.remove(node);
        visited.insert(node.to_string());
        result.push(node.to_string());
        Ok(())
    }

    for node in deps.keys() {
        if !visited.contains(node) {
            visit(node, deps, &mut visited, &mut temp, &mut result)?;
        }
    }
    Ok(result)
}
