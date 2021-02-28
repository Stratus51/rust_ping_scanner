use crate::ping::{PingResult, Pinger};
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct RouteNode {
    addr: Ipv4Addr,
    latency: Duration,
}

#[derive(Debug)]
pub struct ListRouteNodesResult {
    ttl: u8,
    nodes: Vec<Ipv4Addr>,
}

#[derive(Debug)]
pub enum ListRouteNodesData {
    Ping(Duration),
    TimeExceeded(RouteNode),
    Result(ListRouteNodesResult),
}

#[derive(Debug)]
pub enum Error {
    TimeExceeded,
    Timeout,
    Other(PingResult),
}

async fn ttl_stats(
    pinger: &Pinger,
    addr: Ipv4Addr,
    ttl: u8,
    nb_retries: u8,
    tx: mpsc::Sender<Result<ListRouteNodesData, Error>>,
) -> Result<Vec<Ipv4Addr>, ()> {
    let mut list = HashSet::new();
    for _ in 0..nb_retries {
        match pinger.ping(addr, ttl).await {
            PingResult::Ok(latency) => {
                if tx
                    .send(Ok(ListRouteNodesData::Ping(latency)))
                    .await
                    .is_err()
                {
                    return Err(());
                }
                list.insert(addr);
            }
            PingResult::TimeExceeded { addr, latency } => {
                if tx
                    .send(Ok(ListRouteNodesData::TimeExceeded(RouteNode {
                        addr,
                        latency,
                    })))
                    .await
                    .is_err()
                {
                    return Err(());
                }
                list.insert(addr);
            }
            PingResult::Timeout => {
                let _ = tx.send(Err(Error::Timeout)).await;
                return Err(());
            }
            error => {
                let _ = tx.send(Err(Error::Other(error))).await;
                return Err(());
            }
        }
    }
    Ok(list.into_iter().collect())
}

pub async fn list_route_nodes_to_channel(
    pinger: Pinger,
    addr: Ipv4Addr,
    distance: u8,
    stats_retries: u8,
    tx: mpsc::Sender<Result<ListRouteNodesData, Error>>,
) {
    for ttl in 1..=distance {
        match ttl_stats(&pinger, addr, ttl, stats_retries, tx.clone()).await {
            Ok(nodes) => {
                let done = nodes.len() == 1 && nodes[0] == addr;
                if tx
                    .send(Ok(ListRouteNodesData::Result(ListRouteNodesResult {
                        ttl,
                        nodes,
                    })))
                    .await
                    .is_err()
                {
                    return;
                }
                if done {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

pub fn list_route_nodes(
    pinger: Pinger,
    addr: Ipv4Addr,
    distance: u8,
    stats_retries: u8,
) -> mpsc::Receiver<Result<ListRouteNodesData, Error>> {
    let (tx, rx) = mpsc::channel(5);
    tokio::spawn(list_route_nodes_to_channel(
        pinger,
        addr,
        distance,
        stats_retries,
        tx,
    ));
    rx
}
