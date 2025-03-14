use std::{collections::HashMap, path::PathBuf};

use anyhow::Context;
use base64::Engine;
use natsforge::{
    config::{
        AccountConfig, ExportConfig, JetStreamConfig, LeafNodeConfig, NatsConfig, OperatorConfig, ServerConfig,
        UserConfig,
    },
    NatsForge,
};

#[tokio::test]
async fn test_basic_setup_with_accounts() -> anyhow::Result<()> {
    let output_dir = "test-output-basic";
    let _ = std::fs::remove_dir_all(output_dir);
    std::fs::create_dir_all(output_dir)?;

    let config = NatsConfig {
        name: Some("basic-setup".to_string()),
        operator: OperatorConfig {
            name: "test-operator".to_string(),
            reuse_existing: false,
        },
        servers: vec![ServerConfig {
            name: "main-server".to_string(),
            port: 4222,
            jetstream: JetStreamConfig {
                enabled: false,
                store_dir: None,
                domain: None,
                max_memory: None,
                max_storage: None,
                subject_transform: None,
                republish: vec![],
            },
            leafnodes: LeafNodeConfig::default(),
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
                    mappings: HashMap::new(),
                },
                AccountConfig {
                    name: "APP".to_string(),
                    unique_name: "".to_string(),
                    users: vec![UserConfig {
                        name: "app-user".to_string(),
                        allowed_pubsub: None,
                        allowed_publishes: None,
                        allowed_subjects: Some(vec!["app.>".to_string()]),
                        denied_pubsub: None,
                        denied_publishes: None,
                        denied_subjects: None,
                        allow_pub_response: None,
                        expiry: None,
                    }],
                    is_system_account: false,
                    max_connections: Some(5),
                    max_payload: Some(1048576),
                    exports: vec![ExportConfig {
                        subject: "app.data".to_string(),
                        is_service: false,
                    }],
                    imports: vec![],
                    mappings: HashMap::new(),
                },
            ],
            output_dir: PathBuf::from(output_dir),
            tls: None,
            mappings: HashMap::new(),
        }],
    };

    let forge = NatsForge::from_config(config)?;
    let result = forge.initialize().await?;

    assert!(result.operator_jwt_path.exists());
    assert_eq!(result.account_jwt_paths.len(), 2);
    assert_eq!(result.user_creds_paths.len(), 1);
    assert!(result.server_config_path.exists());

    let config_content = std::fs::read_to_string(&result.server_config_path)?;
    assert!(config_content.contains("system_account:"));
    assert!(config_content.contains("port: 4222"));
    assert!(!config_content.contains("jetstream"));

    let mut attempts = 0;
    let max_attempts = 3;
    while attempts < max_attempts {
        match std::fs::remove_dir_all(output_dir) {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                attempts += 1;
                println!("Cleanup attempt {}/{} failed: {}", attempts, max_attempts, e);
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            Err(e) => return Err(anyhow::anyhow!(e)),
        }
    }
    if attempts == max_attempts {
        return Err(anyhow::anyhow!(
            "Failed to clean up {} after {} attempts",
            output_dir,
            max_attempts
        ));
    }

    Ok(())
}

#[tokio::test]
async fn test_temp_setup_with_accounts() -> anyhow::Result<()> {
    let config = NatsConfig {
        name: Some("temp-setup".to_string()),
        operator: OperatorConfig {
            name: "test-operator".to_string(),
            reuse_existing: false,
        },
        servers: vec![ServerConfig {
            name: "main-server".to_string(),
            port: 4222,
            jetstream: JetStreamConfig {
                enabled: true,
                store_dir: Some("ignored/jetstream".to_string()),
                domain: Some("core".to_string()),
                max_memory: Some(1024 * 1024 * 1024),       // 1GB
                max_storage: Some(10 * 1024 * 1024 * 1024), // 10GB
                subject_transform: None,
                republish: vec![],
            },
            leafnodes: LeafNodeConfig::default(),
            accounts: vec![
                AccountConfig {
                    name: "SYS".to_string(),
                    unique_name: "".to_string(),
                    users: vec![], // No perms needed
                    is_system_account: true,
                    max_connections: None,
                    max_payload: None,
                    exports: vec![],
                    imports: vec![],
                    mappings: HashMap::new(),
                },
                AccountConfig {
                    name: "APP".to_string(),
                    unique_name: "".to_string(),
                    users: vec![UserConfig {
                        name: "app-user".to_string(),
                        allowed_pubsub: None,
                        allowed_publishes: None,
                        allowed_subjects: Some(vec!["app.>".to_string()]),
                        denied_pubsub: None,
                        denied_publishes: None,
                        denied_subjects: Some(vec!["forbidden.>".to_string()]),
                        allow_pub_response: None,
                        expiry: Some("2025-12-31T23:59:59Z".to_string()),
                    }],
                    is_system_account: false,
                    max_connections: Some(10),
                    max_payload: Some(2097152),
                    exports: vec![],
                    imports: vec![],
                    mappings: HashMap::new(),
                },
            ],
            output_dir: PathBuf::from("ignored"),
            tls: None,
            mappings: HashMap::new(),
        }],
    };

    let forge = NatsForge::from_config(config)?;
    let result = forge.initialize().await?;

    assert!(result.operator_jwt_path.exists());
    assert_eq!(result.account_jwt_paths.len(), 2);
    assert_eq!(result.user_creds_paths.len(), 1);
    assert!(result.server_config_path.exists());

    let config_content = std::fs::read_to_string(&result.server_config_path)?;
    assert!(config_content.contains("jetstream"));
    assert!(config_content.contains("domain: \"core\""));

    std::fs::remove_dir_all("ignored")?;

    Ok(())
}

#[tokio::test]
async fn test_json_config() -> anyhow::Result<()> {
    let forge = NatsForge::from_json_file("tests/example.json")?;
    let result = forge.initialize().await?;

    assert!(result.operator_jwt_path.exists());
    assert_eq!(result.account_jwt_paths.len(), 2);
    assert_eq!(result.user_creds_paths.len(), 2);
    assert!(result.server_config_path.exists());

    let config_content = std::fs::read_to_string(&result.server_config_path)?;
    assert!(config_content.contains("system_account:"));

    std::fs::remove_dir_all("./nats-setup")?;
    Ok(())
}

#[tokio::test]
async fn test_hub_leaf_json_config() -> anyhow::Result<()> {
    let forge = NatsForge::from_json_file("tests/hub_leaf.json")?;
    let result = forge.initialize().await?;

    assert!(result.operator_jwt_path.exists());
    assert_eq!(result.account_jwt_paths.len(), 2);
    assert_eq!(result.user_creds_paths.len(), 2);
    assert_eq!(result.server_config_paths.as_ref().unwrap().len(), 2);

    let hub_config = std::fs::read_to_string(&result.server_config_paths.as_ref().unwrap()[0])?;
    assert!(hub_config.contains("port: 4222"));
    assert!(hub_config.contains("leafnodes {\n    port: 4248\n}"));

    let leaf_config = std::fs::read_to_string(&result.server_config_paths.as_ref().unwrap()[1])?;
    assert!(leaf_config.contains("port: 4223"));

    if let Some(remote_section) = leaf_config.split("remotes = [").nth(1) {
        if let Some(remote_config) = remote_section.split("]").next() {
            println!("Remote configuration section:\n{}", remote_config);
        }
    }

    assert!(leaf_config.contains("remotes = ["));
    assert!(leaf_config.contains("url: \"nats://localhost:4248\""));

    let has_credentials = leaf_config.contains("credentials: \"app-service-service-user.creds\"")
        || leaf_config.contains("credentials: \"leaf-output/app-service-service-user.creds\"");
    assert!(has_credentials, "Credentials path not found in expected format");

    let account_start = leaf_config.find("account: \"").unwrap() + 10;
    let account_end = leaf_config[account_start..].find("\"").unwrap() + account_start;
    let account_id = &leaf_config[account_start..account_end];
    println!("Found account ID: {}", account_id);

    assert!(account_id.starts_with('A'), "Account ID should start with 'A'");
    assert_eq!(account_id.len(), 56, "Account ID should be 56 characters long");
    assert!(
        account_id.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()),
        "Account ID should only contain uppercase letters and numbers"
    );

    std::fs::remove_dir_all("hub-output")?;
    std::fs::remove_dir_all("leaf-output")?;
    Ok(())
}
#[tokio::test]
async fn test_pub_sub_permissions() -> anyhow::Result<()> {
    let output_dir = "test-output-pubsub";
    let _ = std::fs::remove_dir_all(output_dir);
    std::fs::create_dir_all(output_dir)?;
    println!("Test output directory initialized: {}", output_dir);

    let config = NatsConfig {
        name: Some("pub-sub-test".to_string()),
        operator: OperatorConfig {
            name: "test-operator".to_string(),
            reuse_existing: false,
        },
        servers: vec![ServerConfig {
            name: "test-server".to_string(),
            port: 4222,
            jetstream: JetStreamConfig {
                enabled: true,
                store_dir: Some(format!("{}/jetstream", output_dir)),
                domain: Some("core".to_string()),
                max_memory: None,
                max_storage: None,
                subject_transform: None,
                republish: vec![],
            },
            leafnodes: LeafNodeConfig::default(),
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
                    mappings: HashMap::new(),
                },
                AccountConfig {
                    name: "TEST".to_string(),
                    unique_name: "".to_string(),
                    users: vec![
                        UserConfig {
                            name: "sub-only".to_string(),
                            allowed_pubsub: None,
                            allowed_publishes: None,
                            allowed_subjects: Some(vec!["test.sub.>".to_string()]),
                            denied_pubsub: None,
                            denied_publishes: None,
                            denied_subjects: None,
                            allow_pub_response: None,
                            expiry: None,
                        },
                        UserConfig {
                            name: "pub-only".to_string(),
                            allowed_pubsub: None,
                            allowed_publishes: Some(vec!["test.pub.>".to_string()]),
                            allowed_subjects: None,
                            denied_pubsub: None,
                            denied_publishes: None,
                            denied_subjects: None,
                            allow_pub_response: None,
                            expiry: None,
                        },
                        UserConfig {
                            name: "both".to_string(),
                            allowed_pubsub: None,
                            allowed_publishes: Some(vec!["test.both.pub.>".to_string()]),
                            allowed_subjects: Some(vec!["test.both.sub.>".to_string()]),
                            denied_pubsub: None,
                            denied_publishes: None,
                            denied_subjects: None,
                            allow_pub_response: None,
                            expiry: None,
                        },
                    ],
                    is_system_account: false,
                    max_connections: None,
                    max_payload: None,
                    exports: vec![],
                    imports: vec![],
                    mappings: HashMap::new(),
                },
            ],
            output_dir: PathBuf::from(output_dir),
            tls: None,
            mappings: HashMap::new(),
        }],
    };

    let forge = NatsForge::from_config(config)?;
    let result = forge.initialize().await?;

    println!("Post-initialize creds paths:");
    for path in &result.user_creds_paths {
        println!(" - {} (exists: {})", path.display(), path.exists());
    }
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    assert!(result.operator_jwt_path.exists());
    assert_eq!(result.account_jwt_paths.len(), 2);
    assert_eq!(result.user_creds_paths.len(), 3, "Expected 3 user creds files");
    assert!(result.server_config_path.exists());

    for creds_path in &result.user_creds_paths {
        println!(
            "Checking creds file: {} (exists: {})",
            creds_path.display(),
            creds_path.exists()
        );
        let creds_content = std::fs::read_to_string(creds_path)?;
        let jwt = creds_content
            .lines()
            .skip_while(|line| !line.contains("-----BEGIN NATS USER JWT-----"))
            .skip(1)
            .take_while(|line| !line.contains("------END NATS USER JWT------"))
            .collect::<Vec<_>>()
            .join("\n");
        let _decoded_jwt = natsforge::extract_account_id(&jwt)?;
        let jwt_json: serde_json::Value = serde_json::from_str(
            &String::from_utf8(
                base64::engine::general_purpose::STANDARD_NO_PAD
                    .decode(jwt.split('.').nth(1).unwrap())
                    .context("Failed to decode JWT")?,
            )
            .context("Failed to convert to UTF-8")?,
        )
        .context("Failed to parse JSON")?;

        let default_value = serde_json::Value::Object(serde_json::Map::new());
        let nats_perms = jwt_json["nats"].get("pub").unwrap_or(&default_value);
        let nats_subs = jwt_json["nats"].get("sub").unwrap_or(&default_value);

        if creds_path.to_string_lossy().contains("sub-only") {
            assert!(
                nats_subs.to_string().contains("test.sub.>"),
                "sub-only missing test.sub.>"
            );
            assert!(
                !nats_perms.to_string().contains("test.sub.>"),
                "sub-only has unexpected pub perm"
            );
        } else if creds_path.to_string_lossy().contains("pub-only") {
            assert!(
                nats_perms.to_string().contains("test.pub.>"),
                "pub-only missing test.pub.>"
            );
            assert!(
                !nats_subs.to_string().contains("test.pub.>"),
                "pub-only has unexpected sub perm"
            );
        } else if creds_path.to_string_lossy().contains("both") {
            assert!(
                nats_subs.to_string().contains("test.both.sub.>"),
                "both missing test.both.sub.>"
            );
            assert!(
                nats_perms.to_string().contains("test.both.pub.>"),
                "both missing test.both.pub.>"
            );
        }
    }

    let mut attempts = 0;
    let max_attempts = 3;
    while attempts < max_attempts {
        match std::fs::remove_dir_all(output_dir) {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                attempts += 1;
                println!("Cleanup attempt {}/{} failed: {}", attempts, max_attempts, e);
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            Err(e) => return Err(anyhow::anyhow!(e)),
        }
    }
    if attempts == max_attempts {
        return Err(anyhow::anyhow!(
            "Failed to clean up {} after {} attempts",
            output_dir,
            max_attempts
        ));
    }

    Ok(())
}
