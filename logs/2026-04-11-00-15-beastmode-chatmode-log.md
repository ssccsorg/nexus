Task logs

Actions: traced the EdgeParse failure to a PostgreSQL check constraint mismatch, added a corrective migration for `pdf_documents.valid_extraction_method`, applied the fix to the live local database, and reran fmt, clippy, and workspace Rust tests.
Decisions: fixed the root cause at the schema layer instead of adding fallback logic, treated the screenshot as evidence of successful EdgeParse execution followed by DB rejection, and kept the repair idempotent so startup migrations remain safe.
Next steps: retry the failed EdgeParse upload or reprocess the failed PDF; future server startups will also pick up migration 034 automatically.
Lessons/insights: the system was not choosing Vision at extraction time; EdgeParse completed correctly, but a broken rollout migration left the database enforcing the pre-EdgeParse constraint, which made the failure look like parser instability.
