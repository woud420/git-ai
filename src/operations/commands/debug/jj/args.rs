use super::Error;
use crate::model::jj_observation::validate_source;
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
}

pub(super) fn is_help(input: &[String]) -> bool {
    match input {
        [flag] => matches!(flag.as_str(), "--help" | "-h"),
        [action, flag] => {
            matches!(action.as_str(), "status" | "receipt")
                && matches!(flag.as_str(), "--help" | "-h")
        }
        _ => false,
    }
}

fn usage() -> Error {
    Error::new("usage", "Use git-ai debug jj --help for command syntax.")
}

pub(super) fn parse(input: &[String]) -> Result<Request<'_>, Error> {
    let (action, options) = input.split_first().ok_or_else(usage)?;
    if !matches!(action.as_str(), "status" | "receipt") || input.len() > 8 {
        return Err(usage());
    }
    let (mut journal, mut source, mut admission) = (None, None, None);
    let mut json = false;
    let mut options = options.iter();
    while let Some(flag) = options.next() {
        let slot = match flag.as_str() {
            "--json" if !json => {
                json = true;
                continue;
            }
            "--journal" => &mut journal,
            "--source" => &mut source,
            "--admission" => &mut admission,
            _ => return Err(usage()),
        };
        if slot.is_some() {
            return Err(usage());
        }
        let value = options
            .next()
            .map(String::as_str)
            .filter(|value| !value.is_empty() && !value.starts_with('-'))
            .ok_or_else(usage)?;
        *slot = Some(value);
    }
    if !json {
        return Err(usage());
    }
    let journal = Path::new(journal.ok_or_else(usage)?);
    match (action.as_str(), source, admission) {
        ("status", None, None) => Ok(Request::Status { journal }),
        ("receipt", Some(source), Some(admission)) => {
            validate_source(source).map_err(|_| usage())?;
            validate_source(admission).map_err(|_| usage())?;
            Ok(Request::Receipt {
                journal,
                source,
                admission,
            })
        }
        _ => Err(usage()),
    }
}
