mod configuration;
mod internet;
mod ping;
mod u32_sampling_iterator;

use configuration::Configuration;
use internet::u32_to_ip;
use ping::Pinger;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

pub async fn run_pinger(
    pinger: Pinger,
    index: u32,
    target: u32,
    ttl: u8,
    response_tx: mpsc::Sender<(u32, Option<Duration>)>,
) {
    let res = match pinger.ping(u32_to_ip(target), ttl).await {
        ping::PingResult::Ok(latency) => Some(latency),
        _ => None,
    };
    let _ = response_tx.send((index, res)).await;
}

pub fn encode_result(index: u32, duration: Duration) -> Box<[u8]> {
    [
        index.to_le_bytes(),
        ((duration.as_nanos() / 10) as u32).to_le_bytes(),
    ]
    .concat()
    .into()
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
    for i in 0..parallelism {
        if let Some(target) = iterator.next() {
            tokio::spawn(run_pinger(
                pinger.clone(),
                i as u32,
                target,
                ttl,
                tx.clone(),
            ));
            parallel += 1;
        } else {
            break;
        }
    }

    // Consume data iterator
    println!("Running");
    let mut out_data = vec![];
    let mut i = 0;
    let start = Instant::now();
    while let Some((index, result)) = rx.recv().await {
        // Print progress
        i += 1;
        let percent = 100 * (i as u64) / conf.iterator.nb as u64;
        if percent > (100 * (i as u64 - 1) / conf.iterator.nb as u64) {
            println!(
                "{}% after {:?}",
                percent,
                Instant::now().duration_since(start)
            );
        }

        // Process result
        if let Some(result) = result {
            out_data.push(encode_result(index, result));
            if out_data.len() > 10 * parallelism {
                out_file.write_all(&out_data.concat()).await.unwrap();
                out_data.clear();
            }
        }

        // Schedule next target
        if let Some(target) = iterator.next() {
            tokio::spawn(run_pinger(
                pinger.clone(),
                parallel + i,
                target,
                ttl,
                tx.clone(),
            ));
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
