use crate::model::repository::jj_observation_journal::JournalError;
use crate::operations::jj::ancestry::JjAncestryError;
use crate::operations::jj::baseline_persistence::JjBaselinePersistenceError;
use crate::operations::jj::capture::JjCaptureError;
use crate::operations::jj::registration::JjRegistrationError;
use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum JjNativeAdmissionError {
    Registration(JjRegistrationError),
    Capture(JjCaptureError),
    Journal(JournalError),
    Baseline(JjBaselinePersistenceError),
    Ancestry(JjAncestryError),
    Input(&'static str),
    UnsupportedPlatform,
}

impl fmt::Display for JjNativeAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("jj native admission: ")?;
        match self {
            Self::Registration(error) => fmt::Display::fmt(error, formatter),
            Self::Capture(error) => fmt::Display::fmt(error, formatter),
            Self::Journal(error) => fmt::Display::fmt(error, formatter),
            Self::Baseline(error) => fmt::Display::fmt(error, formatter),
            Self::Ancestry(error) => fmt::Display::fmt(error, formatter),
            Self::Input(message) => formatter.write_str(message),
            Self::UnsupportedPlatform => {
                formatter.write_str("unsupported native admission platform")
            }
        }
    }
}

impl Error for JjNativeAdmissionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Registration(error) => Some(error),
            Self::Capture(error) => Some(error),
            Self::Journal(error) => Some(error),
            Self::Baseline(error) => Some(error),
            Self::Ancestry(error) => Some(error),
            Self::Input(_) | Self::UnsupportedPlatform => None,
        }
    }
}

impl From<JjRegistrationError> for JjNativeAdmissionError {
    fn from(error: JjRegistrationError) -> Self {
        Self::Registration(error)
    }
}
impl From<JjCaptureError> for JjNativeAdmissionError {
    fn from(error: JjCaptureError) -> Self {
        Self::Capture(error)
    }
}
impl From<JournalError> for JjNativeAdmissionError {
    fn from(error: JournalError) -> Self {
        Self::Journal(error)
    }
}
impl From<JjBaselinePersistenceError> for JjNativeAdmissionError {
    fn from(error: JjBaselinePersistenceError) -> Self {
        Self::Baseline(error)
    }
}
impl From<JjAncestryError> for JjNativeAdmissionError {
    fn from(error: JjAncestryError) -> Self {
        Self::Ancestry(error)
    }
}
