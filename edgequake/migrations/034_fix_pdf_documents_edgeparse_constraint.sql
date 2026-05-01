-- Fix Mission 03 rollout bug:
-- Migration 033 targeted the wrong table/constraint (`pdf_records`), so existing
-- databases kept the original `pdf_documents.valid_extraction_method` check and
-- rejected `edgeparse` at write time.
--
-- This migration repairs the actual live constraint in an idempotent way.

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.tables
        WHERE table_schema = 'public'
          AND table_name = 'pdf_documents'
    ) THEN
        IF EXISTS (
            SELECT 1
            FROM information_schema.table_constraints
            WHERE table_schema = 'public'
              AND table_name = 'pdf_documents'
              AND constraint_name = 'valid_extraction_method'
        ) THEN
            ALTER TABLE pdf_documents
                DROP CONSTRAINT valid_extraction_method;
        END IF;

        ALTER TABLE pdf_documents
            ADD CONSTRAINT valid_extraction_method CHECK (
                extraction_method IS NULL OR
                extraction_method IN ('text', 'vision', 'hybrid', 'edgeparse')
            );
    END IF;
END $$;
