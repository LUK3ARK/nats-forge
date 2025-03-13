use natsforge::{NatsConfig, NatsSetup, OperatorConfig, AccountConfig, UserConfig, ServerOptions, ResolverType, ExportConfig, ImportConfig};
use std::path::PathBuf;
use tokio;
use async_nats::Client;
use anyhow::Context;

#[tokio::test]
async fn test_basic_setup_with_accounts() -> Result<(), Box<dyn std::error::Error>> {
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
                    allowed_subjects: vec!["app.>".to_string()],
                    denied_subjects: vec![],
                    expiry: None,
                }],
                is_system_account: false,
                max_connections: Some(5),
                max_payload: Some(1048576), // 1MB
                exports: vec![ExportConfig {
                    subject: "app.data".to_string(),
                    is_service: false,
                }],
                imports: vec![],
            },
        ],
        output_dir: PathBuf::from("test-output"),
        server_options: ServerOptions {
            port: 4222,
            jetstream: false,
            resolver: ResolverType::Memory,
        },
    };

    let setup = NatsSetup::new(config);
    let result = setup.initialize().await?;

    assert!(result.operator_jwt_path.exists());
    assert_eq!(result.account_jwt_paths.len(), 2);
    assert_eq!(result.user_creds_paths.len(), 1);
    assert!(result.server_config_path.exists());

    let config_content = std::fs::read_to_string(&result.server_config_path)?;
    assert!(config_content.contains("system_account:"));

    std::fs::remove_dir_all("test-output")?;
    Ok(())
}

#[tokio::test]
async fn test_temp_setup_with_accounts() -> Result<(), Box<dyn std::error::Error>> {
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
                    allowed_subjects: vec!["app.>".to_string()],
                    denied_subjects: vec!["forbidden.>".to_string()],
                    expiry: Some("2025-12-31T23:59:59Z".to_string()),
                }],
                is_system_account: false,
                max_connections: Some(10),
                max_payload: Some(2097152), // 2MB
                exports: vec![],
                imports: vec![],
            },
        ],
        output_dir: PathBuf::from("ignored"),
        server_options: ServerOptions {
            port: 4222,
            jetstream: true,
            resolver: ResolverType::Memory,
        },
    };

    let setup = NatsSetup::for_test(config);
    let result = setup.initialize().await?;

    assert!(result.operator_jwt_path.exists());
    assert_eq!(result.account_jwt_paths.len(), 2);
    assert_eq!(result.user_creds_paths.len(), 1);
    assert!(result.server_config_path.exists());

    Ok(())
}

#[tokio::test]
async fn test_setup_validation() -> Result<(), Box<dyn std::error::Error>> {
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
                max_connections: Some(1), // Only 1 connection
                max_payload: Some(1024),  // 1KB max
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

    let mut server = tokio::process::Command::new("nats-server")
        .arg("-c")
        .arg(&result.server_config_path)
        .spawn()
        .context("Failed to start NATS server")?;

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let creds = std::fs::read_to_string(&result.user_creds_paths[0])
        .context("Failed to read user creds")?;
    let client = async_nats::ConnectOptions::with_credentials(creds.clone())
        .connect("localhost:4223")
        .await
        .context("Failed to connect to NATS server")?;

    // Test allowed subject
    let sub = client.subscribe("test.foo".into()).await?;
    client.publish("test.foo".into(), "hello".into()).await?;
    if let Some(msg) = sub.next_timeout(tokio::time::Duration::from_secs(1)).await? {
        assert_eq!(msg.payload, b"hello");
    } else {
        return Err(anyhow::anyhow!("No message received on allowed subject").into());
    }

    // Test denied subject (should fail)
    let result = client.publish("forbidden.bar".into(), "nope".into()).await;
    assert!(result.is_err(), "Publish to denied subject should fail");

    // Test max connections (second connection should fail)
    let second_client_result = async_nats::ConnectOptions::with_credentials(creds)
        .connect("localhost:4223")
        .await;
    assert!(second_client_result.is_err(), "Second connection should fail due to max_connections");

    // Test max payload (1025 bytes should fail)
    let large_payload = vec![0u8; 1025];
    let result = client.publish("test.foo".into(), large_payload.into()).await;
    assert!(result.is_err(), "Large payload should fail due to max_payload");

    std::fs::remove_dir_all("test-output-validation")?;
    server.kill().context("Failed to kill NATS server")?;

    Ok(())
}

#[tokio::test]
async fn test_json_config() -> Result<(), Box<dyn std::error::Error>> {
    let setup = NatsSetup::from_json_file("tests/example.json")?;
    let result = setup.initialize().await?;

    assert!(result.operator_jwt_path.exists());
    assert_eq!(result.account_jwt_paths.len(), 2);
    assert_eq!(result.user_creds_paths.len(), 2); // Two users in APP1
    assert!(result.server_config_path.exists());

    let config_content = std::fs::read_to_string(&result.server_config_path)?;
    assert!(config_content.contains("system_account:"));

    std::fs::remove_dir_all("./nats-setup")?;
    Ok(())
}
