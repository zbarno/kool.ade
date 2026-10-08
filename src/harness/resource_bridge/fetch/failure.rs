#[derive(Debug)]
struct RetrievalFailure {
    bytes: usize,
    source: anyhow::Error,
}

impl std::fmt::Display for RetrievalFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for RetrievalFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

pub(super) fn with_bytes(bytes: usize, source: anyhow::Error) -> anyhow::Error {
    anyhow::Error::new(RetrievalFailure { bytes, source })
}

pub fn bytes_from_failure(error: &anyhow::Error) -> Option<usize> {
    error
        .downcast_ref::<RetrievalFailure>()
        .map(|failure| failure.bytes)
}

#[cfg(test)]
mod tests;
