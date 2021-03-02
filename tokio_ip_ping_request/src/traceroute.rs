pub use crate::ping::PingError;
use crate::ping::{icmp, tcp};
use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct RouteNode {
    pub addr: Ipv4Addr,
    pub latency: Duration,
}

pub enum FlowId {
    Fixed(u16),
    WithOffset(u8),
}

pub async fn icmp_traceroute_to_channel(
    pinger: icmp::Pinger,
    addr: Ipv4Addr,
    max_ttl: u8,
    timeout: Duration,
    flow_id: FlowId,
    tx: mpsc::Sender<Result<RouteNode, PingError>>,
) {
    for ttl in 1..max_ttl {
        let flow_id = match flow_id {
            FlowId::Fixed(id_shift) => id_shift,
            // TODO This case should never be used. It exists only for testing purposes
            FlowId::WithOffset(offset) => ((offset as u16) << 8) | ttl as u16,
        };
        match pinger.ping(addr, ttl, timeout, flow_id).await {
            Ok(latency) => {
                if tx.send(Ok(RouteNode { addr, latency })).await.is_err() {
                    return;
                }
                break;
            }
            Err(PingError::TimeExceeded { addr, latency }) => {
                if tx.send(Ok(RouteNode { addr, latency })).await.is_err() {
                    return;
                }
            }
            Err(PingError::Timeout) => {
                let _ = tx.send(Err(PingError::Timeout)).await;
                break;
            }
            Err(error) => {
                let _ = tx.send(Err(error)).await;
                break;
            }
        }
    }
}

pub fn icmp_traceroute(
    pinger: icmp::Pinger,
    addr: Ipv4Addr,
    max_ttl: u8,
    timeout: Duration,
    index: u8,
) -> mpsc::Receiver<Result<RouteNode, PingError>> {
    let (tx, rx) = mpsc::channel(5);
    tokio::spawn(icmp_traceroute_to_channel(
        pinger,
        addr,
        max_ttl,
        timeout,
        FlowId::WithOffset(index),
        tx,
    ));
    rx
}

pub fn paris_icmp_traceroute(
    pinger: icmp::Pinger,
    addr: Ipv4Addr,
    max_ttl: u8,
    timeout: Duration,
    flow_id: u16,
) -> mpsc::Receiver<Result<RouteNode, PingError>> {
    let (tx, rx) = mpsc::channel(5);
    tokio::spawn(icmp_traceroute_to_channel(
        pinger,
        addr,
        max_ttl,
        timeout,
        FlowId::Fixed(flow_id),
        tx,
    ));
    rx
}
