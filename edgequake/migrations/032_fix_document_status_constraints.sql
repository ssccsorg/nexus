-- Migration: 032_fix_document_status_constraints
-- Description: Fix duplicate status check constraints on documents table
-- Phase: 1.2.0
-- Date: 2026-04-08
--
-- Context: Migration 017_add_processing_substates tried to drop 'valid_document_status'
-- but the original constraint was named 'documents_valid_status'. Both constraints
-- ended up coexisting, and 'partial_failure' is missing from both.
-- This migration:
--   1. Drops the old 'documents_valid_status' constraint (from migration 001)
--   2. Drops the newer 'valid_document_status' constraint (from migration 017)
--   3. Creates a single canonical constraint that includes ALL valid statuses

SET search_path = public;

-- Drop old constraint from migration 001 (was never dropped by migration 017)
DO $$
BEGIN
    ALTER TABLE documents DROP CONSTRAINT IF EXISTS documents_valid_status;
    RAISE NOTICE 'Dropped constraint documents_valid_status (if it existed)';
EXCEPTION WHEN undefined_object THEN
    RAISE NOTICE 'Constraint documents_valid_status did not exist, skipping';
END $$;

-- Drop the constraint added by migration 017
DO $$
BEGIN
    ALTER TABLE documents DROP CONSTRAINT IF EXISTS valid_document_status;
    RAISE NOTICE 'Dropped constraint valid_document_status (if it existed)';
EXCEPTION WHEN undefined_object THEN
    RAISE NOTICE 'Constraint valid_document_status did not exist, skipping';
END $$;

-- Create single canonical constraint including ALL status values
ALTER TABLE documents ADD CONSTRAINT documents_valid_status CHECK (
    status IN (
        'pending',          -- Uploaded, waiting for processing
        'processing',       -- Generic processing state (fallback)
        'chunking',         -- Text being split into chunks
        'extracting',       -- LLM extracting entities/relationships
        'embedding',        -- Generating vector embeddings
        'indexing',         -- Storing in graph/vector databases
        'completed',        -- Successfully processed
        'indexed',          -- Legacy: same as completed (kept for compatibility)
        'failed',           -- Processing failed permanently
        'partial_failure',  -- Some stages succeeded, some failed (e.g. entity embeddings)
        'cancelled'         -- User cancelled processing
    )
);

-- Success message
DO $$ BEGIN
    RAISE NOTICE 'Migration 032_fix_document_status_constraints completed successfully!';
    RAISE NOTICE 'Valid status values: pending, processing, chunking, extracting, embedding, indexing, completed, indexed, failed, partial_failure, cancelled';
END $$;
