-- Migration: 035_harden_task_compatibility_defaults
-- Description: Normalize legacy task inserts and enforce safe tenant/workspace defaults
-- Date: 2026-04-19
-- WHY: Older scripts, tests, or queued work can still use pre-refactor task_type/status
-- values or omit tenant/workspace IDs. Normalize them at the database boundary so
-- inserts remain deterministic and safely isolated instead of failing at runtime.

SET search_path = public;

-- Create a deterministic sentinel tenant/workspace used for legacy or context-less rows.
INSERT INTO tenants (tenant_id, name, slug, metadata)
VALUES (
    '00000000-0000-0000-0000-000000000000'::UUID,
    'System Default Tenant',
    'system-default-tenant',
    '{"system": true, "purpose": "legacy-task-compat"}'::JSONB
)
ON CONFLICT (tenant_id) DO NOTHING;

INSERT INTO workspaces (workspace_id, tenant_id, name, slug, metadata)
VALUES (
    '00000000-0000-0000-0000-000000000000'::UUID,
    '00000000-0000-0000-0000-000000000000'::UUID,
    'System Default Workspace',
    'system-default-workspace',
    '{"system": true, "purpose": "legacy-task-compat"}'::JSONB
)
ON CONFLICT (workspace_id) DO NOTHING;

-- Route any context-less tasks into a deterministic sentinel tenant/workspace.
ALTER TABLE tasks
ALTER COLUMN tenant_id SET DEFAULT '00000000-0000-0000-0000-000000000000'::UUID,
ALTER COLUMN workspace_id SET DEFAULT '00000000-0000-0000-0000-000000000000'::UUID;

UPDATE tasks
SET tenant_id = COALESCE(tenant_id, '00000000-0000-0000-0000-000000000000'::UUID),
    workspace_id = COALESCE(workspace_id, '00000000-0000-0000-0000-000000000000'::UUID)
WHERE tenant_id IS NULL OR workspace_id IS NULL;

CREATE OR REPLACE FUNCTION normalize_edgequake_task_legacy_fields()
RETURNS TRIGGER AS $$
BEGIN
    -- Safe isolation defaults for older insert paths.
    IF NEW.tenant_id IS NULL THEN
        NEW.tenant_id := '00000000-0000-0000-0000-000000000000'::UUID;
    END IF;

    IF NEW.workspace_id IS NULL THEN
        NEW.workspace_id := '00000000-0000-0000-0000-000000000000'::UUID;
    END IF;

    -- Normalize legacy task types to the canonical enum used by the runtime.
    IF NEW.task_type = 'document_ingestion' THEN
        NEW.task_type := 'scan';
    ELSIF NEW.task_type = 'embedding_generation' THEN
        NEW.task_type := 'insert';
    END IF;

    -- Normalize legacy lifecycle values before constraint validation.
    IF NEW.status = 'running' THEN
        NEW.status := 'processing';
    ELSIF NEW.status = 'completed' THEN
        NEW.status := 'indexed';
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS normalize_edgequake_task_legacy_fields_trigger ON tasks;
CREATE TRIGGER normalize_edgequake_task_legacy_fields_trigger
BEFORE INSERT OR UPDATE ON tasks
FOR EACH ROW
EXECUTE FUNCTION normalize_edgequake_task_legacy_fields();

DO $$ BEGIN
    RAISE NOTICE 'Migration 035 completed: task legacy compatibility + safe defaults enabled';
END $$;
