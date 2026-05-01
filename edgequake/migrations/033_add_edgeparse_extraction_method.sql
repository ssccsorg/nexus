DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.table_constraints
        WHERE constraint_name = 'pdf_records_extraction_method_check'
    ) THEN
        ALTER TABLE pdf_records
            DROP CONSTRAINT pdf_records_extraction_method_check;

        ALTER TABLE pdf_records
            ADD CONSTRAINT pdf_records_extraction_method_check
            CHECK (extraction_method IN ('text', 'vision', 'hybrid', 'edgeparse'));
    END IF;
END $$;
