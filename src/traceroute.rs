use crate::ping::{PingResult, Pinger};
use std::net::IpAddr;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct RouteNode {
    addr: IpAddr,
    latency: Duration,
}

#[derive(Debug)]
pub enum Error {
    TimeExceeded,
    Timeout,
    Other(PingResult),
}

pub async fn traceroute_to_channel(
    pinger: Pinger,
    addr: IpAddr,
    max_ttl: u8,
    tx: mpsc::Sender<Result<RouteNode, Error>>,
) {
    for ttl in 1..max_ttl {
        match pinger.ping(addr, ttl).await {
            PingResult::Ok(latency) => {
                let _ = tx.send(Ok(RouteNode { addr, latency })).await;
                pinger.stop().await;
                return;
            }
            PingResult::TimeExceeded { addr, latency } => {
                if tx.send(Ok(RouteNode { addr, latency })).await.is_err() {
                    pinger.stop().await;
                    return;
                }
            }
            PingResult::Timeout => {
                let _ = tx.send(Err(Error::Timeout)).await;
                pinger.stop().await;
                return;
            }
            error => {
                let _ = tx.send(Err(Error::Other(error))).await;
                pinger.stop().await;
                return;
            }
        }
    }
    let _ = tx.send(Err(Error::TimeExceeded)).await;
    pinger.stop().await;
}

pub fn traceroute(
    pinger: Pinger,
    addr: IpAddr,
    max_ttl: u8,
) -> mpsc::Receiver<Result<RouteNode, Error>> {
    let (tx, rx) = mpsc::channel(5);
    tokio::spawn(traceroute_to_channel(pinger, addr, max_ttl, tx));
    rx
}
