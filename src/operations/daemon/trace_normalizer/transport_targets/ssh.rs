use super::receive_pack_path;
use serde_json::Value;
use std::path::Path;

pub(super) fn destination(argv: &[Value]) -> Option<String> {
    let (executable, mut args) = argv.split_first()?;
    if !matches!(
        Path::new(executable.as_str()?).file_name()?.to_str()?,
        "ssh" | "ssh.exe"
    ) {
        return None;
    }
    let mut port = None;
    loop {
        let argument = args.first()?.as_str()?;
        match argument {
            "-4" | "-6" | "-v" | "-vv" | "-vvv" | "-q" | "-T" | "-n" | "-x" | "-a" => {
                args = &args[1..];
            }
            "-o" if args.get(1)?.as_str()? == "SendEnv=GIT_PROTOCOL" => args = &args[2..],
            "-p" if port.is_none() => {
                let value = args.get(1)?.as_str()?.parse::<u16>().ok()?;
                if value == 0 {
                    return None;
                }
                port = Some(value);
                args = &args[2..];
            }
            value if value.starts_with('-') => return None,
            _ => break,
        }
    }
    let [host, service] = args else { return None };
    let host = host.as_str()?;
    if host.is_empty()
        || host
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '/' | '?' | '#' | '\0'))
    {
        return None;
    }
    let path = receive_pack_path(service.as_str()?)?;
    match port {
        None => Some(format!("{host}:{path}")),
        Some(port) if path.starts_with('/') => {
            let mut destination = url::Url::parse(&format!("ssh://{host}:{port}/")).ok()?;
            destination.set_path(&path);
            Some(destination.to_string())
        }
        // An SSH URL inserts a leading slash. Do not turn a relative remote
        // path into an absolute one while reconstructing a custom port.
        Some(_) => None,
    }
}
