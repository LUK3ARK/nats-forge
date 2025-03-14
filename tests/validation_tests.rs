use std::path::PathBuf;

use anyhow::Context;
use futures_util::StreamExt;
use natsforge::{
    config::{
        AccountConfig, ExportConfig, JetStreamConfig, LeafNodeConfig, NatsConfig, OperatorConfig, ServerConfig,
        UserConfig,
    },
    NatsForge,
};
use tokio::{self, io::AsyncBufReadExt};

use crate::common::ServerGuard;

mod common;

#[tokio::test]
async fn test_setup_validation() -> anyhow::Result<()> {
    let validation_port = 4223;

    let _ = std::fs::remove_dir_all("test-output-validation");

    // Clean up port
    let _ = tokio::process::Command::new("pkill")
        .args(["-f", &format!("nats-server.*{}", validation_port)])
        .output()
        .await;

    // Verify port is available
    if tokio::net::TcpListener::bind(("0.0.0.0", validation_port))
        .await
        .is_err()
    {
        return Err(anyhow::anyhow!("Port {} is still in use", validation_port));
    }

    let config = NatsConfig {
        name: Some("validation-test".to_string()),
        operator: OperatorConfig {
            name: "test-operator".to_string(),
            reuse_existing: false,
        },
        servers: vec![ServerConfig {
            name: "validation-server".to_string(),
            port: validation_port,
            jetstream: JetStreamConfig {
                enabled: true,
                store_dir: Some("test-output-validation/jetstream".to_string()),
                domain: Some("test".to_string()),
                max_memory: None,
                    max_storage: None,
            },
            leafnodes: LeafNodeConfig::default(),
            accounts: vec![AccountConfig {
                name: "APP".to_string(),
                unique_name: "APP".to_string(),
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
            }],
            output_dir: PathBuf::from("test-output-validation"),
            tls: None,
        }],
    };

    let forge = NatsForge::from_config(config)?;
    let result = forge.initialize().await?;

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let server = tokio::process::Command::new("nats-server")
        .arg("-c")
        .arg(&result.server_config_path)
        .arg("-DV")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to start NATS server")?;
    let mut server_guard = ServerGuard(server);

    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    let app_user_creds = result
        .user_creds_paths
        .iter()
        .find(|path| path.to_string_lossy().contains("app-user"))
        .context("Failed to find app user credentials")?;

    let creds = std::fs::read_to_string(app_user_creds).context("Failed to read user creds")?;

    let mut retry_count = 0;
    let max_retries = 3;
    let client = loop {
        match async_nats::ConnectOptions::with_credentials(&creds)
            .context("Failed to parse credentials")?
            .connect(&format!("localhost:{}", validation_port))
            .await
        {
            Ok(client) => break client,
            Err(e) => {
                retry_count += 1;
                if retry_count >= max_retries {
                    return Err(e.into());
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    };

    let sub_result = client.subscribe("forbidden.bar").await?;
    let mut sub = sub_result;
    let msg = tokio::time::timeout(tokio::time::Duration::from_secs(1), sub.next()).await;
    assert!(
        msg.is_err() || msg.unwrap().is_none(),
        "Should not receive messages on denied subject"
    );

    let second_client_result = async_nats::ConnectOptions::with_credentials(&creds)
        .context("Failed to parse credentials")?
        .connect(&format!("localhost:{}", validation_port))
        .await;
    assert!(
        second_client_result.is_err(),
        "Second connection should fail due to max_connections"
    );

    std::fs::remove_dir_all("test-output-validation")?;
    server_guard.0.kill().await.context("Failed to kill NATS server")?;

    Ok(())
}
#[tokio::test]
async fn test_hub_leaf_validation() -> anyhow::Result<()> {
    let hub_port = 4232;
    let leaf_port = 4233;
    let leaf_remote_port = 4238;

    for dir in ["hub-output", "leaf-output"] {
        let _ = std::fs::remove_dir_all(dir);
    }

    for port in [hub_port, leaf_port, leaf_remote_port] {
        let _ = tokio::process::Command::new("pkill")
            .args(["-f", &format!("nats-server.*{}", port)])
            .output()
            .await;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    let mut config = serde_json::from_str::<NatsConfig>(include_str!("hub_leaf.json"))?;
    if let Some(hub) = config.servers.get_mut(0) {
        hub.port = hub_port;
        if let Some(ref mut leaf) = hub.leafnodes.port {
            *leaf = leaf_remote_port;
        }
    }
    if let Some(leaf) = config.servers.get_mut(1) {
        leaf.port = leaf_port;
        if let Some(remote) = leaf.leafnodes.remotes.get_mut(0) {
            remote.url = format!("nats://localhost:{}", leaf_remote_port);
        }
    }

    let forge = NatsForge::from_config(config)?;
    let result = forge.initialize().await?;

    // Log configs for inspection
    let hub_config = std::fs::read_to_string(&result.server_config_paths.as_ref().unwrap()[0])?;
    let leaf_config = std::fs::read_to_string(&result.server_config_paths.as_ref().unwrap()[1])?;
    println!("Hub config:\n{}", hub_config);
    println!("Leaf config:\n{}", leaf_config);

    let hub_server = tokio::process::Command::new("nats-server")
        .arg("-c")
        .arg(&result.server_config_paths.as_ref().unwrap()[0])
        .arg("-DV")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let mut hub_guard = ServerGuard(hub_server);

    let hub_stderr = hub_guard.0.stderr.take().unwrap();
    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(hub_stderr);
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line).await {
            if n == 0 { break; }
            println!("Hub stderr: {}", line.trim());
            line.clear();
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let leaf_server = tokio::process::Command::new("nats-server")
        .arg("-c")
        .arg(&result.server_config_paths.as_ref().unwrap()[1])
        .arg("-DV")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let mut leaf_guard = ServerGuard(leaf_server);

    let leaf_stderr = leaf_guard.0.stderr.take().unwrap();
    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(leaf_stderr);
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line).await {
            if n == 0 { break; }
            println!("Leaf stderr: {}", line.trim());
            line.clear();
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    let service_user_creds = result
        .user_creds_paths
        .iter()
        .find(|path| path.to_string_lossy().contains("service-user"))
        .ok_or_else(|| anyhow::anyhow!("Failed to find service-user credentials"))?;
    let creds = std::fs::read_to_string(service_user_creds)?;
    println!("Using creds from: {}", service_user_creds.display());

    let mut retry_count = 0;
    let max_retries = 5;

    let leaf_client = loop {
        println!("Attempting to connect to leaf (retry {}/{})", retry_count, max_retries);
        match async_nats::ConnectOptions::with_credentials(&creds)
            .map_err(|e| anyhow::anyhow!("Failed to parse credentials: {}", e))?
            .connect(&format!("localhost:{}", leaf_port))
            .await
        {
            Ok(client) => break client,
            Err(e) => {
                retry_count += 1;
                if retry_count >= max_retries {
                    return Err(anyhow::anyhow!("Failed to connect to leaf after {} retries: {}", max_retries, e));
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    };

    let hub_client = loop {
        println!("Attempting to connect to hub (retry {}/{})", retry_count, max_retries);
        match async_nats::ConnectOptions::with_credentials(&creds)
            .map_err(|e| anyhow::anyhow!("Failed to parse credentials: {}", e))?
            .connect(&format!("localhost:{}", hub_port))
            .await
        {
            Ok(client) => break client,
            Err(e) => {
                retry_count += 1;
                if retry_count >= max_retries {
                    return Err(anyhow::anyhow!("Failed to connect to hub after {} retries: {}", max_retries, e));
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    };

    let mut sub = hub_client.subscribe("events.test")
        .await
        .map_err(|e| anyhow::anyhow!("Failed to subscribe: {}", e))?;

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    leaf_client.publish("events.test", "Hello from leaf".into())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to publish: {}", e))?;

    leaf_client.flush()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to flush: {}", e))?;

    let msg = tokio::time::timeout(tokio::time::Duration::from_secs(2), sub.next())
        .await?
        .ok_or_else(|| anyhow::anyhow!("No message received"))?;

    assert_eq!(msg.payload, "Hello from leaf");

    // TODO: Add TLS testing once implemented
    // todo!("Test TLS configuration when added to NatsForge");

    std::fs::remove_dir_all("hub-output")?;
    std::fs::remove_dir_all("leaf-output")?;
    hub_guard.0.kill().await?;
    leaf_guard.0.kill().await?;

    Ok(())
}
