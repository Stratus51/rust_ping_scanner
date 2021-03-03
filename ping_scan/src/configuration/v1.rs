// TODO Use pnet lib method instead
use super::EncodableConfiguration;
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::time::Duration;

// TODO Add start date
// TODO Add source IP
// TODO Do something JSON based instead
// TODO Add first byte as version on u8 (for cross version compatible parser/converter)
// TODO Add conf length on first u32 to be able to skip config, for simpler parsers
// TODO Add a field to indicate the type of output data

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
    // TODO Output type
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
            out_file: self.out_file.clone(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::super::{
        CursorConfiguration, CursorType, LinkStateMonitorConfiguration, PingConfiguration,
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
            ping: PingConfiguration {
                timeout: Duration::from_secs(33),
                size: 444,
                parallelism: 90,
                ttl: 34,
            },
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
            ping: PingConfiguration {
                timeout: Duration::from_secs(33),
                size: 345,
                parallelism: 4,
                ttl: 22,
            },
            out_file: "".to_string(),
        });
    }
}
