use clap::Parser;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::HashMap;
use std::fs::File;
use std::io::copy;
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

fn download_file(url: &str, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let response = reqwest::blocking::get(url)?;
    let mut dest = File::create(path)?;
    let mut content = response;
    copy(&mut content, &mut dest)?;
    Ok(())
}

fn backup_wleds(
    wleds: Vec<ServiceInfo>,
    out_dir: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut final_result = Ok(());

    for wled in wleds.iter() {
        if let Some(ip) = wled.get_addresses().iter().next() {
            let url = format!("http://{}:{}/presets.json", ip, wled.get_port());
            let minimal_hostname = wled.get_hostname().split('.').next().unwrap_or("wled");
            let mut file = out_dir.clone();
            file.push(format!("{}.json", minimal_hostname));
            print!("Backing up {minimal_hostname}: {url} -> {file:?}: ");
            if let Err(result) = download_file(&url, file.to_str().unwrap()) {
                println!("{result}");
                final_result = Err(result);
            } else {
                println!("SUCCESS");
            }
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
    use std::fs;
    use std::thread;
    use std::vec;
    use tempfile::tempdir;
    use tiny_http::{Response, Server};

    // Mock ServiceInfo for testing
    fn mock_service_info(name: &str, ip: &str, port: u16) -> ServiceInfo {
        ServiceInfo::new("_wled._tcp.local.", name, name, ip, port, None).unwrap()
    }

    fn mock_wled_server(addr: &str) -> thread::JoinHandle<()> {
        // Start server in a background thread
        let server = Server::http(addr).unwrap();
        let handle = thread::spawn(move || {
            if let Ok(request) = server.recv() {
                let response = Response::from_string("backup data");
                request.respond(response).unwrap();
            }
        });

        handle
    }

    fn validate_response_file(expected_file: PathBuf) {
        assert!(expected_file.exists());
        let contents = fs::read_to_string(expected_file).unwrap();
        assert_eq!(contents, "backup data");
    }

    #[test]
    fn test_backup_wleds_creates_files() {
        // TODO: Add IP V6 test case.

        // Start server in a background thread
        let servers = vec![
            mock_wled_server("127.0.0.1:80"),
            mock_wled_server("127.0.0.1:8080"),
        ];

        // Prepare mock WLED device
        let wleds = vec![
            mock_service_info("testwled", "127.0.0.1", 80),
            mock_service_info("testwled_port", "127.0.0.1", 8080),
        ];

        // Use a temp directory
        let dir = tempdir().unwrap();
        let out_dir = dir.path().to_path_buf();

        // Perform the backup.
        let backup_wleds = backup_wleds(wleds, &out_dir);

        assert!(backup_wleds.is_ok(), "Backup failed");

        // Check that the file exists
        validate_response_file(out_dir.join("testwled.json"));
        validate_response_file(out_dir.join("testwled_port.json"));

        // Shutdown the server
        for handle in servers {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_backup_wleds_returns_error() {
        // Start server in a background thread. Use different ports to avoid conflicts.
        let servers = vec![mock_wled_server("127.0.0.1:81")];

        // Prepare mock WLED device
        let wleds = vec![
            mock_service_info("testwled_port", "127.0.0.1", 8081), // Not served, so will fail.
            mock_service_info("testwled", "127.0.0.1", 81),
        ];

        // Use a temp directory
        let dir = tempdir().unwrap();
        let out_dir = dir.path().to_path_buf();

        // Perform the backup.
        let backup_wleds = backup_wleds(wleds, &out_dir);

        assert!(backup_wleds.is_err(), "Backup failed, as it should have.");

        // Check that the file exists for teh value correctly served.
        validate_response_file(out_dir.join("testwled.json"));

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
