-- SPEC-007: Add materialized columns to vector tables for Tier 3 pre-filtering
-- Adds document_id, tenant_id, workspace_id TEXT columns and backfills from JSONB metadata.
-- @implements SPEC-007 R-T3-01, R-T3-04

-- ============================================================
-- PHASE 1: Add columns to all existing vector tables (default + per-workspace)
-- WHY: eq_default_vectors is created by application code (create_table()), not by
-- a prior migration. On fresh databases it may not exist yet.
-- ============================================================
DO $$
DECLARE
    tbl RECORD;
BEGIN
    FOR tbl IN
        SELECT tablename FROM pg_tables
        WHERE schemaname = 'public'
          AND tablename LIKE 'eq_%_vectors'
    LOOP
        EXECUTE format('ALTER TABLE public.%I ADD COLUMN IF NOT EXISTS document_id TEXT', tbl.tablename);
        EXECUTE format('ALTER TABLE public.%I ADD COLUMN IF NOT EXISTS tenant_id TEXT', tbl.tablename);
        EXECUTE format('ALTER TABLE public.%I ADD COLUMN IF NOT EXISTS workspace_id TEXT', tbl.tablename);
    END LOOP;
END $$;

-- ============================================================
-- PHASE 2: Backfill all vector tables from JSONB metadata (batched)
-- ============================================================
DO $$
DECLARE
    tbl RECORD;
    batch_size INT := 10000;
    updated INT;
BEGIN
    FOR tbl IN
        SELECT tablename FROM pg_tables
        WHERE schemaname = 'public'
          AND tablename LIKE 'eq_%_vectors'
    LOOP
        LOOP
            EXECUTE format(
                'UPDATE public.%I SET
                    document_id = COALESCE(metadata->>''document_id'', metadata->>''source_document_id''),
                    tenant_id = metadata->>''tenant_id'',
                    workspace_id = metadata->>''workspace_id''
                WHERE document_id IS NULL
                AND ctid IN (
                    SELECT ctid FROM public.%I WHERE document_id IS NULL LIMIT %s
                )',
                tbl.tablename, tbl.tablename, batch_size
            );
            GET DIAGNOSTICS updated = ROW_COUNT;
            EXIT WHEN updated < batch_size;
            PERFORM pg_sleep(0.05);
        END LOOP;
    END LOOP;
END $$;
