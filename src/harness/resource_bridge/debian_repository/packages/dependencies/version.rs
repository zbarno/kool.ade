pub(super) fn compare(left: &str, right: &str) -> anyhow::Result<std::cmp::Ordering> {
    validate_version(left)?;
    validate_version(right)?;
    let a = parse_version(left);
    let b = parse_version(right);
    Ok(a.0
        .parse::<u64>()?
        .cmp(&b.0.parse::<u64>()?)
        .then_with(|| compare_part(a.1, b.1))
        .then_with(|| compare_part(a.2, b.2)))
}

fn parse_version(version: &str) -> (&str, &str, &str) {
    let (epoch, rest) = version.split_once(':').unwrap_or(("0", version));
    let (upstream, revision) = rest.rsplit_once('-').unwrap_or((rest, "0"));
    (epoch, upstream, revision)
}

fn compare_part(left: &str, right: &str) -> std::cmp::Ordering {
    let (mut i, mut j) = (0, 0);
    let (a, b) = (left.as_bytes(), right.as_bytes());
    while i < a.len() || j < b.len() {
        while (i < a.len() && !a[i].is_ascii_digit()) || (j < b.len() && !b[j].is_ascii_digit()) {
            let ac = if i < a.len() && !a[i].is_ascii_digit() {
                let c = a[i];
                i += 1;
                order_char(c)
            } else {
                0
            };
            let bc = if j < b.len() && !b[j].is_ascii_digit() {
                let c = b[j];
                j += 1;
                order_char(c)
            } else {
                0
            };
            if ac != bc {
                return ac.cmp(&bc);
            }
        }
        while i < a.len() && a[i] == b'0' {
            i += 1;
        }
        while j < b.len() && b[j] == b'0' {
            j += 1;
        }
        let (mut end_a, mut end_b) = (i, j);
        while end_a < a.len() && a[end_a].is_ascii_digit() {
            end_a += 1;
        }
        while end_b < b.len() && b[end_b].is_ascii_digit() {
            end_b += 1;
        }
        let cmp = (end_a - i)
            .cmp(&(end_b - j))
            .then_with(|| a[i..end_a].cmp(&b[j..end_b]));
        if !cmp.is_eq() {
            return cmp;
        }
        i = end_a;
        j = end_b;
    }
    std::cmp::Ordering::Equal
}

fn order_char(byte: u8) -> i32 {
    match byte {
        b'~' => -1,
        b'a'..=b'z' => byte as i32,
        _ => byte as i32 + 256,
    }
}

pub(super) fn validate_version(version: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !version.is_empty()
            && version.len() <= 128
            && version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".+:~_-".contains(&byte)),
        "invalid Debian package version"
    );
    let epoch = version.split_once(':').map_or("0", |pair| pair.0);
    anyhow::ensure!(
        epoch.bytes().all(|byte| byte.is_ascii_digit()) && epoch.parse::<u64>().is_ok(),
        "invalid Debian package epoch"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::compare;

    #[test]
    fn follows_epoch_tilde_revision_and_numeric_rules() {
        assert!(compare("2:1.0", "1:99.0").unwrap().is_gt());
        assert!(compare("1.0~rc1", "1.0").unwrap().is_lt());
        assert!(compare("1.0-2", "1.0-1").unwrap().is_gt());
        assert!(compare("1.0002", "1.2").unwrap().is_eq());
    }
}
