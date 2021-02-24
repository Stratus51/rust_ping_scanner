mod list_route_nodes;
mod ping;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let target = args.next().unwrap();
    let timeout: u64 = args.next().unwrap().parse().unwrap();
    let ttl: u8 = args.next().unwrap().parse().unwrap();
    let pinger = ping::Pinger::new(std::time::Duration::from_secs(timeout), 64, 20).unwrap();

    let mut tr =
        list_route_nodes::list_route_nodes(pinger.clone(), target.parse().unwrap(), ttl, 5);
    while let Some(ret) = tr.recv().await {
        println!("{:?}", ret);
    }

    pinger.stop().await;
}
