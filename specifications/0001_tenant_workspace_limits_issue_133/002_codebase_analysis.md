# SPEC-0001: Tenant Workspace Limits — Codebase Analysis

**Issue**: [#133](https://github.com/raphaelmansuy/edgequake/issues/133)

---

## Current Architecture

```
+--------------------------------------------------------------------+
|                        REQUEST FLOW                                 |
|                                                                     |
|  Client                                                             |
|    |                                                                |
|    v                                                                |
|  POST /api/v1/tenants/:id/workspaces                               |
|    |                                                                |
|    v                                                                |
|  +----------------------------+    +-----------------------------+  |
|  |  edgequake-api             |    |  edgequake-auth             |  |
|  |  handlers/workspace.rs     +--->+  tenant.rs                  |  |
|  |  (route handler)           |    |  check_workspace_limit()    |  |
|  +----------------------------+    +-----------------------------+  |
|                                          |                          |
|                                          v                          |
|                                    +-----------------------------+  |
|                                    |  edgequake-core              |  |
|                                    |  workspace_service.rs        |  |
|                                    |  WorkspaceService trait      |  |
|                                    +-----------------------------+  |
|                                          |                          |
|                                          v                          |
|                                    +-----------------------------+  |
|                                    |  workspace_service_impl.rs   |  |
|                                    |  PostgreSQL-backed impl      |  |
|                                    |  (tenants table + metadata)  |  |
|                                    +-----------------------------+  |
+--------------------------------------------------------------------+
```

## Key Files & Touchpoints

| File                                              | Role                                                                              | Impact                                     |
| ------------------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------ |
| `edgequake-core/src/types/multitenancy/tenant.rs` | `Tenant` struct with `max_workspaces: usize`                                      | **Read**: currently set from plan defaults |
| `edgequake-auth/src/tenant.rs`                    | `TenantPlan::default_max_workspaces()` + `TenantService::check_workspace_limit()` | **Validation**: enforces limit             |
| `edgequake-core/src/workspace_service.rs`         | `WorkspaceService` trait: `update_tenant()`                                       | **Write path** for persistence             |
| `edgequake-core/src/workspace_service_impl.rs`    | PostgreSQL impl: `max_workspaces` stored in `metadata` JSONB                      | **Schema**: no migration needed            |
| `edgequake-api/src/routes.rs`                     | Route registration                                                                | **New routes** to add                      |
| `edgequake-api/src/handlers/`                     | Request handlers                                                                  | **New handler** for quota update           |

## Data Model — Current

The `tenants` table stores:

```sql
CREATE TABLE tenants (
  tenant_id   UUID PRIMARY KEY,
  name        TEXT NOT NULL,
  slug        TEXT NOT NULL UNIQUE,
  is_active   BOOLEAN NOT NULL DEFAULT TRUE,
  metadata    JSONB NOT NULL DEFAULT '{}',   -- plan, max_workspaces, max_users
  settings    JSONB NOT NULL DEFAULT '{}',
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

`max_workspaces` lives inside `metadata`:

```json
{
  "plan": "pro",
  "max_workspaces": 500,
  "max_users": 100,
  "description": "..."
}
```

## Existing Validation Flow

```rust
// edgequake-auth/src/tenant.rs
pub fn check_workspace_limit(
    &self,
    tenant: &Tenant,
    current_count: u32,
) -> Result<(), AuthError> {
    if current_count >= tenant.max_workspaces {
        return Err(AuthError::TenantLimitExceeded { ... });
    }
    Ok(())
}
```

This validation is correct and **will work unchanged** with updated limits.

## Server-Wide Default

Currently, defaults are compiled constants per `TenantPlan`. A server-wide
runtime override requires a new storage location:

- Option A: environment variable `EDGEQUAKE_DEFAULT_MAX_WORKSPACES`
- Option B: row in a `server_config` table
- Option C: special tenant with `slug = "__system__"`
