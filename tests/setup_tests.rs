use std::path::PathBuf;
use std::collections::HashMap;

use anyhow::Result;
use natsforge::{
    config::{
        AccountConfig, ExportConfig, JetStreamConfig, LeafNodeConfig, NatsConfig, OperatorConfig, ServerConfig,
        UserConfig, SubjectTransform, RepublishConfig,
    },
    NatsForge,
};

#[tokio::test]
async fn test_basic_setup_with_accounts() -> Result<(), Box<dyn std::error::Error>> {
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
                        allowed_subjects: vec!["app.>".to_string()],
                        denied_subjects: vec![],
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
            output_dir: PathBuf::from("test-output"),
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

    std::fs::remove_dir_all("test-output")?;
    Ok(())
}

#[tokio::test]
async fn test_temp_setup_with_accounts() -> Result<(), Box<dyn std::error::Error>> {
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
                max_memory: Some(1024 * 1024 * 1024), // 1GB
                max_storage: Some(10 * 1024 * 1024 * 1024), // 10GB
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
                        allowed_subjects: vec!["app.>".to_string()],
                        denied_subjects: vec!["forbidden.>".to_string()],
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

    Ok(())
}

#[tokio::test]
async fn test_json_config() -> Result<(), Box<dyn std::error::Error>> {
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
async fn test_hub_leaf_json_config() -> Result<(), Box<dyn std::error::Error>> {
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

    // Debug print the relevant section
    if let Some(remote_section) = leaf_config.split("remotes = [").nth(1) {
        if let Some(remote_config) = remote_section.split("]").next() {
            println!("Remote configuration section:\n{}", remote_config);
        }
    }

    // Updated assertions for the leaf config
    assert!(leaf_config.contains("remotes = ["));
    assert!(leaf_config.contains("url: \"nats://localhost:4248\""));

    // Check for credentials path with or without directory prefix
    let has_credentials = leaf_config.contains("credentials: \"app-service-service-user.creds\"")
        || leaf_config.contains("credentials: \"leaf-output/app-service-service-user.creds\"");
    assert!(has_credentials, "Credentials path not found in expected format");

    // Verify account ID format
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
