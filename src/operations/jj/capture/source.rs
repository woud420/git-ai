use super::directories::{DirectoryRegistry, validate_locator};
use super::metadata::{MetadataSamples, SampleValue, git_pointer, path_text};
use super::{
    CaptureBudget, CaptureHooks, CapturedJjSourceBinding, DirectoryIdentity, JjCaptureError as E,
};
use crate::model::jj_observation::JJ_OBSERVATION_READER_PROFILE;
use crate::operations::workspace_context::WorkspaceContext;
use std::os::unix::ffi::OsStrExt;

pub(super) struct BoundSource {
    pub(super) repository: usize,
    pub(super) policy_directories: [usize; 3],
    pub(super) operations: usize,
    pub(super) views: usize,
    pub(super) heads: usize,
    pub(super) working_copy: usize,
    pub(super) binding: CapturedJjSourceBinding,
    pub(super) workspace_directories: [DirectoryIdentity; 4],
}

pub(super) fn preflight(context: &WorkspaceContext) -> Result<(), E> {
    if context.schema_version != 1 || context.capability != "discovery_only" || context.vcs != "jj"
    {
        return Err(E::invalid(
            "context",
            "unsupported discovery locator schema or capability",
        ));
    }
    let jj = context
        .jj
        .as_ref()
        .ok_or(E::invalid("context", "jj paths are required"))?;
    for path in [
        &context.workspace_root,
        &jj.repo_dir,
        &jj.store_dir,
        &context.git.git_dir,
        &context.git.common_dir,
    ] {
        validate_locator(path)?;
    }
    Ok(())
}

pub(super) fn bind(
    context: &WorkspaceContext,
    directories: &mut DirectoryRegistry,
    metadata: &mut MetadataSamples,
    budget: &mut CaptureBudget,
    hooks: &mut impl CaptureHooks,
) -> Result<BoundSource, E> {
    let jj_paths = context
        .jj
        .as_ref()
        .ok_or(E::invalid("context", "jj paths are required"))?;
    let mut binder = Binder {
        directories,
        metadata,
        budget,
        hooks,
    };
    let workspace = binder.walk(0, context.workspace_root.as_os_str().as_bytes())?;
    let repo_locator = binder.walk(0, jj_paths.repo_dir.as_os_str().as_bytes())?;
    let store_locator = binder.walk(0, jj_paths.store_dir.as_os_str().as_bytes())?;
    let git_locator = binder.walk(0, context.git.git_dir.as_os_str().as_bytes())?;
    let common_locator = binder.walk(0, context.git.common_dir.as_os_str().as_bytes())?;

    let jj = binder.directory(workspace, b".jj")?;
    let working_copy = binder.directory(jj, b"working_copy")?;
    binder.backend(working_copy, b"local")?;
    let repo = match binder.sample(jj, b"repo")? {
        SampleValue::Directory(_) => binder.directory(jj, b"repo")?,
        SampleValue::File(bytes) => binder.walk(jj, path_text(&bytes)?.as_bytes())?,
        SampleValue::Absent => return Err(E::invalid("source", "jj repository entry missing")),
    };
    binder.require_same(repo, repo_locator)?;
    let store = binder.directory(repo, b"store")?;
    binder.require_same(store, store_locator)?;
    let git_backend = binder.backend(store, b"git")?;
    let operation_store = binder.directory(repo, b"op_store")?;
    let operation_backend = binder.backend(operation_store, b"simple_op_store")?;
    let operations = binder.directory(operation_store, b"operations")?;
    let views = binder.directory(operation_store, b"views")?;
    let head_store = binder.directory(repo, b"op_heads")?;
    let head_backend = binder.backend(head_store, b"simple_op_heads_store")?;
    let heads = binder.directory(head_store, b"heads")?;

    let target = binder.required_file(store, b"git_target")?;
    let git = binder.git_target(store, path_text(&target)?.as_bytes())?;
    binder.require_same(git, git_locator)?;
    let common = match binder.sample(git, b"commondir")? {
        SampleValue::Absent => git,
        SampleValue::File(bytes) => binder.walk(git, path_text(&bytes)?.trim().as_bytes())?,
        SampleValue::Directory(_) => {
            return Err(E::invalid(
                "source",
                "Git commondir is not a regular pointer file",
            ));
        }
    };
    binder.require_same(common, common_locator)?;
    binder.directory(common, b"objects")?;
    binder.required_file(git, b"HEAD")?;

    let dot_git = binder.sample(workspace, b".git")?;
    let present = !matches!(dot_git, SampleValue::Absent);
    if present != context.colocated {
        return Err(E::invalid(
            "source",
            "workspace Git presence differs from colocation locator",
        ));
    }
    if present {
        let colocated = binder.git_entry(workspace, b".git", dot_git)?;
        binder.require_same(colocated, git)?;
    }

    let identities = [
        repo,
        store,
        operation_store,
        operations,
        views,
        head_store,
        heads,
        common,
    ]
    .map(|index| binder.directories.identity(index));
    Ok(BoundSource {
        repository: repo,
        policy_directories: [workspace, git, common],
        operations,
        views,
        heads,
        working_copy,
        binding: CapturedJjSourceBinding {
            profile: JJ_OBSERVATION_READER_PROFILE,
            directories: identities,
            backends: [git_backend, operation_backend, head_backend],
        },
        workspace_directories: [workspace, jj, working_copy, git]
            .map(|index| binder.directories.identity(index)),
    })
}

struct Binder<'a, H> {
    directories: &'a mut DirectoryRegistry,
    metadata: &'a mut MetadataSamples,
    budget: &'a mut CaptureBudget,
    hooks: &'a mut H,
}

impl<H: CaptureHooks> Binder<'_, H> {
    fn directory(&mut self, parent: usize, name: &[u8]) -> Result<usize, E> {
        self.directories
            .open_child(parent, name, self.budget, self.hooks)
    }

    fn walk(&mut self, start: usize, bytes: &[u8]) -> Result<usize, E> {
        self.directories.walk(start, bytes, self.budget, self.hooks)
    }

    fn sample(&mut self, parent: usize, name: &[u8]) -> Result<SampleValue, E> {
        self.metadata
            .sample(self.directories, parent, name, self.budget, self.hooks)
    }

    fn required_file(&mut self, parent: usize, name: &[u8]) -> Result<Vec<u8>, E> {
        self.metadata
            .required_file(self.directories, parent, name, self.budget, self.hooks)
    }

    fn backend(&mut self, parent: usize, expected: &[u8]) -> Result<Vec<u8>, E> {
        self.metadata
            .backend(self.directories, parent, expected, self.budget, self.hooks)
    }

    fn require_same(&self, actual: usize, expected: usize) -> Result<(), E> {
        if self.directories.identity(actual) != self.directories.identity(expected) {
            return Err(E::invalid(
                "source",
                "directory identity differs from discovery locator",
            ));
        }
        Ok(())
    }

    fn git_target(&mut self, start: usize, bytes: &[u8]) -> Result<usize, E> {
        let (parent, name, directory_required) =
            self.directories
                .target_parent(start, bytes, self.budget, self.hooks)?;
        let entry = self.sample(parent, name.to_bytes())?;
        if directory_required && !matches!(entry, SampleValue::Directory(_)) {
            return Err(E::invalid(
                "pointer",
                "trailing separator requires a directory",
            ));
        }
        self.git_entry(parent, name.to_bytes(), entry)
    }

    fn git_entry(&mut self, parent: usize, name: &[u8], entry: SampleValue) -> Result<usize, E> {
        match entry {
            SampleValue::Directory(_) => self.directory(parent, name),
            // Discovery supports one Git-file indirection, relative to that file's parent.
            SampleValue::File(bytes) => self.walk(parent, git_pointer(&bytes)?.as_bytes()),
            SampleValue::Absent => Err(E::invalid("source", "Git directory entry missing")),
        }
    }
}
