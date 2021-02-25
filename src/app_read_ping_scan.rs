mod configuration;
mod internet;
mod ping;
mod u32_sampling_iterator;

use configuration::Configuration;
use internet::u32_to_ip;
use std::convert::TryInto;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;

fn read_le_u32(input: &mut &[u8]) -> u32 {
    let (int_bytes, rest) = input.split_at(std::mem::size_of::<u32>());
    *input = rest;
    u32::from_le_bytes(int_bytes.try_into().unwrap())
}

pub fn decode_result(mut data: &[u8]) -> (u32, Duration) {
    let index = read_le_u32(&mut data);
    let latency = read_le_u32(&mut data);
    (index, Duration::from_nanos(latency as u64 * 10))
}

pub struct IteratorCherryPick<Iter: Iterator<Item = u32>> {
    iter: Iter,
    index: u32,
    buffer: Vec<(u32, u32)>,
}

impl<Iter> IteratorCherryPick<Iter>
where
    Iter: Iterator<Item = u32>,
{
    fn new(iter: Iter) -> Self {
        Self {
            iter,
            index: 0,
            buffer: vec![],
        }
    }

    fn pump_data(&mut self, n: u32) -> bool {
        for _ in 0..n {
            let value = match self.iter.next() {
                Some(value) => value,
                None => return false,
            };
            self.buffer.push((self.index, value));
            self.index += 1;
        }
        true
    }

    fn get(&mut self, index: u32) -> u32 {
        let min_index = if self.buffer.is_empty() {
            self.index
        } else {
            self.buffer[0].0
        };
        if index < min_index {
            panic!("Data index call for data already called.");
        }

        let max_index = if self.buffer.is_empty() {
            self.index
        } else {
            self.buffer.last().unwrap().0
        };
        if index >= max_index {
            let missing = index + 1 - max_index;
            self.pump_data(missing);
            self.buffer.remove(self.buffer.len() - 1).1
        } else {
            let value = match self.buffer.iter().find(|(i, _)| *i == index) {
                Some(value) => value.1,
                None => panic!(
                    "Could not find value in buffer even though it was supposed to be in it ({} < {} < {}).\
                    Did the index get called twice?", self.buffer.first().unwrap().0, index, self.buffer.first().unwrap().1
                ),
            };
            value
        }
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
    let iterator = conf.iterator.generate();
    let mut iterator_db = IteratorCherryPick::new(iterator);
    let mut buffer = [0u8; 8];
    while in_file.read_exact(&mut buffer).await.is_ok() {
        let (index, latency) = decode_result(&buffer);
        let addr = u32_to_ip(iterator_db.get(index));
        println!("{:>width$} => {:?}", addr, latency, width = 15);
    }
}
