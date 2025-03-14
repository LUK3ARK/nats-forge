use std::path::PathBuf;

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD_NO_PAD as BASE64, Engine};
use tokio::process::Command;

use crate::config::{AccountConfig, OperatorConfig, UserConfig};

pub async fn create_operator(operator: &OperatorConfig, store_dir: &PathBuf) -> Result<String> {
    if operator.reuse_existing {
        let operator_jwt_path = store_dir.join(&operator.name).join(format!("{}.jwt", &operator.name));
        if operator_jwt_path.exists() {
            return std::fs::read_to_string(&operator_jwt_path).context("Failed to read existing operator JWT");
        } else {
            return Err(anyhow::anyhow!(
                "reuse_existing set, but no operator JWT found at {}",
                operator_jwt_path.display()
            ));
        }
    }

    let store_path = store_dir.to_str().unwrap();

    // Ensure store directory exists
    std::fs::create_dir_all(store_dir).context("Failed to create store directory")?;

    let output = Command::new("nsc")
        .args([
            "init",
            "--name",
            &operator.name,
            "--dir",
            store_path,
            "--data-dir",
            store_path,
        ])
        .output()
        .await
        .context("Failed to run nsc init")?;

    if !output.status.success() {
        println!("nsc init stdout: {}", String::from_utf8_lossy(&output.stdout));
        println!("nsc init stderr: {}", String::from_utf8_lossy(&output.stderr));
        return Err(anyhow::anyhow!(
            "nsc init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    println!("nsc init stdout: {}", String::from_utf8_lossy(&output.stdout));

    let operator_jwt_path = store_dir.join(&operator.name).join(format!("{}.jwt", &operator.name));

    // Ensure operator JWT directory exists
    if let Some(parent) = operator_jwt_path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create operator JWT directory")?;
    }

    std::fs::read_to_string(&operator_jwt_path).context("Failed to read operator JWT")
}

pub async fn create_account(account: &AccountConfig, operator_name: &str, store_dir: &PathBuf) -> Result<String> {
    let store_path = store_dir.to_str().unwrap();
    let args = vec![
        "add".to_string(),
        "account".to_string(),
        "--name".to_string(),
        account.unique_name.clone(),
        "--data-dir".to_string(),
        store_path.to_string(),
    ];

    let output = Command::new("nsc")
        .args(&args)
        .output()
        .await
        .context(format!("Failed to run nsc add account {}", account.unique_name))?;

    if !output.status.success() {
        println!("nsc add account stdout: {}", String::from_utf8_lossy(&output.stdout));
        println!("nsc add account stderr: {}", String::from_utf8_lossy(&output.stderr));
        return Err(anyhow::anyhow!(
            "nsc add account failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
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
        let edit_output = Command::new("nsc")
            .args(&edit_args)
            .output()
            .await
            .context(format!("Failed to run nsc edit account {}", account.unique_name))?;
        if !edit_output.status.success() {
            return Err(anyhow::anyhow!(
                "nsc edit account failed: {}",
                String::from_utf8_lossy(&edit_output.stderr)
            ));
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
        let export_output = Command::new("nsc")
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
        let import_output = Command::new("nsc")
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

    let account_jwt_path = store_dir
        .join(operator_name)
        .join("accounts")
        .join(&account.unique_name)
        .join(format!("{}.jwt", &account.unique_name));

    std::fs::read_to_string(&account_jwt_path)
        .context(format!("Failed to read JWT for account {}", account.unique_name))
}

pub async fn create_user(
    account: &AccountConfig,
    user: &UserConfig,
    output_dir: &Path,
    store_dir: &Path,
) -> Result<PathBuf> {
    let store_path = store_dir.to_str().unwrap();
    let creds_path = output_dir.join(format!("{}-{}.creds", account.name, user.name));

    let account_name = if account.name == "SYS" && account.is_system_account {
        "SYS".to_string()
    } else {
        account.unique_name.clone()
    };

    let mut args = vec![
        "add".to_string(),
        "user".to_string(),
        "--name".to_string(),
        user.name.clone(),
        "--account".to_string(),
        account_name.clone(),
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

    let output = Command::new("nsc")
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

    let _ = std::fs::remove_file(&creds_path);
    let output = Command::new("nsc")
        .args(&[
            "generate".to_string(),
            "creds".to_string(),
            "--account".to_string(),
            account_name,
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

pub fn extract_account_id(jwt: &str) -> Result<String> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        return Err(anyhow::anyhow!("Invalid JWT format: {} parts", parts.len()));
    }
    let payload = BASE64.decode(parts[1]).context("Failed to decode JWT payload")?;
    let payload_str = String::from_utf8(payload).context("JWT payload is not UTF-8")?;
    let json: serde_json::Value = serde_json::from_str(&payload_str).context("Failed to parse JWT JSON")?;
    json["sub"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("No 'sub' field in JWT"))
}
