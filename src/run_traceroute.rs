mod ping;
mod traceroute;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let target = args.next().unwrap();
    let timeout: u64 = args.next().unwrap().parse().unwrap();
    let ttl: u8 = args.next().unwrap().parse().unwrap();
    let pinger = ping::Pinger::new(std::time::Duration::from_secs(timeout), 64, 20).unwrap();

    let mut tr = traceroute::traceroute(pinger.clone(), target.parse().unwrap(), ttl);
    while let Some(ret) = tr.recv().await {
        println!("{:?}", ret);
    }

    pinger.stop().await;
}
