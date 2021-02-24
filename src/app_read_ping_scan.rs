mod configuration;
mod internet;
mod ping;
mod u32_sampling_iterator;

use configuration::Configuration;
use internet::u32_to_ip;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;

pub fn decode_result(data: [u8; 4]) -> Option<Duration> {
    let val = u32::from_le_bytes(data);
    if val == 0 {
        None
    } else {
        Some(Duration::from_nanos(val as u64 * 10))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let path = std::env::args().nth(1).unwrap();
    let mut in_file = tokio::fs::File::open(path).await.unwrap();

    let mut buffer = vec![0u8; 1024];
    let mut conf = None;

    let mut buffer_len = 0;
    while conf.is_none() {
        let mut remaining_buf: &mut [u8] = &mut buffer[buffer_len..];
        let bytes_read = in_file.read_buf(&mut remaining_buf).await.unwrap();
        buffer_len += bytes_read;
        if let Ok(c) = Configuration::decode(&buffer) {
            conf = Some(c);
        }

        if conf.is_none() && bytes_read == 0 {
            panic!("Reached end of data file without being able to parse configuration.");
        }
    }

    let (conf, conf_size) = conf.unwrap();
    println!("Configuration: {:#?}", conf);

    in_file
        .seek(std::io::SeekFrom::Start(conf_size as u64))
        .await
        .unwrap();
    let mut iterator = conf.iterator.generate();
    let mut buffer = [0u8; 4];
    while in_file.read_exact(&mut buffer).await.is_ok() {
        println!(
            "{} => {:?}",
            u32_to_ip(iterator.next().unwrap()),
            decode_result(buffer)
        );
    }
}
