use mpc_exploration::routes::addition::CreateProcessHttpBody;

// This binary creates a new addition process by generating a new process ID
// and notifying all peer servers about the new process via HTTP requests.
// Run via
// ```
// cargo run --bin new_addition -- ports=<port1,port2,...>
// ```
fn main() {
    // Load peer ports from arguments
    let ports = std::env::args()
        .find(|arg| arg.starts_with("ports="))
        .map(|arg| arg.trim_start_matches("ports=").to_string())
        .expect("ports argument is required, e.g., ports=8001,8002,8003")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<String>>();
    if ports.is_empty() {
        eprintln!("ports argument cannot be empty");
        std::process::exit(1);
    }
    // Construct peer URLs from ports
    let peer_urls: Vec<String> = ports
        .iter()
        .map(|port| format!("http://localhost:{}", port))
        .collect();

    // Generate a new process ID
    let process_id = uuid::Uuid::new_v4();
    println!("Generated new process ID: {}", process_id);

    // Sends a POST /additions request to each peer URL with the new process ID
    let client = reqwest::blocking::Client::new();
    for peer_url in peer_urls {
        let url = format!("{}/additions", peer_url);
        let res = client
            .post(&url)
            .json(&CreateProcessHttpBody { process_id })
            .send();
        match res {
            Ok(response) => {
                if response.status().is_success() {
                    println!(
                        "Successfully notified peer at {}: {}",
                        peer_url,
                        response.status()
                    );
                } else {
                    eprintln!(
                        "Failed to notify peer at {}: {}",
                        peer_url,
                        response.status()
                    );
                }
            }
            Err(e) => {
                eprintln!("Error notifying peer at {}: {}", peer_url, e);
            }
        }
    }
}
