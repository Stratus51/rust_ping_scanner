mod ping;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let target = args.next().unwrap();
    let timeout: u64 = args.next().unwrap().parse().unwrap();
    let ttl: u8 = args.next().unwrap().parse().unwrap();
    let pinger = ping::Pinger::new(std::time::Duration::from_secs(timeout), 64, 50).unwrap();

    println!("{:?}", pinger.ping(target.parse().unwrap(), ttl).await);
    pinger.stop().await;
}
