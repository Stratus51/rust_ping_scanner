mod configuration;
mod cursor;
mod deprecated;
mod internet;

use configuration::{Configuration, EncodableConfiguration};
use futures::future::{select, Either};
use internet::u32_to_ip;
use std::time::Duration;
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
    CpuLoad(f64),
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

async fn monitor_cpu_load(
    refresh_rate: Duration,
    response_tx: mpsc::Sender<Event>,
    mut end: oneshot::Receiver<()>,
) {
    loop {
        if let Ok(sys_info::LoadAvg { one, .. }) = sys_info::loadavg() {
            if response_tx.send(Event::CpuLoad(one)).await.is_err() {
                return;
            }
        }

        // Sleep
        let sleep_future = tokio::time::sleep(refresh_rate);
        tokio::pin!(sleep_future);
        end = match select(end, sleep_future).await {
            Either::Left(_) => break,
            Either::Right((_, end)) => end,
        }
    }
}

async fn monitor_link(
    pinger: Pinger,
    conf: configuration::LinkStateMonitorConfiguration,
    response_tx: mpsc::Sender<Event>,
    mut end: oneshot::Receiver<()>,
) {
    let configuration::LinkStateMonitorConfiguration {
        target,
        ttl,
        timeout,
        period,
        max_consecutive_fails,
    } = conf;
    let mut consecutive_fails = 0;
    let mut bad_state = false;
    loop {
        // Ping
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
                                if response_tx.send(Event::LinkDown).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                };
                end
            }
        };

        // Sleep
        let sleep_future = tokio::time::sleep(period);
        tokio::pin!(sleep_future);
        end = match select(end, sleep_future).await {
            Either::Left(_) => break,
            Either::Right((_, end)) => end,
        }
    }
}

pub async fn scan(conf: Configuration) {
    println!("Setup");
    // Build configuration bound objects
    let pinger = Pinger::new(conf.ping.size, conf.ping.parallelism as usize).unwrap();
    let mut cursor = conf.cursor.generate().unwrap();

    let mut parallelism_target = conf.ping.parallelism as usize - 1;
    let ttl = conf.ping.ttl;
    let mut out_file = tokio::fs::File::create(&conf.out_file).await.unwrap();

    // Setup result channel
    let (tx, mut rx) = mpsc::channel(parallelism_target);

    // Write configuration
    println!("Writing configuration");
    out_file.write_all(&conf.encode()).await.unwrap();

    // Start link monitoring
    let (monitor_link_end, monitor_link_end_rx) = oneshot::channel();
    tokio::spawn(monitor_link(
        pinger.clone(),
        conf.link_state_monitor.clone().unwrap(),
        tx.clone(),
        monitor_link_end_rx,
    ));

    // Start cpu load monitoring
    let (monitor_cpu_end, monitor_cpu_end_rx) = oneshot::channel();
    let monitor_cpu_conf = conf.cpu_load_monitor.as_ref().unwrap();
    tokio::spawn(monitor_cpu_load(
        monitor_cpu_conf.refresh_rate,
        tx.clone(),
        monitor_cpu_end_rx,
    ));

    // Start first thread batch
    println!("Starting first ping batch");
    let mut parallel = 0;
    for i in 0..parallelism_target {
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
    let start = std::time::SystemTime::now();
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
                    println!("{:?}: Progress {}%", start.elapsed(), percent as f32 / 10.0,);
                }
                indices_since_last_checkpoint += 1;

                // Process result
                if let Some(latency) = latency {
                    out_data.push(encode_result(index, latency));
                    if out_data.len() > 10 * parallelism_target {
                        out_file.write_all(&out_data.concat()).await.unwrap();
                        out_data.clear();
                    }
                }

                // Schedule next target
                if !cursor_done && !link_down && parallel as usize >= parallelism_target {
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
                    println!("{:?}: Link up!", start.elapsed());
                    for _ in 0..indices_since_last_checkpoint {
                        let _ = cursor.move_prev();
                    }
                    i -= indices_since_last_checkpoint;
                    link_down = false;
                    if parallelism_target > parallel as usize {
                        let available_slots = parallelism_target - parallel as usize;
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
                            parallel += 1;
                        }
                    }
                }
                indices_since_last_checkpoint = 0;
            }
            Event::LinkDown => {
                if !link_down {
                    println!("{:?}: Link down!", start.elapsed());
                    link_down = true;
                }
            }
            Event::CpuLoad(load) => {
                if load > monitor_cpu_conf.max {
                    parallelism_target =
                        (parallelism_target as f64 * monitor_cpu_conf.max / load) as usize;
                    println!(
                        "{:?}: Lowering CPU load ({} > {}). Setting parallelism to: {}",
                        start.elapsed(),
                        load,
                        monitor_cpu_conf.max,
                        parallelism_target
                    );
                } else if load < monitor_cpu_conf.min {
                    parallelism_target =
                        (parallelism_target as f64 * monitor_cpu_conf.min / load) as usize;
                    println!(
                        "{:?}: Rising CPU load ({} < {}). Setting parallelism to: {}",
                        start.elapsed(),
                        load,
                        monitor_cpu_conf.min,
                        parallelism_target
                    );
                }
            }
        }
    }
    println!("Done");
    out_file.write_all(&out_data.concat()).await.unwrap();
    out_data.clear();

    // Cleanup
    pinger.stop().await;
    let _ = monitor_link_end.send(());
    let _ = monitor_cpu_end.send(());
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let conf = configuration::Configuration::from_args(&args).unwrap();

    scan(conf).await;
}
