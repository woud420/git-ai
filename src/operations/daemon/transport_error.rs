use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;

pub(super) fn transport_error(kind: std::io::ErrorKind, message: String) -> GitAiError {
    PersistenceError::Io {
        // Clients and persisted diagnostics already expose this prefix.
        operation: "Generic error",
        path: String::new(),
        kind,
        message,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_diagnostics_preserve_display_and_io_classification() {
        let error = transport_error(
            std::io::ErrorKind::UnexpectedEof,
            "daemon socket /tmp/control closed without a response".into(),
        );
        assert_eq!(
            error.to_string(),
            "Generic error: daemon socket /tmp/control closed without a response"
        );
        assert!(matches!(
            error,
            GitAiError::Persistence(PersistenceError::Io {
                kind: std::io::ErrorKind::UnexpectedEof,
                ..
            })
        ));
    }
}
