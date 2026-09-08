use super::Error;
use crate::model::jj_observation::{is_root, validate_operation_id, validate_source};
use crate::model::repository::jj_observation_journal::validate_workspace_name;
use crate::operations::jj::admission::NativeAdmissionExpectation;
use crate::operations::jj::baseline::MAX_JJ_BASELINE_HEADS;
use std::path::Path;

pub(super) struct ExpectedArgs<'a> {
    pub source: &'a str,
    pub initialization_receipt: &'a str,
    pub baseline: &'a str,
    pub generation: u64,
    pub heads: Vec<String>,
}

impl ExpectedArgs<'_> {
    pub(super) fn expectation(&self) -> NativeAdmissionExpectation<'_> {
        NativeAdmissionExpectation {
            source_id: self.source,
            initialization_receipt_id: self.initialization_receipt,
            baseline_id: self.baseline,
            generation: self.generation,
            admitted_head_ids: &self.heads,
        }
    }
}

pub(super) struct ObserveArgs<'a> {
    pub journal: &'a Path,
    pub expected: ExpectedArgs<'a>,
    pub workspace: &'a str,
    pub attachment: &'a str,
    pub attempts: u64,
    pub interval_ms: u64,
}

pub(super) enum Request<'a> {
    Status {
        journal: &'a Path,
    },
    Receipt {
        journal: &'a Path,
        source: &'a str,
        admission: &'a str,
    },
    Initialize {
        journal: &'a Path,
    },
    Capture {
        journal: &'a Path,
        expected: ExpectedArgs<'a>,
    },
    Observe(ObserveArgs<'a>),
}

pub(super) fn is_help(input: &[String]) -> bool {
    match input {
        [flag] => matches!(flag.as_str(), "--help" | "-h"),
        [action, flag] => {
            matches!(
                action.as_str(),
                "status" | "receipt" | "initialize" | "capture" | "observe"
            ) && matches!(flag.as_str(), "--help" | "-h")
        }
        _ => false,
    }
}

fn usage() -> Error {
    Error::new("usage", "Use git-ai debug jj --help for command syntax.")
}

fn workspace_value<'a>(options: &mut std::slice::Iter<'a, String>) -> Result<&'a str, Error> {
    options
        .next()
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(usage)
}

fn value<'a>(options: &mut std::slice::Iter<'a, String>) -> Result<&'a str, Error> {
    workspace_value(options).and_then(|value| {
        if value.starts_with('-') {
            Err(usage())
        } else {
            Ok(value)
        }
    })
}

fn generation(value: &str) -> Result<u64, Error> {
    if value.len() > 19
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(usage());
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|value| *value < i64::MAX as u64)
        .ok_or_else(usage)
}

fn bounded_control(
    value: Option<&str>,
    default: u64,
    minimum: u64,
    maximum: u64,
) -> Result<u64, Error> {
    let number = value.map(generation).transpose()?.unwrap_or(default);
    if (minimum..=maximum).contains(&number) {
        Ok(number)
    } else {
        Err(usage())
    }
}

pub(super) fn parse(input: &[String]) -> Result<Request<'_>, Error> {
    let (action, options) = input.split_first().ok_or_else(usage)?;
    let action = action.as_str();
    let limit = match action {
        "status" | "receipt" | "initialize" => 8,
        "capture" => 80,
        "observe" => 88,
        _ => return Err(usage()),
    };
    if input.len() > limit {
        return Err(usage());
    }
    let captures = matches!(action, "capture" | "observe");
    let (mut journal, mut source, mut admission) = (None, None, None);
    let (mut initialization_receipt, mut baseline, mut expected_generation) = (None, None, None);
    let (mut workspace, mut attachment, mut attempts, mut interval_ms) = (None, None, None, None);
    let mut heads = Vec::new();
    let mut json = false;
    let mut options = options.iter();
    while let Some(flag) = options.next() {
        let slot = match flag.as_str() {
            "--json" if !json => {
                json = true;
                continue;
            }
            "--journal" => &mut journal,
            "--source" if action == "receipt" => &mut source,
            "--admission" if action == "receipt" => &mut admission,
            "--expect-source" if captures => &mut source,
            "--expect-initialization-receipt" if captures => &mut initialization_receipt,
            "--expect-baseline" if captures => &mut baseline,
            "--expect-generation" if captures => &mut expected_generation,
            "--expect-workspace" if action == "observe" => {
                if workspace.is_some() {
                    return Err(usage());
                }
                workspace = Some(workspace_value(&mut options)?);
                continue;
            }
            "--expect-attachment" if action == "observe" => &mut attachment,
            "--attempts" if action == "observe" => &mut attempts,
            "--interval-ms" if action == "observe" => &mut interval_ms,
            "--expect-head" if captures => {
                if heads.len() >= MAX_JJ_BASELINE_HEADS {
                    return Err(usage());
                }
                let head = value(&mut options)?;
                validate_operation_id(head).map_err(|_| usage())?;
                if is_root(head) || heads.iter().any(|existing| existing == head) {
                    return Err(usage());
                }
                heads.push(head.to_owned());
                continue;
            }
            _ => return Err(usage()),
        };
        if slot.is_some() {
            return Err(usage());
        }
        *slot = Some(value(&mut options)?);
    }
    if !json {
        return Err(usage());
    }
    let journal = Path::new(journal.ok_or_else(usage)?);
    match action {
        "status" => Ok(Request::Status { journal }),
        "initialize" => Ok(Request::Initialize { journal }),
        "receipt" => {
            let source = source.ok_or_else(usage)?;
            let admission = admission.ok_or_else(usage)?;
            validate_source(source).map_err(|_| usage())?;
            validate_source(admission).map_err(|_| usage())?;
            Ok(Request::Receipt {
                journal,
                source,
                admission,
            })
        }
        "capture" | "observe" => {
            let source = source.ok_or_else(usage)?;
            let initialization_receipt = initialization_receipt.ok_or_else(usage)?;
            let baseline = baseline.ok_or_else(usage)?;
            for id in [source, initialization_receipt, baseline] {
                validate_source(id).map_err(|_| usage())?;
            }
            let generation = generation(expected_generation.ok_or_else(usage)?)?;
            if heads.is_empty() {
                return Err(usage());
            }
            let expected = ExpectedArgs {
                source,
                initialization_receipt,
                baseline,
                generation,
                heads,
            };
            if action == "capture" {
                return Ok(Request::Capture { journal, expected });
            }
            let workspace = workspace.ok_or_else(usage)?;
            let attachment = attachment.ok_or_else(usage)?;
            validate_workspace_name(workspace).map_err(|_| usage())?;
            validate_source(attachment).map_err(|_| usage())?;
            Ok(Request::Observe(ObserveArgs {
                journal,
                expected,
                workspace,
                attachment,
                attempts: bounded_control(attempts, 1, 1, 32)?,
                interval_ms: bounded_control(interval_ms, 1000, 250, 60000)?,
            }))
        }
        _ => Err(usage()),
    }
}
