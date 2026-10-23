pub(super) fn io_failure(operation: &'static str, error: std::io::Error) -> anyhow::Error {
    anyhow::Error::new(error).context(operation)
}
