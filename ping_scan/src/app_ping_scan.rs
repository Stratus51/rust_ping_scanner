mod configuration;
mod cursor;
mod deprecated;
mod internet;

use configuration::{Configuration, EncodableConfiguration};
use futures::future::{select, Either};
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
    CpuUsage(f32),
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

// TODO sys_info::loadavg
async fn monitor_cpu_usage(response_tx: mpsc::Sender<Event>, mut end: oneshot::Receiver<()>) {}

// TODO
async fn monitor_link(
    pinger: Pinger,
    target: Ipv4Addr,
    ttl: u8,
    timeout: Duration,
    period: Duration,
    max_consecutive_fails: u8,
    response_tx: mpsc::Sender<Event>,
    mut end: oneshot::Receiver<()>,
) {
    let mut consecutive_fails = 0;
    let mut bad_state = false;
    loop {
        let ping_future = pinger.ping(target, ttl, timeout, 0);
        tokio::pin!(ping_future);
        end = match select(end, ping_future).await {
            Either::Left(_) => break,
            Either::Right((res, end)) => {
                match res {
                    Ok(_) => {
                        if response_tx.send(Event::LinkUp).await.is_err() {
                            break;
                        };
                        consecutive_fails = 0;
                        bad_state = false;
                    }
                    Err(_) => {
                        if !bad_state {
                            consecutive_fails += 1;
                            if consecutive_fails == max_consecutive_fails {
                                bad_state = true;
                                if response_tx.send(Event::LinkUp).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                };
                end
            }
        };
        tokio::time::sleep(period).await;
    }
}

pub async fn scan(conf: Configuration) {
    println!("Setup");
    // Build configuration bound objects
    let pinger = Pinger::new(conf.ping.size, conf.ping.parallelism as usize).unwrap();
    let mut cursor = conf.cursor.generate().unwrap();

    let parallelism = conf.ping.parallelism as usize - 1;
    let ttl = conf.ping.ttl;
    let mut out_file = tokio::fs::File::create(&conf.out_file).await.unwrap();

    // Setup result channel
    let (tx, mut rx) = mpsc::channel(parallelism);

    // Write configuration
    println!("Writing configuration");
    out_file.write_all(&conf.encode()).await.unwrap();

    // Start link monitoring
    let (monitor_end, monitor_end_rx) = oneshot::channel();
    let monitor_conf = conf.link_state_monitor.as_ref().unwrap();
    // TODO Configuration
    tokio::spawn(monitor_link(
        pinger.clone(),
        monitor_conf.target,
        monitor_conf.ttl,
        monitor_conf.timeout,
        monitor_conf.period,
        monitor_conf.max_consecutive_fails,
        tx.clone(),
        monitor_end_rx,
    ));

    // Start first thread batch
    println!("Starting first ping batch");
    let mut parallel = 0;
    for i in 0..parallelism {
        tokio::spawn(run_pinger(
            pinger.clone(),
            i as u32,
            cursor.value(),
            ttl,
            conf.ping.timeout,
            tx.clone(),
        ));
        parallel += 1;
        if cursor.move_next().is_err() {
            break;
        }
    }

    // TODO Adapt load to current CPU usage
    // Consume data cursor
    println!("Running");
    let mut out_data = vec![];
    let mut i = 0;
    let start = Instant::now();
    let mut cursor_done = false;
    let mut indices_since_last_checkpoint = 0;
    let mut link_down = false;
    while let Some(event) = rx.recv().await {
        match event {
            Event::PingResult { index, latency } => {
                // Print progress
                i += 1;
                let percent = 1000 * (i as u64) / conf.cursor.nb as u64;
                if percent > (1000 * (i as u64 - 1) / conf.cursor.nb as u64) {
                    println!(
                        "{}% after {:?}",
                        percent as f32 / 10.0,
                        Instant::now().duration_since(start)
                    );
                }
                indices_since_last_checkpoint += 1;

                // Process result
                if let Some(latency) = latency {
                    out_data.push(encode_result(index, latency));
                    if out_data.len() > 10 * parallelism {
                        out_file.write_all(&out_data.concat()).await.unwrap();
                        out_data.clear();
                    }
                }

                // Schedule next target
                if !cursor_done && !link_down && parallel as usize >= parallelism {
                    tokio::spawn(run_pinger(
                        pinger.clone(),
                        parallel + i,
                        cursor.value(),
                        ttl,
                        conf.ping.timeout,
                        tx.clone(),
                    ));
                    if cursor.move_next().is_err() {
                        cursor_done = true;
                    }
                } else {
                    parallel -= 1;
                    if parallel == 0 && cursor_done {
                        break;
                    }
                }
            }
            Event::LinkUp => {
                if link_down {
                    for _ in 0..indices_since_last_checkpoint {
                        let _ = cursor.move_prev();
                    }
                    i -= indices_since_last_checkpoint;
                    link_down = false;
                    let available_slots = parallelism - parallel as usize;
                    for _ in 0..available_slots {
                        tokio::spawn(run_pinger(
                            pinger.clone(),
                            parallel + i,
                            cursor.value(),
                            ttl,
                            conf.ping.timeout,
                            tx.clone(),
                        ));
                        if cursor.move_next().is_err() {
                            cursor_done = true;
                            break;
                        }
                        i += 1;
                    }
                }
                indices_since_last_checkpoint = 0;
            }
            Event::LinkDown => {
                link_down = true;
            }
            Event::CpuUsage(load) => {
                // TODO
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
