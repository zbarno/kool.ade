pub(super) fn set_option(args: &mut Vec<String>, option: &str, value: String) {
    if let Some(index) = args
        .iter()
        .position(|arg| arg == option || arg.starts_with(&format!("{option}=")))
    {
        if args[index].starts_with(&format!("{option}=")) {
            args[index] = format!("{option}={value}");
        } else if index + 1 < args.len() {
            args[index + 1] = value;
        } else {
            args.push(value);
        }
    } else {
        args.extend([option.into(), value]);
    }
}

pub(super) fn toml_string(value: &str) -> String {
    serde_json::to_string(value).expect("TOML basic strings share JSON escaping")
}

pub(super) fn toml_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| toml_string(value))
            .collect::<Vec<_>>()
            .join(",")
    )
}
