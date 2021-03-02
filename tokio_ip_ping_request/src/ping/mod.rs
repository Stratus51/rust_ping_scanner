pub mod icmp;
pub mod tcp;

use pnet::packet;
use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::sync::oneshot;

pub type IcmpCode = packet::icmp::IcmpCode;
pub type IcmpType = packet::icmp::IcmpType;

#[derive(Debug)]
pub struct PingRequest {
    addr: Ipv4Addr,
    ttl: u8,
    flow_id: u16,
    timeout: Duration,
    response_channel: oneshot::Sender<Result<Duration, PingError>>,
}

#[derive(Debug, Clone)]
pub enum PingError {
    Timeout,
    TimeExceeded {
        addr: Ipv4Addr,
        latency: Duration,
    },
    FailedToSendPacket,
    BackendClosed,
    IcmpError {
        responder: Ipv4Addr,
        code: IcmpCode,
        ty: IcmpType,
        data: Box<[u8]>,
        latency: Duration,
    },
}
