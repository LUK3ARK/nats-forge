use natsforge::{NatsSetup, OperatorConfig, AccountConfig, UserConfig, ServerOptions, ResolverType, ExportConfig};
use std::path::PathBuf;
use tokio;
use anyhow::{Context, Result};
use futures_util::StreamExt;
use crate::common::ServerGuard;

mod common;

#[tokio::test]
async fn test_setup_validation() -> Result<(), Box<dyn std::error::Error>> {
    let _ = std::fs::remove_dir_all("test-output-validation");

    let _ = tokio::process::Command::new("pkill")
        .args(&["-f", "nats-server.*4223"])
        .output()
        .await;

    let config = NatsConfig {
        operator: OperatorConfig {
            name: "test-operator".to_string(),
            reuse_existing: false,
        },
        accounts: vec![
            AccountConfig {
                name: "SYS".to_string(),
                unique_name: "".to_string(),
                users: vec![],
                is_system_account: true,
                max_connections: None,
                max_payload: None,
                exports: vec![],
                imports: vec![],
            },
            AccountConfig {
                name: "APP".to_string(),
                unique_name: "".to_string(),
                users: vec![UserConfig {
                    name: "app-user".to_string(),
                    allowed_subjects: vec!["test.>".to_string()],
                    denied_subjects: vec!["forbidden.>".to_string()],
                    expiry: Some("2025-12-31T23:59:59Z".to_string()),
                }],
                is_system_account: false,
                max_connections: Some(1),
                max_payload: Some(1024),
                exports: vec![ExportConfig {
                    subject: "test.data".to_string(),
                    is_service: false,
                }],
                imports: vec![],
            },
        ],
        output_dir: PathBuf::from("test-output-validation"),
        server_options: ServerOptions {
            port: 4223,
            jetstream: true,
            resolver: ResolverType::Memory,
        },
    };

    let setup = NatsSetup::new(config);
    let result = setup.initialize().await?;

    let server = tokio::process::Command::new("nats-server")
        .arg("-c")
        .arg(&result.server_config_path)
        .arg("-DV")
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .context("Failed to start NATS server")?;
    let mut server_guard = ServerGuard(server);

    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    let creds = std::fs::read_to_string(&result.user_creds_paths[0])
        .context("Failed to read user creds")?;

    let client = async_nats::ConnectOptions::with_credentials(&creds.clone())
        .context("Failed to parse credentials")?
        .connect("localhost:4223")
        .await
        .context("Failed to connect to NATS server")?;

    let sub_result = client.subscribe("forbidden.bar").await?;
    let mut sub = sub_result;
    let msg = tokio::time::timeout(tokio::time::Duration::from_secs(1), sub.next()).await;
    assert!(msg.is_err() || msg.unwrap().is_none(), "Should not receive messages on denied subject");

    let second_client_result = async_nats::ConnectOptions::with_credentials(&creds)
        .context("Failed to parse credentials")?
        .connect("localhost:4223")
        .await;
    assert!(second_client_result.is_err(), "Second connection should fail due to max_connections");

    std::fs::remove_dir_all("test-output-validation")?;
    server_guard.0.kill().await.context("Failed to kill NATS server")?;

    Ok(())
}

// TODO: Add hub-leaf validation test in next iteration
