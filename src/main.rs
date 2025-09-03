use clap::Parser;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Write, copy};
use std::net::IpAddr;
use std::path::PathBuf;

/// Backup WLED presets from discovered devices.
#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Directory to save backups in
    #[arg(short, long, default_value = ".")]
    out_dir: PathBuf,

    /// Search duration in seconds
    #[arg(short, long, default_value_t = 4)]
    search_secs: u64,
}

fn discover_wleds(search_duration: std::time::Duration) -> Vec<ServiceInfo> {
    let mut wleds = HashMap::new();

    // Create a daemon
    let mdns = ServiceDaemon::new().expect("Failed to create daemon");

    // Browse for a service type.
    let service_type = "_wled._tcp.local.";
    let receiver = mdns.browse(service_type).expect("Failed to browse");

    while let Ok(event) = receiver.recv_timeout(search_duration) {
        match event {
            ServiceEvent::ServiceResolved(info) => {
                // Sometimes we get multiple responses for the same device. We use the
                // HashMap as we way to deduplicate them based on hostname.
                if !wleds.contains_key(&info.get_hostname().to_string()) {
                    wleds.insert(info.get_hostname().to_string(), info.clone());
                    println!("Discovered: {}", info.get_fullname());
                }
            }
            _other_event => {}
        }
    }

    wleds.into_values().collect()
}

fn get_hostname_from_cfg(cfg_json: &Value) -> Result<&str, Box<dyn std::error::Error>> {
    let hostname = cfg_json
        .get("id")
        .ok_or_else(|| "Missing 'id' field in cfg.json")?
        .get("name")
        .ok_or_else(|| "Missing 'name' field in cfg.json")?
        .as_str()
        .ok_or_else(|| "Expected 'name' to be a string in cfg.json")?;

    if hostname.trim().is_empty() {
        return Err("Hostname is empty or contains only whitespace".into());
    }

    Ok(hostname)
}

fn backup_wled(
    ip: &IpAddr,
    port: u16,
    out_dir: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let url_cfg = format!("http://{ip}:{port}/cfg.json");
    let url_presets = format!("http://{ip}:{port}/presets.json");

    let cfg_response_str = reqwest::blocking::get(url_cfg)?.text()?;
    let cfg_json: Value = serde_json::from_str(&cfg_response_str)?;

    let hostname = get_hostname_from_cfg(&cfg_json)?;

    println!("  host name: {hostname}");

    // Save out cfg.json
    let cfg_file_name = format!("{hostname}_cfg.json");
    let cfg_path = out_dir.join(cfg_file_name.clone());
    let mut cfg_file = File::create(cfg_path.to_str().unwrap())?;
    cfg_file.write_all(cfg_response_str.as_bytes())?;
    cfg_file.flush()?;
    println!("  saved: {cfg_file_name}");

    // Save out presets.json
    let presets_file_name = format!("{hostname}_presets.json");
    let mut presets_response = reqwest::blocking::get(url_presets)?;
    let presets_path = out_dir.join(presets_file_name.clone());
    let mut presets_file = File::create(presets_path)?;
    copy(&mut presets_response, &mut presets_file)?;
    println!("  saved: {presets_file_name}");

    Ok(())
}

fn backup_wleds(
    wleds: Vec<ServiceInfo>,
    out_dir: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut final_result = Ok(());

    for wled in wleds.iter() {
        if let Some(ip) = wled.get_addresses().iter().next() {
            println!("Backing up {}", wled.get_hostname());
            if let Err(result) = backup_wled(&ip, wled.get_port(), out_dir) {
                println!("  FAILED: {result}");
                final_result = Err(result);
            }
            println!("  SUCCESS");
        }
    }

    final_result
}

fn main() {
    let args = Args::parse();

    if !args.out_dir.exists() {
        std::fs::create_dir_all(&args.out_dir).expect("Failed to create output directory");
    }

    println!(
        "Saving backups to {:?}, searching for {} seconds...",
        args.out_dir, args.search_secs
    );

    let wleds = discover_wleds(std::time::Duration::from_secs(args.search_secs));

    if let Err(_result) = backup_wleds(wleds, &args.out_dir) {
        std::process::exit(1);
    }

    println!("Finished");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::net::Ipv4Addr;
    use std::thread;
    use std::vec;
    use tempfile::tempdir;
    use tiny_http::{Response, Server};

    // Mock ServiceInfo for testing
    fn mock_service_info(name: &str, ip: &str, port: u16) -> ServiceInfo {
        ServiceInfo::new("_wled._tcp.local.", name, name, ip, port, None).unwrap()
    }

    fn cfg_body(hostname: &str) -> String {
        format!(r#"{{"id":{{"name":"{}"}}}}"#, hostname)
    }

    fn mock_wled_server(
        addr: &str,
        cfg_body: &str,
        presets_body: Option<&str>,
    ) -> thread::JoinHandle<()> {
        // Start server in a background thread

        let cfg_body = cfg_body.to_string();
        let presets_body = presets_body.map(|s| s.to_string());

        let server = Server::http(addr).unwrap();
        let handle = thread::spawn(move || {
            let max_requests = if presets_body.is_some() { 2 } else { 1 };

            for _ in 0..max_requests {
                if let Ok(request) = server.recv() {
                    let url = request.url();
                    let response = if url.ends_with("/cfg.json") {
                        Response::from_string(cfg_body.clone())
                        // .with_header("Content-Type: application/json".parse().unwrap())
                    } else if url.ends_with("/presets.json") {
                        if let Some(ref presets) = presets_body {
                            Response::from_string(presets.clone())
                            // .with_header("Content-Type: application/json".parse().unwrap())
                        } else {
                            Response::from_string("not found").with_status_code(404)
                        }
                    } else {
                        Response::from_string("not found").with_status_code(404)
                    };
                    let _ = request.respond(response);
                }
            }
        });

        handle
    }

    fn validate_response_file(expected_file: PathBuf, expected_content: &str) {
        assert!(expected_file.exists());
        let contents = fs::read_to_string(expected_file).unwrap();
        assert_eq!(contents, expected_content);
    }

    fn validate_response_files(out_dir: &PathBuf, hostname: &str) {
        let cfg_path = out_dir.join(format!("{hostname}_cfg.json"));
        let presets_path = out_dir.join(format!("{hostname}_presets.json"));

        validate_response_file(cfg_path, &cfg_body(hostname));
        validate_response_file(presets_path, "presets data");
    }

    #[test]
    fn test_get_hostname_from_cfg_success() {
        let cfg = json!({
            "id": {
                "name": "test_device"
            }
        });

        let result = get_hostname_from_cfg(&cfg);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test_device");
    }

    #[test]
    fn test_get_hostname_from_cfg_missing_id() {
        let cfg = json!({
            "other": "value"
        });

        let result = get_hostname_from_cfg(&cfg);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Missing 'id' field in cfg.json"
        );
    }

    #[test]
    fn test_get_hostname_from_cfg_missing_name() {
        let cfg = json!({
            "id": {
                "other": "value"
            }
        });

        let result = get_hostname_from_cfg(&cfg);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Missing 'name' field in cfg.json"
        );
    }

    #[test]
    fn test_get_hostname_from_cfg_name_not_string() {
        let cfg = json!({
            "id": {
                "name": 123
            }
        });

        let result = get_hostname_from_cfg(&cfg);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Expected 'name' to be a string in cfg.json"
        );
    }

    #[test]
    fn test_get_hostname_from_cfg_empty_hostname() {
        let cfg = json!({
            "id": {
                "name": ""
            }
        });

        let result = get_hostname_from_cfg(&cfg);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Hostname is empty or contains only whitespace"
        );
    }

    #[test]
    fn test_get_hostname_from_cfg_whitespace_only_hostname() {
        let cfg = json!({
            "id": {
                "name": "   \t\n  "
            }
        });

        let result = get_hostname_from_cfg(&cfg);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Hostname is empty or contains only whitespace"
        );
    }

    #[test]
    fn test_get_hostname_from_cfg_hostname_with_whitespace() {
        let cfg = json!({
            "id": {
                "name": "  test_device  "
            }
        });

        let result = get_hostname_from_cfg(&cfg);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "  test_device  ");
    }

    #[test]
    fn test_backup_wled_creates_file() {
        // Start server in a background thread
        let servers = vec![mock_wled_server(
            "127.0.0.1:88",
            &cfg_body("testwled"),
            Some("presets data"),
        )];

        // Use a temp directory
        let dir = tempdir().unwrap();
        let out_dir = dir.path().to_path_buf();

        // Perform the backup.
        let backup_wled = backup_wled(
            &IpAddr::V4("127.0.0.1".parse::<Ipv4Addr>().unwrap()),
            88,
            &out_dir,
        );

        assert!(backup_wled.is_ok(), "Backup failed");

        // Check that the file exists
        validate_response_files(&out_dir, "testwled");

        // Shutdown the server
        for handle in servers {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_backup_wleds_creates_files() {
        // TODO: Add IP V6 test case.

        // Start server in a background thread
        let servers = vec![
            mock_wled_server("127.0.0.1:80", &cfg_body("testwled"), Some("presets data")),
            mock_wled_server(
                "127.0.0.1:8080",
                &cfg_body("testwled_port"),
                Some("presets data"),
            ),
        ];

        // Prepare mock WLED device
        let wleds = vec![
            mock_service_info("mdns_name", "127.0.0.1", 80),
            mock_service_info("mdns_name_port", "127.0.0.1", 8080),
        ];

        // Use a temp directory
        let dir = tempdir().unwrap();
        let out_dir = dir.path().to_path_buf();

        // Perform the backup.
        let backup_wleds = backup_wleds(wleds, &out_dir);

        assert!(backup_wleds.is_ok(), "Backup failed");

        // Check that the file exists
        validate_response_files(&out_dir, "testwled");
        validate_response_files(&out_dir, "testwled_port");

        // Shutdown the server
        for handle in servers {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_backup_wled_invalid_cfg_json_no_files_written() {
        let servers = vec![mock_wled_server(
            "127.0.0.1:89",
            "invalid json content",
            None,
        )];

        let dir = tempdir().unwrap();
        let out_dir = dir.path().to_path_buf();

        let backup_result = backup_wled(
            &IpAddr::V4("127.0.0.1".parse::<Ipv4Addr>().unwrap()),
            89,
            &out_dir,
        );

        assert!(
            backup_result.is_err(),
            "Backup should fail with invalid JSON"
        );

        let entries: Vec<_> = fs::read_dir(&out_dir).unwrap().collect();
        assert_eq!(
            entries.len(),
            0,
            "No files should be written when cfg.json parsing fails"
        );

        for handle in servers {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_backup_wleds_returns_error() {
        // Start server in a background thread. Use different ports to avoid conflicts.
        let servers = vec![mock_wled_server(
            "127.0.0.1:81",
            &cfg_body("testwled"),
            Some("presets data"),
        )];

        // Prepare mock WLED device
        let wleds = vec![
            mock_service_info("mdns_name_port", "127.0.0.1", 8081), // Not served, so will fail.
            mock_service_info("mdns_name", "127.0.0.1", 81),
        ];

        // Use a temp directory
        let dir = tempdir().unwrap();
        let out_dir = dir.path().to_path_buf();

        // Perform the backup.
        let backup_wleds = backup_wleds(wleds, &out_dir);

        assert!(backup_wleds.is_err(), "Backup failed, as it should have.");

        // Check that the file exists for teh value correctly served.
        validate_response_files(&out_dir, "testwled");

        // Shutdown the server
        for handle in servers {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_args_defaults() {
        let args = Args::parse_from(&["test"]);
        assert_eq!(args.out_dir, PathBuf::from("."));
        assert_eq!(args.search_secs, 4);
    }

    #[test]
    fn test_args_custom() {
        let args = Args::parse_from(&["test", "--out-dir", "mydir", "--search-secs", "10"]);
        assert_eq!(args.out_dir, PathBuf::from("mydir"));
        assert_eq!(args.search_secs, 10);
    }
}
