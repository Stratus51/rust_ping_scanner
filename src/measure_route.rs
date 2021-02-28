use crate::ping::{PingResult, Pinger};
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
    tx: mpsc::Sender<Result<RouteMeasureData, Error>>,
) -> Result<u8, ()> {
    let mut ret = 0;
    for _ in 0..nb_retries {
        match pinger.ping(addr, ttl).await {
            PingResult::Ok(latency) => {
                if tx.send(Ok(RouteMeasureData::Ping(latency))).await.is_err() {
                    return Err(());
                }
                ret += 1;
            }
            PingResult::TimeExceeded { addr, latency } => {
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
    Ok(ret)
}

pub async fn measure_route_to_channel(
    pinger: Pinger,
    addr: Ipv4Addr,
    max_ttl: u8,
    stats_retries: u8,
    tx: mpsc::Sender<Result<RouteMeasureData, Error>>,
) {
    // Check that the target is reachable
    match pinger.ping(addr, max_ttl).await {
        PingResult::Ok(latency) => {
            if tx.send(Ok(RouteMeasureData::Ping(latency))).await.is_err() {
                return;
            }
        }
        PingResult::TimeExceeded { .. } => {
            let _ = tx.send(Err(Error::TimeExceeded)).await;
            return;
        }
        PingResult::Timeout => {
            let _ = tx.send(Err(Error::Timeout)).await;
            return;
        }
        error => {
            let _ = tx.send(Err(Error::Other(error))).await;
            return;
        }
    }

    // Dichotomic search
    let mut diff = max_ttl / 2;
    let mut distance = max_ttl - diff;
    while diff > 0 {
        match pinger.ping(addr, distance).await {
            PingResult::Ok(latency) => {
                if tx.send(Ok(RouteMeasureData::Ping(latency))).await.is_err() {
                    return;
                }
                diff /= 2;
                distance -= diff;
            }
            PingResult::TimeExceeded { addr, latency } => {
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
            PingResult::Timeout => {
                let _ = tx.send(Err(Error::Timeout)).await;
                return;
            }
            error => {
                let _ = tx.send(Err(Error::Other(error))).await;
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
        stat = match ttl_stats(&pinger, addr, ttl, stats_retries, tx.clone()).await {
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
        stat = match ttl_stats(&pinger, addr, ttl - 1, stats_retries, tx.clone()).await {
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

pub fn measure_route(
    pinger: Pinger,
    addr: Ipv4Addr,
    max_ttl: u8,
    stats_retries: u8,
) -> mpsc::Receiver<Result<RouteMeasureData, Error>> {
    let (tx, rx) = mpsc::channel(5);
    tokio::spawn(measure_route_to_channel(
        pinger,
        addr,
        max_ttl,
        stats_retries,
        tx,
    ));
    rx
}
