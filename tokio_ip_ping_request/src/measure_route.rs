use crate::ping::icmp;
pub use crate::ping::PingError;
use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct RouteNode {
    addr: Ipv4Addr,
    latency: Duration,
}

#[derive(Debug)]
pub struct TtlStat {
    ttl: u8,
    stat: u8,
}

#[derive(Debug)]
pub enum RouteMeasureResult {
    Stable(u8),
    Unstable(Vec<TtlStat>),
}

#[derive(Debug)]
pub enum RouteMeasureData {
    Ping(Duration),
    TimeExceeded(RouteNode),
    Result(RouteMeasureResult),
}

async fn icmp_ttl_stats(
    pinger: &icmp::Pinger,
    addr: Ipv4Addr,
    ttl: u8,
    timeout: Duration,
    flow_id: u16,
    nb_retries: u8,
    tx: mpsc::Sender<Result<RouteMeasureData, PingError>>,
) -> Result<u8, ()> {
    let mut ret = 0;
    for _ in 0..nb_retries {
        match pinger.ping(addr, ttl, timeout, flow_id).await {
            Ok(latency) => {
                if tx.send(Ok(RouteMeasureData::Ping(latency))).await.is_err() {
                    return Err(());
                }
                ret += 1;
            }
            Err(PingError::TimeExceeded { addr, latency }) => {
                if tx
                    .send(Ok(RouteMeasureData::TimeExceeded(RouteNode {
                        addr,
                        latency,
                    })))
                    .await
                    .is_err()
                {
                    return Err(());
                }
            }
            Err(PingError::Timeout) => {
                let _ = tx.send(Err(PingError::Timeout)).await;
                return Err(());
            }
            Err(error) => {
                let _ = tx.send(Err(error)).await;
                return Err(());
            }
        }
    }
    Ok(ret)
}

pub async fn icmp_measure_route_to_channel(
    pinger: icmp::Pinger,
    addr: Ipv4Addr,
    max_ttl: u8,
    timeout: Duration,
    flow_id: u16,
    stats_retries: u8,
    tx: mpsc::Sender<Result<RouteMeasureData, PingError>>,
) {
    // Check that the target is reachable
    match pinger.ping(addr, max_ttl, timeout, flow_id).await {
        Ok(latency) => {
            if tx.send(Ok(RouteMeasureData::Ping(latency))).await.is_err() {
                return;
            }
        }
        Err(error) => {
            let _ = tx.send(Err(error)).await;
            return;
        }
    }

    // Dichotomic search
    let mut diff = max_ttl / 2;
    let mut distance = max_ttl - diff;
    while diff > 0 {
        match pinger.ping(addr, distance, timeout, flow_id).await {
            Ok(latency) => {
                if tx.send(Ok(RouteMeasureData::Ping(latency))).await.is_err() {
                    return;
                }
                diff /= 2;
                distance -= diff;
            }
            Err(PingError::TimeExceeded { addr, latency }) => {
                if tx
                    .send(Ok(RouteMeasureData::TimeExceeded(RouteNode {
                        addr,
                        latency,
                    })))
                    .await
                    .is_err()
                {
                    return;
                }
                diff /= 2;
                if diff == 0 {
                    diff = 1;
                }
                distance += diff;
            }
            Err(error) => {
                let _ = tx.send(Err(error)).await;
                return;
            }
        }
    }

    // Stability check
    let mut ret_list = vec![];

    let mut stat = 0;
    // Catch upper ttl limit
    let mut ttl = distance;
    while stat < stats_retries && ttl <= max_ttl {
        stat = match icmp_ttl_stats(
            &pinger,
            addr,
            ttl,
            timeout,
            flow_id,
            stats_retries,
            tx.clone(),
        )
        .await
        {
            Err(_) => return,
            Ok(stat) => stat,
        };
        ret_list.push(TtlStat { ttl, stat });
        ttl += 1;
    }

    // Catch lower_limit
    let mut ttl = distance - 1;
    stat = 1;
    while stat > 0 && ttl > 0 {
        stat = match icmp_ttl_stats(
            &pinger,
            addr,
            ttl - 1,
            timeout,
            flow_id,
            stats_retries,
            tx.clone(),
        )
        .await
        {
            Err(_) => return,
            Ok(stat) => stat,
        };
        if stat > 0 {
            ret_list.push(TtlStat { ttl, stat });
        }
        ttl -= 1;
    }

    let _ = if ret_list.len() == 1 {
        tx.send(Ok(RouteMeasureData::Result(RouteMeasureResult::Stable(
            ret_list[0].ttl,
        ))))
    } else {
        tx.send(Ok(RouteMeasureData::Result(RouteMeasureResult::Unstable(
            ret_list,
        ))))
    }
    .await;
}

pub fn icmp_measure_route(
    pinger: icmp::Pinger,
    addr: Ipv4Addr,
    max_ttl: u8,
    timeout: Duration,
    flow_id: u16,
    stats_retries: u8,
) -> mpsc::Receiver<Result<RouteMeasureData, PingError>> {
    let (tx, rx) = mpsc::channel(5);
    tokio::spawn(icmp_measure_route_to_channel(
        pinger,
        addr,
        max_ttl,
        timeout,
        flow_id,
        stats_retries,
        tx,
    ));
    rx
}
