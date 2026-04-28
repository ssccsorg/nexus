# UX Spec — SPEC-0001: Tenant Workspace Quota Management

**Issue**: #133 | **ADR**: [004_adr.md](004_adr.md) | **Status**: Proposed

---

## Placement

Admin quota management lives as a **role-gated section** inside the existing Settings page (`app/(dashboard)/settings/page.tsx`). No new route — just a new `<Card>` block rendered only when `user.role === "admin"`.

This follows the existing Settings layout: `ScrollArea` → `max-w-4xl` → stacked `Card` sections.

---

## Wireframe: Admin Section in Settings

```
Settings
  Appearance | Graph | Query | Ingestion | Data
  ───────────────────────────────────────────────

  ... existing cards ...

  ┌─ Admin ──────────────────────────────────────┐
  │                                              │
  │  Server Default                              │
  │  Max workspaces for new tenants              │
  │  ┌──────┐                                    │
  │  │  200 │  [Save]          text-xs, p-3      │
  │  └──────┘                                    │
  │  Current: 100 (compile-time)                 │
  │                                              │
  │  ──────────────────────────────────          │
  │                                              │
  │  Tenant Quotas                               │
  │                                              │
  │  Acme Corp    Pro    42/500   [Edit]         │
  │  Beta Inc     Free    3/10   [Edit]          │
  │  Gamma LLC    Ent   120/500   [Edit]         │
  │                                              │
  │  text-[11px] rows, inline [Edit] button      │
  │  No full table — just compact label rows     │
  └──────────────────────────────────────────────┘
```

---

## Edit Quota Dialog

Minimal `Dialog` (shadcn). No progress bar — use a compact inline display.

```
  ┌─ Edit Quota ─────────────────────┐
  │                                  │
  │  Acme Corp · Pro                 │
  │                                  │
  │  Current usage   42 workspaces   │
  │  Current max    500              │
  │                                  │
  │  New max  ┌──────┐              │
  │           │  750 │              │
  │           └──────┘              │
  │  Min: 42 (in use) · Max: 10000  │
  │                                  │
  │   [Cancel]       [Update Quota]  │
  └──────────────────────────────────┘
```

---

## Components

| Component           | File                                          | Pattern       |
| ------------------- | --------------------------------------------- | ------------- |
| `AdminQuotaSection` | `components/settings/admin-quota-section.tsx` | Card, p-3     |
| `EditQuotaDialog`   | `components/settings/edit-quota-dialog.tsx`   | Dialog, Input |

---

## Store / API

```typescript
// use-tenant-store.ts additions
updateTenantQuota(tenantId: string, max: number): Promise<void>
updateServerDefault(defaultMax: number): Promise<void>
```

Backend endpoints (from ADR):

- `PATCH /api/v1/admin/tenants/:id/quota`
- `PATCH /api/v1/admin/config/defaults`

---

## Validation Rules

| #   | Rule                                                         |
| --- | ------------------------------------------------------------ |
| 1   | Input `min` = current workspace count (can't go below usage) |
| 2   | Input `max` = 10000 (sanity limit)                           |
| 3   | Non-admin users: section not rendered (not hidden — absent)  |
| 4   | Success toast: "Quota updated: 500 → 750 for Acme Corp"      |

---

## Design Alignment

- Uses existing `Card` / `Dialog` / `Input` / `Button` from shadcn
- Row density: `text-[11px]` for tenant rows, `text-xs` for labels
- No new colors — uses neutral `text-muted-foreground` for secondary text
- Inline `[Edit]` as `Button variant="ghost" size="sm"` (h-6 px-2)
