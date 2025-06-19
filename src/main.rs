use clap::Parser;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
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
    let mut wleds: Vec<ServiceInfo> = Vec::new();

    // Create a daemon
    let mdns = ServiceDaemon::new().expect("Failed to create daemon");

    // Browse for a service type.
    let service_type = "_wled._tcp.local.";
    let receiver = mdns.browse(service_type).expect("Failed to browse");

    while let Ok(event) = receiver.recv_timeout(search_duration) {
        match event {
            ServiceEvent::ServiceResolved(info) => {
                wleds.push(info.clone());
                println!("Discovered: {}", info.get_fullname());
            }
            _other_event => {}
        }
    }

    wleds
}

fn download_file(url: &str, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let response = reqwest::blocking::get(url)?;
    let mut dest = File::create(path)?;
    let mut content = response;
    copy(&mut content, &mut dest)?;
    Ok(())
}

fn backup_wleds(wleds: Vec<ServiceInfo>, out_dir: &PathBuf) {
    for wled in wleds.iter() {
        if let Some(ip) = wled.get_addresses().iter().next() {
            let url = format!("http://{ip}/presets.json");
            let minimal_hostname = wled.get_hostname().split('.').next().unwrap_or("wled");
            let mut file = out_dir.clone();
            file.push(format!("{}.json", minimal_hostname));
            if let Err(result) = download_file(&url, file.to_str().unwrap()) {
                eprintln!("Failed to backup {}: {result}", minimal_hostname);
            } else {
                println!("Backed up {}: {url} -> {:?}", minimal_hostname, file);
            }
        }
    }
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
    backup_wleds(wleds, &args.out_dir);
    println!("Finished");
}
