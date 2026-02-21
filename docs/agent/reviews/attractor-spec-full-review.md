# Attractor Spec Compliance Review

Full review of `crates/attractor/` against `docs/specs/attractor-spec.md`.
Reviewed 2026-02-20 by 5 parallel agents. Second-pass false-positive analysis applied.

---

## Section 1: Overview and Goals

| # | Sub-section | Verdict | Notes |
|---|------------|---------|-------|
| 1 | 1.1 Problem Statement | ALIGNED | Narrative; no code requirements |
| 2 | 1.2 Why DOT Syntax | ALIGNED | Parser accepts DOT as specified |
| 3 | 1.3 Design Principles | ALIGNED | Graph layer supports pluggable handlers, checkpoint, HITL, edge routing |
| 4 | 1.4 Layering and LLM Backends | ALIGNED | Backend-agnostic; `CodergenBackend` trait exists |

---

## Section 2: DOT DSL Schema

| # | Sub-section | Verdict | Notes |
|---|------------|---------|-------|
| 5 | 2.1 Supported Subset | ALIGNED | Strict DOT subset enforced; one digraph per file (`parser/mod.rs:23-29`) |
| 6 | 2.2 BNF Grammar | ALIGNED | All productions implemented in `grammar.rs` and `lexer.rs` |
| 7 | 2.3 Key Constraints | ALIGNED | Directed-only, commas required, optional semicolons, comments stripped |
| 8 | 2.4 Value Types | ALIGNED | String, Integer, Float, Boolean, Duration all supported |
| 9 | 2.5 Graph Attributes | ALIGNED | `goal`, `model_stylesheet`, `default_max_retry`, `retry_target`, `fallback_retry_target`, `default_fidelity` all have accessors |
| 10 | 2.6 Node Attributes | ALIGNED | All 17 node attributes have typed accessors in `graph/types.rs` |
| 11 | 2.7 Edge Attributes | ALIGNED | All 7 edge attributes implemented |
| 12 | 2.8 Shape-to-Handler Mapping | ALIGNED | Complete 9-shape mapping at `types.rs:72-85` with tests |
| 13 | 2.9 Chained Edges | ALIGNED | `A -> B -> C` expanded via `windows(2)` in semantic analysis |
| 14 | 2.10 Subgraphs | ALIGNED | Scoped defaults, class derivation from label |
| 15 | 2.11 Default Blocks | ALIGNED | `node [...]` and `edge [...]` defaults applied correctly |
| 16 | 2.12 Class Attribute | ALIGNED | Comma-separated, trimmed, deduplicated |
| 17 | 2.13 Minimal Examples | ALIGNED | All 3 spec examples parse; tested |

**Minor gaps in Section 2:**
- `Direction` values (`TB`/`LR`/`BT`/`RL`) not validated to allowed set
- No explicit error for `strict` modifier or undirected `graph` keyword
- `Graph` missing a `label()` convenience accessor
- `Node::max_retries()` returns `Option<i64>` instead of defaulting to `0`
- Subgraph class derivation only checks `GraphAttrDecl`, not `graph [label=...]` block form

---

## Section 3: Pipeline Execution Engine

| # | Sub-section | Verdict | Notes |
|---|------------|---------|-------|
| 18 | 3.1 Run Lifecycle | ALIGNED | 5 phases present; FINALIZE does not clean up resources (minor) |
| 19 | 3.2 Core Execution Loop | GAP | `loop_restart` (step 7) not implemented -- just jumps to target |
| 20 | 3.3 Edge Selection | ALIGNED | All 5 steps match spec; `normalize_label` handles prefixes |
| 21 | 3.4 Goal Gate Enforcement | ALIGNED | 4-level retry_target fallback implemented |
| 22 | 3.5 Retry Logic | GAP | `should_retry` predicate missing; retry counter not tracked; `reset_retry_counter` absent |
| 23 | 3.6 Retry Policy | GAP | Presets defined but never selectable from node attributes; `default_max_retry=50` vs spec default `0` |
| 24 | 3.7 Failure Routing | GAP | retry_target/fallback_retry_target not consulted on node FAIL -- only used for goal gates |
| 25 | 3.8 Concurrency Model | ALIGNED | Single-threaded traversal; parallel handler manages branches |

---

## Section 4: Node Handlers

| # | Sub-section | Verdict | Notes |
|---|------------|---------|-------|
| 26 | 4.1 Handler Interface | ALIGNED | `Handler` trait matches spec signature |
| 27 | 4.2 Handler Registry | ALIGNED | 3-step priority: explicit type > shape > default |
| 28 | 4.3 Start Handler | ALIGNED | Returns SUCCESS immediately |
| 29 | 4.4 Exit Handler | ALIGNED | Returns SUCCESS immediately |
| 30 | 4.5 Codergen Handler | ALIGNED | Prompt expansion, backend call, artifact writes all present |
| 31 | 4.6 Wait For Human | ALIGNED | Choices, freeform, accelerator keys, timeout/skip handling |
| 32 | 4.7 Conditional Handler | ALIGNED | Pass-through SUCCESS; routing via edge selection |
| 33 | 4.8 Parallel Handler | GAP | **Stub** -- no actual concurrent execution, no context cloning, no join/error policies |
| 34 | 4.9 Fan-In Handler | GAP | Heuristic select works; no LLM-based evaluation path |
| 35 | 4.10 Tool Handler | ALIGNED | Shell execution via `sh -c`; no command timeout (minor) |
| 36 | 4.11 Manager Loop Handler | GAP | **Stub** -- always returns FAIL |
| 37 | 4.12 Custom Handlers | ALIGNED | Trait + registry supports registration; panics not caught (minor) |

---

## Section 5: State and Context

| # | Sub-section | Verdict | Notes |
|---|------------|---------|-------|
| 38 | 5.1 PipelineContext | ALIGNED | Key-value store with get/set/merge; `internal.retry_count.<node_id>` never written (minor) |
| 39 | 5.2 Outcome Model | ALIGNED | All fields and status values present |
| 40 | 5.3 Checkpoint/Resume | GAP | `Checkpoint::save` works; `load` exists but **no resume logic in engine**; `node_retries` never populated |
| 41 | 5.4 Context Fidelity | GAP | Attributes parsed/validated but **fidelity resolution precedence and session/thread management not implemented** in engine |
| 42 | 5.5 Artifact Store | ALIGNED | Full implementation including file-backing |
| 43 | 5.6 Run Directory | GAP | Missing `manifest.json`; per-node dirs only created by CodergenHandler, not other handlers |

---

## Section 6: Human-in-the-Loop (Interviewer Pattern)

| # | Sub-section | Verdict | Notes |
|---|------------|---------|-------|
| 44 | 6.1 Interviewer Trait | ALIGNED | `ask(Question) -> Answer` interface |
| 45 | 6.2 Question Model | ALIGNED | Options, allow_freeform, timeout, stage |
| 46 | 6.3 Answer Model | ALIGNED | AnswerValue variants cover spec cases |
| 47 | 6.4 Built-in Interviewers | GAP | AutoApprove, Callback, Queue, Recording present; **ConsoleInterviewer missing** |
| 48 | 6.5 Timeout Handling | GAP | Data model present but **no runtime timeout enforcement** in any interviewer |

---

## Section 7: Validation and Linting

| # | Sub-section | Verdict | Notes |
|---|------------|---------|-------|
| 49 | 7.1 Diagnostic Model | ALIGNED | Struct matches spec exactly (rule, severity, message, node_id, edge, fix) |
| 50 | 7.2 Built-In Rules | GAP | 14 rules present; `start_node` and `terminal_node` only check shape, not ID fallback; `stylesheet_syntax` only checks brace balance |
| 51 | 7.3 Validation API | GAP | No `extra_rules` parameter on `validate()`/`validate_or_raise()` |
| 52 | 7.4 Custom Lint Rules | GAP | `LintRule` trait exists but **no registration mechanism** |

---

## Section 8: Model Stylesheet

| # | Sub-section | Verdict | Notes |
|---|------------|---------|-------|
| 53 | 8.1 Overview | ALIGNED | Stylesheet applied as transform after parsing |
| 54 | 8.2 Grammar | ALIGNED | `*`, `.class`, `#id` selectors; ClassName accepts uppercase (minor) |
| 55 | 8.3 Specificity | ALIGNED | Universal(0) < Class(1) < ID(2); explicit attrs never overridden |
| 56 | 8.4 Recognized Properties | ALIGNED | `llm_model`, `llm_provider`, `reasoning_effort` |
| 57 | 8.5 Application Order | ALIGNED | Explicit > stylesheet > default |
| 58 | 8.6 Example | ALIGNED | Spec example validated by dedicated test |

**Minor gap:** No shape selector support (e.g., `box { ... }`) -- spec section 11.10 mentions it.

---

## Section 9: Transforms and Extensibility

| # | Sub-section | Verdict | Notes |
|---|------------|---------|-------|
| 59 | 9.1 AST Transforms | GAP | Trait takes `&mut Graph` (in-place) vs spec's "returns new Graph"; no `prepare_pipeline` function |
| 60 | 9.2 Built-In Transforms | GAP | Variable expansion and stylesheet transforms present; **Preamble Transform missing** |
| 61 | 9.3 Custom Transforms | GAP | Trait is public but **no `register_transform` API** on engine |
| 62 | 9.4 Pipeline Composition | GAP | Manager loop stub; no graph merging transform |
| 63 | 9.5 HTTP Server Mode | ALIGNED | Spec says "may expose" -- not required |
| 64 | 9.6 Events | ALIGNED | All 16 event types defined and serializable |
| 65 | 9.7 Tool Call Hooks | GAP | `tool_hooks.pre`/`tool_hooks.post` not read or executed |

**Minor gaps in 9.6:** Parallel events and interview events defined but never emitted by their handlers.

---

## Section 10: Condition Expression Language

| # | Sub-section | Verdict | Notes |
|---|------------|---------|-------|
| 66 | 10.1 Overview | ALIGNED | Dedicated condition module |
| 67 | 10.2 Grammar | ALIGNED | `&&`-separated clauses, `=`/`!=` operators |
| 68 | 10.3 Semantics | ALIGNED | AND-combined, exact case-sensitive string comparison |
| 69 | 10.4 Variable Resolution | ALIGNED | `outcome`, `preferred_label`, `context.*`, missing-as-empty |
| 70 | 10.5 Evaluation | GAP | Bare key truthiness check returns error instead of evaluating as truthy |
| 71 | 10.6 Examples | ALIGNED | All spec examples verified by tests |
| 72 | 10.7 Extended Operators | ALIGNED | Not implemented (spec says future) |

---

## Section 11: Definition of Done

| # | Sub-section | Verdict | Notes |
|---|------------|---------|-------|
| 73 | 11.1 DOT Parsing | ALIGNED | Integration tests verify all spec examples |
| 74 | 11.2 Validation | ALIGNED | 14 rules, `validate_or_raise` blocks on errors |
| 75 | 11.3 Execution Engine | ALIGNED | Full loop, edge selection, handler dispatch |
| 76 | 11.4 Goal Gates | ALIGNED | Checked at terminal; retry_target fallback chain |
| 77 | 11.5 Retry Logic | ALIGNED | Exponential backoff with jitter; `allow_partial` |
| 78 | 11.6 Node Handlers | GAP | Manager loop stub; parallel stub |
| 79 | 11.7 State and Context | GAP | Checkpoint resume not implemented |
| 80 | 11.8 Human-in-the-Loop | GAP | No ConsoleInterviewer; no SINGLE_SELECT/MULTI_SELECT distinction |
| 81 | 11.9 Conditions | ALIGNED | With bare-key gap noted above |
| 82 | 11.10 Stylesheet | GAP | No shape selector |
| 83 | 11.11 Transforms | GAP | No `register_transform` API; no preamble transform |
| 84 | 11.12 Cross-Feature Matrix | GAP | Missing integration tests: retry-on-failure, checkpoint resume, 10+ node pipeline |
| 85 | 11.13 Integration Smoke Test | GAP | No end-to-end test with real LLM callback |

---

## Gap Summary by Severity (after false-positive analysis)

9 of 36 original gaps were false positives. **27 legitimate gaps remain.**

### False Positives Removed

| Original # | Reason |
|------------|--------|
| 8 (Retry Presets) | Spec does not require presets be selectable from node attributes |
| 9 (default_max_retry) | Spec attribute tables say default is 50; code matches |
| 24 (Direction) | Direction is a Graphviz layout hint, not an Attractor semantic attribute |
| 25 (strict/graph) | Implicit rejection via grammar is sufficient |
| 26 (Graph::label()) | Attribute is stored and accessible via `attrs` map |
| 27 (max_retries default) | Spec tables and code agree on graph-level default of 50 |
| 29 (Variable Expansion) | Spec says only `$goal` from graph; code matches exactly |
| 32 (Transform Signature) | `&mut Graph` is Rust-idiomatic; spec allows "modified graph" |
| 34 (QuestionType) | Spec 6.2 defines the actual types; 11.8 uses inconsistent names |

### HIGH (5 gaps -- blocking or core functionality missing)

| # | Location | Gap |
|---|----------|-----|
| 1 | 3.7 Failure Routing | retry_target/fallback_retry_target not consulted on node FAIL (only on goal gate) |
| 2 | 4.8 Parallel Handler | Stub -- no concurrent execution, no join/error policies |
| 3 | 4.11 Manager Loop | Stub -- always returns FAIL |
| 4 | 5.3 Checkpoint Resume | `Checkpoint::load` exists but engine has no resume-from-checkpoint logic |
| 5 | 5.4 Context Fidelity | Fidelity resolution and session/thread management not implemented |

### MEDIUM (15 gaps -- functional but incomplete)

| # | Location | Gap |
|---|----------|-----|
| 6 | 3.2 loop_restart | Not implemented -- jumps to target instead of restarting run |
| 7 | 3.5 should_retry | No retryable vs non-retryable error classification |
| 10 | 4.9 Fan-In | No LLM-based evaluation path |
| 11 | 4.12 Panic Safety | Handler panics not caught by engine |
| 12 | 5.6 Run Directory | Missing `manifest.json`; per-node dirs only from CodergenHandler |
| 13 | 6.4 ConsoleInterviewer | Not implemented |
| 14 | 6.5 Timeout Handling | No runtime timeout enforcement |
| 15 | 7.2 start_node rule | Only checks shape=Mdiamond, not ID-based fallback |
| 16 | 7.2 terminal_node rule | Only checks shape=Msquare, not ID-based fallback |
| 17 | 7.3/7.4 Custom Rules | LintRule trait exists but no registration or extra_rules param |
| 18 | 8.2/11.10 Shape Selector | Stylesheet only supports `*`, `.class`, `#id` -- no shape selector |
| 19 | 9.1 prepare_pipeline | No function chaining parse -> transforms -> validate |
| 20 | 9.2 Preamble Transform | Not implemented |
| 21 | 9.3 register_transform | No API on engine for registering transforms |
| 22 | 9.6 Event Emission | Parallel and interview events defined but never emitted |
| 23 | 9.7 Tool Call Hooks | pre/post hooks not read or executed |

### LOW (7 gaps -- minor, cosmetic, or optional)

| # | Location | Gap |
|---|----------|-----|
| 28 | 2.10 Subgraph label | Class derivation misses `graph [label=...]` block form |
| 30 | 4.10 Tool Timeout | No command timeout on shell execution |
| 31 | 8.2 ClassName | Accepts uppercase; spec says `[a-z0-9-]+` |
| 33 | 10.5 Bare Key | Returns error instead of truthiness check |
| 35 | 11.12 Test Coverage | Missing integration tests for retry, resume, 10+ nodes |
| 36 | 11.13 Smoke Test | No real LLM end-to-end test |

---

**Overall: 58 of 85 items ALIGNED. 27 legitimate gaps (5 HIGH, 15 MEDIUM, 7 LOW).**
