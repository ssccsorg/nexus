# Problem Statement: Custom Entity Configuration from UI

**Issue**: [#85](https://github.com/raphaelmansuy/edgequake/issues/85)

---

## WHY

Every domain has its own vocabulary of important concepts. The default entity
types (PERSON, ORGANIZATION, LOCATION, EVENT, CONCEPT, TECHNOLOGY, PRODUCT,
DATE, DOCUMENT) work well for general-purpose knowledge extraction. But they
fail for specialized domains:

| Domain | Key Entity Types | Default Coverage |
|--------|-----------------|-----------------|
| Manufacturing | MACHINE, COMPONENT, DEFECT, MEASUREMENT, PROCESS | 0/5 |
| Healthcare | SYMPTOM, DRUG, DIAGNOSIS, PROCEDURE, PATIENT | 0/5 |
| Legal | CONTRACT, CLAUSE, PARTY, REGULATION, JURISDICTION | 0/5 |
| Research | PAPER, METHOD, DATASET, HYPOTHESIS, FINDING | 0/5 |
| Finance | FUND, SECURITY, RISK, REGULATION, COUNTERPARTY | 0/5 |

When the entity extractor is told to look for PERSON, ORGANIZATION, etc. in a
manufacturing maintenance log, it misses critical entities like MACHINE_A,
BEARING_42, or VIBRATION_ANOMALY. These domain-specific entities are the
exact knowledge the user needs to capture.

### The Impact

Without custom entity types:
1. **Low recall**: Domain-specific entities are not extracted
2. **Wrong typing**: A machine may be classified as PRODUCT instead of MACHINE
3. **Poor graph quality**: Missing entities = missing relationships = incomplete KG
4. **User frustration**: System doesn't understand the domain

### Current State

The backend already supports custom entity types:
- `SOTAExtractor::with_entity_types(Vec<String>)` ← builder method exists
- `LLMExtractor::with_entity_types(Vec<String>)` ← builder method exists
- Entity types are injected into the LLM extraction prompt as a type list

**But there is no way for users to configure this.** The types are hardcoded
at extractor initialization. No API field, no workspace setting, no UI control.

---

## Problem Decomposition

### P1: Storage — Where to persist custom entity types

Entity types must be stored per-workspace because different workspaces serve
different domains. Options:
- Workspace `metadata` JSONB field (existing column)
- New `entity_types` column on workspaces table
- Separate configuration table

### P2: API — How to set entity types during workspace creation

`CreateWorkspaceRequest` needs an `entity_types` field. The API must validate
the types and store them in workspace metadata.

### P3: Pipeline — How to thread entity types from workspace to extractor

When a document is uploaded, the pipeline must read entity types from the
workspace and configure the extractor accordingly.

### P4: UI — Entity type selection during workspace creation

The workspace creation dialog needs an entity type selector. Users must be able
to:
- Use a preset (General, Manufacturing, Healthcare, Legal, Research)
- Customize individual types (add/remove from the list)
- See what types are currently configured for the workspace

### P5: Backward Compatibility — What happens to existing workspaces

Existing workspaces have no `entity_types` in their metadata. The system must
fall back to default entity types when none are configured.

---

## Success Criteria

1. User can select entity types during workspace creation
2. Custom entity types are persisted in workspace metadata
3. Pipeline uses workspace-specific entity types for extraction
4. Default entity types are used when workspace has no custom config
5. Presets available for common domains (General, Manufacturing, etc.)
6. User can add custom entity types beyond presets
7. Existing workspaces continue to work with default types
