mod ping;
mod traceroute;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::sync::mpsc;

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
    mut rx: mpsc::Receiver<(u8, Result<traceroute::RouteNode, traceroute::Error>)>,
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

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let target = args.next().unwrap();
    let timeout: u64 = args.next().unwrap().parse().unwrap();
    let ttl: u8 = args.next().unwrap().parse().unwrap();
    let pinger = ping::Pinger::new(std::time::Duration::from_secs(timeout), 64, 20).unwrap();

    println!("========================================================");
    println!("STD traceroute");
    println!("========================================================");
    let std_res = aggregate_traceroute(traceroute::traceroute(
        pinger.clone(),
        target.parse().unwrap(),
        ttl,
        10,
    ))
    .await;

    println!("========================================================");
    println!("Paris traceroute");
    println!("========================================================");
    let paris_res = aggregate_traceroute(traceroute::paris_traceroute(
        pinger.clone(),
        target.parse().unwrap(),
        ttl,
        10,
    ))
    .await;

    println!("========================================================");
    println!("Results");
    println!("========================================================");
    println!("STD traceroute");
    println!("{}\n", std_res);
    println!("Paris traceroute");
    println!("{}\n", paris_res);

    pinger.stop().await;
}
