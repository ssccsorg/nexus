Task logs

Actions: added workspace/upload PDF parser controls, fixed EdgeParse NUL-byte persistence handling, corrected lineage inference for non-Vision failures, updated parser docs, ran fmt/clippy/build/tests, and checked CI workflow gates.
Decisions: kept backend resolution explicit, sanitized extracted markdown before persistence, limited `vision_model` propagation to true Vision flows, and treated `bun test` failures as pre-existing/non-CI because repo CI runs Rust gates plus Playwright separately.
Next steps: none for this mission change; remaining frontend test-suite hygiene is a separate repo-wide cleanup if desired.
Lessons/insights: the user-facing bugs came from three independent causes: missing discoverability in the UI, unsafe raw EdgeParse output for PostgreSQL text storage, and lineage fallback logic inferring more than the data justified.
