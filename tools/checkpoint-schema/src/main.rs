use std::path::{Path, PathBuf};
use std::process::Command;

const COMPILER_VERSION: &str = "Cap'n Proto version 1.5.0";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let check = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--check") => true,
        Some(_) => return Err("usage: checkpoint-schema [--check]".into()),
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let compiler = std::env::var_os("CAPNP").unwrap_or_else(|| "capnp".into());
    let version = Command::new(&compiler).arg("--version").output()?;
    if !version.status.success() || String::from_utf8(version.stdout)?.trim() != COMPILER_VERSION {
        return Err(format!("schema generation requires {COMPILER_VERSION}").into());
    }
    let target = root.join("schemas/generated/checkpoint_capnp.rs");
    let scratch = ScratchDirectory::new()?;
    capnpc::CompilerCommand::new()
        .capnp_executable(compiler)
        .src_prefix(root.join("schemas"))
        .file(root.join("schemas/checkpoint.capnp"))
        .output_path(&scratch.0)
        .run()?;
    let generated = scratch.0.join("checkpoint_capnp.rs");
    let bytes = std::fs::read(generated)?;
    if check {
        if std::fs::read(&target)? != bytes {
            return Err("checkpoint schema artifact is stale; regenerate it".into());
        }
    } else {
        std::fs::create_dir_all(target.parent().unwrap())?;
        std::fs::write(target, bytes)?;
    }
    Ok(())
}

struct ScratchDirectory(PathBuf);

impl ScratchDirectory {
    fn new() -> std::io::Result<Self> {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("git-ai-schema-{}-{id}", std::process::id()));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
