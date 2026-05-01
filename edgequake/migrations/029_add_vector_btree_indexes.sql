-- SPEC-007: Add B-tree indexes on materialized columns for Tier 3 filtering
-- @implements SPEC-007 R-T3-02

-- All vector tables (default + per-workspace, discovered dynamically)
-- WHY: eq_default_vectors is created by application code (create_table()), not by
-- a prior migration. On fresh databases it may not exist yet.
DO $$
DECLARE
    tbl RECORD;
BEGIN
    FOR tbl IN
        SELECT tablename FROM pg_tables
        WHERE schemaname = 'public'
          AND tablename LIKE 'eq_%_vectors'
    LOOP
        EXECUTE format(
            'CREATE INDEX IF NOT EXISTS %s_doc_id_idx ON public.%I (document_id) WHERE document_id IS NOT NULL',
            tbl.tablename, tbl.tablename
        );
        EXECUTE format(
            'CREATE INDEX IF NOT EXISTS %s_tenant_ws_idx ON public.%I (tenant_id, workspace_id) WHERE tenant_id IS NOT NULL',
            tbl.tablename, tbl.tablename
        );
    END LOOP;
END $$;
