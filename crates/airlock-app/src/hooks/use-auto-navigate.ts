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

  useAirlockEvent<RunCreatedEvent>(AIRLOCK_EVENTS.RUN_CREATED, (event) => {
    // Don't navigate away from an active run (or one still loading)
    if (currentRunId && (!detail || ACTIVE_STATUSES.has(detail.run.status))) {
      return;
    }

    navigate(`/repos/${event.repo_id}/runs/${event.run_id}`);
  });
}
