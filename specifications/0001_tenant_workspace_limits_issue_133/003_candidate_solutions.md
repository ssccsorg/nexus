# SPEC-0001: Tenant Workspace Limits — Candidate Solutions

**Issue**: [#133](https://github.com/raphaelmansuy/edgequake/issues/133)

---

## Candidate A: Thin API Layer (Recommended)

Add two new endpoints that directly update the `metadata` JSONB.

```
PATCH /api/v1/admin/tenants/:tenant_id/quota
  Body: { "max_workspaces": 750 }
  Auth: Admin role required
  Rules:
    - new_value >= current_workspace_count  (monotonic increase)
    - new_value >  0
  Response: 200 { tenant_id, max_workspaces, previous_value }

PATCH /api/v1/admin/config/defaults
  Body: { "default_max_workspaces": 200 }
  Auth: Admin role required
  Rules:
    - value > 0
    - value <= 10000  (sanity cap)
    - NOT retroactive (only affects NEW tenants)
  Response: 200 { default_max_workspaces }
```

**Default override storage**: Environment variable `EDGEQUAKE_DEFAULT_MAX_WORKSPACES`
with runtime reload through the admin config endpoint. Value stored in
`server_config` table.

### Pros

- Minimal code change (~200 LOC across 4 files)
- No schema migration (JSONB update)
- Respects existing validation in `check_workspace_limit`
- Clear audit trail via `updated_at` timestamp

### Cons

- No history of quota changes (only latest value)
- Env var override requires restart unless stored in DB

### Risks

| Risk                                 | Mitigation                                    |
| ------------------------------------ | --------------------------------------------- |
| Race condition on concurrent updates | PostgreSQL row-level locking via `FOR UPDATE` |
| Decrease below usage                 | Server-side validation before update          |
| Missing audit                        | Log every quota change with `tracing::info!`  |

---

## Candidate B: Quota Event Log

Same as A, but add an immutable `quota_events` table:

```sql
CREATE TABLE quota_events (
  event_id    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id   UUID NOT NULL REFERENCES tenants(tenant_id),
  field       TEXT NOT NULL,  -- 'max_workspaces'
  old_value   INTEGER NOT NULL,
  new_value   INTEGER NOT NULL,
  changed_by  UUID,   -- user who made the change
  reason      TEXT,   -- optional justification
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

### Pros

- Full audit history of every change
- Supports compliance requirements
- Can reconstruct quota timeline

### Cons

- Requires DB migration
- Adds complexity for a feature request that may be optional
- Over-engineering for current scale

---

## Candidate C: Plan-Based Tiers with Admin Override

Keep plan defaults as the primary source but allow per-tenant overrides
stored in `settings` JSONB:

```json
// settings JSONB
{
  "quota_override": {
    "max_workspaces": 750,
    "set_by": "admin-uuid",
    "set_at": "2026-04-01T12:00:00Z"
  }
}
```

Resolution order:

1. `settings.quota_override.max_workspaces` (if present)
2. `metadata.max_workspaces` (from plan)

### Pros

- Separates plan defaults from manual overrides
- Can revert to plan default by deleting override

### Cons

- Two locations for the same value (confusing)
- Resolution logic adds indirection
- Violates SRP (settings stores both config and quotas)

---

## Decision Matrix

| Criterion             | Weight | A: Thin API | B: Event Log | C: Plan Override |
| --------------------- | ------ | ----------- | ------------ | ---------------- |
| Implementation effort | 30%    | 5           | 3            | 4                |
| Correctness guarantee | 25%    | 5           | 5            | 4                |
| Audit/compliance      | 20%    | 3           | 5            | 3                |
| Simplicity/DRY        | 15%    | 5           | 3            | 2                |
| Future extensibility  | 10%    | 4           | 5            | 3                |
| **Weighted Score**    |        | **4.55**    | **4.00**     | **3.35**         |

## Recommendation

**Candidate A** wins. If audit requirements emerge later, the event log
(Candidate B) can be added as a follow-up without breaking changes.
