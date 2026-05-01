# SPEC-0001: Tenant Workspace Limits — Problem Statement

**Issue**: [#133](https://github.com/raphaelmansuy/edgequake/issues/133)
**Author**: EdgeQuake Team
**Status**: Draft
**Created**: 2026-04-02

---

## WHY — The Problem

Multi-tenant SaaS platforms require **runtime-configurable tenant quotas**.
Today, EdgeQuake hard-codes workspace limits per `TenantPlan` tier:

```text
Free       →  10 workspaces
Basic      → 100 workspaces
Pro        → 500 workspaces
Enterprise → 500 workspaces
```

These limits are burned into `TenantPlan::default_max_workspaces()` at compile
time. There is **no API** to:

1. Change the **default** limit applied to newly-created tenants.
2. **Increase** the limit for an existing tenant at runtime.
3. Prevent **decrease below current usage** (which would orphan workspaces).

### Impact

| Stakeholder       | Pain Point                                         |
| ----------------- | -------------------------------------------------- |
| Platform operator | Cannot adjust quotas without redeployment          |
| Sales team        | Cannot grant a customer more capacity in real-time |
| Existing tenant   | No self-service or API-driven quota increase       |
| System integrity  | Decreasing below usage creates undefined state     |

### Root Cause

`max_workspaces` is set once during `Tenant::new()` from a plan default and
persisted in the `metadata` JSONB column. No update path exists through the
API layer.

---

## Scope Boundaries

### In Scope

- API endpoint to **increase** a tenant's `max_workspaces`.
- API endpoint to set the **server-wide default** for new tenants.
- Validation: reject decrease below current workspace count.
- Audit log entry for every quota change.

### Out of Scope

- Self-service tenant quota changes (requires billing integration).
- Per-workspace sub-quotas (documents, users).
- UI for quota management (API-first; UI can be added later).
