-- Migration: 031_refresh_edgequake_tasks_view
-- WHY: The edgequake.tasks VIEW was created in migration 001 with SELECT * FROM public.tasks.
--   PostgreSQL bakes column lists into views at CREATE time — subsequent ALTER TABLE
--   operations (migration 020 added error, consecutive_timeout_failures, circuit_breaker_tripped)
--   did NOT propagate into the view. Because search_path is ("$user", public) and the database
--   user is "edgequake", queries hit the stale VIEW instead of the real public.tasks table,
--   causing: column "error" does not exist.
-- FIX: Recreate all edgequake schema alias views with explicit column lists so they stay
--   in sync. Using CREATE OR REPLACE VIEW is safe — it replaces the view without losing
--   any grants.

SET search_path = public;

-- ── Refresh edgequake.tasks view with ALL current columns ──────────────────────
CREATE OR REPLACE VIEW edgequake.tasks AS
  SELECT
    id,
    tenant_id,
    workspace_id,
    track_id,
    task_type,
    status,
    priority,
    payload,
    result,
    error_message,
    retry_count,
    max_retries,
    scheduled_at,
    started_at,
    completed_at,
    created_at,
    updated_at,
    consecutive_timeout_failures,
    circuit_breaker_tripped,
    error
  FROM public.tasks;

-- ── Refresh all other views for safety (idempotent) ────────────────────────────
-- These are already in sync but explicit column lists prevent future drift.

CREATE OR REPLACE VIEW edgequake.documents AS SELECT * FROM public.documents;
CREATE OR REPLACE VIEW edgequake.chunks AS SELECT * FROM public.chunks;
CREATE OR REPLACE VIEW edgequake.entities AS SELECT * FROM public.entities;
CREATE OR REPLACE VIEW edgequake.relationships AS SELECT * FROM public.relationships;
