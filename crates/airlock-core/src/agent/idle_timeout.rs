//! Idle timeout wrapper for agent adapters.
//!
//! Wraps any [`AgentAdapter`] so that its event stream is automatically
//! terminated if no events arrive within a configurable idle timeout. This
//! prevents deadlocked or hung agent subprocesses from blocking the pipeline
//! indefinitely.

use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use tracing::warn;

use super::types::{AgentEventStream, AgentRequest};
use super::AgentAdapter;
use crate::error::{AirlockError, Result};

/// Default idle timeout: 10 minutes between consecutive events.
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Decorator adapter that wraps an inner [`AgentAdapter`] and applies an idle
/// timeout to the event stream it produces.
pub struct IdleTimeoutAdapter {
    inner: Box<dyn AgentAdapter>,
    timeout: Duration,
}

impl IdleTimeoutAdapter {
    pub fn new(inner: Box<dyn AgentAdapter>) -> Self {
        Self {
            inner,
            timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }

    #[cfg(test)]
    fn with_timeout(inner: Box<dyn AgentAdapter>, timeout: Duration) -> Self {
        Self { inner, timeout }
    }
}

#[async_trait]
impl AgentAdapter for IdleTimeoutAdapter {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn is_available(&self) -> bool {
        self.inner.is_available()
    }

    async fn run(&self, request: &AgentRequest) -> Result<AgentEventStream> {
        let inner_stream = self.inner.run(request).await?;
        Ok(with_idle_timeout(inner_stream, self.timeout))
    }
}

/// Wrap an event stream with a per-event idle timeout.
///
/// Each call to poll the stream is guarded by `timeout`. If the inner stream
/// produces no event within that window a fatal error is emitted and the
/// stream ends.
fn with_idle_timeout(inner: AgentEventStream, timeout: Duration) -> AgentEventStream {
    let stream = futures::stream::unfold((inner, false), move |(mut inner, done)| async move {
        if done {
            return None;
        }

        match tokio::time::timeout(timeout, inner.next()).await {
            // Inner stream yielded an event — pass it through.
            Ok(Some(event)) => Some((event, (inner, false))),
            // Inner stream ended normally.
            Ok(None) => None,
            // Idle timeout — emit a fatal error and stop.
            // Drop the inner stream immediately so that the underlying
            // subprocess (if any) is killed via `kill_on_drop` right now,
            // rather than waiting for the caller to poll or drop us.
            Err(_) => {
                warn!(
                    "Agent produced no output for {:?} — terminating stream",
                    timeout
                );
                drop(inner);
                Some((
                    Err(AirlockError::Agent(format!(
                        "Agent produced no output for {} minutes (idle timeout)",
                        timeout.as_secs() / 60
                    ))),
                    (Box::pin(futures::stream::empty()) as AgentEventStream, true),
                ))
            }
        }
    });

    Box::pin(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentEvent;
    use futures::stream;

    #[tokio::test]
    async fn test_passthrough_when_active() {
        let events: Vec<Result<AgentEvent>> = vec![
            Ok(AgentEvent::TextDelta { text: "hi".into() }),
            Ok(AgentEvent::Complete {
                session_id: None,
                usage: Default::default(),
            }),
        ];
        let inner: AgentEventStream = Box::pin(stream::iter(events));
        let mut wrapped = with_idle_timeout(inner, DEFAULT_IDLE_TIMEOUT);

        let first = wrapped.next().await.unwrap().unwrap();
        assert!(matches!(first, AgentEvent::TextDelta { .. }));

        let second = wrapped.next().await.unwrap().unwrap();
        assert!(matches!(second, AgentEvent::Complete { .. }));

        assert!(wrapped.next().await.is_none());
    }

    #[tokio::test]
    async fn test_timeout_on_stalled_stream() {
        let stalled: AgentEventStream = Box::pin(stream::pending());
        let mut wrapped = with_idle_timeout(stalled, Duration::from_millis(1));

        let result = wrapped.next().await.unwrap();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("idle timeout"));

        // Stream should be done after the error.
        assert!(wrapped.next().await.is_none());
    }

    /// End-to-end test through the [`IdleTimeoutAdapter`] decorator.
    #[tokio::test]
    async fn test_adapter_kills_stalled_inner() {
        struct StallAdapter;

        #[async_trait]
        impl AgentAdapter for StallAdapter {
            fn name(&self) -> &str {
                "Stall"
            }
            fn is_available(&self) -> bool {
                true
            }
            async fn run(&self, _request: &AgentRequest) -> Result<AgentEventStream> {
                Ok(Box::pin(stream::pending()))
            }
        }

        let adapter =
            IdleTimeoutAdapter::with_timeout(Box::new(StallAdapter), Duration::from_millis(1));
        let request = AgentRequest {
            prompt: "test".into(),
            ..Default::default()
        };

        let mut stream = adapter.run(&request).await.unwrap();
        let result = stream.next().await.unwrap();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("idle timeout"));
    }
}
