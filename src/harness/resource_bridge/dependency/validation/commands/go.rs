pub(crate) fn safe_go_restore_command(command: &str) -> bool {
    if command
        .chars()
        .any(|character| ";&|$`<>\\\n\r".contains(character))
    {
        return false;
    }
    let tokens = command.split_ascii_whitespace().collect::<Vec<_>>();
    tokens.len() == 4
        && tokens[0] == "go"
        && tokens[1] == "mod"
        && tokens[2] == "download"
        && tokens[3] == "all"
}
