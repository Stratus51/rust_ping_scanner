use tokio_ip_ping_request::measure_route;
use tokio_ip_ping_request::ping;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let target = args.next().unwrap();
    let timeout: u64 = args.next().unwrap().parse().unwrap();
    let timeout = std::time::Duration::from_secs(timeout);
    let ttl: u8 = args.next().unwrap().parse().unwrap();
    let pinger = ping::icmp::Pinger::new(64, 20).unwrap();

    // TODO Loop over flow ids to check that the route size stays stable
    let mut tr = measure_route::icmp_measure_route(
        pinger.clone(),
        target.parse().unwrap(),
        ttl,
        timeout,
        0,
        10,
    );
    while let Some(ret) = tr.recv().await {
        println!("{:?}", ret);
    }

    pinger.stop().await;
}
