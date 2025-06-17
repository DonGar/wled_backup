use std::fs::File;
use std::io::copy;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

fn discover_wleds() -> Vec<ServiceInfo> {
    let mut wleds: Vec<ServiceInfo> = Vec::new();

    // Create a daemon
    let mdns = ServiceDaemon::new().expect("Failed to create daemon");

    // Browse for a service type.
    let service_type = "_wled._tcp.local.";
    let receiver = mdns.browse(service_type).expect("Failed to browse");

    let search_duration = std::time::Duration::from_secs(4);

    while let Ok(event) = receiver.recv_timeout(search_duration) {
        match event {
            ServiceEvent::ServiceResolved(info) => {
                wleds.push(info.clone());
                println!("Discovered: {}", info.get_fullname());
            }
            _other_event => {}
        }
    }

    return wleds;
}

fn download_file(url: &str, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let response = reqwest::blocking::get(url)?;
    let mut dest = File::create(path)?;
    let mut content = response;
    copy(&mut content, &mut dest)?;
    Ok(())
}

fn backup_wleds(wleds: Vec<ServiceInfo>) {
    for wled in wleds.iter() {
        if let Some(ip) = wled.get_addresses().iter().next() {
            let url = format!("http://{ip}/presets.json");
            let minimal_hostname = wled.get_hostname().split('.').next().unwrap_or("wled");
            let file = format!("{}.json", minimal_hostname);
            if let Err(result) = download_file(&url, &file) {
                eprintln!("Failed to backup {}: {result}", minimal_hostname);
            } else {
                println!("Backed up {}: {url} -> {file}", minimal_hostname);
            }
        }
    }
}

fn main() {
    let wleds = discover_wleds();
    backup_wleds(wleds);
    println!("Finished");
}
