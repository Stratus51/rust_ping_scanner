// TODO Use pnet lib method instead
use super::EncodableConfiguration;
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::time::Duration;

// TODO Add source IP

fn read_le_u32(input: &mut &[u8]) -> u32 {
    let (int_bytes, rest) = input.split_at(std::mem::size_of::<u32>());
    *input = rest;
    u32::from_le_bytes(int_bytes.try_into().unwrap())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Configuration {
    pub cursor: super::CursorConfiguration,
    pub ping: super::PingConfiguration,
    pub link_state_monitor: Option<super::LinkStateMonitorConfiguration>,
    pub cpu_load_monitor: Option<super::CpuLoadMonitorConfiguration>,
    pub data_type: super::DataType,
    pub start_date: u64,
    pub out_file: String,
}

impl EncodableConfiguration for Configuration {
    type Error = serde_json::Error;
    fn encode(&self) -> Box<[u8]> {
        let conf_data: Box<[u8]> = serde_json::to_string(self).unwrap().as_bytes().into();
        let size: Box<[u8]> = (conf_data.len() as u32).to_le_bytes().into();
        [size, conf_data].concat().into()
    }

    // TODO Add proper errors, for the data feed to be able to react intelligently to data
    // starvation
    fn decode(mut data: &[u8]) -> Result<(Self, usize), serde_json::Error> {
        let tot_size = read_le_u32(&mut data) as usize;
        let conf = serde_json::from_slice(&data[..tot_size])?;
        Ok((conf, tot_size + 4))
    }

    fn as_standard_configuration(&self) -> super::Configuration {
        super::Configuration {
            cursor: self.cursor,
            ping: self.ping,
            link_state_monitor: self.link_state_monitor.clone(),
            cpu_load_monitor: self.cpu_load_monitor,
            start_date: self.start_date,
            data_type: self.data_type,
            out_file: self.out_file.clone(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::super::{
        CpuLoadMonitorConfiguration, CursorConfiguration, CursorType, DataType,
        LinkStateMonitorConfiguration, PingConfiguration,
    };
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn decode_consistency() {
        fn check(conf: Configuration) {
            let encoded = conf.encode();
            let (decoded, decoded_size) = Configuration::decode(&encoded).unwrap();
            assert_eq!(conf, decoded);
            assert_eq!(encoded.len(), decoded_size);
        }

        check(Configuration {
            cursor: CursorConfiguration {
                ty: CursorType::Sampling,
                offset: 42,
                nb: 99,
                native_cursor: false,
            },
            link_state_monitor: Some(LinkStateMonitorConfiguration {
                target: Ipv4Addr::new(8, 8, 8, 8),
                ttl: 30,
                timeout: Duration::from_secs(2),
                period: Duration::from_secs(2),
                max_consecutive_fails: 2,
            }),
            cpu_load_monitor: Some(CpuLoadMonitorConfiguration {
                min: 0.8,
                max: 0.9,
                refresh_rate: Duration::from_secs(60),
            }),
            ping: PingConfiguration {
                timeout: Duration::from_secs(33),
                size: 444,
                parallelism: 90,
                ttl: 34,
            },
            data_type: DataType::PingLatency,
            start_date: std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            out_file: "/root/plop".to_string(),
        });

        check(Configuration {
            cursor: CursorConfiguration {
                ty: CursorType::Sequential,
                offset: 0,
                nb: 0xFF_FF_FF_FF,
                native_cursor: true,
            },
            link_state_monitor: Some(LinkStateMonitorConfiguration {
                target: Ipv4Addr::new(192, 168, 1, 1),
                ttl: 2,
                timeout: Duration::from_secs(1),
                period: Duration::from_secs(20),
                max_consecutive_fails: 5,
            }),
            cpu_load_monitor: Some(CpuLoadMonitorConfiguration {
                min: 0.1,
                max: 0.2,
                refresh_rate: Duration::from_secs(120),
            }),
            ping: PingConfiguration {
                timeout: Duration::from_secs(33),
                size: 345,
                parallelism: 4,
                ttl: 22,
            },
            data_type: DataType::PingLatency,
            start_date: 0,
            out_file: "".to_string(),
        });
    }
}
