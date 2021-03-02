use tokio_ip_ping_request::ping;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let target = args.next().unwrap();
    let timeout: u64 = args.next().unwrap().parse().unwrap();
    let timeout = std::time::Duration::from_secs(timeout);
    let ttl: u8 = args.next().unwrap().parse().unwrap();

    let icmp_pinger = ping::icmp::Pinger::new(64, 50).unwrap();
    println!(
        "ICMP: {:?}",
        icmp_pinger
            .ping(target.parse().unwrap(), ttl, timeout, 0)
            .await
    );
    icmp_pinger.stop().await;

    let tcp_pinger = ping::tcp::Pinger::new(64, 50).unwrap();
    println!(
        "TCP: {:?}",
        tcp_pinger
            .ping(target.parse().unwrap(), ttl, timeout, 0)
            .await
    );
    tcp_pinger.stop().await;
}
