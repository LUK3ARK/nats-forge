// use futures_util::StreamExt;
// use natsforge::{NatsConfig, NatsSetup, OperatorConfig, AccountConfig, UserConfig, ServerOptions,
// ResolverType, ExportConfig}; use std::path::PathBuf;
// use tokio;
// use anyhow::{Context, Result};
// use tokio::process::Child;

// struct ServerGuard(Child);
// impl Drop for ServerGuard {
//     fn drop(&mut self) {
//         let _ = self.0.start_kill(); // Start killing on drop
//     }
// }

// #[tokio::test]
// async fn test_basic_setup_with_accounts() -> Result<(), Box<dyn std::error::Error>> {
//     let config = NatsConfig {
//         operator: OperatorConfig {
//             name: "test-operator".to_string(),
//             reuse_existing: false,
//         },
//         accounts: vec![
//             AccountConfig {
//                 name: "SYS".to_string(),
//                 unique_name: "".to_string(),
//                 users: vec![],
//                 is_system_account: true,
//                 max_connections: None,
//                 max_payload: None,
//                 exports: vec![],
//                 imports: vec![],
//             },
//             AccountConfig {
//                 name: "APP".to_string(),
//                 unique_name: "".to_string(),
//                 users: vec![UserConfig {
//                     name: "app-user".to_string(),
//                     allowed_subjects: vec!["app.>".to_string()],
//                     denied_subjects: vec![],
//                     expiry: None,
//                 }],
//                 is_system_account: false,
//                 max_connections: Some(5),
//                 max_payload: Some(1048576), // 1MB
//                 exports: vec![ExportConfig {
//                     subject: "app.data".to_string(),
//                     is_service: false,
//                 }],
//                 imports: vec![],
//             },
//         ],
//         output_dir: PathBuf::from("test-output"),
//         server_options: ServerOptions {
//             port: 4222,
//             jetstream: false,
//             resolver: ResolverType::Memory,
//         },
//     };

//     let setup = NatsSetup::new(config);
//     let result = setup.initialize().await?;

//     assert!(result.operator_jwt_path.exists());
//     assert_eq!(result.account_jwt_paths.len(), 2);
//     assert_eq!(result.user_creds_paths.len(), 1);
//     assert!(result.server_config_path.exists());

//     let config_content = std::fs::read_to_string(&result.server_config_path)?;
//     assert!(config_content.contains("system_account:"));

//     std::fs::remove_dir_all("test-output")?;
//     Ok(())
// }

// #[tokio::test]
// async fn test_temp_setup_with_accounts() -> Result<(), Box<dyn std::error::Error>> {
//     let config = NatsConfig {
//         operator: OperatorConfig {
//             name: "test-operator".to_string(),
//             reuse_existing: false,
//         },
//         accounts: vec![
//             AccountConfig {
//                 name: "SYS".to_string(),
//                 unique_name: "".to_string(),
//                 users: vec![],
//                 is_system_account: true,
//                 max_connections: None,
//                 max_payload: None,
//                 exports: vec![],
//                 imports: vec![],
//             },
//             AccountConfig {
//                 name: "APP".to_string(),
//                 unique_name: "".to_string(),
//                 users: vec![UserConfig {
//                     name: "app-user".to_string(),
//                     allowed_subjects: vec!["app.>".to_string()],
//                     denied_subjects: vec!["forbidden.>".to_string()],
//                     expiry: Some("2025-12-31T23:59:59Z".to_string()),
//                 }],
//                 is_system_account: false,
//                 max_connections: Some(10),
//                 max_payload: Some(2097152), // 2MB
//                 exports: vec![],
//                 imports: vec![],
//             },
//         ],
//         output_dir: PathBuf::from("ignored"),
//         server_options: ServerOptions {
//             port: 4222,
//             jetstream: true,
//             resolver: ResolverType::Memory,
//         },
//     };

//     let setup = NatsSetup::for_test(config);
//     let result = setup.initialize().await?;

//     assert!(result.operator_jwt_path.exists());
//     assert_eq!(result.account_jwt_paths.len(), 2);
//     assert_eq!(result.user_creds_paths.len(), 1);
//     assert!(result.server_config_path.exists());

//     Ok(())
// }

// #[tokio::test]
// async fn test_setup_validation() -> Result<(), Box<dyn std::error::Error>> {
//     let _ = std::fs::remove_dir_all("test-output-validation");

//     let _ = tokio::process::Command::new("pkill")
//         .args(&["-f", "nats-server.*4223"])
//         .output()
//         .await;

//     let config = NatsConfig {
//         operator: OperatorConfig {
//             name: "test-operator".to_string(),
//             reuse_existing: false,
//         },
//         accounts: vec![
//             AccountConfig {
//                 name: "SYS".to_string(),
//                 unique_name: "".to_string(),
//                 users: vec![],
//                 is_system_account: true,
//                 max_connections: None,
//                 max_payload: None,
//                 exports: vec![],
//                 imports: vec![],
//             },
//             AccountConfig {
//                 name: "APP".to_string(),
//                 unique_name: "".to_string(),
//                 users: vec![UserConfig {
//                     name: "app-user".to_string(),
//                     allowed_subjects: vec!["test.>".to_string()],
//                     denied_subjects: vec!["forbidden.>".to_string()],
//                     expiry: Some("2025-12-31T23:59:59Z".to_string()),
//                 }],
//                 is_system_account: false,
//                 max_connections: Some(1),
//                 max_payload: Some(1024),
//                 exports: vec![ExportConfig {
//                     subject: "test.data".to_string(),
//                     is_service: false,
//                 }],
//                 imports: vec![],
//             },
//         ],
//         output_dir: PathBuf::from("test-output-validation"),
//         server_options: ServerOptions {
//             port: 4223,
//             jetstream: true,
//             resolver: ResolverType::Memory,
//         },
//     };

//     let setup = NatsSetup::new(config);
//     let result = setup.initialize().await?;

//     let conf_content = std::fs::read_to_string(&result.server_config_path)?;
//     println!("nats.conf:\n{}", conf_content);

//     let server = tokio::process::Command::new("nats-server")
//         .arg("-c")
//         .arg(&result.server_config_path)
//         .arg("-DV")
//         .stdout(std::process::Stdio::inherit())
//         .stderr(std::process::Stdio::inherit())
//         .spawn()
//         .context("Failed to start NATS server")?;
//     let mut server_guard = ServerGuard(server);

//     tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

//     let creds = std::fs::read_to_string(&result.user_creds_paths[0])
//         .context("Failed to read user creds")?;
//     println!("Creds:\n{}", creds);

//     let client = async_nats::ConnectOptions::with_credentials(&creds.clone())
//         .context("Failed to parse credentials")?
//         .connect("localhost:4223")
//         .await
//         .context("Failed to connect to NATS server")?;

//     // let mut sub = client.subscribe("test.foo").await?;
//     // client.publish("test.foo", "hello".into()).await?;
//     // client.flush().await?;
//     // let msg = tokio::time::timeout(tokio::time::Duration::from_secs(1), sub.next())
//     //     .await
//     //     .context("Timeout waiting for message")?
//     //     .ok_or_else(|| anyhow::anyhow!("No message received on allowed subject"))?;
//     // assert_eq!(&*msg.payload, b"hello");

//     // Test denied subject
//     let sub_result = client.subscribe("forbidden.bar").await;
//     println!("Subscribe to forbidden.bar result: {:?}", sub_result);
//     assert!(sub_result.is_ok(), "Subscribe returns Ok despite server rejection (async_nats
// behavior)");

//     let mut sub = sub_result.unwrap();
//     let msg = tokio::time::timeout(tokio::time::Duration::from_secs(1), sub.next()).await;
//     println!("Message on forbidden.bar: {:?}", msg);
//     assert!(msg.is_err() || msg.unwrap().is_none(), "Should not receive messages on denied
// subject");

//     // Publish to trigger enforcement
//     let pub_result = client.publish("forbidden.bar", "nope".into()).await;
//     println!("Publish to forbidden.bar result: {:?}", pub_result);
//     assert!(pub_result.is_ok(), "Publish returns Ok despite server rejection (async_nats
// behavior)");     client.flush().await?;
//     tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
//     let state = client.connection_state();
//     println!("Connection state after forbidden publish: {:?}", state);
//     // Note: If state stays Connected, we’ll adjust this
//     // assert!(state != async_nats::connection::State::Connected, "Publish to denied subject
// should disconnect");

//     let second_client_result = async_nats::ConnectOptions::with_credentials(&creds)
//         .context("Failed to parse credentials")?
//         .connect("localhost:4223")
//         .await;
//     assert!(second_client_result.is_err(), "Second connection should fail due to
// max_connections");

//     std::fs::remove_dir_all("test-output-validation")?;
//     server_guard.0.kill().await.context("Failed to kill NATS server")?;

//     Ok(())
// }

// #[tokio::test]
// async fn test_json_config() -> Result<(), Box<dyn std::error::Error>> {
//     let setup = NatsSetup::from_json_file("tests/example.json")?;
//     let result = setup.initialize().await?;

//     assert!(result.operator_jwt_path.exists());
//     assert_eq!(result.account_jwt_paths.len(), 2);
//     assert_eq!(result.user_creds_paths.len(), 2); // Two users in APP1
//     assert!(result.server_config_path.exists());

//     let config_content = std::fs::read_to_string(&result.server_config_path)?;
//     assert!(config_content.contains("system_account:"));

//     std::fs::remove_dir_all("./nats-setup")?;
//     Ok(())
// }
