// src/modules/registry.ts
// Module registry — loads and manages reasoning modules.
//
// Each module is a self-contained cognitive function. The registry
// is the DJ mixing desk: it knows which modules are active, what
// contract rules they must satisfy, and which KG backends they access.

import type { ReasoningModule, ModuleContext, Finding, ModuleManifest } from "./types";

// ---------------------------------------------------------------------------
// Module loader — auto-discover modules in the registry
// ---------------------------------------------------------------------------
interface ModuleConstructor {
  new (): ReasoningModule;
}

const REGISTERED: Record<string, ModuleConstructor> = {};

/**
 * Register a module class under a given key.
 */
export function register(key: string, ctor: ModuleConstructor): void {
  REGISTERED[key] = ctor;
}

/**
 * List all registered module keys with their manifests.
 */
export function listModules(): Array<{ key: string; manifest: ModuleManifest }> {
  return Object.entries(REGISTERED).map(([key, ctor]) => ({
    key,
    manifest: new ctor().manifest,
  }));
}

// ---------------------------------------------------------------------------
// Orchestrator — runs one or more modules
// ---------------------------------------------------------------------------
export interface RunResult {
  moduleName: string;
  findings: Finding[];
  durationMs: number;
  error?: string;
}

/**
 * Run a single module by key.
 * Loads the module, calls init + run, collects findings.
 */
export async function runModule(
  key: string,
  ctx: ModuleContext,
): Promise<RunResult> {
  const Ctor = REGISTERED[key];
  if (!Ctor) {
    return {
      moduleName: key,
      findings: [],
      durationMs: 0,
      error: `Unknown module: ${key}. Available: ${Object.keys(REGISTERED).join(", ")}`,
    };
  }

  const start = Date.now();
  const module = new Ctor();

  try {
    await module.init(ctx);
    const findings = await module.run(ctx);
    await module.teardown(ctx);

    return {
      moduleName: key,
      findings,
      durationMs: Date.now() - start,
    };
  } catch (e) {
    return {
      moduleName: key,
      findings: [],
      durationMs: Date.now() - start,
      error: String(e),
    };
  }
}

/**
 * Run multiple modules in parallel and collect results.
 */
export async function runModules(
  keys: string[],
  ctx: ModuleContext,
): Promise<RunResult[]> {
  return Promise.all(keys.map((key) => runModule(key, ctx)));
}

/**
 * Run all registered modules.
 */
export async function runAll(ctx: ModuleContext): Promise<RunResult[]> {
  return runModules(Object.keys(REGISTERED), ctx);
}
