use super::super::{source, unix::sample_source};
use super::*;

impl<'a> RetainedCapture<'a> {
    pub(super) fn open(
        context: &WorkspaceContext,
        budget: &'a mut CaptureBudget,
        created: Option<CreatedSourceSeal>,
        history_mode: bool,
        hooks: &mut impl SealHooks,
    ) -> Result<Self, E> {
        budget.check(hooks)?;
        source::preflight(context)?;
        let directories = DirectoryRegistry::new(budget, hooks)?;
        let mut session = Self {
            directories,
            metadata: MetadataSamples::default(),
            budget,
            captured: None,
            repository: 0,
            policy_directories: [0; 3],
            operation_directories: [0; 2],
            policy_paths: SampledPolicyPaths {
                workspace_root: context.workspace_root.clone(),
                git_dir: context.git.git_dir.clone(),
                git_common_dir: context.git.common_dir.clone(),
            },
            canonical_paths: None,
            colocated: context.colocated,
            namespace: None,
            held: None,
            expected_policy: None,
            final_checked: false,
            final_succeeded: false,
            history_mode,
            history_state: HistoryState::NotStarted,
        };
        let bound = source::bind(
            context,
            &mut session.directories,
            &mut session.metadata,
            session.budget,
            hooks,
        )?;
        session.repository = bound.repository;
        session.operation_directories = [bound.operations, bound.views];
        session.policy_directories = bound.policy_directories;
        if let Some(created) = created {
            if created.continuity.source != bound.binding
                || created.continuity.workspace != bound.workspace_directories
                || created.continuity.colocated != context.colocated
            {
                return Err(E::invalid(
                    "source binding",
                    "registration sessions have different source or workspace binding",
                ));
            }
            session.expected_policy = Some(created.continuity.policy);
            session.held = Some(created.held);
        }
        match kind_at(
            session.directories.file(bound.repository),
            c"git-ai",
            session.budget,
            hooks,
        )? {
            None if session.held.is_none() => {}
            Some(EntryKind::Directory(_)) => {
                let namespace = session.directories.open_child(
                    bound.repository,
                    b"git-ai",
                    session.budget,
                    hooks,
                )?;
                if let Some(held) = &session.held {
                    held.recheck(
                        session.directories.file(namespace),
                        SealSample::Initial,
                        session.budget,
                        hooks,
                    )?;
                } else {
                    session.held = Some(HeldSeal::open(
                        session.directories.file(namespace),
                        session.budget,
                        hooks,
                    )?);
                }
                session.namespace = Some(namespace);
            }
            _ => {
                return Err(E::invalid(
                    "seal",
                    "source namespace is absent or not a supported directory",
                ));
            }
        }
        session.captured = Some(sample_source(
            bound,
            &session.directories,
            &session.metadata,
            session.budget,
            hooks,
        )?);
        session.budget.check(hooks)?;
        Ok(session)
    }

    pub(crate) fn publish_new(self, source_id: &str) -> Result<CreatedSourceSeal, E> {
        self.publish_new_with(source_id, &mut DirectCapture)
    }

    pub(super) fn publish_new_with(
        mut self,
        source_id: &str,
        hooks: &mut impl SealHooks,
    ) -> Result<CreatedSourceSeal, E> {
        if self.history_mode {
            return Err(E::invalid(
                "history",
                "read-only session cannot publish a source seal",
            ));
        }
        if self.held.is_some() || self.namespace.is_some() || self.final_checked {
            return Err(E::invalid(
                "seal publication",
                "session is not an unpublished source",
            ));
        }
        let seal = SourceSeal::new(source_id)?;
        let policy = self.canonical_paths.take().ok_or(E::invalid(
            "policy binding",
            "canonical policy paths were not validated",
        ))?;
        let captured = self.captured.take().ok_or(E::invalid(
            "registration capture",
            "missing captured source",
        ))?;
        let CapturedJjCurrentState {
            source_binding,
            _workspace_directories,
            head_ids,
            anchors,
            checkout_bytes,
            checkout,
            checkout_evidence,
            _metadata,
        } = captured;
        drop((
            head_ids,
            anchors,
            checkout_bytes,
            checkout,
            checkout_evidence,
            _metadata,
        ));
        let continuity = Continuity {
            source: source_binding,
            workspace: _workspace_directories,
            colocated: self.colocated,
            policy,
        };
        // Destructure and drop the raw evidence before the first mutation;
        // only source/attachment comparison metadata survives into C1.
        let held = seal::publication::publish(
            &mut self.directories,
            self.repository,
            seal,
            self.budget,
            hooks,
        )?;
        self.metadata
            .recheck(&self.directories, self.budget, hooks)?;
        self.directories.recheck(self.budget, hooks)?;
        Ok(CreatedSourceSeal { held, continuity })
    }
}
