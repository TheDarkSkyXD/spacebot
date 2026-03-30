# Spacebot Code Graph Memory System
## Product Requirements Document — v2.0

**Version:** 2.0 | **Status:** Updated Draft | **Last Updated:** 2026-03-29
**Supersedes:** v1.0 (2026-03-23)

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [What Changed in v2.0](#2-what-changed-in-v20)
3. [Problem Statement](#3-problem-statement)
4. [Goals & Non-Goals](#4-goals--non-goals)
5. [Architecture Overview](#5-architecture-overview)
6. [Reference Implementation — GitNexus](#6-reference-implementation--gitnexus)
7. [GitNexus File & Folder Structure](#7-gitnexus-file--folder-structure)
8. [Centralized Project Memory Layer](#8-centralized-project-memory-layer)
9. [Automatic Stale Memory Removal](#9-automatic-stale-memory-removal)
10. [Project Lifecycle — Add, Remove, Cascade Delete](#10-project-lifecycle--add-remove-cascade-delete)
11. [Graph Storage — LadybugDB](#11-graph-storage--ladybugdb)
12. [Graph Schema](#12-graph-schema)
13. [Bidirectional Cortex ↔ Code Graph Communication](#13-bidirectional-cortex--code-graph-communication)
14. [The 10-Phase Indexing Pipeline](#14-the-10-phase-indexing-pipeline)
15. [Real-Time File Watching & Incremental Updates](#15-real-time-file-watching--incremental-updates)
16. [Worker Agent Read/Write Access](#16-worker-agent-readwrite-access)
17. [Agent Code Navigation Flow](#17-agent-code-navigation-flow)
18. [Supported Languages](#18-supported-languages)
19. [MCP Tools](#19-mcp-tools)
20. [Cortex Synthesis — Layer 2 → Layer 1](#20-cortex-synthesis--layer-2--layer-1)
21. [Frontend UI — Projects Tab & Code Graph Sub-Tab](#21-frontend-ui--projects-tab--code-graph-sub-tab)
22. [Security & Hardening](#22-security--hardening)
23. [Performance & Scaling](#23-performance--scaling)
24. [Configuration](#24-configuration)
25. [Implementation Phases](#25-implementation-phases)
26. [Open Questions & Risks](#26-open-questions--risks)

---

## 1. Executive Summary

This document specifies the Spacebot Code Graph Memory System — a Spacebot-native rebuild of GitNexus that functions as a parallel Layer 2 memory layer alongside the existing Layer 1 semantic memory system.

When a user adds a project, the system automatically fires a 10-phase AST parsing and graph construction pipeline. The resulting graph captures every function, class, import, call relationship, inheritance chain, and execution flow in the codebase and stores it in LadybugDB. A real-time file watcher keeps the graph current as code changes.

**v2.0 introduces three major changes from v1.0:**

- **Centralized Project Memory Layer** — a single unified memory store for all projects, not per-project silos. All project memories live together, tagged by project ID. Removing a project cascades and deletes all its associated memories and indexed graph data.
- **Automatic Stale Memory Removal** — the cortex continuously evaluates project memories for relevance and removes memories that are no longer accurate (e.g., a UI theme the project has moved away from, a dependency that was removed).
- **Code Graph as the Agent's Navigation Layer** — when a user asks an agent to work on a project, the agent queries the code graph first to identify exactly which files need to be read or modified, then hands that targeted file list to a worker. No more guessing.

**Storage: LadybugDB only. No SQLite. Non-negotiable.**

---

## 2. What Changed in v2.0

| Area | v1.0 | v2.0 |
|---|---|---|
| Memory architecture | Per-project separate memory layers implied | Single centralized project memory layer — all projects in one store, tagged by project ID |
| Memory removal | Not specified | Automatic stale memory eviction — cortex removes memories that are no longer accurate for the project |
| Project removal | Delete graph data | Full cascade delete — removes graph data + all Layer 1 memories tagged to that project |
| Agent workflow | General code graph query | Code graph → targeted file list → worker — explicit navigation pattern |
| GitNexus reference | External reference only | Spacebot-native rebuild — we build the same system inside Spacebot, same structure, same capabilities |
| UI clarification | Sub-tab inside project detail | Same, but centralized memory view also lives in the Projects tab |

---

## 3. Problem Statement

AI agents working on codebases today operate with structural blindness and memory fragmentation:

- **No persistent code understanding.** Every session starts from zero. The agent has no memory of what it learned about the codebase last time.
- **Raw file reading is inefficient.** To understand how `AuthService` relates to `UserController`, the agent must read both files, trace imports manually, and hope the context window fits both.
- **Context windows are finite.** Large codebases can't fit in context. The agent guesses which files are relevant — and frequently guesses wrong.
- **No structural navigation.** There is no "show me all callers of this function" or "what changed in this module" without reading every file.
- **Memory is per-session and fragmented.** If project context exists at all, it lives in disconnected facts with no lifecycle — no one removes the memory when the project moves on.
- **Stale memories pollute context.** A memory about a UI theme the project dropped 3 months ago is still injected into every conversation, wasting tokens and causing confusion.

**What is needed:** A persistent, queryable, always-current structural representation of each codebase — centrally managed, with a lifecycle that mirrors the project lifecycle, that the AI navigates the way a senior engineer navigates a codebase they know deeply.

---

## 4. Goals & Non-Goals

### Goals

- **Spacebot-native GitNexus rebuild** — same capabilities, same structure, built inside Spacebot's architecture
- **Centralized project memory layer** — one unified store for all project memories, tagged by project ID
- **Automatic stale memory removal** — cortex evicts memories that are no longer accurate for the project
- **Full cascade delete on project removal** — removing a project deletes all graph data and all associated Layer 1 memories
- **Code graph as agent navigation** — agent queries code graph → gets targeted file list → sends to worker
- **Automatic indexing** — fires when a project is added, no manual step
- **Real-time updates** — file watcher keeps graph current, changes propagate in < 2 seconds
- **Bidirectional cortex ↔ code graph communication**
- **Worker agent read/write access to the graph**
- **LadybugDB as sole storage backend**

### Non-Goals

- Per-project isolated memory silos — all project memories are centralized
- SQLite fallback — LadybugDB only, non-negotiable
- Cloud sync in v1
- Real-time collaborative multi-user write safety
- Visual force-directed graph rendering
- Cross-repo query federation in v1

---

## 5. Architecture Overview

```
┌──────────────────────────────────────────────────────────────┐
│              LAYER 1: Semantic Memory (existing)              │
│  facts · preferences · decisions · goals · todos             │
│  observations · Knowledge Context                            │
│                                                              │
│  ┌─────────────────────────────────────────────────────┐    │
│  │  PROJECT MEMORY PARTITION (new, centralized)        │    │
│  │  All project facts tagged by project_id             │    │
│  │  Stale memories evicted automatically               │    │
│  │  Cascade-deleted when project is removed            │    │
│  └─────────────────────────────────────────────────────┘    │
│                                                              │
│   ← Cortex synthesizes summaries from Layer 2 up here       │
└──────────────────────────┬───────────────────────────────────┘
                           │  synthesis ↑  /  queries ↓
┌──────────────────────────▼───────────────────────────────────┐
│              LAYER 2: Code Graph Memory (new)                 │
│  GitNexus-compatible schema, LadybugDB store                 │
│                                                              │
│  Nodes: 30+ types (Function, Class, Community, Process…)    │
│  Edges: 16 types (CALLS, IMPORTS, INHERITS…)                │
│  Search: BM25 + semantic + RRF                              │
│  File Watcher: real-time incremental updates                │
│  AGENTS.md / CLAUDE.md generation per project               │
└──────────────────────────▲───────────────────────────────────┘
                           │  indexing ↑
┌──────────────────────────┴───────────────────────────────────┐
│              PROJECT TRIGGER                                  │
│  project_manage → auto-fires 10-phase indexing pipeline      │
│  File watcher monitors for real-time changes                 │
│  Cascade delete on project removal                          │
└──────────────────────────────────────────────────────────────┘
```

### Agent Navigation Flow (new in v2.0)

```
User: "Add dark mode to the settings page"
        │
        ▼
Agent queries Code Graph (Layer 2)
  → search("settings", "theme", "dark mode")
  → get_community("UI/Settings")
  → get_callers("SettingsPage.render")
        │
        ▼
Code Graph returns targeted file list:
  [src/pages/Settings.tsx, src/styles/theme.ts,
   src/components/ThemeToggle.tsx, src/store/uiSlice.ts]
        │
        ▼
Agent sends targeted file list → Worker
Worker reads ONLY those 4 files → makes changes
        │
        ▼
File watcher detects changes → incremental re-index
Code graph updated → cortex notified
```

---

## 6. Reference Implementation — GitNexus

**Repository:** https://github.com/abhigyanpatwari/GitNexus

GitNexus is the direct reference. Spacebot builds the same system natively.

### What GitNexus Is

GitNexus is a TypeScript/Node.js CLI and MCP server that indexes a codebase into a LadybugDB graph database and exposes it to AI agents. It uses tree-sitter for AST parsing, the Leiden algorithm for community detection, and hybrid BM25 + semantic + RRF search. It generates `AGENTS.md` / `CLAUDE.md` files that tell AI agents how to navigate the codebase.

### GitNexus Core Capabilities

| Capability | How it works |
|---|---|
| AST parsing | tree-sitter per language, extracts symbols into typed nodes |
| Graph storage | LadybugDB (embedded, Cypher, VECTOR + FTS extensions) |
| Community detection | Leiden algorithm on call/import graph |
| Search | BM25 (FTS) + semantic (VECTOR) + RRF fusion |
| File watching | Incremental re-index on file changes |
| MCP server | stdio transport, 7 tools exposed to AI agents |
| AGENTS.md generation | Auto-writes navigation guide for AI tools |
| Augmentation | <200ms IDE hook, BM25-only fast path |
| Wiki generation | 3-phase LLM pipeline for per-module docs |
| Skill files | Per-community SKILL.md files with symbol inventory |

### Key GitNexus Design Decisions We Adopt

- **`runFullAnalysis` orchestrator** — single shared core for both full and incremental runs. Not two separate pipelines.
- **50k-node auto-skip threshold** — embedding generation is skipped for repos above 50k nodes (too expensive); BM25-only search used instead.
- **`<!-- gitnexus:start/end -->` markers in AGENTS.md** — safe idempotent regeneration without clobbering custom content.
- **RFC 2119 directives in generated docs** — MUST/SHOULD/MAY language in AGENTS.md to give AI tools clear behavioral guidance.
- **Cohesion-ranked augmentation output** — when the augmentation engine runs, results are ranked by cohesion score, not just relevance.
- **Incremental embedding cache** — embeddings are cached per symbol hash; unchanged symbols don't get re-embedded on incremental runs.
- **4-query batched Cypher for augmentation** — callers, callees, processes, and cohesion fetched in a single batched round-trip.

---

## 7. GitNexus File & Folder Structure

This is the structure we replicate in Spacebot, adapted to TypeScript/Rust as appropriate:

```
GitNexus/
├── src/
│   ├── analysis/
│   │   ├── pipeline.ts          ← 10-phase indexing pipeline core
│   │   ├── orchestrator.ts      ← runFullAnalysis() shared entry point
│   │   ├── incremental.ts       ← incremental update logic
│   │   ├── phases/
│   │   │   ├── 01-extract.ts
│   │   │   ├── 02-structure.ts
│   │   │   ├── 03-parse.ts
│   │   │   ├── 04-imports.ts
│   │   │   ├── 05-calls.ts
│   │   │   ├── 06-heritage.ts
│   │   │   ├── 07-communities.ts
│   │   │   ├── 08-processes.ts
│   │   │   ├── 09-enrich.ts
│   │   │   └── 10-complete.ts
│   ├── graph/
│   │   ├── db.ts                ← LadybugDB connection pool + Cypher helpers
│   │   ├── schema.ts            ← Node/edge type definitions
│   │   ├── migrations/          ← Schema version migration scripts
│   │   └── queries.ts           ← Reusable named Cypher queries
│   ├── search/
│   │   ├── hybrid.ts            ← BM25 + semantic + RRF fusion
│   │   ├── bm25.ts              ← FTS full-text search
│   │   └── semantic.ts          ← Vector similarity search
│   ├── watch/
│   │   └── watcher.ts           ← File watcher + debounce + event dispatch
│   ├── mcp/
│   │   ├── server.ts            ← MCP stdio server entry point
│   │   └── tools/               ← One file per MCP tool
│   ├── agents/
│   │   ├── agents-md.ts         ← AGENTS.md / CLAUDE.md generator
│   │   └── skills.ts            ← Per-community SKILL.md generator
│   ├── augment/
│   │   └── augment.ts           ← <200ms augmentation engine
│   ├── wiki/
│   │   └── wiki.ts              ← 3-phase LLM wiki generator
│   └── cli/
│       └── index.ts             ← CLI entry point (15 commands)
├── tests/                       ← 3,579 tests
├── pyproject.toml / package.json
└── README.md
```

### Spacebot-Native Equivalent Structure

We build this inside the Spacebot repo at:

```
spacebot/
├── src/
│   ├── codegraph/               ← NEW top-level module
│   │   ├── analysis/
│   │   │   ├── pipeline.ts
│   │   │   ├── orchestrator.ts
│   │   │   ├── incremental.ts
│   │   │   └── phases/          ← 01 through 10
│   │   ├── graph/
│   │   │   ├── db.ts
│   │   │   ├── schema.ts
│   │   │   ├── migrations/
│   │   │   └── queries.ts
│   │   ├── search/
│   │   │   ├── hybrid.ts
│   │   │   ├── bm25.ts
│   │   │   └── semantic.ts
│   │   ├── watch/
│   │   │   └── watcher.ts
│   │   ├── mcp/
│   │   │   ├── server.ts
│   │   │   └── tools/
│   │   ├── agents/
│   │   │   ├── agents-md.ts
│   │   │   └── skills.ts
│   │   ├── memory/
│   │   │   ├── centralized-store.ts   ← NEW: centralized project memory
│   │   │   ├── stale-eviction.ts      ← NEW: automatic stale removal
│   │   │   └── cascade-delete.ts      ← NEW: project removal cascade
│   │   └── index.ts             ← Module entry point
```

---

## 8. Centralized Project Memory Layer

### Design Decision

v1.0 implied separate memory layers per project. v2.0 specifies a single centralized project memory layer — all project memories live in the same store, tagged by `project_id`.

This mirrors how LadybugDB itself works: one graph database, multiple labeled partitions. It avoids per-project database sprawl, makes cross-project queries possible, and simplifies the cascade-delete lifecycle.

### Structure

All project-scoped Layer 1 memories carry a `project_id` tag in their metadata:

```typescript
interface ProjectMemory {
  id: string;
  project_id: string;           // which project this belongs to
  memory_type: MemoryType;      // fact | observation | goal | etc.
  content: string;
  tags: string[];
  created_at: string;
  last_verified_at: string;     // when cortex last confirmed this is still true
  relevance_score: number;      // 0.0–1.0, updated by stale eviction
  source: "indexer" | "cortex" | "agent" | "user";
}
```

### What Gets Stored Per Project

| Memory Type | Example |
|---|---|
| `fact` | "The auth module (Community #3) contains 14 files centered around AuthService" |
| `fact` | "Main entry points: server.ts:main(), cli.ts:run(), worker.ts:processJob()" |
| `fact` | "This project uses React 18 with Vite. No Next.js." |
| `observation` | "The payment module has high cyclomatic complexity — 3 functions over 200 lines" |
| `observation` | "Test coverage for the auth module is 94%. Payment module has 0 tests." |
| `goal` | "User is actively working on dark mode feature in Settings" |

### Querying the Centralized Store

```typescript
// Get all memories for a project
getProjectMemories(project_id: string): ProjectMemory[]

// Get memories of a specific type for a project
getProjectMemories(project_id: string, type: MemoryType): ProjectMemory[]

// Search across all project memories
searchProjectMemories(project_id: string, query: string): ProjectMemory[]

// Get memories across ALL projects (cross-project view in UI)
getAllProjectMemories(): Map<string, ProjectMemory[]>
```

### UI: Centralized Memory View in Projects Tab

The Projects tab shows a **Project Memory** panel for each project — all memories for that project in one place. This is the single source of truth for what the AI knows about the project.

```
Projects Tab
├── [Repo card: spacebot]
│   └── [View Details] → Project Detail Page
│       ├── 📋 Overview
│       ├── 🧠 Code Graph      ← indexed structure
│       ├── 🗂 Project Memory  ← NEW: all Layer 1 memories for this project
│       └── ⚙️ Settings
```

---

## 9. Automatic Stale Memory Removal

### The Problem

Without active eviction, project memories accumulate stale facts. Examples:

- "This project uses Tailwind CSS" — but you migrated to CSS Modules 2 months ago
- "Dark theme is not implemented" — but you shipped it last week
- "The auth module uses JWT" — but you switched to sessions
- "PaymentService is the main entry point" — but that file was deleted

These stale memories are injected into every conversation, waste tokens, and cause the AI to give wrong answers.

### Eviction Architecture

The cortex runs a relevance evaluation pass after every `graph_changed` or `graph_indexed` event. It also runs on a scheduled cadence (default: every 24h).

```typescript
interface StalenessCheck {
  memory_id: string;
  project_id: string;
  check_type: "code_change" | "scheduled" | "explicit";
  result: "current" | "stale" | "uncertain";
  reason: string;
  action: "keep" | "update" | "remove";
}
```

### Eviction Rules

| Trigger | Rule | Action |
|---|---|---|
| A file is deleted | Any memory referencing that file's symbols is marked stale | Remove |
| A symbol is removed from graph | Memories referencing that symbol are re-evaluated | Update or remove |
| A dependency changes (`package.json`/`Cargo.toml`) | Technology-fact memories re-evaluated | Update or remove |
| A community is restructured by re-clustering | Community summary memories are replaced | Replace with new |
| `last_verified_at` > 30 days old | Memory is re-verified against current graph state | Update or remove |
| `relevance_score` drops below 0.2 | Memory flagged for removal | Remove after 24h grace period |
| User explicitly changes something ("we switched to X") | Related opposing memory removed immediately | Remove |

### What the Cortex Does

After receiving a `graph_changed` event:

1. Read `changed_symbols[]`, `removed_symbols[]`, `added_symbols[]` from event
2. Query centralized store: which memories reference any of these symbols?
3. For each affected memory: re-evaluate against current graph state
4. If memory is contradicted by graph → mark `result: "stale"`, set `action: "remove"`
5. If memory is outdated but not wrong → mark `result: "uncertain"`, `action: "update"`
6. Execute removes and updates
7. Log all eviction decisions to `.spacebot/codegraph/<project_id>/memory_eviction.log`

### Eviction Examples

```
REMOVED: "This project uses Tailwind CSS"
  Reason: tailwind.config.js deleted at 2026-03-29 14:22:11
  Replaced with: "This project uses CSS Modules (migrated from Tailwind)"

REMOVED: "PaymentService handles all billing logic"
  Reason: src/services/PaymentService.ts deleted at 2026-03-28 09:14:55
  No replacement (symbol no longer exists)

UPDATED: "Auth module has 3 entry points"
  Reason: graph_changed added refreshToken() as new entry point
  Updated to: "Auth module has 4 entry points: login, logout, validateSession, refreshToken"

KEPT: "React 18 with Vite"
  Reason: package.json unchanged, react@18 still present
  last_verified_at updated
```

---

## 10. Project Lifecycle — Add, Remove, Cascade Delete

### States

```
              project_manage(create / add_repo)
[unregistered] ──────────────────────────────► [indexing N/10]
                                                      │
                                               pipeline complete
                                                      │
                                                      ▼
                                               [indexed + active]
                                               file watcher running
                                               memories accumulating
                                                      │
                                         ┌────────────┴────────────┐
                                    file change               project_manage
                                         │                    (remove)
                                         ▼                         │
                                    [stale → re-index]             ▼
                                                          [cascade delete]
                                                          graph deleted
                                                          memories deleted
                                                          watcher stopped
```

### Cascade Delete — What Gets Removed

When a user removes a project (via UI or `project_manage(action="remove")`):

```typescript
async function cascadeDeleteProject(project_id: string) {
  // 1. Stop file watcher
  await watcher.stop(project_id);

  // 2. Delete LadybugDB graph data
  await db.execute(`
    MATCH (n) WHERE n.project_id = $project_id
    DETACH DELETE n
  `, { project_id });

  // 3. Delete all Layer 1 memories tagged to this project
  await memoryStore.deleteByProjectId(project_id);

  // 4. Delete meta.json, events.log, memory_eviction.log
  await fs.rm(`.spacebot/codegraph/${project_id}`, { recursive: true });

  // 5. Remove from project registry
  await registry.remove(project_id);

  // 6. Emit project_removed event to cortex
  await eventBus.emit({ event_type: "project_removed", project_id });
}
```

### What the User Sees

```
⚠️ Remove Project: spacebot?

This will permanently delete:
  • Code graph index (4,821 nodes, 12,403 edges)
  • 47 project memories
  • All index history and logs

This cannot be undone.

[Cancel]  [Remove Project]
```

---

## 11. Graph Storage — LadybugDB

> ⚠️ **LadybugDB is the ONLY storage backend. No SQLite. No fallback. Non-negotiable.**

### Background

KuzuDB (original GitNexus backend) was acquired by Apple in October 2025 and archived. The community forked it as LadybugDB, maintaining full API compatibility. GitNexus migrated to LadybugDB. Spacebot uses LadybugDB from day one.

### Storage Layout

```
.spacebot/
└── codegraph/
    ├── registry.json            ← all registered projects + status
    └── <project_id>/
        ├── lbug/                ← LadybugDB database files
        ├── meta.json            ← phase progress, status, schema_version
        ├── events.log           ← all graph events (audit)
        ├── agent_writes.log     ← all agent write operations (audit)
        └── memory_eviction.log  ← stale memory removal audit trail
```

### LadybugDB Configuration

| Setting | Value |
|---|---|
| Extensions | VECTOR (semantic search), FTS (BM25) |
| Connection pool | 5 connections per project |
| Retry on BUSY | 3 retries, linear backoff (100ms / 200ms / 300ms) |
| Query language | Cypher |
| WASM adapter | Enabled (Tauri desktop environment) |

### Schema Versioning

- `schema_version` tracked in `meta.json`
- On startup: check version → run migration if mismatch
- Migrations in `src/codegraph/graph/migrations/<from>_to_<to>.ts`
- Breaking changes trigger full re-index

---

## 12. Graph Schema

### Node Types (30+)

**Structural:**

| Node | Description |
|---|---|
| `Project` | Repo root |
| `Package` | npm/pip/cargo package |
| `Module` | Logical grouping |
| `Folder` | Directory |
| `File` | Source file |

**Code entities:**

| Node | Description |
|---|---|
| `Class` | Class definition |
| `Function` | Standalone function |
| `Method` | Class method |
| `Variable` | Module/class-level variable |
| `Interface` | Interface / protocol |
| `Enum` | Enumeration |
| `Decorator` | Decorator / annotation |
| `Import` | Import statement |
| `Type` | Type alias / definition |

**Language-specific:**

| Node | Languages |
|---|---|
| `Struct` | Rust, C, C++, Go |
| `Macro` | Rust, C, C++ |
| `Trait` | Rust |
| `Impl` | Rust |
| `Namespace` | C++, C#, TypeScript |
| `TypeAlias` | TypeScript, Rust |
| `Const` | TypeScript, Rust, Go |
| `Record` | Java, Kotlin |
| `Template` | C++ |

**Semantic (AI-optimized):**

| Node | Description |
|---|---|
| `Community` | Leiden cluster — a functional area of the codebase |
| `Process` | Execution flow traced from an entry point |

**Documentation & Testing:**

| Node | Description |
|---|---|
| `Section` | Markdown heading (H1–H6) with content body |
| `Test` | Test function or test method |

### Edge Types (16)

| Edge | Direction | Description |
|---|---|---|
| `CONTAINS` | Parent → Child | Structural containment |
| `DEFINES` | File → Symbol | File defines a symbol |
| `CALLS` | Caller → Callee | Function call (confidence-scored) |
| `IMPORTS` | File → Symbol | Import relationship |
| `EXTENDS` | Child → Parent | Class inheritance |
| `IMPLEMENTS` | Class → Interface | Interface implementation |
| `INHERITS` | Child → Parent | Inheritance alias |
| `OVERRIDES` | Method → Method | Method override |
| `HAS_METHOD` | Class → Method | Class owns method |
| `HAS_PROPERTY` | Class → Variable | Class owns field |
| `ACCESSES` | Function → Variable | Function reads/writes variable |
| `USES` | Symbol → Symbol | General usage |
| `DECORATES` | Decorator → Symbol | Decorator applied |
| `MEMBER_OF` | Symbol → Community | Belongs to Leiden cluster |
| `STEP_IN_PROCESS` | Step → Step | Sequential execution step |
| `TESTED_BY` | Function → Test | Function has a test |

### CALLS Confidence Tiers

| Resolution | Confidence | Example |
|---|---|---|
| Same-file direct | 0.95 | `foo()` in same file as `foo` definition |
| Import-scoped | 0.90 | `import foo; foo()` — explicit import |
| Global fuzzy | 0.50 | `foo()` — multiple candidate definitions |

---

## 13. Bidirectional Cortex ↔ Code Graph Communication

Communication is via an in-process `asyncio.Queue` (or TypeScript `EventEmitter`). No external broker.

### Direction A — Cortex → Code Graph (Queries)

| Function | Returns |
|---|---|
| `codegraph_query(project_id, cypher, params)` | Cypher result set |
| `codegraph_get_communities(project_id)` | All community nodes |
| `codegraph_get_symbol(project_id, qualified_name)` | 360° symbol context |
| `codegraph_synthesize(project_id)` | Trigger fresh synthesis |
| `codegraph_is_stale(project_id)` | Staleness flag + timestamp |
| `codegraph_get_processes(project_id)` | All Process nodes |
| `codegraph_search(project_id, query)` | Hybrid BM25+semantic+RRF results |
| `codegraph_get_files_for_task(project_id, task_description)` | **NEW** — targeted file list for a task |

### Direction B — Code Graph → Cortex (Events)

| Event | Fired when | Cortex response |
|---|---|---|
| `graph_indexed` | Full pipeline completes | Full synthesis → create all project memories |
| `graph_changed` | Incremental update completes | Partial synthesis + stale eviction pass |
| `graph_stale` | File watcher detects changes before re-index | Schedule re-index |
| `graph_error` | Pipeline phase fails | Log + surface to user if unrecoverable |
| `project_removed` | Cascade delete completes | Remove all synthesis from Knowledge Context |

---

## 14. The 10-Phase Indexing Pipeline

Matches GitNexus's `runFullAnalysis` orchestrator. Single shared core for full and incremental runs.

| Phase | Name | Description | Output |
|---|---|---|---|
| 1 | `extracting` | Walk filesystem, respect `.gitignore`, skip binaries/build artifacts | Filtered file list |
| 2 | `structure` | Create Folder, File, Package, Module, Project nodes + CONTAINS edges | Structural skeleton |
| 3 | `parsing` | tree-sitter AST parse per file, extract all symbol nodes + DEFINES/CONTAINS edges | Symbol table |
| 4 | `imports` | Resolve import/require/use statements, create Import nodes + IMPORTS edges | Import graph |
| 5 | `calls` | Resolve call-sites with confidence scoring, create CALLS edges | Call graph |
| 6 | `heritage` | Resolve extends/implements/inherits, create heritage edges | Inheritance graph |
| 7 | `communities` | Leiden community detection, create Community nodes + MEMBER_OF edges | Community map |
| 8 | `processes` | Score entry-point likelihood, trace call chains, create Process + STEP_IN_PROCESS | Execution flows |
| 9 | `enriching` | *(Optional)* LLM labels for community and process names | Human-readable labels |
| 10 | `complete` | Write `meta.json`, fire `graph_indexed`, start file watcher, generate AGENTS.md | Index live |

### 50k-Node Auto-Skip (from GitNexus)

If node count > 50,000 after phase 3:

- Phase 9 (LLM enrichment) is auto-skipped regardless of config
- Semantic embedding generation is skipped; BM25-only search is used
- This threshold is configurable

### AGENTS.md Generation (Phase 10)

After indexing completes, the system generates an `AGENTS.md` (and `CLAUDE.md`) file in the repo root. This file:

- Uses `<!-- codegraph:start -->` / `<!-- codegraph:end -->` markers for safe idempotent regeneration
- Contains RFC 2119 directives (MUST / SHOULD / MAY) for AI behavioral guidance
- Lists all communities with their key symbols and entry points
- Includes a skills table linking to per-community `SKILL.md` files
- Is regenerated automatically on every full re-index and on significant `graph_changed` events

---

## 15. Real-Time File Watching & Incremental Updates

### File Watcher

Uses `chokidar` (Node.js) or `watchdog` (Python) with:

- 500ms debounce window
- `.gitignore` respected
- Recursive monitoring
- Binary/non-source files excluded

Starts automatically at end of Phase 10. Restarts on Spacebot startup for all `status: indexed` projects.

### Incremental Update Flow

```
File change detected (after debounce)
    │
    ▼
1. DETECT    — identify changed files from watcher event
    │
    ▼
2. REMOVE    — DELETE all nodes/edges where source_file IN changed_files
    │
    ▼
3. RE-PARSE  — run phases 3–6 on changed files only
    │
    ▼
4. MERGE     — INSERT new nodes/edges, resolve cross-file refs against existing graph
    │
    ▼
5. RE-CLUSTER — IF delta > re_index_threshold (default 5%) → run Leiden re-cluster
                 ELSE → keep existing communities
    │
    ▼
6. EMIT      — fire graph_changed with full diff payload
    │
    ▼
7. EVICT     — cortex runs stale memory eviction pass for affected symbols
    │
    ▼
8. SYNTHESIZE — cortex partial synthesis → update affected Layer 1 memories
```

### Per-Event Handling

| Event | Action |
|---|---|
| `file_created` | Parse new file → extract → merge into graph |
| `file_modified` | Remove old nodes/edges → re-parse → merge fresh |
| `file_deleted` | Remove nodes/edges → scan for dangling edges → remove or confidence → 0.0 |
| `file_moved` | Update `source_file` on all nodes → update IMPORTS edges → no re-parse |

### Performance Targets

| Scenario | Target |
|---|---|
| Single file modified | < 2 seconds end-to-end |
| Batch of 10 files | < 10 seconds |
| Full re-index | Background, non-blocking |
| Watcher memory overhead | < 50MB per project |

---

## 16. Worker Agent Read/Write Access

When an agent is working on a project, it queries the code graph to navigate the codebase efficiently.

### Read Tools

| Tool | Returns |
|---|---|
| `codegraph_query(project_id, cypher, params)` | Cypher result set |
| `codegraph_get_symbol(project_id, qualified_name)` | 360° symbol context |
| `codegraph_search(project_id, query, limit)` | Ranked search results |
| `codegraph_get_community(project_id, community_id)` | Community detail |
| `codegraph_get_impact(project_id, qualified_name, depth)` | Blast-radius BFS |
| `codegraph_list_projects()` | All indexed projects with stats |
| `codegraph_get_review_context(project_id, symbol_list)` | Token-optimized bundle |
| `codegraph_find_large_functions(project_id, min_lines)` | Functions over line threshold |
| `codegraph_get_files_for_task(project_id, task_description)` | Targeted file list for a task |

### Write Tools

| Tool | Description |
|---|---|
| `codegraph_add_node(project_id, node_type, properties)` | Insert agent-created node |
| `codegraph_add_edge(project_id, from_id, edge_type, to_id, properties)` | Insert relationship |
| `codegraph_annotate_node(project_id, node_id, annotation, confidence)` | Add AI annotation |
| `codegraph_add_community_label(project_id, community_id, label, description)` | Enrich community |
| `codegraph_mark_tested(project_id, function_qname, test_qname)` | Create TESTED_BY edge |

### Write Safety

- Agents cannot delete `source="indexer"` nodes/edges
- Agents can delete their own writes (`source="agent"` AND `written_by=self`)
- All writes require explicit `allow_write: true` parameter
- All writes logged to `agent_writes.log` with full provenance

---

## 17. Agent Code Navigation Flow

This is the core workflow change in v2.0. When a user asks an agent to do something to a project, the agent must query the code graph before dispatching a worker.

### Flow

```
1. User request arrives
   "Refactor the auth middleware to use async/await"

2. Agent queries code graph (Layer 2):
   → codegraph_search(project_id, "auth middleware")
   → codegraph_get_community(project_id, "auth_community_id")
   → codegraph_get_symbol(project_id, "AuthMiddleware")
   → codegraph_get_impact(project_id, "AuthMiddleware", depth=2)

3. Code graph returns:
   Primary files:
     src/middleware/auth.ts          (AuthMiddleware class)
     src/middleware/auth.test.ts     (tests — TESTED_BY edges)
   Secondary files (callers):
     src/routes/api.ts               (calls AuthMiddleware)
     src/app.ts                      (registers middleware)
   Depth-2 callers:
     src/routes/admin.ts             (calls api router)

4. Agent sends targeted file list to worker:
   Worker reads ONLY: [auth.ts, auth.test.ts, api.ts, app.ts]
   Worker makes changes
   Worker reports back

5. File watcher detects changes → incremental re-index
   graph_changed event fires
   Stale eviction pass runs
   Cortex partial synthesis updates Layer 1 memories
```

### `codegraph_get_files_for_task` Tool

This is a purpose-built tool for the agent navigation flow:

```typescript
async function codegraph_get_files_for_task(
  project_id: string,
  task_description: string,
  options?: {
    max_files?: number;        // default: 20
    include_tests?: boolean;   // default: true
    include_callers?: number;  // depth of caller traversal, default: 1
  }
): Promise<{
  primary_files: string[];      // direct symbol matches
  secondary_files: string[];    // callers and importers
  community: string;            // which community this task touches
  confidence: number;           // how confident the file selection is
}>;
```

This runs hybrid search on the task description, resolves to symbol nodes, then traverses CALLS/IMPORTS edges to build the complete affected file set.

---

## 18. Supported Languages

14 languages at launch:

| Language | Parser | Key Node Types |
|---|---|---|
| TypeScript | `tree-sitter-typescript` | Class, Function, Method, Interface, Type, Enum, Decorator, Namespace, TypeAlias, Const |
| JavaScript | `tree-sitter-javascript` | Class, Function, Method, Variable, Decorator |
| Python | `tree-sitter-python` | Class, Function, Method, Variable, Decorator, Import |
| Rust | `tree-sitter-rust` | Struct, Enum, Trait, Impl, Function, Macro, TypeAlias, Const |
| Go | `tree-sitter-go` | Struct, Function, Method, Interface, Const, TypeAlias |
| Java | `tree-sitter-java` | Class, Method, Interface, Enum, Record, Variable |
| C | `tree-sitter-c` | Struct, Function, Variable, Macro |
| C++ | `tree-sitter-cpp` | Class, Struct, Function, Method, Namespace, Template, Macro |
| C# | `tree-sitter-c-sharp` | Class, Method, Interface, Enum, Namespace, Variable |
| Ruby | `tree-sitter-ruby` | Class, Method, Function, Module |
| PHP | `tree-sitter-php` | Class, Function, Method, Interface |
| Kotlin | `tree-sitter-kotlin` | Class, Function, Method, Object, Record |
| Swift | `tree-sitter-swift` | Class, Struct, Function, Method, Protocol, Enum |
| Markdown | `tree-sitter-markdown` | Section (H1–H6 with content bodies) |

---

## 19. MCP Tools

7 tools exposed to external AI agents via stdio MCP transport:

| Tool | Description |
|---|---|
| `list_projects` | All indexed projects with stats and status |
| `query` | Natural language → ranked results with context snippets |
| `cypher` | Raw Cypher → Markdown table results |
| `context` | 360° view: callers, callees, imports, accesses, community, tests |
| `detect_changes` | Git diff → affected symbols → affected processes → risk |
| `impact` | Blast-radius BFS: upstream/downstream, risk level |
| `rename` | Graph-aware multi-file rename manifest with confidence-tagged usages |

**Transport:** stdio. Buffer cap: 10MB. Closed-state guards on all reads/writes.

---

## 20. Cortex Synthesis — Layer 2 → Layer 1

After `graph_indexed` or `graph_changed`, the cortex synthesizes structured facts into the centralized project memory layer.

### Types of Facts Synthesized

| Fact type | Example |
|---|---|
| Community summary | "The auth module (Community #3) contains 14 files, 87 functions, centered around AuthService and JWTManager." |
| Entry points | "Main entry points: server.ts:main(), cli.ts:run(), worker.ts:processJob()" |
| Dependency map | "AuthService is called by UserController, PaymentService, AdminRouter, WebSocketHandler" |
| Repo stats | "spacebot: 312 files, 4,821 functions, 14 communities, 8 processes. TypeScript 89%, JavaScript 11%." |
| Stale alert | "Code graph for spacebot is stale — 3 files modified since last index." |

### Synthesis Triggers

| Trigger | Type | Scope |
|---|---|---|
| `graph_indexed` | Full synthesis | All project memories created/replaced |
| `graph_changed` | Partial synthesis + stale eviction | Only affected symbols/communities |
| Cortex-initiated (`codegraph_synthesize`) | Full synthesis | All project memories |
| Staleness > 24h | Stale alert fact | Single stale alert memory |

---

## 21. Frontend UI — Projects Tab & Code Graph Sub-Tab

### Navigation Structure

```
Spacebot UI (top-level tabs)
└── Projects Tab
    │
    ├── [Repo card: spacebot]      ← status badge, stats, actions
    ├── [Repo card: my-api]
    └── [+ Add Project]
         │
         └── [click on a project] → Project Detail Page
                 ├── 📋 Overview       ← repo info, worktrees, disk usage (existing)
                 ├── 🧠 Code Graph     ← indexed structure (sub-tab)
                 ├── 🗂 Project Memory ← all Layer 1 memories for this project
                 └── ⚙️ Settings       ← per-project config overrides
```

### Repo Card Design

```
┌──────────────────────────────────────────────────────┐
│  🗂  spacebot                        TypeScript       │
│                                                      │
│  ✅ Indexed  •  2h ago                               │
│  4,821 symbols  •  14 communities  •  312 files      │
│  47 project memories                                 │
│                                                      │
│  [View Details]                    [Re-index]        │
└──────────────────────────────────────────────────────┘
```

**Status badges:**

| Badge | Condition |
|---|---|
| ✅ Indexed | Index complete, no stale files |
| ⏳ Indexing 6/10 — calls | Pipeline running |
| ⚠️ Stale — 3 files changed | Files modified since last index |
| ❌ Error — phase 5 failed | Pipeline failed |

### Code Graph Sub-Tab (4 Views)

**1. Communities View** — Leiden cluster grid. Each card: community name, file count, function count, top symbols, [View Members].

**2. Entry Points View** — Table of Process nodes: entry function, file, call depth, community, [Trace] button.

**3. Search View** — Hybrid BM25+semantic+RRF. Results ranked by fusion score, showing file, community, callers/callees.

**4. Index Log View** — Live pipeline progress (phase by phase with timing) + historical run log.

### Project Memory Sub-Tab (new in v2.0)

```
🗂 Project Memory — spacebot

[All]  [Facts]  [Observations]  [Goals]  [Stale Eviction Log]

┌────────────────────────────────────────────────────────┐
│  fact  •  Verified 2h ago                              │
│  "The auth module (Community #3) contains 14 files,   │
│   87 functions, centered around AuthService"           │
│  Source: indexer  •  Relevance: 0.97                  │
└────────────────────────────────────────────────────────┘
┌────────────────────────────────────────────────────────┐
│  observation  •  Verified 2h ago                       │
│  "Test coverage for auth module: 94%. Payment: 0%"    │
│  Source: cortex  •  Relevance: 0.88                   │
└────────────────────────────────────────────────────────┘

Stale Eviction Log:
  REMOVED  "This project uses Tailwind CSS"
    tailwind.config.js deleted at 2026-03-28 14:22
  UPDATED  "Auth module has 3 entry points"
    refreshToken() added → now 4 entry points
```

### Persistent Sidebar Status Indicator

| State | Display |
|---|---|
| Active indexing | ⏳ spacebot: indexing 7/10 |
| Stale | ⚠️ spacebot: stale |
| Error | ❌ spacebot: index error |
| All fresh | Hidden |

Clicking navigates to the project's Code Graph → Index Log.

---

## 22. Security & Hardening

| Threat | Mitigation |
|---|---|
| Command injection | All exec calls use argument lists — never `shell=true` |
| Symlink traversal | Max 1-level symlink follow, cycle detection |
| Path traversal | All paths normalized to absolute within repo root |
| Large MCP payload | 10MB buffer cap |
| Cypher injection | Parameterized queries only — no string interpolation |
| Agent write spoofing | `written_by` set server-side from authenticated `agent_id` |
| Unauthorized deletion | `source="indexer"` nodes protected from agent deletion |
| Stale memory audit | All eviction decisions logged to `memory_eviction.log` |

---

## 23. Performance & Scaling

| Scenario | Target |
|---|---|
| Single file incremental update | < 2 seconds |
| Batch update (10 files) | < 10 seconds |
| Symbol search (BM25+semantic+RRF) | < 500ms |
| `codegraph_get_files_for_task` | < 300ms |
| Raw Cypher traversal query | < 200ms |
| Community list | < 100ms |
| Impact BFS (depth 3) | < 300ms |
| RAM during indexing | < 500MB per project |
| RAM at rest (watcher only) | < 50MB per project |
| Disk (LadybugDB) | ~10–30% of source codebase size |

**50k-node threshold:** Above 50k nodes, semantic embedding generation is skipped. BM25-only search used. Leiden clustering uses progressive subsampling.

---

## 24. Configuration

All settings under **Settings → Code Graph**. Per-project overrides available.

| Setting | Default | Description |
|---|---|---|
| `auto_index_on_add` | `true` | Auto-fire indexing when project added |
| `real_time_watching` | `true` | Enable file watcher |
| `debounce_ms` | `500` | Watcher debounce window |
| `llm_enrichment` | `false` | LLM labels in phase 9 |
| `language_filter` | `[]` | If set, only parse specified languages |
| `re_index_threshold` | `5` | % delta to trigger full Leiden re-run |
| `staleness_threshold_hours` | `24` | Hours before auto full re-index |
| `max_process_depth` | `10` | Max call-chain depth for Process tracing |
| `community_min_size` | `3` | Min nodes per persisted community |
| `agent_writes_enabled` | `true` | Allow worker agent writes |
| `stale_eviction_enabled` | `true` | Auto-remove stale memories |
| `stale_eviction_cadence_hours` | `24` | Scheduled eviction cadence |
| `stale_relevance_threshold` | `0.2` | Score below which memory is flagged |
| `node_embedding_skip_threshold` | `50000` | Auto-skip semantic embeddings above this |
| `generate_agents_md` | `true` | Auto-generate AGENTS.md on index complete |

---

## 25. Implementation Phases

### Phase 1 — Foundation

*Scope: Core pipeline, storage, manual trigger only. No UI.*

- [ ] `src/codegraph/` module scaffold matching GitNexus structure
- [ ] LadybugDB integration (connection pool, VECTOR + FTS, schema versioning)
- [ ] 10-phase pipeline, phases 1–6
- [ ] Basic node/edge types (File, Folder, Class, Function, Method, Variable, Import)
- [ ] `meta.json` phase progress tracking
- [ ] `project_manage` trigger hook (auto-fire on `create`/`add_repo`)
- [ ] Read tools: `codegraph_query`, `codegraph_search`, `codegraph_get_symbol`
- [ ] Centralized project memory store (data model only — no eviction yet)

**Success criteria:** User adds a project, indexing fires, symbols queryable via Cypher within 5 minutes.

### Phase 2 — Full Schema + Cortex + Memory Lifecycle

*Scope: Complete schema, communities, bidirectional cortex, centralized memory + cascade delete.*

- [ ] Complete 30+ node types + all 16 edge types
- [ ] Phase 7: Leiden community detection
- [ ] Phase 8: Process/entry-point detection
- [ ] Bidirectional event system (all 5 event types)
- [ ] Cortex query API (all 7 functions)
- [ ] Layer 1 synthesis → centralized project memory store
- [ ] Cascade delete on project removal
- [ ] Stale memory eviction (code-change triggered)

**Success criteria:** After indexing, community facts in Knowledge Context. Removing a project deletes all its memories.

### Phase 3 — Real-Time + Write Access + Navigation

*Scope: File watching, incremental updates, agent write tools, navigation flow.*

- [ ] chokidar/watchdog file watcher with debounce
- [ ] Incremental update pipeline (all 4 event types)
- [ ] Partial synthesis on `graph_changed` + stale eviction pass
- [ ] All worker agent write tools + provenance + audit log
- [ ] `codegraph_get_files_for_task` tool
- [ ] Scheduled stale eviction cadence
- [ ] AGENTS.md / CLAUDE.md generation (Phase 10)

**Success criteria:** File edit updates graph < 2s. Agent queries code graph before dispatching worker. Stale memories auto-evicted.

### Phase 4 — UI + MCP + Wiki

*Scope: Full frontend UI, MCP server, skill files, wiki generator.*

- [ ] Projects tab: repo card redesign with status badges + memory count
- [ ] Project Detail: Code Graph sub-tab (4 views) + Project Memory sub-tab
- [ ] Eviction log UI
- [ ] Remove Project modal with cascade delete confirmation
- [ ] Settings → Code Graph panel + per-project overrides
- [ ] External MCP server (all 7 tools)
- [ ] Phase 9: LLM enrichment
- [ ] Per-community SKILL.md generation
- [ ] Optional: wiki generator (3-phase LLM)

**Success criteria:** Full end-to-end — add project → auto-index → community cards in UI → agent navigates via code graph → stale memories removed → removing project cleans up everything.

---

## 26. Open Questions & Risks

| Risk | Severity | Mitigation |
|---|---|---|
| Leiden clustering on very large repos (> 100k nodes) | Medium | Progressive subsampling above threshold |
| LadybugDB WASM adapter in Tauri environment | Medium | Test early; fallback to IPC bridge if needed |
| tree-sitter edge cases (Ruby metaprogramming, Rust macros) | Low | Lower-fidelity nodes, not failures |
| Agent write conflicts (two agents same community label) | Medium | Per-node advisory write lock, last-write-wins, audit log |
| Stale eviction false positives (removing a still-valid memory) | Medium | Grace period (24h) before removal executes; eviction log for audit; user can manually pin memories |
| File watcher on Windows (`ReadDirectoryChangesW` buffer overflow on mass changes) | Medium | Fallback to git-diff staleness check on buffer overflow |
| Schema migration complexity as schema evolves | Low | Strict versioning from day 1, migration scripts per version bump |
| `codegraph_get_files_for_task` returning too many files | Low | `max_files` cap (default 20), confidence score filtering |
| Embeddings load on large repos (50k+ symbols) | Low | Auto-skip threshold + batched generation during phase 3 |

---

*End of Document — Spacebot Code Graph Memory System v2.0*
