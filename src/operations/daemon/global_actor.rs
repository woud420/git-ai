use crate::error::GitAiError;
use crate::model::domain::{AppliedCommand, GlobalState, NormalizedCommand};
use crate::operations::daemon::analyzers::AnalyzerRegistry;
use crate::operations::daemon::reducer;
use tokio::sync::{mpsc, oneshot};

pub enum GlobalMsg {
    Apply(
        Box<NormalizedCommand>,
        oneshot::Sender<Result<AppliedCommand, GitAiError>>,
    ),
    Shutdown,
}

#[derive(Clone)]
pub struct GlobalActorHandle {
    tx: mpsc::Sender<GlobalMsg>,
}

impl GlobalActorHandle {
    pub async fn apply(&self, cmd: NormalizedCommand) -> Result<AppliedCommand, GitAiError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(GlobalMsg::Apply(Box::new(cmd), tx))
            .await
            .map_err(|_| {
                super::state_error::state_error(
                    std::io::ErrorKind::BrokenPipe,
                    "global actor apply send failed".to_string(),
                )
            })?;
        rx.await.map_err(|_| {
            super::state_error::state_error(
                std::io::ErrorKind::BrokenPipe,
                "global actor apply receive failed".to_string(),
            )
        })?
    }

    pub async fn shutdown(&self) -> Result<(), GitAiError> {
        self.tx.send(GlobalMsg::Shutdown).await.map_err(|_| {
            super::state_error::state_error(
                std::io::ErrorKind::BrokenPipe,
                "global actor shutdown send failed".to_string(),
            )
        })
    }
}

pub fn spawn_global_actor() -> GlobalActorHandle {
    let (tx, mut rx) = mpsc::channel::<GlobalMsg>(1024);
    let handle = GlobalActorHandle { tx };

    tokio::spawn(async move {
        let analyzers = AnalyzerRegistry::new();
        let mut state = GlobalState { applied_seq: 0 };

        while let Some(msg) = rx.recv().await {
            match msg {
                GlobalMsg::Apply(cmd, respond_to) => {
                    let result = reducer::reduce_global_command(&mut state, *cmd, &analyzers);
                    let _ = respond_to.send(result);
                }
                GlobalMsg::Shutdown => break,
            }
        }
    });

    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::domain::NormalizedCommand;
    use crate::operations::daemon::tests_fixtures::normalized_command;

    fn global_cmd(seq: u128) -> NormalizedCommand {
        NormalizedCommand {
            root_sid: format!("global-{}", seq),
            started_at_ns: seq,
            finished_at_ns: seq + 1,
            ..normalized_command("help", &["git", "help"])
        }
    }

    #[tokio::test]
    async fn global_actor_applies_commands() {
        let actor = spawn_global_actor();
        let ack = actor.apply(global_cmd(1)).await.unwrap();
        assert_eq!(ack.seq, 1);
        actor.shutdown().await.unwrap();
    }
}
