mod target_iterator;
use target_iterator::InternetAddressIterator;
use tokio::sync::mpsc;

#[derive(Debug)]
struct Link {
    ttl: u8,
    target: u32,
    latency: u32,
}

#[derive(Debug)]
struct TracerouteReturn {
    index: usize,
    list: Vec<Link>,
}

fn as_ip_address(n: u32) -> String {
    format!(
        "{}.{}.{}.{}:33480",
        (n >> 24) & 0xFF,
        (n >> 16) & 0xFF,
        (n >> 8) & 0xFF,
        n & 0xFF,
    )
}

fn traceroute_n_signal(index: usize, target: u32, response: mpsc::Sender<TracerouteReturn>) {
    std::thread::spawn(move || {
        let mut list = vec![];
        let address = as_ip_address(target);
        println!("{}: Starting {}", index, address);
        let res = traceroute::execute_with_timeout(
            address,
            time::Duration::from_std(std::time::Duration::from_secs(1)).unwrap(),
        );
        match res {
            Ok(res) => {
                for hop in res {
                    println!("{}: {:?}", index, hop);
                    match hop {
                        Ok(hop) => {
                            if let std::net::IpAddr::V4(addr) = hop.host.ip() {
                                list.push(Link {
                                    ttl: hop.ttl,
                                    target: u32::from_be_bytes(addr.octets()),
                                    latency: hop.rtt.num_microseconds().unwrap() as u32,
                                })
                            } else {
                                println!("{}: Ignoring IPv6!: {:?}", index, hop);
                            }
                        }
                        Err(e) => {
                            println!("{}: HOP ERR: {:?}", index, e);
                        }
                    }
                }
            }
            Err(e) => {
                println!("{}: GLOBAL ERR: {:?}", index, e);
            }
        }
        let ret = TracerouteReturn { index, list };
        response.try_send(ret).unwrap();
    });
}

struct TracerouteExecutor<Iter: Iterator<Item = u32>> {
    parallelism: usize,
    iter: Option<Iter>,
    tx: Option<mpsc::Sender<TracerouteReturn>>,
    rx: mpsc::Receiver<TracerouteReturn>,
    index: usize,
}

impl<Iter> TracerouteExecutor<Iter>
where
    Iter: Iterator<Item = u32>,
{
    pub fn new(iter: Iter, parallelism: usize) -> Self {
        let (tx, rx) = mpsc::channel(parallelism);
        Self {
            parallelism,
            iter: Some(iter),
            tx: Some(tx),
            rx,
            index: 0,
        }
    }

    pub fn start(&mut self) {
        for i in 0..self.parallelism {
            match self.iter.as_mut().unwrap().next() {
                Some(target) => self.start_traceroute(target),
                None => {
                    self.parallelism = i;
                    break;
                }
            }
        }
    }

    fn start_traceroute(&mut self, target: u32) {
        traceroute_n_signal(self.index, target, self.tx.as_ref().unwrap().clone());
        self.index += 1;
    }

    pub async fn wait_for_next(&mut self) -> Option<TracerouteReturn> {
        match self.rx.recv().await {
            Some(ret) => {
                if let Some(iter) = &mut self.iter {
                    if let Some(target) = iter.next() {
                        self.start_traceroute(target);
                    } else {
                        self.iter = None;
                        self.parallelism -= 1;
                    }
                } else {
                    self.parallelism -= 1;
                }
                if self.parallelism == 0 {
                    self.tx.take();
                }
                Some(ret)
            }
            None => None,
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // let result_folder = std::env::args().nth(1).unwrap();

    // let mut traceroute_query = Traceroute::new(
    //     as_ip_address(123456789).parse().unwrap(),
    //     Default::default(),
    // );
    // for (i, hop) in traceroute_query.enumerate() {
    //     println!("Hop {}: ttl = {}, results = {}", i, hop.ttl, hop.query_result.iter().map(|| ));
    // }

    let iterator = InternetAddressIterator::new();
    let mut tracer = TracerouteExecutor::new(iterator.take(4), 2);
    tracer.start();
    while let Some(ret) = tracer.wait_for_next().await {
        println!("Ret: {:?}", ret);
    }
}
