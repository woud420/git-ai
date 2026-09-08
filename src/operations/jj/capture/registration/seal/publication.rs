use super::*;
use crate::operations::jj::capture::directories::DirectoryRegistry;

const MAX_WRITES: usize = 256;
const MAX_SYNCS: usize = 8;

pub(in crate::operations::jj::capture::registration) fn publish(
    directories: &mut DirectoryRegistry,
    repository: usize,
    seal: SourceSeal,
    budget: &mut CaptureBudget,
    hooks: &mut impl SealHooks,
) -> Result<HeldSeal, E> {
    // Exclusive access to this session keeps the checked registry headroom
    // available until open_child admits the one new namespace descriptor.
    budget.check_namespace_capacity(hooks)?;
    phase(PublicationPhase::BeforeMutation, budget, hooks)?;
    let created = hooks.mkdir(directories.file(repository).as_raw_fd(), c"git-ai");
    budget.check(hooks)?;
    created.map_err(|error| E::caused("seal namespace creation", error))?;
    phase(PublicationPhase::NamespaceCreated, budget, hooks)?;
    let namespace = directories.open_child(repository, b"git-ai", budget, hooks)?;
    let parent = directories.file(namespace);
    namespace_policy(parent, budget, hooks)?;

    budget.check(hooks)?;
    let created = hooks.create(parent.as_raw_fd(), c".registration.tmp");
    budget.check(hooks)?;
    let file = created.map_err(|error| E::caused("seal temporary creation", error))?;
    phase(PublicationPhase::TemporaryCreated, budget, hooks)?;
    let changed = hooks.fchmod(&file);
    budget.check(hooks)?;
    changed.map_err(|error| E::caused("seal permissions", error))?;
    let initial = file_policy(&file, true, budget, hooks)?;
    check_named(parent, c".registration.tmp", &initial, budget, hooks)?;

    write_bytes(&file, seal.bytes(), budget, hooks)?;
    phase(PublicationPhase::LeafWritten, budget, hooks)?;
    let written = file_policy(&file, false, budget, hooks)?;
    if written.length != seal.bytes().len() as u64 {
        return Err(E::invalid(
            "seal publication",
            "written seal length differs",
        ));
    }
    check_named(parent, c".registration.tmp", &written, budget, hooks)?;
    let mut syncs = 0;
    sync(&file, &mut syncs, budget, hooks)?;
    phase(PublicationPhase::LeafSynced, budget, hooks)?;
    check_named(parent, c".registration.tmp", &written, budget, hooks)?;
    budget.check(hooks)?;
    let renamed = hooks.rename(parent.as_raw_fd(), c".registration.tmp", c"registration");
    if let Err(error) = renamed {
        budget.check(hooks)?;
        return Err(E::caused("seal no-replace publication", error));
    }
    // A successful rename has published the seal even if its deadline expired
    // during the syscall. Report that state before the next fallible check.
    phase(PublicationPhase::SealPublished, budget, hooks)?;
    sync(parent, &mut syncs, budget, hooks)?;
    phase(PublicationPhase::NamespaceSynced, budget, hooks)?;
    sync(directories.file(repository), &mut syncs, budget, hooks)?;
    phase(PublicationPhase::RepositorySynced, budget, hooks)?;
    HeldSeal::published(file, seal, parent, budget, hooks)
}

fn write_bytes(
    file: &File,
    bytes: &[u8],
    budget: &CaptureBudget,
    hooks: &mut impl SealHooks,
) -> Result<(), E> {
    let mut offset = 0;
    for _ in 0..MAX_WRITES {
        budget.check(hooks)?;
        let result = hooks.write(file, &bytes[offset..]);
        budget.check(hooks)?;
        match result {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(E::caused("seal write", error)),
            Ok(0) => return Err(E::invalid("seal write", "zero-byte write")),
            Ok(count) if count <= bytes.len() - offset => offset += count,
            Ok(_) => return Err(E::invalid("seal write", "write exceeded supplied bytes")),
        }
        if offset == bytes.len() {
            return Ok(());
        }
    }
    Err(E::invalid("seal write", "raw write attempt limit exceeded"))
}

fn sync(
    file: &File,
    attempts: &mut usize,
    budget: &CaptureBudget,
    hooks: &mut impl SealHooks,
) -> Result<(), E> {
    while *attempts < MAX_SYNCS {
        budget.check(hooks)?;
        *attempts += 1;
        let result = hooks.sync(file);
        budget.check(hooks)?;
        match result {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(E::caused("seal sync", error)),
            Ok(()) => return Ok(()),
        }
    }
    Err(E::invalid("seal sync", "raw sync attempt limit exceeded"))
}

fn phase(
    at: PublicationPhase,
    budget: &CaptureBudget,
    hooks: &mut impl SealHooks,
) -> Result<(), E> {
    hooks.publication_phase(at);
    budget.check(hooks)
}
