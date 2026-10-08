use super::network::public_ip;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[test]
fn only_publicly_routable_ip_ranges_are_accepted() {
    for address in [
        IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)),
        IpAddr::V4(Ipv4Addr::new(192, 88, 99, 1)),
        IpAddr::V6("2001:db8::1".parse::<Ipv6Addr>().unwrap()),
        IpAddr::V6("2001:2::1".parse::<Ipv6Addr>().unwrap()),
        IpAddr::V6("2001:10::1".parse::<Ipv6Addr>().unwrap()),
        IpAddr::V6("2001:20::1".parse::<Ipv6Addr>().unwrap()),
        IpAddr::V6("2002::1".parse::<Ipv6Addr>().unwrap()),
        IpAddr::V6("3fff::1".parse::<Ipv6Addr>().unwrap()),
        IpAddr::V6("fec0::1".parse::<Ipv6Addr>().unwrap()),
        IpAddr::V6("fd00::1".parse::<Ipv6Addr>().unwrap()),
    ] {
        assert!(
            !public_ip(address),
            "reserved address was accepted: {address}"
        );
    }
    assert!(public_ip("1.1.1.1".parse().unwrap()));
    assert!(public_ip("2606:4700:4700::1111".parse().unwrap()));
}

#[test]
fn custom_registry_redirects_stay_within_the_approved_origin() {
    let registry = url::Url::parse("https://packages.example.net/").unwrap();
    assert!(matches!(
        super::policy::classify_npm_registry_redirect(
            "https://packages.example.net/zod/-/zod-1.0.0.tgz",
            &registry,
        )
        .unwrap(),
        super::policy::Decision::Allow(_)
    ));
    for redirect in [
        "https://packages.example.net/repository/npm/zod%2f..%2fsecret/-/zod-1.0.0.tgz",
        "https://cdn.example.net/zod-1.0.0.tgz",
        "https://registry.npmjs.org/zod/-/zod-1.0.0.tgz",
    ] {
        assert!(matches!(
            super::policy::classify_npm_registry_redirect(redirect, &registry).unwrap(),
            super::policy::Decision::NeedsAttention(_)
        ));
    }
}
