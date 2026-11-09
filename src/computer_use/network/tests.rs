use super::{
    acquire_slot,
    policy::{public_ip, validate_public_answers},
    proxy::parse_connect_target,
};
use std::{
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[test]
fn research_proxy_accepts_only_an_explicit_https_connect_authority() {
    assert_eq!(
        parse_connect_target("CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n"),
        Some("example.com".into())
    );
    for value in [
        "CONNECT 127.0.0.1:443 HTTP/1.1\r\nHost: 127.0.0.1:443\r\n\r\n",
        "CONNECT 169.254.169.254:443 HTTP/1.1\r\nHost: 169.254.169.254:443\r\n\r\n",
        "CONNECT [::1]:443 HTTP/1.1\r\nHost: [::1]:443\r\n\r\n",
        "CONNECT user@example.com:443 HTTP/1.1\r\nHost: user@example.com:443\r\n\r\n",
        "CONNECT example.com:80 HTTP/1.1\r\nHost: example.com:80\r\n\r\n",
        "GET https://example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n",
        "CONNECT example.com:443 HTTP/1.1\r\nHost: attacker.example\r\n\r\n",
        "CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\nProxy-Authorization: Basic token\r\n\r\n",
    ] {
        assert_eq!(parse_connect_target(value), None, "{value}");
    }
}

#[test]
fn public_address_filter_rejects_reserved_and_private_ranges() {
    for denied in [
        "0.1.2.3",
        "10.0.0.1",
        "100.64.0.1",
        "127.0.0.1",
        "169.254.1.1",
        "172.16.0.1",
        "192.168.1.1",
        "192.0.2.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "::",
        "::1",
        "fc00::1",
        "fe80::1",
        "2001:db8::1",
        "::ffff:127.0.0.1",
    ] {
        assert!(!public_ip(denied.parse::<IpAddr>().unwrap()), "{denied}");
    }
    for allowed in ["8.8.8.8", "93.184.216.34", "2606:4700:4700::1111"] {
        assert!(public_ip(allowed.parse::<IpAddr>().unwrap()), "{allowed}");
    }
}

#[test]
fn research_dns_rejects_empty_or_mixed_public_private_answer_sets() {
    use std::net::SocketAddr;

    assert!(validate_public_answers(&[]).is_err());
    assert!(validate_public_answers(&["93.184.216.34:443".parse::<SocketAddr>().unwrap()]).is_ok());
    assert!(
        validate_public_answers(&[
            "93.184.216.34:443".parse::<SocketAddr>().unwrap(),
            "10.0.0.2:443".parse::<SocketAddr>().unwrap(),
        ])
        .is_err()
    );
}

#[test]
fn gateway_connection_slots_are_bounded_and_released() {
    let active = Arc::new(AtomicUsize::new(0));
    let slots = (0..8)
        .map(|_| {
            assert!(acquire_slot(&active));
            super::ConnectionSlot(active.clone())
        })
        .collect::<Vec<_>>();
    assert!(!acquire_slot(&active));
    drop(slots);
    assert_eq!(active.load(Ordering::Acquire), 0);
    assert!(acquire_slot(&active));
    assert_eq!(active.load(Ordering::Acquire), 1);
}
