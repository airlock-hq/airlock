//! Intent-related IPC methods.

use super::error::IpcError;
use super::types::DaemonUpdateIntentDescriptionResult;
use super::IpcClient;
use crate::{ApproveIntentResult, IntentDiffResult, IntentTourResult, RejectIntentResult};

impl IpcClient {
    /// Get diff hunks for an intent
    pub async fn get_intent_diff(&self, intent_id: &str) -> Result<IntentDiffResult, IpcError> {
        let result = self
            .send_request(
                "get_intent_diff",
                serde_json::json!({ "intent_id": intent_id }),
            )
            .await?;

        Ok(serde_json::from_value(result)?)
    }

    /// Get guided tour for an intent
    pub async fn get_intent_tour(&self, intent_id: &str) -> Result<IntentTourResult, IpcError> {
        let result = self
            .send_request(
                "get_intent_tour",
                serde_json::json!({ "intent_id": intent_id }),
            )
            .await?;

        Ok(serde_json::from_value(result)?)
    }

    /// Update the description of an intent
    pub async fn update_intent_description(
        &self,
        intent_id: &str,
        description: &str,
    ) -> Result<String, IpcError> {
        let result = self
            .send_request(
                "update_intent_description",
                serde_json::json!({ "intent_id": intent_id, "description": description }),
            )
            .await?;

        let daemon_result: DaemonUpdateIntentDescriptionResult = serde_json::from_value(result)?;
        Ok(daemon_result.description)
    }

    /// Approve an intent (mark as ready for forwarding)
    pub async fn approve_intent(&self, intent_id: &str) -> Result<ApproveIntentResult, IpcError> {
        let result = self
            .send_request(
                "approve_intent",
                serde_json::json!({ "intent_id": intent_id }),
            )
            .await?;

        Ok(serde_json::from_value(result)?)
    }

    /// Reject an intent with an optional reason
    pub async fn reject_intent(
        &self,
        intent_id: &str,
        reason: Option<&str>,
    ) -> Result<RejectIntentResult, IpcError> {
        let result = self
            .send_request(
                "reject_intent",
                serde_json::json!({ "intent_id": intent_id, "reason": reason }),
            )
            .await?;

        Ok(serde_json::from_value(result)?)
    }
}
