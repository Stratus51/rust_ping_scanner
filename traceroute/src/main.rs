use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_ip_ping_request::ping;
use tokio_ip_ping_request::traceroute::{self, PingError, RouteNode};

struct RouteGraph {
    routes: Vec<HashMap<Ipv4Addr, (Duration, Duration)>>,
}

impl std::fmt::Display for RouteGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, hop) in self.routes.iter().enumerate() {
            writeln!(f, "Hop {}", i)?;
            for (ip, (latency, std_dev)) in hop.iter() {
                writeln!(f, "  - {}: {:?} (+/- {:?})", ip, latency, std_dev)?;
            }
        }
        Ok(())
    }
}

async fn aggregate_traceroute(
    mut rx: mpsc::Receiver<(u8, Result<RouteNode, PingError>)>,
) -> RouteGraph {
    let mut route_data = vec![];
    while let Some((ttl, ret)) = rx.recv().await {
        println!("{} {:?}", ttl, ret);
        if let Ok(ret) = ret {
            if route_data.len() < ttl as usize {
                route_data.push(HashMap::new());
            }
            let route_data = &mut route_data[(ttl - 1) as usize];
            if route_data.get(&ret.addr).is_none() {
                route_data.insert(ret.addr, vec![]);
            }
            let route_data = route_data.get_mut(&ret.addr).unwrap();
            route_data.push(ret.latency);
        }
    }

    RouteGraph {
        routes: route_data
            .into_iter()
            .map(|step| {
                let mut ret = HashMap::new();
                for (addr, latencies) in step.into_iter() {
                    let average =
                        latencies.iter().sum::<Duration>() / latencies.iter().count() as u32;
                    let std_dev = Duration::from_secs_f32(
                        (latencies
                            .iter()
                            .map(|v| {
                                if *v > average {
                                    *v - average
                                } else {
                                    average - *v
                                }
                                .as_secs_f32()
                            })
                            .map(|n| n * n)
                            .sum::<f32>())
                        .sqrt()
                            / latencies.iter().count() as f32,
                    );
                    ret.insert(addr, (average, std_dev));
                }
                ret
            })
            .collect(),
    }
}

async fn run_multi_icmp_traceroute(
    pinger: ping::icmp::Pinger,
    target: Ipv4Addr,
    ttl: u8,
    timeout: Duration,
    attempts: u8,
    tx: mpsc::Sender<(u8, Result<RouteNode, PingError>)>,
) {
    for i in 0..attempts {
        let mut rx = traceroute::icmp_traceroute(pinger.clone(), target, ttl, timeout, i);
        let mut i = 1;
        while let Some(res) = rx.recv().await {
            if tx.send((i, res)).await.is_err() {
                return;
            }
            i += 1;
        }
    }
}

fn multi_icmp_traceroute(
    pinger: ping::icmp::Pinger,
    target: Ipv4Addr,
    ttl: u8,
    timeout: Duration,
    attempts: u8,
) -> mpsc::Receiver<(u8, Result<RouteNode, PingError>)> {
    let (tx, rx) = mpsc::channel(1);
    tokio::spawn(run_multi_icmp_traceroute(
        pinger, target, ttl, timeout, attempts, tx,
    ));
    rx
}

async fn run_multi_paris_icmp_traceroute(
    pinger: ping::icmp::Pinger,
    target: Ipv4Addr,
    ttl: u8,
    timeout: Duration,
    attempts: u8,
    tx: mpsc::Sender<(u8, Result<RouteNode, PingError>)>,
) {
    for _ in 0..attempts {
        let mut rx = traceroute::paris_icmp_traceroute(pinger.clone(), target, ttl, timeout, 0);
        let mut i = 1;
        while let Some(res) = rx.recv().await {
            if tx.send((i, res)).await.is_err() {
                return;
            }
            i += 1;
        }
    }
}

fn multi_paris_icmp_traceroute(
    pinger: ping::icmp::Pinger,
    target: Ipv4Addr,
    ttl: u8,
    timeout: Duration,
    attempts: u8,
) -> mpsc::Receiver<(u8, Result<RouteNode, PingError>)> {
    let (tx, rx) = mpsc::channel(1);
    tokio::spawn(run_multi_paris_icmp_traceroute(
        pinger, target, ttl, timeout, attempts, tx,
    ));
    rx
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let target = args.next().unwrap();
    let timeout: u64 = args.next().unwrap().parse().unwrap();
    let timeout = Duration::from_millis(timeout);
    let ttl: u8 = args.next().unwrap().parse().unwrap();
    let attempts: u8 = args.next().unwrap().parse().unwrap();
    let pinger = ping::icmp::Pinger::new(64, 20).unwrap();

    println!("========================================================");
    println!("ICMP");
    println!("========================================================");

    println!("--------------------------------------------------------");
    println!("STD traceroute");
    println!("--------------------------------------------------------");
    let res_rx = multi_icmp_traceroute(
        pinger.clone(),
        target.parse().unwrap(),
        ttl,
        timeout,
        attempts,
    );
    let std_res = aggregate_traceroute(res_rx).await;

    println!("--------------------------------------------------------");
    println!("Paris traceroute");
    println!("--------------------------------------------------------");
    let res_rx = multi_paris_icmp_traceroute(
        pinger.clone(),
        target.parse().unwrap(),
        ttl,
        timeout,
        attempts,
    );
    let paris_res = aggregate_traceroute(res_rx).await;

    println!("========================================================");
    println!("Results");
    println!("========================================================");
    println!("STD traceroute");
    println!("{}\n", std_res);
    println!("Paris traceroute");
    println!("{}\n", paris_res);

    pinger.stop().await;
}
