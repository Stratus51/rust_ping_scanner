use crate::internet::ip_is_valid;
use crate::ping::Pinger;
use crate::u32_sampling_iterator;
use crate::u32_sampling_iterator::U32SamplingIterator;
use std::convert::TryInto;
use std::time::Duration;

fn field_from_args<T: std::str::FromStr>(args: &[String], field_name: &str) -> Result<T, String>
where
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    let pos = args
        .iter()
        .position(|r| r == field_name)
        .ok_or(format!("Missing {} options", field_name))?
        + 1;
    if args.len() <= pos {
        return Err(format!("Empty {} value", field_name));
    }
    args[pos]
        .parse::<T>()
        .map_err(|e| format!("Failed to parse nb_targets value: {:?}", e))
}

fn read_le_u16(input: &mut &[u8]) -> u16 {
    let (int_bytes, rest) = input.split_at(std::mem::size_of::<u16>());
    *input = rest;
    u16::from_le_bytes(int_bytes.try_into().unwrap())
}

fn read_le_u32(input: &mut &[u8]) -> u32 {
    let (int_bytes, rest) = input.split_at(std::mem::size_of::<u32>());
    *input = rest;
    u32::from_le_bytes(int_bytes.try_into().unwrap())
}

fn read_le_u64(input: &mut &[u8]) -> u64 {
    let (int_bytes, rest) = input.split_at(std::mem::size_of::<u64>());
    *input = rest;
    u64::from_le_bytes(int_bytes.try_into().unwrap())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PingConfiguration {
    pub timeout: Duration,
    pub size: u16,
    pub parallelism: u32,
    pub ttl: u8,
}
impl PingConfiguration {
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        let timeout: u64 = field_from_args(args, "--ping_timeout")?;
        let size: u16 = field_from_args(args, "--ping_size")?;
        let parallelism: u32 = field_from_args(args, "--ping_parallelism")?;
        let ttl: u8 = field_from_args(args, "--ping_ttl")?;

        Ok(Self {
            timeout: Duration::from_millis(timeout),
            size,
            parallelism,
            ttl,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut data: Vec<Box<[u8]>> = vec![];
        data.push((self.timeout.as_nanos() as u64).to_le_bytes().into());
        data.push(self.size.to_le_bytes().into());
        data.push(self.parallelism.to_le_bytes().into());
        data.push([self.ttl].into());
        let data_size = data.iter().map(|d| d.len()).sum::<usize>() as u32;
        data.insert(0, data_size.to_le_bytes().into());
        data.concat()
    }

    pub fn decode(mut data: &[u8]) -> Result<(Self, usize), String> {
        let tot_size = read_le_u32(&mut data);
        let timeout = read_le_u64(&mut data);
        let size = read_le_u16(&mut data);
        let parallelism = read_le_u32(&mut data);
        let ttl = data[0];
        let expected_size = 8 + 2 + 4 + 1;
        if tot_size as usize != expected_size {
            return Err(format!(
                "Bad tot_size value: {} != {}",
                tot_size, expected_size
            ));
        }
        Ok((
            Self {
                timeout: Duration::from_nanos(timeout),
                size,
                parallelism,
                ttl,
            },
            (4 + tot_size) as usize,
        ))
    }

    pub fn generate(&self) -> Result<Pinger, String> {
        Pinger::new(self.timeout, self.size, self.parallelism as usize)
            .map_err(|e| format!("Failed to generate pinger: {:?}", e))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IteratorType {
    Sampling = 0,
    Sequential = 1,
    Periodic = 2,
    ReverseEndianSequential = 3,
    // TODO Add AES based iterator
}

pub struct PeriodicIterator {
    index: u32,
    period: u32,
    nb_period: u32,
    offset: u32,
}

impl Iterator for PeriodicIterator {
    type Item = u32;
    fn next(&mut self) -> Option<Self::Item> {
        self.index += 1;
        if self.index >= self.nb_period {
            self.index = 0;
            self.offset += 1;

            if self.offset >= self.period {
                return None;
            }
        }
        Some(self.offset + self.index * self.period)
    }
}

impl IteratorType {
    pub fn from_str(s: &str) -> Result<Self, ()> {
        Ok(match s {
            "sampling" => Self::Sampling,
            "sequential" => Self::Sequential,
            "periodic" => Self::Periodic,
            "reverse_endian" => Self::ReverseEndianSequential,
            _ => return Err(()),
        })
    }
    pub fn from_u8(n: u8) -> Result<Self, ()> {
        Ok(match n {
            0 => Self::Sampling,
            1 => Self::Sequential,
            2 => Self::Periodic,
            3 => Self::ReverseEndianSequential,
            _ => return Err(()),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IteratorConfiguration {
    pub ty: IteratorType,
    pub offset: u32,
    pub nb: u32,
}
impl IteratorConfiguration {
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        let ty: String = field_from_args(args, "--iterator_type")?;
        let offset: u32 = field_from_args(args, "--iterator_offset")?;
        let nb: u32 = field_from_args(args, "--iterator_nb")?;

        Ok(Self {
            ty: IteratorType::from_str(&ty).map_err(|_| format!("Unknown iterator_type {}", ty))?,
            offset,
            nb,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut data: Vec<Box<[u8]>> = vec![];
        data.push([self.ty as u8].into());
        data.push(self.offset.to_le_bytes().into());
        data.push(self.nb.to_le_bytes().into());
        let data_size = data.iter().map(|d| d.len()).sum::<usize>() as u32;
        data.insert(0, data_size.to_le_bytes().into());
        data.concat()
    }

    pub fn decode(mut data: &[u8]) -> Result<(Self, usize), String> {
        let tot_size = read_le_u32(&mut data);
        let ty = data[0];
        data = &data[1..];
        let offset = read_le_u32(&mut data);
        let nb = read_le_u32(&mut data);
        let expected_size = 1 + 4 + 4;
        if tot_size as usize != expected_size {
            return Err(format!(
                "Bad tot_size value: {} != {}",
                tot_size, expected_size
            ));
        }
        Ok((
            Self {
                ty: IteratorType::from_u8(ty)
                    .map_err(|_| format!("Unknown iterator_type: {}", ty))?,
                offset,
                nb,
            },
            (4 + tot_size) as usize,
        ))
    }

    pub fn generate(&self) -> Box<dyn Iterator<Item = u32>> {
        match self.ty {
            IteratorType::Sampling => Box::new(
                U32SamplingIterator::new(0, 0, u32_sampling_iterator::MAX_DEPTH)
                    .filter(ip_is_valid)
                    .skip(self.offset as usize)
                    .take(self.nb as usize),
            ),
            IteratorType::Sequential => Box::new(
                (0u32..0xFF_FF_FF_FF)
                    .filter(ip_is_valid)
                    .skip(self.offset as usize)
                    .take(self.nb as usize),
            ),
            IteratorType::Periodic => Box::new(
                PeriodicIterator {
                    period: 256,
                    nb_period: 0xFF_FF_FF,
                    index: 0,
                    offset: 0,
                }
                .filter(ip_is_valid)
                .skip(self.offset as usize)
                .take(self.nb as usize),
            ),
            IteratorType::ReverseEndianSequential => Box::new(
                (0u32..0xFF_FF_FF_FF)
                    .map(|n| u32::from_be_bytes(n.to_le_bytes()))
                    .filter(ip_is_valid)
                    .skip(self.offset as usize)
                    .take(self.nb as usize),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Configuration {
    pub iterator: IteratorConfiguration,
    pub ping: PingConfiguration,
    // TODO Output type
    pub out_file: String,
}

pub struct ConfigurationObject {
    pub iterator: Box<dyn Iterator<Item = u32>>,
    pub ping: Pinger,
}

impl Configuration {
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        let iterator = IteratorConfiguration::from_args(args)?;
        let ping = PingConfiguration::from_args(args)?;
        let out_file = field_from_args(args, "--out_file")?;
        Ok(Self {
            iterator,
            ping,
            out_file,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut data: Vec<Box<[u8]>> = vec![];
        data.push(self.iterator.encode().into());
        data.push(self.ping.encode().into());
        data.push((self.out_file.len() as u32).to_le_bytes().into());
        data.push(self.out_file.as_bytes().into());
        let data_size = data.iter().map(|d| d.len()).sum::<usize>() as u32;
        data.insert(0, data_size.to_le_bytes().into());
        data.concat()
    }

    // TODO Add proper errors, for the data feed to be able to react intelligently to data
    // starvation
    pub fn decode(mut data: &[u8]) -> Result<(Self, usize), String> {
        let tot_size = read_le_u32(&mut data);
        let (iterator, iterator_size) =
            IteratorConfiguration::decode(data).map_err(|e| format!("iterator: {}", e))?;
        data = &data[iterator_size..];
        let (ping, ping_size) =
            PingConfiguration::decode(data).map_err(|e| format!("ping: {}", e))?;
        data = &data[ping_size..];
        let out_file_size = read_le_u32(&mut data) as usize;
        let out_file = std::str::from_utf8(&data[..out_file_size])
            .unwrap()
            .to_string();
        let expected_size = iterator_size + ping_size + 4 + out_file_size;
        if tot_size as usize != expected_size {
            return Err(format!("Bad tot size: {} != {}", tot_size, expected_size));
        }
        Ok((
            Self {
                iterator,
                ping,
                out_file,
            },
            (4 + tot_size) as usize,
        ))
    }

    pub fn generate(&self) -> Result<ConfigurationObject, String> {
        Ok(ConfigurationObject {
            iterator: self.iterator.generate(),
            ping: self.ping.generate()?,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn decode_consistency() {
        fn check(conf: Configuration) {
            let encoded = conf.encode();
            let (decoded, decoded_size) = Configuration::decode(&encoded).unwrap();
            assert_eq!(conf, decoded);
            assert_eq!(encoded.len(), decoded_size);
        }

        check(Configuration {
            iterator: IteratorConfiguration {
                ty: IteratorType::Sampling,
                offset: 42,
                nb: 99,
            },
            ping: PingConfiguration {
                timeout: Duration::from_secs(33),
                size: 444,
                parallelism: 90,
                ttl: 34,
            },
            out_file: "/root/plop".to_string(),
        });

        check(Configuration {
            iterator: IteratorConfiguration {
                ty: IteratorType::Sequential,
                offset: 0,
                nb: 0xFF_FF_FF_FF,
            },
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
