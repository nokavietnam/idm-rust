use std::env;

use idm_rust::downloader;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: idm-rust <URL> [connections]");
        eprintln!("  connections  Number of parallel segments (default: 4)");
        eprintln!();
        eprintln!("Press Ctrl+C during a download to pause.");
        eprintln!("Re-run the same command to resume automatically.");
        std::process::exit(1);
    }

    let url = &args[1];
    let connections: usize = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);

    // CancellationToken wired to Ctrl+C for graceful pause.
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for Ctrl+C");
        eprintln!("\nCtrl+C received — saving progress…");
        cancel_clone.cancel();
    });

    match downloader::download(url, connections, cancel, None).await {
        Ok(()) => {}
        Err(downloader::DownloadError::Cancelled) => {
            // State was already saved inside download().
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}
