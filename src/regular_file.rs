use std::ffi::OsStr;
use std::fmt;
use std::fs::File;
use std::io;
use std::time::Instant;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::open_record_at;

/// Shared limits for a batch of descriptor-relative metadata reads.
#[derive(Debug)]
pub struct MetadataReadBudget {
    remaining_bytes: usize,
    remaining_file_attempts: usize,
    #[cfg(unix)]
    deadline: Instant,
}

impl MetadataReadBudget {
    /// Byte permits include one EOF/growth probe per admitted file.
    pub fn new(max_read_bytes: usize, max_file_attempts: usize, deadline: Instant) -> Self {
        #[cfg(not(unix))]
        let _ = deadline;
        Self {
            remaining_bytes: max_read_bytes,
            remaining_file_attempts: max_file_attempts,
            #[cfg(unix)]
            deadline,
        }
    }

    pub fn remaining_bytes(&self) -> usize {
        self.remaining_bytes
    }

    pub fn remaining_file_attempts(&self) -> usize {
        self.remaining_file_attempts
    }

    #[cfg(unix)]
    fn check_deadline(&self) -> Result<(), MetadataReadError> {
        if Instant::now() >= self.deadline {
            return Err(MetadataReadError::Deadline);
        }
        Ok(())
    }

    #[cfg(unix)]
    fn begin_attempt(&mut self) -> Result<(), MetadataReadError> {
        self.check_deadline()?;
        self.remaining_file_attempts = self
            .remaining_file_attempts
            .checked_sub(1)
            .ok_or(MetadataReadError::AttemptLimit)?;
        Ok(())
    }

    #[cfg(unix)]
    fn reserve(&mut self, length: usize) -> Result<(), MetadataReadError> {
        let charge = length.checked_add(1).ok_or(MetadataReadError::ByteLimit)?;
        self.remaining_bytes = self
            .remaining_bytes
            .checked_sub(charge)
            .ok_or(MetadataReadError::ByteLimit)?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum MetadataReadError {
    UnsupportedPlatform,
    InvalidName,
    NotDirectory,
    NotRegular,
    Changed,
    ByteLimit,
    AttemptLimit,
    Deadline,
    Io(io::Error),
}

impl fmt::Display for MetadataReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::UnsupportedPlatform => "metadata file reading is unsupported on this platform",
            Self::InvalidName => "metadata file name must be one nonempty path component",
            Self::NotDirectory => "metadata parent descriptor is not a directory",
            Self::NotRegular => "metadata entry is not a regular file",
            Self::Changed => "metadata file changed during reading",
            Self::ByteLimit => "metadata file byte limit exceeded",
            Self::AttemptLimit => "metadata file attempt limit exceeded",
            Self::Deadline => "metadata file read deadline expired",
            Self::Io(error) => return write!(formatter, "metadata file I/O failed: {error}"),
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for MetadataReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for MetadataReadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Reads one regular basename of at most 255 bytes relative to an open Unix directory.
///
/// The descriptor remains anchored if its former path is renamed. Ancestor path
/// binding and content authentication belong to the caller. The shared deadline
/// is cooperative: a blocking filesystem syscall cannot be forcibly cancelled.
#[cfg(unix)]
pub fn read_regular_at(
    parent: &File,
    name: &OsStr,
    per_file_maximum: usize,
    budget: &mut MetadataReadBudget,
) -> Result<Vec<u8>, MetadataReadError> {
    unix::read_with(
        parent,
        name,
        per_file_maximum,
        budget,
        &mut unix::DirectRead,
    )
}

#[cfg(not(unix))]
pub fn read_regular_at(
    _parent: &File,
    _name: &OsStr,
    _per_file_maximum: usize,
    _budget: &mut MetadataReadBudget,
) -> Result<Vec<u8>, MetadataReadError> {
    Err(MetadataReadError::UnsupportedPlatform)
}
