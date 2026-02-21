# Spec Compliance Review: Sections 7-9 (Subagents + Definition of Done)

## Section 7: Subagents

### 7.1 Concept -- ALIGNED
- `SubAgent` in `subagent.rs` spawns a child session via `SubAgentManager::spawn()` which takes a `Session` and runs `session.process_input()` in a tokio task.
- The child session has its own conversation history (its own `History` instance).
- The child session shares the parent's execution environment (passed through the `SessionFactory` / session construction).

### 7.2 Spawn Interface -- ALIGNED
All four tools are implemented in `subagent.rs`:
- `spawn_agent`: Correct params (`task` required, `working_dir`/`model`/`max_turns` optional). Returns agent ID.
- `send_input`: Correct params (`agent_id`, `message` required). Returns acknowledgement.
- `wait`: Correct params (`agent_id` required). Returns `SubAgentResult` (output, success, turns_used).
- `close_agent`: Correct params (`agent_id` required). Returns final status.

**GAP**: `spawn_agent` tool executor does not use the `working_dir`, `model`, or `max_turns` optional parameters. They are defined in the schema but ignored in the executor at line 185-197. The session factory creates a default session regardless of these overrides.

### 7.3 SubAgent Lifecycle -- ALIGNED (with one minor gap)
- `SubAgentResult` record: matches spec (`output: String`, `success: bool`, `turns_used: usize`).
- `SubAgent` struct has `id` and `depth` fields but no explicit `status` enum (`"running" | "completed" | "failed"`). Status is implicit via whether the tokio task is running.
- `SubAgentHandle` is not a separate record; `SubAgent` serves this role.
- Depth limiting: Implemented in `SubAgentManager::spawn()` at line 56 (`depth >= self.max_depth`). Default `max_subagent_depth: 1` in `SessionConfig`.
- Independent history: Each subagent gets its own `Session` with its own `History`.

**GAP**: No explicit `SubAgentHandle` record with a `status` field as spec defines. Status is implicit.

### 7.4 Use Cases -- ALIGNED
The architecture supports all listed use cases (parallel exploration, focused refactoring, test execution, alternative approaches) through the spawn/wait/close interface. Subagents run as independent tokio tasks sharing the execution environment.

---

## Section 9: Definition of Done

### 9.1 Core Loop

| Item | Status | Evidence |
|------|--------|----------|
| Session created with ProviderProfile + ExecutionEnvironment | DONE | `Session::new(client, profile, env, config)` in `session.rs:37` |
| `process_input()` runs agentic loop | DONE | `session.rs:118-157` -- LLM call -> tool exec -> loop |
| Natural completion (text only, no tool calls) | DONE | `session.rs:259-261` -- breaks when `tool_calls.is_empty()` |
| Round limits (`max_tool_rounds_per_input`) | DONE | `session.rs:184-191` -- checked each iteration |
| Session turn limits (`max_turns`) | DONE | `session.rs:194-201` -- checked each iteration |
| Abort signal -> CLOSED | DONE | `session.rs:204-207` -- checks `abort_flag`, transitions to Closed |
| Loop detection -> warning SteeringTurn | DONE | `session.rs:278-290` -- calls `detect_loop`, injects Steering turn |
| Multiple sequential inputs | DONE | Test `sequential_inputs` in `session.rs:1437-1457` confirms this works |

**Result: 8/8 DONE**

### 9.2 Provider Profiles

| Item | Status | Evidence |
|------|--------|----------|
| OpenAI profile with `apply_patch` (v4a) | DONE | `profiles/openai.rs` -- registers `apply_patch`, full v4a parser + applier |
| Anthropic profile with `edit_file` (old_string/new_string) | DONE | `profiles/anthropic.rs` -- registers `edit_file` tool |
| Gemini profile with gemini-cli-aligned tools | DONE | `profiles/gemini.rs` -- registers read/write/edit/shell/grep/glob |
| Each profile has provider-specific system prompt | DONE | Each profile's `build_system_prompt()` includes identity + env context + tool guidance |
| Custom tools can be registered | DONE | `tool_registry_mut()` exposed on `ProviderProfile` trait, `ToolRegistry::register()` available |
| Tool name collisions resolved (override) | DONE | `ToolRegistry::register()` uses `HashMap::insert` which overwrites. Test `name_collision_overrides` in `tool_registry.rs:111` |

**Result: 6/6 DONE**

### 9.3 Tool Execution

| Item | Status | Evidence |
|------|--------|----------|
| Tool calls dispatched through ToolRegistry | DONE | `session.rs:352-353` -- `registry.get(tool_name)` |
| Unknown tool -> error result to LLM | DONE | `session.rs:386-393` -- returns `is_error: true` with "Unknown tool" |
| Tool argument JSON validated against schema | DONE | `session.rs:356-366` -- `validate_tool_args()` using `jsonschema` crate |
| Tool execution errors caught and returned as error results | DONE | `session.rs:377-384` -- `Err(err)` mapped to `is_error: true` |
| Parallel tool execution when `supports_parallel_tool_calls` | DONE | `session.rs:400-404` -- routes to `execute_tool_calls_parallel` when supported |

**Result: 5/5 DONE**

### 9.4 Execution Environment

| Item | Status | Evidence |
|------|--------|----------|
| `LocalExecutionEnvironment` implements all file/command ops | DONE | `local_env.rs` -- read/write/exists/list/exec/grep/glob all implemented |
| Command timeout default is 10 seconds | DONE | `config.rs:23` -- `default_command_timeout_ms: 10_000` |
| Command timeout overridable per-call via `timeout_ms` param | DONE | `tools.rs:180-184` -- shell tool reads `timeout_ms` from args |
| Timed-out: SIGTERM then SIGKILL after 2 seconds | DONE | `local_env.rs:152-173` -- sends SIGTERM, waits 2s, then SIGKILL |
| Env var filtering excludes sensitive variables | DONE | `local_env.rs:31-38` -- filters `*_API_KEY`, `*_SECRET`, `*_TOKEN`, `*_PASSWORD`, `*_CREDENTIAL` |
| `ExecutionEnvironment` interface implementable by consumers | DONE | `execution_env.rs:27` -- `trait ExecutionEnvironment: Send + Sync` with all methods |

**Result: 6/6 DONE**

### 9.5 Tool Output Truncation

| Item | Status | Evidence |
|------|--------|----------|
| Character-based truncation runs FIRST | DONE | `truncation.rs:99-109` -- char truncation applied first |
| Line-based truncation runs SECOND (shell:256, grep:200, glob:500) | DONE | `truncation.rs:111-121` -- line truncation after chars; `default_line_limits()` has correct values |
| Truncation inserts visible marker | DONE | `truncation.rs:54-55, 62-63` -- `[WARNING: Output truncated...]` markers |
| Full untruncated output in `TOOL_CALL_END` event | DONE | `session.rs:554-571` -- emits event with full output BEFORE truncation |
| Default char limits match spec Section 5.2 | DONE | `truncation.rs:10-21` -- read_file:50k, shell:30k, grep:20k, glob:20k, edit_file:10k, write_file:1k |
| Both char and line limits overridable via `SessionConfig` | DONE | `config.rs:10-11` -- `tool_output_limits` and `tool_line_limits` HashMaps; `truncation.rs:100-103, 112-116` checks config first |

**Result: 6/6 DONE**

### 9.6 Steering

| Item | Status | Evidence |
|------|--------|----------|
| `steer()` queues message injected after current tool round | DONE | `session.rs:80-85` -- pushes to `steering_queue`; `session.rs:275` -- `drain_steering()` called after tool execution |
| `follow_up()` queues message processed after current input completes | DONE | `session.rs:87-92` -- pushes to `followup_queue`; `session.rs:136-146` -- processed after `run_single_input` |
| Steering messages appear as SteeringTurn in history | DONE | `session.rs:304` -- `Turn::Steering` pushed to history |
| SteeringTurns converted to user-role messages for LLM | DONE | `history.rs:75-81` -- `Turn::Steering` maps to `Role::User` message |

**Result: 4/4 DONE**

### 9.7 Reasoning Effort

| Item | Status | Evidence |
|------|--------|----------|
| `reasoning_effort` passed through to LLM SDK Request | DONE | `session.rs:340` -- `reasoning_effort: self.config.reasoning_effort.clone()` |
| Changing mid-session takes effect on next LLM call | DONE | `session.rs:110-112` -- `set_reasoning_effort()` mutates config; test at line 1722 confirms |
| Valid values: "low", "medium", "high", null | DONE | Stored as `Option<String>` and passed through to SDK; no validation in this layer (SDK handles it) |

**Result: 3/3 DONE**

### 9.8 System Prompts

| Item | Status | Evidence |
|------|--------|----------|
| Provider-specific base instructions | DONE | Each profile has distinct identity text ("You are Claude...", "You are a coding assistant", "powered by Gemini") |
| Environment context (platform, git, working dir, date, model) | DONE | `profiles/mod.rs:21-48` -- `build_env_context_block` includes platform, working_directory, OS version, git branch, date, model |
| Tool descriptions from active profile | DONE | `session.rs:323` -- `self.provider_profile.tools()` included in request |
| Project docs (AGENTS.md + provider-specific) discovered and included | DONE | `project_docs.rs` -- discovers AGENTS.md plus provider-specific files |
| User instruction overrides appended last | NOT DONE | No mechanism for user instruction overrides in the system prompt pipeline. The `build_system_prompt` method appends project docs but has no separate "user overrides" parameter. |
| Only relevant project files loaded per provider | DONE | `project_docs.rs:13-18` -- filters by provider_id: anthropic gets CLAUDE.md, openai gets .codex/instructions.md, gemini gets GEMINI.md |

**Result: 5/6 DONE**

**GAP**: No explicit user instruction override mechanism in the system prompt. The spec says "User instruction overrides are appended last (highest priority)". The current `build_system_prompt` takes `project_docs` but has no separate parameter or config field for user-supplied instruction overrides.

### 9.9 Subagents

| Item | Status | Evidence |
|------|--------|----------|
| Subagents spawned with scoped task via `spawn_agent` tool | DONE | `subagent.rs:153-200` |
| Subagents share parent's execution environment | DONE | Session factory creates session with shared env |
| Subagents maintain independent conversation history | DONE | Each Session has its own History |
| Depth limiting prevents recursive spawning (default max: 1) | DONE | `subagent.rs:56` and `config.rs:30` -- `max_subagent_depth: 1` |
| Subagent results returned to parent as tool results | DONE | `wait` tool returns formatted result string |
| `send_input`, `wait`, `close_agent` tools work correctly | DONE | All three tools implemented with correct params and tested |

**GAP (minor)**: The `spawn_agent` tool ignores `working_dir`, `model`, and `max_turns` optional parameters. They are in the schema but not wired to session creation.

**Result: 6/6 DONE** (core behavior works; optional param wiring is a gap but not a DoD blocker)

### 9.10 Event System

| Item | Status | Evidence |
|------|--------|----------|
| All event kinds from Section 2.9 emitted at correct times | PARTIAL | Most events are emitted. `ASSISTANT_TEXT_START` is defined in `EventKind` enum but never emitted in `session.rs`. `CONTEXT_WINDOW_WARNING` is emitted but not in the spec's enum (it's an extension). |
| Events delivered via async iterator / equivalent | DONE | `event.rs` -- `tokio::sync::broadcast` channel with `subscribe()` returning `Receiver<SessionEvent>` |
| `TOOL_CALL_END` events carry full untruncated output | DONE | `session.rs:554-571` -- emits before truncation |
| Session lifecycle events (SESSION_START, SESSION_END) bracket session | DONE | `session.rs:123-127` emits SessionStart; `session.rs:150-154` emits SessionEnd |

**Result: 3/4 DONE, 1 PARTIAL**

**GAP**: `AssistantTextStart` event kind is defined in the enum but never emitted anywhere in the session code. The spec lists `ASSISTANT_TEXT_START` as a required event.

### 9.11 Error Handling

| Item | Status | Evidence |
|------|--------|----------|
| Tool execution errors -> error result sent to LLM | DONE | `session.rs:377-384` -- tool errors returned as `is_error: true` ToolResult |
| LLM API transient errors -> retry with backoff (via SDK) | DONE | Spec explicitly says "handled by Unified LLM SDK layer" |
| Authentication errors -> surface immediately, session CLOSED | DONE | `session.rs:226-229` -- `is_auth_error()` check, transitions to Closed |
| Context window overflow -> emit warning event | DONE | `session.rs:629-654` -- `check_context_usage()` emits `ContextWindowWarning` |
| Graceful shutdown: abort -> cancel -> kill -> flush -> SESSION_END | PARTIAL | Abort flag checked, returns `AgentError::Aborted`, transitions to Closed. But `SESSION_END` is NOT emitted on abort (the abort short-circuits before the `emit(SessionEnd)` call). Also no explicit process killing on abort -- the session just stops looping. |

**Result: 4/5 DONE, 1 PARTIAL**

**GAP**: On abort, `SESSION_END` event is not emitted. The abort at `session.rs:204-207` returns an `Err(AgentError::Aborted)` which skips the `SessionEnd` emit at line 150-154. Running processes are not explicitly killed on abort either (only the loop stops).

---

## Summary of All Gaps

### Functional Gaps (should fix):

1. **Spawn agent ignores optional params** (`subagent.rs:185-197`): `working_dir`, `model`, `max_turns` params are in the tool schema but the executor does not use them when creating the session. The session factory ignores these overrides.

2. **`AssistantTextStart` event never emitted** (`session.rs`): The event kind exists in the enum but is never emitted. Should be emitted before/when the LLM starts generating text.

3. **No `SESSION_END` event on abort** (`session.rs:204-207`): When abort triggers, the method returns early with `Err(AgentError::Aborted)` without emitting `SESSION_END`. The spec says graceful shutdown should "flush events -> emit SESSION_END".

4. **No user instruction overrides in system prompt** (`session.rs` / `ProviderProfile`): Spec 9.8 item 5 says "User instruction overrides are appended last (highest priority)". There is no mechanism to pass user instruction overrides into the system prompt pipeline. `SessionConfig` lacks an `instructions` or `user_overrides` field.

### Minor / Non-blocking Gaps:

5. **No explicit `SubAgentHandle` with `status` field**: Status is implicit based on tokio task state rather than an explicit enum field. Functionally equivalent but structurally different from spec.

6. **`SESSION_END` not emitted on abort path for running processes**: No explicit kill of running child processes on abort. The session just stops the loop, but any child process from `exec_command` may continue running. The `close()` method for subagents does handle this properly.
