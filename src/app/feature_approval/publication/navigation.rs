/// Treat the index strictly as navigation data: append one deterministic link
/// inside its Navigation section and never inspect or execute other text.
pub(in crate::app::root::feature_approval) fn add_navigation_link(
    index: &str,
    uid: &str,
) -> anyhow::Result<String> {
    add_navigation_entry(
        index,
        &format!("{uid}.md"),
        &format!("./{uid}.md"),
        "Approved feature specification.",
    )
}

pub(in crate::app::root::feature_approval) fn add_navigation_entry(
    index: &str,
    label: &str,
    path: &str,
    description: &str,
) -> anyhow::Result<String> {
    let start = index
        .find("# Navigation")
        .ok_or_else(|| anyhow::anyhow!("navigation index has no Navigation section"))?;
    let after = start + "# Navigation".len();
    let end = index[after..]
        .find("\n# ")
        .map(|offset| after + offset)
        .unwrap_or(index.len());
    let section = &index[after..end];
    if section
        .lines()
        .any(|line| line.contains(&format!("]({path})")))
    {
        return Ok(index.to_owned());
    }
    let link = format!("\n- [{label}]({path}) - {description}\n");
    Ok(format!("{}{}{}", &index[..end], link, &index[end..]))
}
