#!/usr/bin/env node
/**
 * Node.js agent: FIH Blackboard consumer via HTTP.
 *
 * Demonstrates how a JavaScript/TypeScript agent (simulating a web
 * frontend, edge function, or monitoring dashboard) communicates
 * with the Blackboard through the gateway API. No Rust crates needed.
 *
 * Usage:
 *   node gateway/api/examples/node_agent.mjs
 *
 * Requires:
 *   - Node.js 18+ (built-in fetch)
 *   - gateway server running on localhost:30922
 *     (cd apps/nex-api && GATEWAY_PORT=30922 cargo run)
 */

const API = "http://localhost:30922/api/v1/fih";

async function post(path, body) {
  const res = await fetch(`${API}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`HTTP ${res.status}: ${text}`);
  }
  const contentType = res.headers.get("content-type");
  if (contentType && contentType.includes("application/json")) {
    return res.json();
  }
  return null;
}

async function get(path) {
  const res = await fetch(`${API}${path}`);
  return res.json();
}

async function main() {
  console.log("=== Node.js Agent: FIH Lifecycle via HTTP ===\n");

  // Step 1: Submit a structured fact
  console.log("1. Submitting fact...");
  const factResult = await post("/facts", {
    origin: "nodejs-dashboard",
    content: {
      event: "deploy_complete",
      service: "api-gateway",
      version: "v2.4.1",
      duration_ms: 3420,
      status: "success",
    },
    creator: "ci-bot",
  });
  const factId = factResult.id;
  console.log(`   Fact ID: ${factId}`);

  // Step 2: Submit intent
  console.log("\n2. Submitting intent...");
  const intentResult = await post("/intents", {
    from_facts: [factId],
    description: "Investigate deploy duration regression (3.4s vs 1.2s baseline)",
    creator: "sre-agent",
  });
  const intentId = intentResult.id;
  console.log(`   Intent ID: ${intentId}`);

  // Step 3: Claim and heartbeat
  console.log("\n3. Claiming intent...");
  await post(`/intents/${intentId}/claim`, { agent: "sre-agent" });
  console.log("   Claimed");

  console.log("\n4. Heartbeat...");
  await post(`/intents/${intentId}/heartbeat`, { agent: "sre-agent" });
  console.log("   OK");

  // Step 4: Conclude with structured result
  console.log("\n5. Concluding intent...");
  const concludeResult = await post(`/intents/${intentId}/conclude`, {
    result: {
      finding: "Deploy duration regression caused by new healthcheck timeout (30s default, should be 5s)",
      fix: "Reduce healthcheck timeout to 5s in deploy config",
      effort_hours: 0.5,
    },
  });
  console.log(`   New fact: ${concludeResult.fact.id}`);

  // Step 5: Read final state
  console.log("\n6. Final board state...");
  const state = await get("/state");
  console.log(`   Facts:   ${state.facts.length}`);
  console.log(`   Intents: ${state.intents.length}`);
  console.log(`   Hints:   ${state.hints.length}`);

  console.log("\nNode.js agent: FIH lifecycle complete via HTTP/JSON");
}

main().catch(console.error);
