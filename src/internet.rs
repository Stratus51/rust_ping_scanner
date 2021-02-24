use std::net::{IpAddr, Ipv4Addr};

pub fn u32_to_ip(n: u32) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(
        (n >> 24) as u8,
        ((n >> 16) & 0xFF) as u8,
        ((n >> 8) & 0xFF) as u8,
        (n & 0xFF) as u8,
    ))
}

pub fn ip_is_valid(ip: &u32) -> bool {
    // 0.0.0.0/8
    if ip & 0xFF_00_00_00 == 0x00_00_00_00 {
        return false;
    }
    // 10.0.0.0/8
    if ip & 0xFF_00_00_00 == 0x0A_00_00_00 {
        return false;
    }
    // 100.64.0.0/10
    if ip & 0xFF_C0_00_00 == 0x64_40_00_00 {
        return false;
    }
    // 127.0.0.0/8
    if ip & 0xFF_00_00_00 == 0x7F_00_00_00 {
        return false;
    }
    // 169.254.0.0/16
    if ip & 0xFF_FF_00_00 == 0xA9_FE_00_00 {
        return false;
    }
    // 172.16.0.0/12
    if ip & 0xFF_F0_00_00 == 0xAC_10_00_00 {
        return false;
    }
    // 192.0.0.0/24
    if ip & 0xFF_FF_FF_00 == 0xC0_00_00_00 {
        return false;
    }
    // 192.0.2.0/24
    if ip & 0xFF_FF_FF_00 == 0xC0_00_02_00 {
        return false;
    }
    // 192.31.196.0/24
    if ip & 0xFF_FF_FF_00 == 0xC0_1F_C4_00 {
        return false;
    }
    // 192.52.193.0/24
    if ip & 0xFF_FF_FF_00 == 0xC0_34_C1_00 {
        return false;
    }
    // 192.88.99.0/24
    if ip & 0xFF_FF_FF_00 == 0xC0_58_63_00 {
        return false;
    }
    // 192.168.0.0/16
    if ip & 0xFF_FF_00_00 == 0xC0_A8_00_00 {
        return false;
    }
    // 192.175.48.0/24
    if ip & 0xFF_FF_FF_00 == 0xC0_AF_30_00 {
        return false;
    }
    // 198.18.0.0/15
    if ip & 0xFF_FE_00_00 == 0xC6_12_00_00 {
        return false;
    }
    // 198.51.100.0/24
    if ip & 0xFF_FF_FF_00 == 0xC6_33_64_00 {
        return false;
    }
    // 203.0.113.0/24
    if ip & 0xFF_FF_FF_00 == 0xCB_00_71_00 {
        return false;
    }
    // 240.0.0.0/4
    if ip & 0xF0_00_00_00 == 0xF0_00_00_00 {
        return false;
    }
    // 255.255.255.255/32
    //
    // # Also to be considered, multicast addresses subnet:
    // 224.0.0.0/4
    if ip & 0xF0_00_00_00 == 0xE0_00_00_00 {
        return false;
    }
    true
}
