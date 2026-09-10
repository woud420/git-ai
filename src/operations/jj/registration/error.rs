use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub struct JjRegistrationError {
    stage: &'static str,
    detail: &'static str,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl JjRegistrationError {
    pub(super) fn invalid(stage: &'static str, detail: &'static str) -> Self {
        Self {
            stage,
            detail,
            source: None,
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) fn caused(stage: &'static str, source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            stage,
            detail: "",
            source: Some(Box::new(source)),
        }
    }
}

impl fmt::Display for JjRegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "jj registration {}: ", self.stage)?;
        match &self.source {
            Some(source) => fmt::Display::fmt(source, formatter),
            None => formatter.write_str(self.detail),
        }
    }
}

impl Error for JjRegistrationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}
