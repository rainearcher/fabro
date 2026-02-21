# Spec Compliance Review: Sections 5-6

## Section 5: State and Context

### 5.1 Context

**ALIGNED** (with minor gaps)

The `Context` struct in `/Users/bhelmkamp/p/brynary/attractor-rust/crates/attractor/src/context.rs` correctly implements:

- Thread-safe key-value store using `Arc<RwLock<HashMap<String, Value>>>` (context.rs:9)
- Append-only logs using `Arc<RwLock<Vec<String>>>` (context.rs:10)
- `set(key, value)` with write lock (context.rs:33-38)
- `get(key)` with read lock, returns `Option<Value>` (context.rs:46-52) -- spec says `default=NONE` which maps to Rust's `Option::None`
- `get_string(key, default)` with string coercion (context.rs:56-60)
- `append_log(entry)` with write lock (context.rs:67-72)
- `snapshot()` returning a cloned map (context.rs:80-85)
- `clone_context()` for deep copy / parallel isolation (context.rs:99-106) -- called `clone_context` instead of `clone` to avoid conflict with Rust's `Clone` trait
- `apply_updates(updates)` merging a map into context (context.rs:113-118)

**Minor gap**: `get()` does not accept a `default` parameter like the spec's `get(key, default=NONE)`. The Rust version returns `Option<Value>` instead, which is idiomatic but means callers must handle the default themselves. This is an acceptable Rust adaptation.

**Built-in context keys** set by the engine:

| Key | Status | Evidence |
|-----|--------|----------|
| `outcome` | ALIGNED | engine.rs:533 sets `context.set("outcome", ...)` |
| `preferred_label` | ALIGNED | engine.rs:535 sets `context.set("preferred_label", ...)` |
| `graph.goal` | ALIGNED | engine.rs:349-351 mirrors graph goal |
| `current_node` | ALIGNED | engine.rs:492 sets `context.set("current_node", ...)` |
| `last_stage` | ALIGNED | codergen.rs:100-103 sets `last_stage` via context_updates |
| `last_response` | ALIGNED | codergen.rs:104-107 sets `last_response` (truncated to 200 chars) |
| `internal.retry_count.<node_id>` | **GAP** | Not implemented anywhere. The engine tracks retry attempts locally in `execute_with_retry` but never writes `internal.retry_count.<node_id>` to the context. |

**Context key namespace conventions**: The code uses `graph.*` namespace (engine.rs:354) and `context.*` would be user-driven. No enforcement of namespaces exists (which is expected -- they're conventions).

### 5.2 Outcome

**ALIGNED**

The `Outcome` struct in `/Users/bhelmkamp/p/brynary/attractor-rust/crates/attractor/src/outcome.rs` matches the spec exactly:

- `status: StageStatus` (outcome.rs:49) -- all five values present: `Success`, `Fail`, `PartialSuccess`, `Retry`, `Skipped` (outcome.rs:10-16)
- `preferred_label: Option<String>` (outcome.rs:51)
- `suggested_next_ids: Vec<String>` (outcome.rs:53)
- `context_updates: HashMap<String, Value>` (outcome.rs:55)
- `notes: Option<String>` (outcome.rs:57)
- `failure_reason: Option<String>` (outcome.rs:59)

Factory methods: `success()`, `fail(reason)`, `retry(reason)`, `skipped()` all present (outcome.rs:63-108).

Serialization with `serde` roundtrips correctly (outcome.rs:173-188).

### 5.3 Checkpoint

**ALIGNED** (with minor gaps)

The `Checkpoint` struct in `/Users/bhelmkamp/p/brynary/attractor-rust/crates/attractor/src/checkpoint.rs` implements:

- `timestamp: DateTime<Utc>` (checkpoint.rs:14)
- `current_node: String` (checkpoint.rs:15)
- `completed_nodes: Vec<String>` (checkpoint.rs:16)
- `node_retries: HashMap<String, u32>` (checkpoint.rs:17)
- `context_values: HashMap<String, Value>` (checkpoint.rs:18)
- `logs: Vec<String>` (checkpoint.rs:19)
- `save(path)` serializes to JSON (checkpoint.rs:44-49)
- `load(path)` deserializes from JSON (checkpoint.rs:56-61)

**GAP: node_retries not populated by engine**: The `Checkpoint::from_context` (checkpoint.rs:24-37) always initializes `node_retries` to an empty map. The engine (engine.rs:539-543) never populates retry counts into the checkpoint. Tests manually set retries (checkpoint.rs:103) but the engine never does.

**GAP: Resume behavior not implemented**: Spec 5.3 describes a 6-step resume process (load checkpoint, restore context, restore completed_nodes, restore retry counters, determine next node, degrade fidelity). The engine has no `resume_from_checkpoint` method. The `Checkpoint::load` exists but nothing consumes it for resumption.

### 5.4 Context Fidelity

**GAP** (data model present, runtime not implemented)

The spec defines `FidelityMode` with values: `full`, `truncate`, `compact`, `summary:low`, `summary:medium`, `summary:high`.

- Graph types support `fidelity` attribute on nodes (graph/types.rs:159-160) and edges (graph/types.rs:257-258)
- Graph supports `default_fidelity` (graph/types.rs:368-372)
- Nodes and edges support `thread_id` attribute (graph/types.rs:164-165, 262-263)
- Validation rule `fidelity_valid` validates fidelity modes (validation/rules.rs:381-446)

**However**:
- No `FidelityMode` enum exists as a first-class type -- fidelity is only a string attribute
- The fidelity resolution precedence (edge -> node -> graph default -> `compact`) is not implemented in the engine
- Thread resolution for `full` fidelity is not implemented
- The engine does not use fidelity to control context passing between nodes
- No session reuse / thread management exists

### 5.5 Artifact Store

**ALIGNED**

The `ArtifactStore` in `/Users/bhelmkamp/p/brynary/attractor-rust/crates/attractor/src/artifact.rs` fully implements the spec:

- `store(id, name, data) -> ArtifactInfo` (artifact.rs:62-96) with file-backing for large artifacts
- `retrieve(id) -> Value` (artifact.rs:107-130) reading from memory or disk
- `has(id) -> bool` (artifact.rs:137-142)
- `list() -> Vec<ArtifactInfo>` (artifact.rs:150-157)
- `remove(id)` (artifact.rs:164-169) including disk cleanup
- `clear()` (artifact.rs:176-184) including disk cleanup
- `FILE_BACKING_THRESHOLD = 100 * 1024` (artifact.rs:12) matching spec's 100KB

`ArtifactInfo` fields match spec (artifact.rs:16-22):
- `id`, `name`, `size_bytes`, `stored_at`, `is_file_backed`

Thread safety via `RwLock` (artifact.rs:33).

### 5.6 Run Directory Structure

**PARTIALLY ALIGNED**

Spec directory structure:
```
{logs_root}/
    checkpoint.json              -- present (engine.rs:544)
    manifest.json                -- MISSING
    {node_id}/
        status.json              -- present (codergen.rs:82, 111)
        prompt.md                -- present (codergen.rs:74)
        response.md              -- present (codergen.rs:95)
    artifacts/
        {artifact_id}.json       -- present (artifact.rs:75)
```

**GAP: manifest.json**: The spec requires a `manifest.json` with pipeline metadata (name, goal, start time). This file is never written by the engine.

**GAP: Per-node directories only for codergen**: Only the `CodergenHandler` creates `{node_id}/` subdirectories with `status.json`, `prompt.md`, and `response.md`. Other handlers (start, exit, tool, parallel, etc.) do not write any per-node log files.

---

## Section 6: Human-in-the-Loop (Interviewer Pattern)

### 6.1 Interviewer Interface

**ALIGNED**

The `Interviewer` trait in `/Users/bhelmkamp/p/brynary/attractor-rust/crates/attractor/src/interviewer/mod.rs` matches the spec:

- `ask(question: Question) -> Answer` (mod.rs:133)
- `ask_multiple(questions: Vec<Question>) -> Vec<Answer>` with default sequential implementation (mod.rs:135-140)
- `inform(message, stage)` with default no-op (mod.rs:143-145)

The trait is `async` (using `#[async_trait]`) and requires `Send + Sync` (mod.rs:132), which is appropriate for Rust.

### 6.2 Question Model

**ALIGNED**

`Question` struct (mod.rs:29-38):
- `text: String`
- `question_type: QuestionType` (named `question_type` instead of `type` since `type` is a Rust keyword)
- `options: Vec<QuestionOption>`
- `allow_freeform: bool`
- `default: Option<Answer>`
- `timeout_seconds: Option<f64>`
- `stage: String`
- `metadata: HashMap<String, Value>`

`QuestionType` enum (mod.rs:13-18):
- `YesNo`, `MultipleChoice`, `Freeform`, `Confirmation` -- all four spec variants present

`QuestionOption` (mod.rs:21-24):
- `key: String`, `label: String` -- matches spec's `Option` (renamed to avoid Rust keyword collision)

### 6.3 Answer Model

**ALIGNED**

`Answer` struct (mod.rs:68-72):
- `value: AnswerValue`
- `selected_option: Option<QuestionOption>`
- `text: Option<String>`

`AnswerValue` enum (mod.rs:57-64):
- `Yes`, `No`, `Skipped`, `Timeout` -- matches spec
- `Selected(String)` -- represents a multiple-choice selection (spec used `value: String`)
- `Text(String)` -- represents freeform text

The spec uses a single `value` field that can be either an `AnswerValue` enum or a string. The Rust implementation cleanly separates these via enum variants, which is a good adaptation.

### 6.4 Built-In Interviewer Implementations

**AutoApproveInterviewer: ALIGNED**

`/Users/bhelmkamp/p/brynary/attractor-rust/crates/attractor/src/interviewer/auto_approve.rs`:
- YesNo/Confirmation -> `Answer::yes()` (auto_approve.rs:12)
- MultipleChoice -> first option or "auto-approved" text (auto_approve.rs:13-20)
- Freeform -> `Answer::text("auto-approved")` (auto_approve.rs:21)
- Matches spec pseudocode exactly

**ConsoleInterviewer: GAP (not implemented)**

No `ConsoleInterviewer` exists in the codebase. Grep for `ConsoleInterviewer` returns no matches. The spec describes a CLI-based interviewer that reads from stdin.

**CallbackInterviewer: ALIGNED**

`/Users/bhelmkamp/p/brynary/attractor-rust/crates/attractor/src/interviewer/callback.rs`:
- Accepts a `Fn(Question) -> Answer` callback (callback.rs:7)
- `ask()` delegates to callback (callback.rs:21)
- Matches spec exactly

**QueueInterviewer: ALIGNED**

`/Users/bhelmkamp/p/brynary/attractor-rust/crates/attractor/src/interviewer/queue.rs`:
- Pre-filled `VecDeque<Answer>` (queue.rs:10)
- `ask()` dequeues or returns `Answer::skipped()` (queue.rs:24-26)
- Thread-safe via `Mutex` (queue.rs:10)
- Matches spec exactly

**RecordingInterviewer: ALIGNED**

`/Users/bhelmkamp/p/brynary/attractor-rust/crates/attractor/src/interviewer/recording.rs`:
- Wraps an inner `Box<dyn Interviewer>` (recording.rs:9)
- Records `(Question, Answer)` pairs in `Mutex<Vec<...>>` (recording.rs:10)
- `ask()` delegates to inner, then records (recording.rs:32-38)
- `recordings()` accessor (recording.rs:25-27)
- Matches spec exactly

### 6.5 Timeout Handling

**GAP** (partially modeled, not implemented at runtime)

- The `Question` struct has a `timeout_seconds: Option<f64>` field (mod.rs:35)
- The `Answer` has a `timeout()` factory (mod.rs:103-108) and `AnswerValue::Timeout` variant (mod.rs:61)
- The `Question` struct has a `default: Option<Answer>` field (mod.rs:34)

**However**:
- No interviewer implementation actually enforces timeouts (no tokio timeout wrapper)
- The spec's timeout behavior (use default if available, else return Timeout) is not implemented in any interviewer
- `wait.human` node's `human.default_choice` for timeout behavior is not checked at runtime

---

## Summary of Gaps

| Section | Status | Gap Description |
|---------|--------|-----------------|
| 5.1 | ALIGNED (minor) | `internal.retry_count.<node_id>` context key never written by engine |
| 5.2 | ALIGNED | Fully matches spec |
| 5.3 | GAP | Checkpoint save works but resume from checkpoint not implemented; node_retries never populated by engine |
| 5.4 | GAP | Fidelity attributes parsed and validated, but fidelity resolution/application not in engine; no session/thread management |
| 5.5 | ALIGNED | Fully matches spec |
| 5.6 | GAP | Missing `manifest.json`; per-node directories only created by CodergenHandler, not other handlers |
| 6.1 | ALIGNED | Trait matches spec interface |
| 6.2 | ALIGNED | Question model complete |
| 6.3 | ALIGNED | Answer model complete |
| 6.4 | GAP | Missing `ConsoleInterviewer`; other four implementations aligned |
| 6.5 | GAP | Timeout data model present but no runtime enforcement in any interviewer |
