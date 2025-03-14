use std::collections::HashMap;

use crate::{config::ServerConfig, extract_account_id};

pub fn generate_server_config(
    server: &ServerConfig,
    operator_jwt: &str,
    system_account_id: &str,
    resolver_preload: &str,
    account_jwts: &HashMap<String, String>,
) -> String {
    let mut config = format!("port: {}\nserver_name: \"{}\"\n\n", server.port, server.name);
    if server.jetstream.enabled {
        config.push_str("jetstream {\n");
        config.push_str(&format!(
            "    store_dir: \"{}\"\n    domain: \"{}\"\n",
            server.jetstream.store_dir.as_ref().unwrap_or(&"jetstream".to_string()),
            server.jetstream.domain.as_ref().unwrap_or(&"core".to_string())
        ));
        if let Some(max_mem) = server.jetstream.max_memory {
            config.push_str(&format!("    max_memory_store: {}\n", max_mem));
        }
        if let Some(max_store) = server.jetstream.max_storage {
            config.push_str(&format!("    max_file_store: {}\n", max_store));
        }
        if let Some(transform) = &server.jetstream.subject_transform {
            config.push_str(&format!(
                "    subject_transform {{ src: \"{}\", dest: \"{}\" }}\n",
                transform.src, transform.dest
            ));
        }
        if !server.jetstream.republish.is_empty() {
            config.push_str("    republish = [\n");
            for repub in &server.jetstream.republish {
                config.push_str(&format!(
                    "        {{ src: \"{}\", dest: \"{}\" }},\n",
                    repub.src, repub.dest
                ));
            }
            config.push_str("    ]\n");
        }
        config.push_str("}\n\n");
    }
    if let Some(tls) = &server.tls {
        config.push_str(&format!(
            "tls {{\n    cert_file: \"{}\"\n    key_file: \"{}\"\n",
            tls.cert_file, tls.key_file
        ));
        if let Some(ca_file) = &tls.ca_file {
            config.push_str(&format!("    ca_file: \"{}\"\n", ca_file));
        }
        config.push_str("}\n\n");
    }
    if !server.mappings.is_empty() {
        config.push_str("mappings: {\n");
        for (src, dest) in &server.mappings {
            config.push_str(&format!("    \"{}\": \"{}\",\n", src, dest));
        }
        config.push_str("}\n\n");
    }
    if let Some(port) = server.leafnodes.port {
        config.push_str(&format!("leafnodes {{\n    port: {}\n}}\n\n", port));
    }
    if !server.leafnodes.remotes.is_empty() {
        config.push_str("leafnodes {\n    remotes = [\n");
        for remote in &server.leafnodes.remotes {
            let account_jwt = account_jwts
                .get(&remote.account)
                .unwrap_or_else(|| panic!("Missing JWT for {}", remote.account));
            let account_id = extract_account_id(account_jwt)
                .unwrap_or_else(|_| panic!("Failed to extract ID for {}", remote.account));
            let creds_path = server.output_dir.join(&remote.credentials);
            config.push_str(&format!(
                "        {{ url: \"{}\", account: \"{}\", credentials: \"{}\" }},\n",
                remote.url,
                account_id,
                creds_path.to_string_lossy()
            ));
        }
        config.push_str("    ]\n}\n\n");
    }
    config.push_str(&format!(
        "operator: \"{}\"\nsystem_account: \"{}\"\nresolver: MEMORY\n",
        operator_jwt, system_account_id
    ));
    if !resolver_preload.is_empty() {
        config.push_str("resolver_preload: {\n");
        config.push_str(resolver_preload);
        config.push_str("\n}\n");
    }
    config
}
