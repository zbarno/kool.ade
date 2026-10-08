use std::net::{IpAddr, ToSocketAddrs};
use url::Url;

pub fn public_address(url: &Url) -> anyhow::Result<IpAddr> {
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Resource URL has no host"))?;
    let mut addresses = (host, 443).to_socket_addrs()?.map(|address| address.ip());
    let address = addresses
        .next()
        .ok_or_else(|| anyhow::anyhow!("Resource host did not resolve"))?;
    anyhow::ensure!(
        public_ip(address) && addresses.all(public_ip),
        "Resource host resolves to a private or reserved network address"
    );
    Ok(address)
}

pub(super) fn public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || (a == 100 && (64..=127).contains(&b))
                || a == 127
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 192 && b == 0 && c == 0)
                || (a == 192 && b == 88 && c == 99)
                || (a == 192 && b == 0 && c == 2)
                || (a == 198 && (b == 18 || b == 19))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113)
                || a >= 224)
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            (segments[0] & 0xe000 == 0x2000)
                && !(segments[0] == 0x2001 && segments[1] <= 0x01ff)
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
                && segments[0] != 0x2002
                && !(segments[0] == 0x3fff && segments[1] & 0xf000 == 0)
                && !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && !ip.is_unique_local()
                && !ip.is_unicast_link_local()
                && ip
                    .to_ipv4_mapped()
                    .is_none_or(|mapped| public_ip(mapped.into()))
        }
    }
}
