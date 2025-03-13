use natsforge::{NatsSetup, NatsDeployment, OperatorConfig, AccountConfig, UserConfig, ServerOptions, ResolverType, ExportConfig};
use std::path::PathBuf;
use anyhow::{Context, Result};

#[tokio::test]
async fn test_basic_setup_with_accounts() -> Result<(), Box<dyn std::error::Error>> {
    let config = natsforge::NatsConfig {
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
                max_payload: Some(1048576),
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
    assert!(config_content.contains("port: 4222"));
    assert!(!config_content.contains("jetstream"));

    std::fs::remove_dir_all("test-output")?;
    Ok(())
}

#[tokio::test]
async fn test_temp_setup_with_accounts() -> Result<(), Box<dyn std::error::Error>> {
    let config = natsforge::NatsConfig {
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
                max_payload: Some(2097152),
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

    let config_content = std::fs::read_to_string(&result.server_config_path)?;
    assert!(config_content.contains("jetstream"));
    assert!(config_content.contains("domain: \"core\""));

    Ok(())
}

#[tokio::test]
async fn test_json_config() -> Result<(), Box<dyn std::error::Error>> {
    let setup = NatsSetup::from_json_file("tests/example.json")?;
    let result = setup.initialize().await?;

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
    let deployment = NatsDeployment::from_json_file("tests/hub_leaf.json")?;
    let result = deployment.initialize().await?;

    assert!(result.operator_jwt_path.exists());
    assert_eq!(result.account_jwt_paths.len(), 2); // SYS + app-service
    assert_eq!(result.user_creds_paths.len(), 1); // service-user
    assert_eq!(result.server_config_paths.len(), 2); // hub + leaf

    // Hub config
    let hub_config = std::fs::read_to_string(&result.server_config_paths[0])?;
    assert!(hub_config.contains("port: 4222"));
    assert!(hub_config.contains("leafnodes {\n    port: 4248\n}"));
    assert!(hub_config.contains("jetstream"));
    assert!(hub_config.contains("resolver_preload"));
    assert!(hub_config.contains("app-service"));

    // Leaf config
    let leaf_config = std::fs::read_to_string(&result.server_config_paths[1])?;
    assert!(leaf_config.contains("port: 4223"));
    assert!(leaf_config.contains("leafnodes {\n    remotes = [\n        { url: \"nats://localhost:4248\", account: \"app-service\", credentials: \"app-service-service-user.creds\" }\n    ]\n}"));
    assert!(leaf_config.contains("jetstream"));

    // Creds file
    assert!(result.user_creds_paths[0].exists());
    let creds = std::fs::read_to_string(&result.user_creds_paths[0])?;
    assert!(creds.contains("BEGIN NATS USER JWT"));

    std::fs::remove_dir_all("hub-output")?;
    std::fs::remove_dir_all("leaf-output")?;
    Ok(())
}
