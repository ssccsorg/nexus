-- SPEC-007: Add GIN index on vector metadata JSONB column
-- Tier 2: Enables SQL-level metadata pre-filtering with GIN index acceleration
-- @implements SPEC-007 R-T2-02

-- All vector tables (including default and per-workspace, discovered dynamically)
-- WHY: eq_default_vectors is created by application code (create_table()), not by
-- a prior migration. On fresh databases the table may not exist yet, so we must
-- discover it dynamically rather than referencing it directly.
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
            'CREATE INDEX IF NOT EXISTS %s_metadata_idx ON public.%I USING GIN (metadata jsonb_path_ops)',
            tbl.tablename, tbl.tablename
        );
    END LOOP;
END $$;
