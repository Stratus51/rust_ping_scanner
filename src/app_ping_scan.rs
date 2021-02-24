mod configuration;
mod internet;
mod ping;
mod u32_sampling_iterator;

use configuration::Configuration;
use internet::u32_to_ip;
use ping::Pinger;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

pub async fn run_pinger(
    pinger: Pinger,
    target: u32,
    ttl: u8,
    response_tx: mpsc::Sender<Option<Duration>>,
) {
    let _ = response_tx
        .send(match pinger.ping(u32_to_ip(target), ttl).await {
            ping::PingResult::Ok(latency) => Some(latency),
            _ => None,
        })
        .await;
}

pub fn encode_result(result: Option<Duration>) -> [u8; 4] {
    match result {
        Some(duration) => ((duration.as_nanos() / 10) as u32).to_le_bytes(),
        None => [0, 0, 0, 0],
    }
}

pub async fn scan(conf: Configuration) {
    println!("Setup");
    // Build configuration bound objects
    let configuration::ConfigurationObject {
        mut iterator,
        ping: pinger,
    } = conf.generate().unwrap();
    let parallelism = conf.ping.parallelism as usize;
    let ttl = conf.ping.ttl;
    let mut out_file = tokio::fs::File::create(&conf.out_file).await.unwrap();

    // Setup result channel
    let (tx, mut rx) = mpsc::channel(parallelism);

    // Write configuration
    println!("Writing configuration");
    out_file.write_all(&conf.encode()).await.unwrap();

    // Start first thread batch
    println!("Starting first ping batch");
    let mut parallel = 0;
    for _ in 0..parallelism {
        if let Some(target) = iterator.next() {
            tokio::spawn(run_pinger(pinger.clone(), target, ttl, tx.clone()));
            parallel += 1;
        } else {
            break;
        }
    }

    // Consume data iterator
    println!("Running");
    let mut out_data = vec![];
    while let Some(result) = rx.recv().await {
        println!("Ping: {:?}", result);
        // Process result
        out_data.push(encode_result(result));
        if out_data.len() > 1000000 {
            out_file.write_all(&out_data.concat()).await.unwrap();
            out_data.clear();
        }

        // Schedule next target
        if let Some(target) = iterator.next() {
            tokio::spawn(run_pinger(pinger.clone(), target, ttl, tx.clone()));
        } else {
            parallel -= 1;
            if parallel == 0 {
                break;
            }
        }
    }
    println!("Done");
    out_file.write_all(&out_data.concat()).await.unwrap();
    out_data.clear();

    // Cleanup
    pinger.stop().await;
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let conf = configuration::Configuration::from_args(&args).unwrap();

    scan(conf).await;
}
