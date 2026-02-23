# Attractor Spec Gap Analysis

Comparison of the implementation in `crates/attractor/` against `docs/specs/attractor-spec.md`.

## Summary

The core pipeline engine, DOT parsing, edge selection, condition evaluation, retry logic, checkpoint/resume, validation, and all 10 handler types are implemented. The HTTP server with SSE is implemented. Context fidelity preamble synthesis, thread ID plumbing to backends, engine cancellation, recording/replay, and preset retry policies are all implemented. The remaining gaps are around **SVG graph rendering** and some smaller optional features.

---

## Implemented Features (Complete or Substantially Complete)

| Spec Section | Feature | Status |
|---|---|---|
| 2. DOT DSL | Parser, grammar, value types, chained edges, subgraphs, defaults, class attr | Done |
| 3.1 | Run lifecycle (parse, validate, initialize, execute, finalize) | Done |
| 3.2 | Core execution loop | Done |
| 3.3 | Edge selection (5-step: condition, preferred label, suggested IDs, weight, lexical) | Done |
| 3.4 | Goal gate enforcement with retry target fallback chain | Done |
| 3.5-3.6 | Retry logic with backoff, jitter, preset policies, allow_partial | Done |
| 3.6 | Preset retry policies selectable by name from DOT (`retry_policy` attr) | Done |
| 3.7 | Failure routing (fail edge, retry_target, fallback, termination) | Done |
| 3.8 | Single-threaded traversal with parallel handler isolation | Done |
| 4.1-4.2 | Handler interface and registry (explicit type > shape > default) | Done |
| 4.3-4.4 | Start/Exit handlers | Done |
| 4.5 | Codergen handler with CodergenBackend, simulation mode, $goal expansion, log files | Done |
| 4.5 | CodergenBackend receives thread_id for session reuse | Done |
| 4.6 | Wait.human handler with accelerator keys, freeform edges, timeout, choice matching | Done |
| 4.7 | Conditional handler (no-op, routing via edge selection) | Done |
| 4.8 | Parallel handler (fan-out, join policies, error policies, bounded concurrency) | Done |
| 4.9 | Fan-in handler (heuristic + LLM-based evaluation) | Done |
| 4.10 | Tool handler (shell command, timeout) | Done |
| 4.11 | Manager loop handler (observe/steer/wait, child autostart, stop condition) | Done |
| 4.12 | Custom handler registration | Done |
| 5.1 | Context (key-value store, thread-safe, snapshot, clone, apply_updates) | Done |
| 5.2 | Outcome (all StageStatus values, context_updates, preferred_label, suggested_next_ids) | Done |
| 5.3 | Checkpoint (save/load, resume from checkpoint, node_outcomes, retry counters) | Done |
| 5.3 | Checkpoint resume fidelity degradation (full → summary:high on first resumed node) | Done |
| 5.4 | Fidelity resolution (edge > node > graph > default) | Done |
| 5.4 | Thread ID resolution (5-level precedence) | Done |
| 5.4 | Context fidelity preamble synthesis (truncate, compact, summary:low/medium/high) | Done |
| 5.5 | Artifact store | Done |
| 5.6 | Run directory structure (manifest.json, status.json, prompt.md, response.md) | Done |
| 6.1 | Interviewer.inform() called at pipeline/stage lifecycle points | Done |
| 6.1-6.5 | Interviewer interface + all implementations (AutoApprove, Console, Callback, Queue, Recording, Web) | Done |
| 6.4 | RecordingInterviewer serialization (to_json/from_json, save/load file) | Done |
| 6.4 | ReplayInterviewer for replaying recorded Q&A sessions | Done |
| 7.1-7.4 | Validation with 15 lint rules, diagnostic model, custom rules | Done |
| 8.1-8.6 | Model stylesheet (parse, selectors: *, .class, #id, specificity, application) | Done |
| 9.1-9.3 | Transforms (variable expansion, stylesheet application, custom transforms) | Done |
| 9.4 | Graph merging transform (namespace-prefixed node/edge merge) | Done |
| 9.5 | HTTP server mode (POST /pipelines, GET status, SSE events, question answering, cancel) | Done |
| 9.5 | GET /pipelines/{id}/checkpoint and GET /pipelines/{id}/context endpoints | Done |
| 9.5 | Pipeline cancel with engine-level cancellation token (checked between nodes) | Done |
| 9.6 | Event emitter (pipeline/stage/parallel/interview/checkpoint events) | Done |
| 9.7 | Tool call hooks (pre/post hooks on codergen handler) | Done |
| 10.1-10.6 | Condition expression language (=, !=, &&, outcome, preferred_label, context.*) | Done |
| N/A | Sub-pipeline handler (inline DOT, context isolation with diff propagation) | Done (bonus) |
| 2.7 | Edge `loop_restart` attribute | Done |
| 2.6 | Node `auto_status` attribute | Done |
| 2.6 | Node `timeout` attribute enforcement | Done |

---

## Missing or Incomplete Features

### 1. GET /pipelines/{id}/graph (SVG Rendering) (Spec 9.5)

**Status: Not implemented**

The spec lists `GET /pipelines/{id}/graph` to return a rendered graph visualization (SVG). The HTTP server does not implement this endpoint. This would require either a Graphviz dependency or a custom SVG renderer.

### 2. Subgraph Class Derivation (Spec 2.10)

**Status: Needs verification**

The spec says subgraph labels should produce derived CSS-like classes: lowercasing, replacing spaces with hyphens, stripping non-alphanumeric. The parser has `derive_class_from_label()` which does this, but end-to-end verification that subgraph labels produce correct classes on contained nodes is not fully covered by tests.

---

## Optional / Future Features Mentioned in Spec

### 3. Extended Condition Operators (Spec 10.7)

**Status: Not implemented (explicitly marked "Future")**

The spec lists these as potential extensions: `contains`, `matches`, `OR`, `NOT`, `>`, `<`, `>=`, `<=`. Currently only `=`, `!=`, and `&&` are supported, which matches the current spec requirements.

### 4. `should_retry` Predicate Customization (Spec 3.6)

**Status: Default predicate only**

The default predicate retries transient errors (Handler, Engine, Io) and rejects terminal errors (Parse, Validation, Stylesheet, Checkpoint). There's no mechanism for DOT authors or custom handlers to provide a custom `should_retry` predicate per node.

### 5. `rankdir` Graph Attribute (Spec 2.13)

**Status: Parsed but not used**

The DOT examples show `rankdir=LR` but this is a Graphviz visual hint. It's parsed as a graph attribute but has no execution semantics, which is correct behavior.

### 6. Stylesheet Shape Selectors (Spec 8)

**Status: Not implemented**

The spec mentions shape selectors for stylesheets, but the implementation only supports `*` (universal), `.class`, and `#id` selectors. Shape-based selectors (e.g., `box { ... }`) are not supported.

---

## Cross-Feature Parity Matrix Status

Based on code review, these items from Spec 11.12 appear covered:

- [x] Parse simple linear pipeline
- [x] Parse pipeline with graph-level attributes
- [x] Parse multi-line node attributes
- [x] Validate: missing start/exit node -> error
- [x] Execute linear 3-node pipeline end-to-end
- [x] Execute with conditional branching
- [x] Execute with retry on failure
- [x] Goal gate blocks exit when unsatisfied
- [x] Goal gate allows exit when all satisfied
- [x] Wait.human presents choices and routes on selection
- [x] Wait.human with freeform edge routes free-text input
- [x] Edge selection: condition match wins over weight
- [x] Edge selection: weight breaks ties
- [x] Edge selection: lexical tiebreak
- [x] Context updates visible to next node
- [x] Checkpoint save and resume
- [x] Stylesheet applies model override by class/ID
- [x] Prompt variable expansion ($goal)
- [x] Parallel fan-out and fan-in
- [x] Custom handler registration and execution
- [x] Pipeline with 10+ nodes (via integration tests)

Not yet verified:
- [ ] Validate: orphan node -> warning (reachability rule exists, need to verify it's warning not error)
- [ ] Stylesheet applies by shape name (spec says shape selectors, implementation has `*`, `.class`, `#id`)
