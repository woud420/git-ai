use super::Error;
use crate::model::jj_observation::{is_root, validate_operation_id, validate_source};
use crate::operations::jj::baseline::MAX_JJ_BASELINE_HEADS;
use std::path::Path;

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
        source: &'a str,
        initialization_receipt: &'a str,
        baseline: &'a str,
        generation: u64,
        heads: Vec<String>,
    },
}

pub(super) fn is_help(input: &[String]) -> bool {
    match input {
        [flag] => matches!(flag.as_str(), "--help" | "-h"),
        [action, flag] => {
            matches!(
                action.as_str(),
                "status" | "receipt" | "initialize" | "capture"
            ) && matches!(flag.as_str(), "--help" | "-h")
        }
        _ => false,
    }
}

fn usage() -> Error {
    Error::new("usage", "Use git-ai debug jj --help for command syntax.")
}

fn value<'a>(options: &mut std::slice::Iter<'a, String>) -> Result<&'a str, Error> {
    options
        .next()
        .map(String::as_str)
        .filter(|value| !value.is_empty() && !value.starts_with('-'))
        .ok_or_else(usage)
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

pub(super) fn parse(input: &[String]) -> Result<Request<'_>, Error> {
    let (action, options) = input.split_first().ok_or_else(usage)?;
    let action = action.as_str();
    let limit = match action {
        "status" | "receipt" | "initialize" => 8,
        "capture" => 80,
        _ => return Err(usage()),
    };
    if input.len() > limit {
        return Err(usage());
    }
    let (mut journal, mut source, mut admission) = (None, None, None);
    let (mut initialization_receipt, mut baseline, mut expected_generation) = (None, None, None);
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
            "--expect-source" if action == "capture" => &mut source,
            "--expect-initialization-receipt" if action == "capture" => &mut initialization_receipt,
            "--expect-baseline" if action == "capture" => &mut baseline,
            "--expect-generation" if action == "capture" => &mut expected_generation,
            "--expect-head" if action == "capture" => {
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
        "capture" => {
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
            Ok(Request::Capture {
                journal,
                source,
                initialization_receipt,
                baseline,
                generation,
                heads,
            })
        }
        _ => Err(usage()),
    }
}
