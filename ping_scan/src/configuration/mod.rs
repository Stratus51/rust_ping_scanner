use crate::cursor::{self, Cursor, CursorExt};
use crate::internet::ip_is_valid;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::time::Duration;

pub mod v0;
pub mod v1;

// TODO Add start date
// TODO Add source IP
// TODO Do something JSON based instead
// TODO Add first byte as version on u8 (for cross version compatible parser/converter)
// TODO Add conf length on first u32 to be able to skip config, for simpler parsers

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

pub enum Error {
    V0(v0::Error),
    V1(serde_json::Error),
    UnknownConfigurationVersion(u8),
}

pub trait EncodableConfiguration {
    type Error;
    fn encode(&self) -> Box<[u8]>;
    fn decode(data: &[u8]) -> Result<(Self, usize), Self::Error>
    where
        Self: Sized;
    fn as_standard_configuration(&self) -> Configuration;
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CursorType {
    Sampling = 0,
    Sequential = 1,
    Periodic = 2,
    ReverseEndianSequential = 3,
    // TODO Add AES based cursor
}

impl CursorType {
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CursorConfiguration {
    pub ty: CursorType,
    pub offset: u32,
    pub nb: u32,
    pub native_cursor: bool,
}

impl CursorConfiguration {
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        let ty: String = field_from_args(args, "--cursor_type")?;
        let offset: u32 = field_from_args(args, "--cursor_offset")?;
        let nb: u32 = field_from_args(args, "--cursor_nb")?;

        Ok(Self {
            ty: CursorType::from_str(&ty).map_err(|_| format!("Unknown cursor_type {}", ty))?,
            offset,
            nb,
            native_cursor: true,
        })
    }

    pub fn generate(&self) -> Result<Box<dyn Cursor<Item = u32>>, ()> {
        let cursor: Box<dyn Cursor<Item = u32>> = match self.ty {
            CursorType::Sampling => Box::new(cursor::u32_sampling::U32SamplingCursor::new(
                if self.native_cursor { 0 } else { 1 },
                0,
                cursor::u32_sampling::MAX_DEPTH,
            )),
            CursorType::Sequential => Box::new(cursor::u32::U32Cursor::new()),
            CursorType::Periodic => Box::new(cursor::periodic::PeriodicCursor::new(
                256,
                0xFF_FF_FF,
                if self.native_cursor { 0 } else { 1 },
                0,
            )),
            CursorType::ReverseEndianSequential => {
                Box::new(cursor::u32::U32Cursor::new().map(|n| u32::from_be_bytes(n.to_le_bytes())))
            }
        };
        Ok(Box::new(
            cursor
                .filter(ip_is_valid)
                .skip(self.offset as usize)?
                .take(self.nb as usize),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkStateMonitorConfiguration {
    pub target: Ipv4Addr,
    pub ttl: u8,
    pub timeout: Duration,
    pub period: Duration,
    pub max_consecutive_fails: u8,
}
impl LinkStateMonitorConfiguration {
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        let target: Ipv4Addr = field_from_args::<String>(args, "--monitor_target")?
            .parse()
            .map_err(|e| format!("Bad monitor target: {:?}", e))?;
        let ttl: u8 = field_from_args(args, "--monitor_ttl")?;
        let timeout: u64 = field_from_args(args, "--monitor_timeout")?;
        let period: u64 = field_from_args(args, "--monitor_period")?;
        let max_consecutive_fails: u8 = field_from_args(args, "--monitor_max_fails")?;

        Ok(Self {
            target,
            ttl,
            timeout: Duration::from_millis(timeout),
            period: Duration::from_millis(period),
            max_consecutive_fails,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Configuration {
    pub cursor: CursorConfiguration,
    pub ping: PingConfiguration,
    pub link_state_monitor: Option<LinkStateMonitorConfiguration>,
    // TODO Output type
    pub out_file: String,
}

impl Configuration {
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        let cursor = CursorConfiguration::from_args(args)?;
        let ping = PingConfiguration::from_args(args)?;
        let link_state_monitor = Some(LinkStateMonitorConfiguration::from_args(args)?);
        let out_file = field_from_args(args, "--out_file")?;
        Ok(Self {
            cursor,
            ping,
            link_state_monitor,
            out_file,
        })
    }
}

impl EncodableConfiguration for Configuration {
    type Error = Error;
    fn encode(&self) -> Box<[u8]> {
        let data = v1::Configuration {
            cursor: self.cursor,
            ping: self.ping,
            link_state_monitor: self.link_state_monitor.clone(),
            out_file: self.out_file.clone(),
        }
        .encode();
        [Box::new([1]), data].concat().into()
    }

    fn decode(data: &[u8]) -> Result<(Self, usize), Self::Error> {
        Ok(match data[0] {
            0 => {
                let (conf, size) = v0::Configuration::decode(&data).map_err(Error::V0)?;
                (conf.as_standard_configuration(), size)
            }
            1 => {
                let (conf, size) = v1::Configuration::decode(&data[1..]).map_err(Error::V1)?;
                (conf.as_standard_configuration(), size)
            }
            version => return Err(Error::UnknownConfigurationVersion(version)),
        })
    }

    fn as_standard_configuration(&self) -> Configuration {
        self.clone()
    }
}
