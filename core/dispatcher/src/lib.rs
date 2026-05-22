// nexus-dispatcher — OODA loop runtime: scheduler, eviction, agent lifecycle.
//
// Architecture
// ============
//
//   nexus-dispatcher
//     ├── scheduler/      ← polling loop, heartbeat monitor, Intent dispatch
//     ├── runtime/        ← agent lifecycle (claim/release/conclude)
//     ├── eviction/       ← flush + evict_before memory management
//     └── tasks/          ← gap detectors, pattern matchers (stigmergy)

pub mod scheduler;
pub mod eviction;
