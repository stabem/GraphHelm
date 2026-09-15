//! Reads owner-change notices immediately before plain cognitive dispatch. Existing model
//! calls are not interrupted; tool/gate work and the blind judge retain their original input.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use graphhelm_runtime::classify::NodeWorkKind;
use graphhelm_runtime::executor::{AsyncNodeExecutor, ExecutorRefusal, NodeWork, WorkOutcome};

use crate::commands::execution::signal::SignalKeyring;

type Reader = dyn Fn(&str) -> Result<String, ()> + Send + Sync;

pub(super) struct OwnerNoticeExecutor {
    inner: Arc<dyn AsyncNodeExecutor>,
    reader: Arc<Reader>,
}

impl OwnerNoticeExecutor {
    pub(super) fn new(
        inner: Arc<dyn AsyncNodeExecutor>,
        events: PathBuf,
        keyring: Option<Arc<SignalKeyring>>,
    ) -> Self {
        Self {
            inner,
            reader: Arc::new(move |execution| {
                let keyring = keyring.as_deref().ok_or(())?;
                crate::commands::execution::documents::notice_context(&events, execution, keyring)
                    .map_err(|_| ())
            }),
        }
    }
}

impl AsyncNodeExecutor for OwnerNoticeExecutor {
    fn execute<'a>(
        &'a self,
        work: &'a NodeWork,
    ) -> Pin<Box<dyn Future<Output = Result<WorkOutcome, ExecutorRefusal>> + Send + 'a>> {
        Box::pin(async move {
            if !matches!(work.kind, NodeWorkKind::Cognitive) || work.judge.is_some() {
                return self.inner.execute(work).await;
            }
            let reader = self.reader.clone();
            let execution = work.execution_id.clone();
            let notices = tokio::task::spawn_blocking(move || reader(&execution))
                .await
                .map_err(|_| ExecutorRefusal::Unassemblable)?
                .map_err(|_| ExecutorRefusal::Unassemblable)?;
            // The reader's contract is bounded; keep a guard at the model boundary too.
            if notices.len() > 64 * 1024 {
                return Err(ExecutorRefusal::Unassemblable);
            }
            let mut delivered = work.clone();
            delivered.prompt = delivered.prompt.with_owner_notices(&notices);
            self.inner.execute(&delivered).await
        })
    }

    fn cancel_all(&self) {
        self.inner.cancel_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_runtime::executor::ToolFailureSemantics;
    use std::sync::Mutex;

    struct Observer(Mutex<Vec<NodeWork>>);

    impl AsyncNodeExecutor for Observer {
        fn execute<'a>(
            &'a self,
            work: &'a NodeWork,
        ) -> Pin<Box<dyn Future<Output = Result<WorkOutcome, ExecutorRefusal>> + Send + 'a>>
        {
            Box::pin(async move {
                self.0.lock().unwrap().push(work.clone());
                Err(ExecutorRefusal::Unsupported)
            })
        }
    }

    fn work(kind: NodeWorkKind) -> NodeWork {
        NodeWork {
            execution_id: "exec-notices".to_owned(),
            node_id: "node-next".to_owned(),
            attempt: 1,
            prompt: graphhelm_runtime::prompt::tool_placeholder(),
            kind,
            tool_failure_semantics: ToolFailureSemantics::default(),
            tool_call: None,
            gate_check: None,
            judge: None,
            context: None,
        }
    }

    #[tokio::test]
    async fn actual_delegated_prompt_contains_notices_and_new_digest() {
        let observer = Arc::new(Observer(Mutex::new(vec![])));
        let wrapper = OwnerNoticeExecutor {
            inner: observer.clone(),
            reader: Arc::new(|execution| {
                assert_eq!(execution, "exec-notices");
                Ok(r#"{"changes":[{"path":"docs/rule.md"}]}"#.to_owned())
            }),
        };
        let original = work(NodeWorkKind::Cognitive);
        let _ = wrapper.execute(&original).await;
        let seen = observer.0.lock().unwrap();
        assert!(seen[0].prompt.task.contains("docs/rule.md"));
        assert_ne!(seen[0].prompt.sha256, original.prompt.sha256);
        assert_eq!(seen[0].prompt.system, original.prompt.system);
        assert_eq!(seen[0].prompt.context, original.prompt.context);
    }

    #[tokio::test]
    async fn empty_notice_preserves_exact_prompt() {
        let observer = Arc::new(Observer(Mutex::new(vec![])));
        let wrapper = OwnerNoticeExecutor {
            inner: observer.clone(),
            reader: Arc::new(|_| Ok(String::new())),
        };
        let original = work(NodeWorkKind::Cognitive);
        let _ = wrapper.execute(&original).await;
        assert_eq!(observer.0.lock().unwrap()[0].prompt, original.prompt);
    }

    #[tokio::test]
    async fn tools_and_gates_do_not_read_notices() {
        let observer = Arc::new(Observer(Mutex::new(vec![])));
        let wrapper = OwnerNoticeExecutor {
            inner: observer.clone(),
            reader: Arc::new(|_| panic!("notice reader must not run")),
        };
        for kind in [NodeWorkKind::Tool, NodeWorkKind::GateCheck] {
            let original = work(kind);
            let _ = wrapper.execute(&original).await;
            assert_eq!(
                observer.0.lock().unwrap().last().unwrap().prompt,
                original.prompt
            );
        }
    }

    #[tokio::test]
    async fn blind_judge_keeps_original_diet_without_reading_notices() {
        let observer = Arc::new(Observer(Mutex::new(vec![])));
        let wrapper = OwnerNoticeExecutor {
            inner: observer.clone(),
            reader: Arc::new(|_| panic!("the blind judge must not receive project notices")),
        };
        let mut original = work(NodeWorkKind::Cognitive);
        let judge = graphhelm_runtime::judge::JudgeWork {
            judge_id: "judge-owner-journey".to_owned(),
            user_story: "Read the delivered document".to_owned(),
            mcp_surface: "fixture-surface".to_owned(),
        };
        original.prompt = graphhelm_runtime::judge::assemble(&judge);
        original.judge = Some(judge);
        let _ = wrapper.execute(&original).await;
        assert_eq!(observer.0.lock().unwrap()[0].prompt, original.prompt);
    }

    #[tokio::test]
    async fn unreadable_or_oversized_notices_refuse_before_dispatch() {
        for response in [Err(()), Ok("x".repeat(64 * 1024 + 1))] {
            let observer = Arc::new(Observer(Mutex::new(vec![])));
            let wrapper = OwnerNoticeExecutor {
                inner: observer.clone(),
                reader: Arc::new(move |_| response.clone()),
            };
            assert!(matches!(
                wrapper.execute(&work(NodeWorkKind::Cognitive)).await,
                Err(ExecutorRefusal::Unassemblable)
            ));
            assert!(observer.0.lock().unwrap().is_empty());
        }
    }
}
