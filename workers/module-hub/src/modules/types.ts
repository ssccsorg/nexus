// src/modules/types.ts
// Module interface definitions for the SSCCS Nexus reasoning module ecosystem.
//
// Each module is a self-contained cognitive function that communicates
// through a shared contract. Modules do not call each other directly;
// they publish findings and consume findings from the shared knowledge space.

// ---------------------------------------------------------------------------
// Contract rules
// ---------------------------------------------------------------------------
export type ContractLevel = "strict" | "normal" | "exploratory";

export interface Contract {
  /** How strictly evidence is required */
  level: ContractLevel;
  /** Minimum KG evidence count per finding */
  minEvidence: number;
  /** Whether human review is required before publishing */
  requireHumanReview: boolean;
}

// ---------------------------------------------------------------------------
// Module manifest — declares what a module needs and does
// ---------------------------------------------------------------------------
export interface ModuleManifest {
  name: string;
  description: string;
  version: string;
  /** "periodic" = cron-scheduled, "on-demand" = triggered by event */
  runMode: "periodic" | "on-demand";
  /** Default schedule expression (if periodic) */
  schedule?: string;
  /** Which MCP/KG backends this module requires */
  requiresKgs: string[];
  /** Contract rules the module must satisfy */
  contract: Contract;
}

// ---------------------------------------------------------------------------
// Finding — the universal output unit
// ---------------------------------------------------------------------------
export type FindingSeverity = "info" | "warning" | "critical";
export type FindingStatus = "open" | "closed" | "escalated";

export interface Finding {
  id: string;
  moduleName: string;
  severity: FindingSeverity;
  status: FindingStatus;
  title: string;
  description: string;
  /** KG evidence citations (edge IDs, node names, etc.) */
  evidence: string[];
  /** ISO 8601 timestamps */
  createdAt: string;
  updatedAt: string;
}

// ---------------------------------------------------------------------------
// Module runtime context
// ---------------------------------------------------------------------------
export interface ModuleContext {
  /** KV namespace for module state persistence */
  kv: KVNamespace;
  /** R2 bucket for artifact storage */
  bucket: R2Bucket;
  /** Cloudflare Workers AI binding (optional) */
  ai: any;
  /** Environment bindings */
  env: Record<string, string>;
}

// ---------------------------------------------------------------------------
// Module interface — every module must implement this
// ---------------------------------------------------------------------------
export interface ReasoningModule {
  manifest: ModuleManifest;

  /** Called once when the module is loaded */
  init(ctx: ModuleContext): Promise<void>;

  /** Main execution — run the module's logic */
  run(ctx: ModuleContext): Promise<Finding[]>;

  /** Called when the module is being unloaded */
  teardown(ctx: ModuleContext): Promise<void>;
}

// ---------------------------------------------------------------------------
// KG client interface — abstracts any graph backend
// ---------------------------------------------------------------------------
export interface KgClient {
  /** Execute a raw Cypher query and return results */
  query(cypher: string, params?: Record<string, any>): Promise<any[]>;
  /** Check if the backend is reachable */
  ping(): Promise<boolean>;
}
