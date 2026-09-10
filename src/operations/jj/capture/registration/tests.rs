use super::{RegistrationCaptureBudget, RetainedCapture, SampledPolicyPaths, SourceSeal};
use crate::operations::jj::capture::JjCaptureError;
use crate::operations::jj::capture::tests::Fixture;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::time::{Duration, Instant};

mod framing;
mod lifecycle;
#[cfg(target_os = "macos")]
mod macos_acl;
mod retained;
mod support;

use support::*;

mod additional_source_binding;

mod additional_rename_phase;

mod history;
