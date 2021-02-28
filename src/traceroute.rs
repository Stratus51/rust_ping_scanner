use crate::ping::{PingResult, Pinger};
use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct RouteNode {
    pub addr: Ipv4Addr,
    pub latency: Duration,
}

#[derive(Debug)]
pub enum Error {
    TimeExceeded,
    Timeout,
    Other(PingResult),
}

pub async fn traceroute_to_channel(
    pinger: Pinger,
    addr: Ipv4Addr,
    max_ttl: u8,
    repeat: u8,
    with_shift_per_packet: bool,
    tx: mpsc::Sender<(u8, Result<RouteNode, Error>)>,
) {
    for i in 0..repeat {
        let mut success = false;
        for ttl in 1..max_ttl {
            let id_shift = if with_shift_per_packet { i as u16 } else { 0 };
            match pinger.ping_with_id_shift(addr, ttl, id_shift).await {
                PingResult::Ok(latency) => {
                    if tx
                        .send((ttl, Ok(RouteNode { addr, latency })))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    success = true;
                    break;
                }
                PingResult::TimeExceeded { addr, latency } => {
                    if tx
                        .send((ttl, Ok(RouteNode { addr, latency })))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                PingResult::Timeout => {
                    let _ = tx.send((ttl, Err(Error::Timeout))).await;
                    break;
                }
                error => {
                    let _ = tx.send((ttl, Err(Error::Other(error)))).await;
                    break;
                }
            }
        }
        if !success {
            let _ = tx.send((max_ttl, Err(Error::TimeExceeded))).await;
        }
    }
}

pub fn traceroute(
    pinger: Pinger,
    addr: Ipv4Addr,
    max_ttl: u8,
    repeat: u8,
) -> mpsc::Receiver<(u8, Result<RouteNode, Error>)> {
    let (tx, rx) = mpsc::channel(5);
    tokio::spawn(traceroute_to_channel(
        pinger, addr, max_ttl, repeat, true, tx,
    ));
    rx
}

pub fn paris_traceroute(
    pinger: Pinger,
    addr: Ipv4Addr,
    max_ttl: u8,
    repeat: u8,
) -> mpsc::Receiver<(u8, Result<RouteNode, Error>)> {
    let (tx, rx) = mpsc::channel(5);
    tokio::spawn(traceroute_to_channel(
        pinger, addr, max_ttl, repeat, false, tx,
    ));
    rx
}
