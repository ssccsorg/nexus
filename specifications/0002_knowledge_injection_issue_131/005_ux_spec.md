# UX Spec — SPEC-0002: Knowledge Injection

**Issue**: #131 | **ADR**: [004_adr.md](004_adr.md) | **Status**: Proposed

---

## Placement — Dedicated `/knowledge` Page

Knowledge injection gets its own route at `app/(dashboard)/knowledge/page.tsx` and a new sidebar entry between "Documents" and "Pipeline".

```
Sidebar
  Home         /
  Graph        /graph
  Documents    /documents
  Knowledge    /knowledge    ← NEW (BookOpen icon)
  Pipeline     /pipeline
  Query        /query
  ...
```

**Why a dedicated page instead of a card on `/workspace`?**

- Knowledge injection involves two distinct input modes (text + file upload)
- File upload requires progress tracking, status display, and error handling
- Knowledge entries accumulate over time — need a list view
- SOLID: workspace page handles LLM/embedding config (SRP); knowledge is a separate concern

---

## Architecture — DRY via Existing Components

The `/knowledge` page reuses the mature document upload infrastructure. No reimplementation of dropzone, progress tracking, or file validation.

### Reused Components (no changes needed)

| Existing Component    | File                                            | Reused For                   |
| --------------------- | ----------------------------------------------- | ---------------------------- |
| `DocumentDropzone`    | `components/documents/document-dropzone.tsx`    | File drag-and-drop zone      |
| `useDocumentDropzone` | `hooks/use-document-dropzone.ts`                | Dropzone config + validation |
| `UploadProgressList`  | `components/documents/upload-progress-list.tsx` | Upload progress display      |
| `PaginationControls`  | `components/documents/pagination-controls.tsx`  | List pagination              |
| `EmptyState`          | `components/shared/empty-state.tsx`             | Empty knowledge state        |
| `StatusBadge`         | `components/documents/status-badge.tsx`         | Entry processing status      |
| `DocumentSearchBar`   | `components/documents/document-search-bar.tsx`  | Search knowledge entries     |

### New Components (knowledge-specific)

| New Component         | File                                             | Responsibility                                |
| --------------------- | ------------------------------------------------ | --------------------------------------------- |
| `KnowledgePage`       | `app/(dashboard)/knowledge/page.tsx`             | Page layout + orchestration                   |
| `KnowledgeHeader`     | `components/knowledge/knowledge-header.tsx`      | Title + stats + actions                       |
| `KnowledgeEntryList`  | `components/knowledge/knowledge-entry-list.tsx`  | Table of injection entries                    |
| `KnowledgeEntryRow`   | `components/knowledge/knowledge-entry-row.tsx`   | Single entry row                              |
| `KnowledgeTextEditor` | `components/knowledge/knowledge-text-editor.tsx` | Inline text definition input                  |
| `KnowledgeAddDialog`  | `components/knowledge/knowledge-add-dialog.tsx`  | Add dialog (tabs: Text/File)                  |
| `useInjection`        | `hooks/use-injection.ts`                         | CRUD hook for injection API                   |
| `useInjectionUpload`  | `hooks/use-injection-upload.ts`                  | File upload (extends `useFileUpload` pattern) |

### Dependency Graph (SOLID)

```
KnowledgePage (orchestration — no business logic)
  ├── KnowledgeHeader (display)
  ├── KnowledgeAddDialog
  │     ├── KnowledgeTextEditor (textarea + char counter)
  │     └── DocumentDropzone ← REUSED (DRY)
  │           └── useDocumentDropzone ← REUSED (DRY)
  ├── UploadProgressList ← REUSED (DRY)
  ├── DocumentSearchBar ← REUSED (DRY)
  ├── KnowledgeEntryList
  │     └── KnowledgeEntryRow
  │           └── StatusBadge ← REUSED (DRY)
  ├── PaginationControls ← REUSED (DRY)
  └── EmptyState ← REUSED (DRY)

Hooks:
  useInjection (GET/DELETE — react-query)
  useInjectionUpload (wraps useFileUpload pattern for injection endpoint)
```

---

## Wireframe: Knowledge Page

### With Entries

```
  Knowledge                                    [+ Add Knowledge]
  3 entries · 42 entities extracted
  ─────────────────────────────────────────────────────────────

  ┌ Search ─────────────────────────────────────────────────┐
  │ 🔍 Search knowledge entries...                          │
  └─────────────────────────────────────────────────────────┘

  ┌─────────────────────────────────────────────────────────┐
  │  Name            Type   Status     Entities  Updated    │
  │  ─────────────────────────────────────────────────────  │
  │  Domain Glossary  Text  Completed     15     2h ago     │
  │  Industry Terms   File  Completed     27     1d ago     │
  │  Acronyms v2      Text  Processing    --     just now   │
  └─────────────────────────────────────────────────────────┘

  Rows: text-xs, compact p-2. Click row → expand inline preview.
  Each row has [⋮] menu: View · Edit · Delete
```

### Empty State

```
  Knowledge
  ─────────────────────────────────────────────────────────────

  ┌─────────────────────────────────────────────────────────┐
  │                                                         │
  │     📖                                                  │
  │     No knowledge entries yet                            │
  │     Add domain definitions, glossaries, or              │
  │     reference documents to enrich your graph.           │
  │                                                         │
  │     [+ Add Knowledge]                                   │
  │                                                         │
  └─────────────────────────────────────────────────────────┘

  Uses EmptyState component (components/shared/empty-state.tsx)
```

---

## Add Knowledge Dialog

Triggered by `[+ Add Knowledge]` button. Uses `Dialog` with `Tabs` for input mode.

### Text Tab

```
  ┌─ Add Knowledge ─────────────────────────────┐
  │                                              │
  │  Name  ┌──────────────────────────────────┐  │
  │        │ Domain Glossary                  │  │
  │        └──────────────────────────────────┘  │
  │                                              │
  │  [Text] [File]                               │
  │                                              │
  │  ┌────────────────────────────────────────┐  │
  │  │ OEE = Overall Equipment Effectiveness │  │
  │  │ NLP = Natural Language Processing     │  │
  │  │ MTBF = Mean Time Between Failures     │  │
  │  │                                       │  │
  │  │                                       │  │
  │  └────────────────────────────────────────┘  │
  │  font-mono text-xs, min-h-[200px]            │
  │                                              │
  │  2,340 / 100,000        text-[10px] muted    │
  │                                              │
  │   [Cancel]              [Save & Process]     │
  └──────────────────────────────────────────────┘
```

### File Tab

```
  ┌─ Add Knowledge ─────────────────────────────┐
  │                                              │
  │  Name  ┌──────────────────────────────────┐  │
  │        │ Industry Reference               │  │
  │        └──────────────────────────────────┘  │
  │                                              │
  │  [Text] [File]                               │
  │                                              │
  │  ┌─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┐  │
  │    Drag & drop or click to upload            │
  │    PDF, MD, TXT, CSV (max 10 MB)             │
  │  └─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘  │
  │                                              │
  │  Uses DocumentDropzone (REUSED)              │
  │                                              │
  │   [Cancel]              [Upload & Process]   │
  └──────────────────────────────────────────────┘
```

### File Selected

```
  ┌─ Add Knowledge ─────────────────────────────┐
  │                                              │
  │  Name  ┌──────────────────────────────────┐  │
  │        │ Industry Reference               │  │
  │        └──────────────────────────────────┘  │
  │                                              │
  │  [Text] [File]                               │
  │                                              │
  │  ┌────────────────────────────────────────┐  │
  │  │  📄 industry_terms.pdf    1.2 MB  [✕] │  │
  │  └────────────────────────────────────────┘  │
  │                                              │
  │   [Cancel]              [Upload & Process]   │
  └──────────────────────────────────────────────┘
```

---

## Entry Detail (inline expand or separate view)

Click a row to expand an inline detail panel:

```
  │  Domain Glossary  Text  Completed  15  2h ago        │
  │  ┌──────────────────────────────────────────────┐    │
  │  │ OEE = Overall Equipment Effectiveness        │    │
  │  │ NLP = Natural Language Processing            │    │
  │  │ MTBF = Mean Time Between Failures            │    │
  │  │ ...                                          │    │
  │  └──────────────────────────────────────────────┘    │
  │  15 entities extracted · version 3 · source: text    │
  │  [Edit]  [Delete]                                    │
```

---

## Hook: useInjection

```typescript
function useInjection(workspaceId: string) {
  // List all injection entries for this workspace
  const list = useQuery(["injections", workspaceId], () =>
    getInjections(workspaceId),
  );

  // Get single injection detail
  const get = (id: string) =>
    useQuery(["injection", id], () => getInjection(workspaceId, id));

  // Create injection from text
  const createText = useMutation((data: { name: string; content: string }) =>
    putInjection(workspaceId, data),
  );

  // Create injection from file upload
  const createFile = useMutation((data: { name: string; file: File }) =>
    putInjectionFile(workspaceId, data),
  );

  // Delete injection
  const remove = useMutation((id: string) => deleteInjection(workspaceId, id));

  return { list, get, createText, createFile, remove };
}
```

### useInjectionUpload

Follows the same pattern as `useFileUpload` but targets the injection endpoint. Reuses `useDocumentDropzone` for validation (file type/size checks).

```typescript
function useInjectionUpload(workspaceId: string) {
  // Delegates to same dropzone validation logic as document upload
  // but POSTs to /api/v1/workspaces/:id/injection/file
  // Reuses UploadProgressList for progress display
}
```

---

## Backend Endpoints (from ADR + extensions)

| Method   | Path                                     | Body                  | Purpose              |
| -------- | ---------------------------------------- | --------------------- | -------------------- |
| `GET`    | `/api/v1/workspaces/:id/injections`      | —                     | List all entries     |
| `GET`    | `/api/v1/workspaces/:id/injections/:iid` | —                     | Get single entry     |
| `PUT`    | `/api/v1/workspaces/:id/injection`       | `{ name, content }`   | Create/update (text) |
| `PUT`    | `/api/v1/workspaces/:id/injection/file`  | `multipart/form-data` | Create/update (file) |
| `DELETE` | `/api/v1/workspaces/:id/injections/:iid` | —                     | Delete entry         |

File upload flow: backend extracts text from the uploaded file (PDF → markdown via `edgequake-pdf2md`, or reads `.md`/`.txt`/`.csv` directly), then processes through the standard injection pipeline.

---

## Sidebar Update

```typescript
// sidebar.tsx — add between Documents and Pipeline
{ href: '/knowledge', icon: BookOpen, labelKey: 'nav.knowledge' },
```

Import `BookOpen` from `lucide-react`.

---

## Validation Rules

| #   | Rule                                                                     |
| --- | ------------------------------------------------------------------------ |
| 1   | Textarea max length: 100,000 chars. Live char counter.                   |
| 2   | File upload max size: 10 MB. Accepted: `.pdf`, `.md`, `.txt`, `.csv`.    |
| 3   | File validation reuses `useDocumentDropzone` (DRY — same checks).        |
| 4   | Save returns `202 Accepted` — show processing status via `StatusBadge`.  |
| 5   | Delete shows confirmation dialog (reuse `ClearDocumentsDialog` pattern). |
| 6   | Success toast: "15 entities extracted from glossary."                    |
| 7   | Name field required, max 100 chars.                                      |
| 8   | Each entry is independent — no "replaces previous" constraint.           |

---

## Design Alignment

- Page layout follows `/documents` structure: header → toolbar → list → pagination
- `DocumentDropzone` reused as-is — same drag/drop, same file validation, same visual style
- `UploadProgressList` reused as-is for file upload progress tracking
- `DocumentSearchBar` reused as-is for search
- `PaginationControls` reused as-is
- `EmptyState` reused with `BookOpen` icon and knowledge-specific copy
- `StatusBadge` reused for entry processing status
- Entry rows: `text-xs`, compact `p-2`, same density as document table rows
- Dialog: `Tabs` with `text-xs` labels — "Text" / "File"
- Textarea: `font-mono text-xs`, char counter `text-[10px] text-muted-foreground`
- No new design tokens or colors — uses existing palette throughout
