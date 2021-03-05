use pnet::packet::icmp;
use pnet::packet::icmp::echo_request;
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::Packet;
use pnet::transport::icmp_packet_iter;
use pnet::transport::transport_channel;
use pnet::transport::TransportChannelType::Layer4;
use pnet::transport::TransportProtocol::Ipv4;
use pnet::transport::{TransportReceiver, TransportSender};
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use super::*;

#[derive(Debug)]
pub struct OngoingRequest {
    start: Instant,
    stop: Instant,
    response_channel: oneshot::Sender<Result<Duration, PingError>>,
}

#[derive(Debug, Clone, Copy)]
pub struct PingIdentifier {
    responder: Ipv4Addr,
    id: u16,
    sn: u16,
    stop: Instant,
    destination: Ipv4Addr,
}

#[derive(Debug, Clone)]
pub enum PingRequestResponse {
    PingResponse,
    PingTimeExceeded,
    IcmpError(Box<[u8]>),
}

impl PingRequestResponse {
    fn build_input(self, id: PingIdentifier) -> Input {
        match self {
            Self::PingResponse => Input::PingResponse(id),
            Self::PingTimeExceeded => Input::PingTimeExceeded(id),
            Self::IcmpError(v) => {
                let p = icmp::IcmpPacket::new(&v).unwrap();
                let ty = p.get_icmp_type();
                let code = p.get_icmp_code();
                Input::IcmpError {
                    id,
                    ty,
                    code,
                    data: v,
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum Input {
    PingRequest(PingRequest),
    PingResponse(PingIdentifier),
    PingTimeExceeded(PingIdentifier),
    IcmpError {
        id: PingIdentifier,
        ty: icmp::IcmpType,
        code: icmp::IcmpCode,
        data: Box<[u8]>,
    },
    Timeout,
    Stop,
}

pub struct PingerBackend {
    // Size in bytes of the payload to send.  Default is 16 bytes
    size: usize,
    // Request reception
    command_rx: mpsc::Receiver<Input>,
    // Request sending
    command_tx: mpsc::Sender<Input>,
    // IPv4
    tx: TransportSender,
}

impl PingerBackend {
    pub fn start(
        size: u16,
        command_rx: mpsc::Receiver<Input>,
        command_tx: mpsc::Sender<Input>,
    ) -> Result<(), std::io::Error> {
        // TODO We need Layer3 to have access to the IP header.
        let protocol = Layer4(Ipv4(IpNextHeaderProtocols::Icmp));
        let (tx, rx) = match transport_channel(4096, protocol) {
            Ok((tx, rx)) => (tx, rx),
            Err(e) => return Err(e),
        };

        let listener_tx = command_tx.clone();
        std::thread::spawn(move || Self::run_ip_listener(rx, listener_tx));

        let backend = Self {
            size: size as usize,
            command_rx,
            command_tx,
            tx,
        };
        tokio::spawn(backend.run());
        Ok(())
    }

    pub fn run_ip_listener(mut rx: TransportReceiver, command_tx: mpsc::Sender<Input>) {
        let mut iter = icmp_packet_iter(&mut rx);
        loop {
            match iter.next() {
                Ok((packet, addr)) => {
                    let addr = match addr {
                        IpAddr::V4(addr) => addr,
                        _ => continue,
                    };
                    let command;
                    if packet.get_icmp_type() == icmp::IcmpTypes::EchoReply {
                        let packet =
                            icmp::echo_reply::EchoReplyPacket::new(packet.packet()).unwrap();
                        command = Some((
                            PingRequestResponse::PingResponse,
                            PingIdentifier {
                                responder: addr,
                                destination: addr,
                                id: packet.get_identifier(),
                                sn: packet.get_sequence_number(),
                                stop: Instant::now(),
                            },
                        ));
                    } else {
                        let packet = if let Some(packet) =
                            icmp::time_exceeded::TimeExceededPacket::new(packet.packet())
                        {
                            packet
                        } else {
                            continue;
                        };
                        let ipv4_packet = if let Some(packet) =
                            pnet::packet::ipv4::Ipv4Packet::new(packet.payload())
                        {
                            packet
                        } else {
                            continue;
                        };
                        let icmp_packet = if let Some(packet) =
                            echo_request::EchoRequestPacket::new(ipv4_packet.payload())
                        {
                            packet
                        } else {
                            continue;
                        };

                        let id = PingIdentifier {
                            responder: addr,
                            id: icmp_packet.get_identifier(),
                            sn: icmp_packet.get_sequence_number(),
                            destination: ipv4_packet.get_destination(),
                            stop: Instant::now(),
                        };

                        if packet.get_icmp_type() == icmp::IcmpTypes::TimeExceeded {
                            command = Some((PingRequestResponse::PingTimeExceeded, id));
                        } else {
                            command =
                                Some((PingRequestResponse::IcmpError(packet.packet().into()), id));
                        }
                    }
                    if let Some((command, id)) = command {
                        loop {
                            match command_tx.try_send(command.clone().build_input(id)) {
                                Ok(_) => break,
                                Err(mpsc::error::TrySendError::Full(_)) => {
                                    std::thread::sleep(Duration::from_millis(100))
                                }
                                Err(mpsc::error::TrySendError::Closed(_)) => return,
                            }
                        }
                    }
                }
                Err(e) => {
                    // TODO Return proper errors somehow
                    eprintln!("An error occurred while reading: {}", e);
                }
            }
        }
    }

    async fn start_timeout(tx: mpsc::Sender<Input>, timeout: Duration) {
        let tx = tx.clone();
        tokio::time::sleep(timeout).await;
        let _ = tx.send(Input::Timeout).await;
    }

    async fn run(self) {
        let Self {
            size,
            mut command_rx,
            command_tx,
            mut tx,
        } = self;
        let mut index = 0u16;
        let mut timer_running = false;
        let mut ongoing = BTreeMap::new();
        let mut last_ttl = 0;

        while let Some(input) = command_rx.recv().await {
            match input {
                Input::PingRequest(request) => {
                    let mut vec: Vec<u8> = vec![0; size];
                    let mut echo_packet =
                        echo_request::MutableEchoRequestPacket::new(&mut vec[..]).unwrap();
                    let id = ((index as u32 + request.flow_id as u32) % 0x1_00_00) as u16;
                    let sn = 0xFF_FF - index;
                    echo_packet.set_identifier(id);
                    echo_packet.set_sequence_number(sn);
                    echo_packet.set_icmp_type(icmp::IcmpTypes::EchoRequest);
                    let csum = pnet::util::checksum(echo_packet.packet(), 1);
                    echo_packet.set_checksum(csum);

                    if request.ttl != last_ttl {
                        tx.set_ttl(request.ttl).unwrap();
                        last_ttl = request.ttl;
                    }
                    if tx.send_to(echo_packet, IpAddr::V4(request.addr)).is_err() {
                        // TODO Signal error?
                        let _ = request
                            .response_channel
                            .send(Err(PingError::FailedToSendPacket));
                    } else {
                        let start = Instant::now();
                        ongoing.insert(
                            (request.addr, id),
                            OngoingRequest {
                                start,
                                stop: start + request.timeout,
                                response_channel: request.response_channel,
                            },
                        );
                        if index == 0xFF_FF {
                            index = 0;
                        } else {
                            index += 1;
                        }
                        if !timer_running {
                            tokio::spawn(Self::start_timeout(command_tx.clone(), request.timeout));
                            timer_running = true;
                        }
                    }
                }
                Input::PingResponse(response) => {
                    if let Some(ongoing) = ongoing.remove(&(response.destination, response.id)) {
                        let duration = if response.stop > ongoing.start {
                            response.stop.duration_since(ongoing.start)
                        } else {
                            Duration::from_nanos(10)
                        };
                        let _ = ongoing.response_channel.send(Ok(duration));
                    }
                }
                Input::PingTimeExceeded(response) => {
                    if let Some(ongoing) = ongoing.remove(&(response.destination, response.id)) {
                        let latency = if response.stop > ongoing.start {
                            response.stop.duration_since(ongoing.start)
                        } else {
                            Duration::from_nanos(10)
                        };
                        let _ = ongoing.response_channel.send(Err(PingError::TimeExceeded {
                            addr: response.responder,
                            latency,
                        }));
                    }
                }
                Input::IcmpError { id, code, ty, data } => {
                    if let Some(ongoing) = ongoing.remove(&(id.destination, id.id)) {
                        let latency = if id.stop > ongoing.start {
                            id.stop.duration_since(ongoing.start)
                        } else {
                            Duration::from_nanos(10)
                        };
                        let _ = ongoing.response_channel.send(Err(PingError::IcmpError {
                            ty,
                            code,
                            data,
                            responder: id.responder,
                            latency,
                        }));
                    }
                }
                Input::Timeout => {
                    let now = Instant::now();
                    let lost: Vec<_> = ongoing
                        .iter()
                        .filter(|(_, v)| now > v.stop)
                        .map(|(k, _)| *k)
                        .collect();
                    for k in lost.into_iter() {
                        let v = ongoing.remove(&k).unwrap();
                        let _ = v.response_channel.send(Err(PingError::Timeout));
                    }
                    if !ongoing.is_empty() {
                        let earliest_timeout = ongoing.values().map(|v| v.stop).min().unwrap();
                        let delay = earliest_timeout.duration_since(now);
                        tokio::spawn(Self::start_timeout(command_tx.clone(), delay));
                    } else {
                        timer_running = false;
                    }
                }
                Input::Stop => break,
            }
        }
    }
}

#[derive(Clone)]
pub struct Pinger {
    command_tx: mpsc::Sender<Input>,
}

impl Pinger {
    // initialize the pinger and start the icmp and icmpv6 listeners
    pub fn new(size: u16, parallelism: usize) -> Result<Self, std::io::Error> {
        let (command_tx, command_rx) = mpsc::channel(parallelism);
        PingerBackend::start(size, command_rx, command_tx.clone())?;
        Ok(Self { command_tx })
    }

    pub async fn ping(
        &self,
        addr: Ipv4Addr,
        ttl: u8,
        timeout: Duration,
        flow_id: u16,
    ) -> Result<Duration, PingError> {
        let (tx, rx) = oneshot::channel();
        if self
            .command_tx
            .send(Input::PingRequest(PingRequest {
                addr,
                ttl,
                flow_id,
                timeout,
                response_channel: tx,
            }))
            .await
            .is_err()
        {
            return Err(PingError::BackendClosed);
        }
        rx.await.unwrap_or(Err(PingError::BackendClosed))
    }

    // TODO This has to be called to stop the backend thread
    pub async fn stop(self) {
        let _ = self.command_tx.send(Input::Stop).await;
    }
}
