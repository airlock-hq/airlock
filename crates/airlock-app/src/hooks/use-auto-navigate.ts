import { useEffect, useRef } from 'react';
import { useNavigate, useMatch } from 'react-router-dom';
import { useAirlockEvent, AIRLOCK_EVENTS } from './use-airlock-events';
import type { RunCreatedEvent } from './use-airlock-events';
import { useRunDetail } from './use-daemon';

const ACTIVE_STATUSES = new Set(['running', 'pending', 'pending_review', 'awaiting_approval']);

export function useAutoNavigateToNewRun() {
  const navigate = useNavigate();

  const match = useMatch('/repos/:repoId/runs/:runId');
  const currentRunId = match?.params.runId ?? null;

  const { detail } = useRunDetail(currentRunId);

  // Track whether the current run was superseded, so we can navigate
  // immediately on the next RUN_CREATED without waiting for a refetch.
  const supersededRef = useRef(false);

  // Reset when the viewed run changes (manual navigation, etc.)
  useEffect(() => {
    supersededRef.current = false;
  }, [currentRunId]);

  useAirlockEvent<{ run_id: string }>(AIRLOCK_EVENTS.RUN_SUPERSEDED, (event) => {
    if (currentRunId && event.run_id === currentRunId) {
      supersededRef.current = true;
    }
  });

  useAirlockEvent<RunCreatedEvent>(AIRLOCK_EVENTS.RUN_CREATED, (event) => {
    // Don't navigate away from an active run (or one still loading),
    // unless it was just superseded by the incoming push.
    if (currentRunId && !supersededRef.current && (!detail || ACTIVE_STATUSES.has(detail.run.status))) {
      return;
    }

    navigate(`/repos/${event.repo_id}/runs/${event.run_id}`);
  });
}
