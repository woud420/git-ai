use super::{Error, args::usage};
use std::path::Path;

pub(super) enum Request<'a> {
    Enable { journal: &'a Path },
    Status,
    Disable,
    Resume,
}

pub(super) fn parse(input: &[String]) -> Result<Request<'_>, Error> {
    let (action, options) = input.split_first().ok_or_else(usage)?;
    if input.len() > 4 {
        return Err(usage());
    }
    if action != "enable" {
        if options.len() != 1 || options[0] != "--json" {
            return Err(usage());
        }
        return match action.as_str() {
            "status" => Ok(Request::Status),
            "disable" => Ok(Request::Disable),
            "resume" => Ok(Request::Resume),
            _ => Err(usage()),
        };
    }
    let mut options = options.iter();
    let mut journal = None;
    let mut json = false;
    while let Some(flag) = options.next() {
        match flag.as_str() {
            "--json" if !json => json = true,
            "--journal" if journal.is_none() => {
                journal = Some(
                    options
                        .next()
                        .filter(|value| !value.is_empty() && !value.starts_with('-'))
                        .ok_or_else(usage)?,
                );
            }
            _ => return Err(usage()),
        }
    }
    if !json {
        return Err(usage());
    }
    Ok(Request::Enable {
        journal: Path::new(journal.ok_or_else(usage)?),
    })
}
