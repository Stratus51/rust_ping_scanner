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
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::sync::oneshot;

#[derive(Debug)]
pub struct PingRequest {
    ttl: u8,
    addr: IpAddr,
    response_channel: oneshot::Sender<PingResult>,
}

#[derive(Debug, Clone)]
pub enum PingResult {
    Ok(Duration),
    Timeout,
    TimeExceeded {
        addr: IpAddr,
        latency: Duration,
    },
    FailedToSendPacket,
    BackendClosed,
    IcmpError {
        responder: IpAddr,
        code: icmp::IcmpCode,
        ty: icmp::IcmpType,
        data: Box<[u8]>,
        latency: Duration,
    },
}

#[derive(Debug)]
pub struct OngoingRequest {
    start: Instant,
    response_channel: oneshot::Sender<PingResult>,
}

#[derive(Debug, Clone, Copy)]
pub struct PingIdentifier {
    responder: IpAddr,
    id: u16,
    sn: u16,
    stop: Instant,
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
    max_rtt: Duration,
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
        max_rtt: Duration,
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
            max_rtt,
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
                    let command;
                    if packet.get_icmp_type() == icmp::IcmpTypes::EchoReply {
                        let packet =
                            icmp::echo_reply::EchoReplyPacket::new(packet.packet()).unwrap();
                        command = Some((
                            PingRequestResponse::PingResponse,
                            PingIdentifier {
                                responder: addr,
                                id: packet.get_identifier(),
                                sn: packet.get_sequence_number(),
                                stop: Instant::now(),
                            },
                        ));
                    } else {
                        let packet =
                            icmp::time_exceeded::TimeExceededPacket::new(packet.packet()).unwrap();
                        let ipv4_packet =
                            pnet::packet::ipv4::Ipv4Packet::new(packet.payload()).unwrap();
                        let icmp_packet =
                            echo_request::EchoRequestPacket::new(ipv4_packet.payload()).unwrap();

                        let id = PingIdentifier {
                            responder: addr,
                            id: icmp_packet.get_identifier(),
                            sn: icmp_packet.get_sequence_number(),
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
            max_rtt,
            size,
            mut command_rx,
            command_tx,
            mut tx,
        } = self;
        let mut index = 0x42_42_42_42u32;
        let mut timer_running = false;
        let mut ongoing = BTreeMap::new();

        while let Some(input) = command_rx.recv().await {
            match input {
                Input::PingRequest(request) => {
                    let mut vec: Vec<u8> = vec![0; size];
                    let mut echo_packet =
                        echo_request::MutableEchoRequestPacket::new(&mut vec[..]).unwrap();
                    echo_packet.set_identifier((index >> 16) as u16);
                    echo_packet.set_sequence_number((index & 0xFF_FF) as u16);
                    echo_packet.set_icmp_type(icmp::IcmpTypes::EchoRequest);
                    let csum = pnet::util::checksum(echo_packet.packet(), 1);
                    echo_packet.set_checksum(csum);

                    tx.set_ttl(request.ttl).unwrap();
                    if tx.send_to(echo_packet, request.addr).is_err() {
                        // TODO Signal error?
                        request
                            .response_channel
                            .send(PingResult::FailedToSendPacket)
                            .unwrap();
                    } else {
                        ongoing.insert(
                            index,
                            OngoingRequest {
                                start: Instant::now(),
                                response_channel: request.response_channel,
                            },
                        );
                        index += 1;
                        if !timer_running {
                            tokio::spawn(Self::start_timeout(command_tx.clone(), max_rtt));
                            timer_running = true;
                        }
                    }
                }
                Input::PingResponse(response) => {
                    let index = (response.id as u32) << 16 | (response.sn as u32);
                    if let Some(ongoing) = ongoing.remove(&index) {
                        ongoing
                            .response_channel
                            .send(PingResult::Ok(response.stop.duration_since(ongoing.start)))
                            .unwrap()
                    }
                }
                Input::PingTimeExceeded(response) => {
                    let index = (response.id as u32) << 16 | (response.sn as u32);
                    if let Some(ongoing) = ongoing.remove(&index) {
                        ongoing
                            .response_channel
                            .send(PingResult::TimeExceeded {
                                addr: response.responder,
                                latency: response.stop.duration_since(ongoing.start),
                            })
                            .unwrap()
                    }
                }
                Input::IcmpError { id, code, ty, data } => {
                    let index = (id.id as u32) << 16 | (id.sn as u32);
                    if let Some(ongoing) = ongoing.remove(&index) {
                        ongoing
                            .response_channel
                            .send(PingResult::IcmpError {
                                ty,
                                code,
                                data,
                                responder: id.responder,
                                latency: id.stop.duration_since(ongoing.start),
                            })
                            .unwrap()
                    }
                }
                Input::Timeout => {
                    let now = Instant::now();
                    let lost: Vec<_> = ongoing
                        .iter()
                        .filter(|(_, v)| now.duration_since(v.start) > max_rtt)
                        .map(|(k, _)| *k)
                        .collect();
                    for k in lost.into_iter() {
                        let v = ongoing.remove(&k).unwrap();
                        v.response_channel.send(PingResult::Timeout).unwrap();
                    }
                    if !ongoing.is_empty() {
                        let earliest_start = ongoing.values().map(|v| v.start).min().unwrap();
                        let delay = max_rtt - (now.duration_since(earliest_start));
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
    pub fn new(max_rtt: Duration, size: u16, parallelism: usize) -> Result<Self, std::io::Error> {
        let (command_tx, command_rx) = mpsc::channel(parallelism);
        PingerBackend::start(max_rtt, size, command_rx, command_tx.clone())?;
        Ok(Self { command_tx })
    }

    pub async fn ping(&self, addr: IpAddr, ttl: u8) -> PingResult {
        let (tx, rx) = oneshot::channel();
        if self
            .command_tx
            .send(Input::PingRequest(PingRequest {
                addr,
                ttl,
                response_channel: tx,
            }))
            .await
            .is_err()
        {
            return PingResult::BackendClosed;
        }
        rx.await.unwrap_or(PingResult::BackendClosed)
    }

    // TODO This has to be called to stop the backend thread
    pub async fn stop(self) {
        let _ = self.command_tx.send(Input::Stop).await;
    }
}