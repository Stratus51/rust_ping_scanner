mod configuration;
mod internet;
mod u32_sampling_iterator;

use configuration::Configuration;
// TODO Use pnet lib method instead
use internet::u32_to_ip;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, oneshot};
use tokio_ip_ping_request::ping::{self, icmp::Pinger};

enum Event {
    PingResult {
        index: u32,
        latency: Option<Duration>,
    },
    LinkUp,
    LinkDown,
}

async fn run_pinger(
    pinger: Pinger,
    index: u32,
    target: u32,
    ttl: u8,
    timeout: Duration,
    response_tx: mpsc::Sender<Event>,
) {
    let res = match pinger.ping(u32_to_ip(target), ttl, timeout, 0).await {
        Ok(latency) => Some(latency),
        _ => None,
    };
    let _ = response_tx
        .send(Event::PingResult {
            index,
            latency: res,
        })
        .await;
}

pub fn encode_result(index: u32, duration: Duration) -> Box<[u8]> {
    [
        index.to_le_bytes(),
        ((duration.as_nanos() / 10) as u32).to_le_bytes(),
    ]
    .concat()
    .into()
}

async fn monitor_link(
    pinger: Pinger,
    target: Ipv4Addr,
    ttl: u8,
    timeout: Duration,
    response_tx: mpsc::Sender<Event>,
    mut end: oneshot::Receiver<()>,
) {
    // TODO Use future::either instead to stop on either event
    while let Err(oneshot::error::TryRecvError::Empty) = end.try_recv() {
        // TODO Implement
        let res = match pinger.ping(target, ttl, timeout, 0).await {
            Ok(latency) => Some(latency),
            _ => None,
        };
    }
}

pub async fn scan(conf: Configuration) {
    println!("Setup");
    // Build configuration bound objects
    let configuration::ConfigurationObject {
        mut iterator,
        ping: pinger,
    } = conf.generate().unwrap();
    let parallelism = conf.ping.parallelism as usize - 1;
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
                conf.ping.timeout,
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
    while let Some(event) = rx.recv().await {
        match event {
            Event::PingResult { index, latency } => {
                // Print progress
                i += 1;
                let percent = 1000 * (i as u64) / conf.iterator.nb as u64;
                if percent > (1000 * (i as u64 - 1) / conf.iterator.nb as u64) {
                    println!(
                        "{}% after {:?}",
                        percent as f32 / 10.0,
                        Instant::now().duration_since(start)
                    );
                }

                // Process result
                if let Some(latency) = latency {
                    out_data.push(encode_result(index, latency));
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
                        conf.ping.timeout,
                        tx.clone(),
                    ));
                } else {
                    parallel -= 1;
                    if parallel == 0 {
                        break;
                    }
                }
            }
            // TODO Manage anti DDoS mitigation
            _ => (),
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
