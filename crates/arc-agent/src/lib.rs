#[cfg(feature = "docker")]
pub mod docker_sandbox;

pub mod cli;
pub mod compaction;
pub mod config;
pub mod error;
pub mod event;
pub mod file_tracker;
pub mod history;
pub mod local_sandbox;
pub mod loop_detection;
pub mod mcp_integration;
pub mod profiles;
pub mod project_docs;
pub mod provider_profile;
pub mod read_before_write_sandbox;
pub mod sandbox;
pub mod session;
pub mod skills;
pub mod subagent;
pub mod tool_execution;
pub mod tool_registry;
pub mod tools;
pub mod truncation;
pub mod types;
pub mod v4a_patch;

pub use arc_mcp::config::McpServerConfig;
pub use config::{SessionConfig, ToolApprovalFn};
#[cfg(feature = "docker")]
pub use docker_sandbox::{DockerSandbox, DockerSandboxConfig};
pub use error::AgentError;
pub use event::EventEmitter;
pub use history::History;
pub use local_sandbox::LocalSandbox;
pub use loop_detection::detect_loop;
pub use profiles::{AnthropicProfile, EnvContext, GeminiProfile, OpenAiProfile};
pub use project_docs::discover_project_docs;
pub use provider_profile::{ProfileCapabilities, ProviderProfile};
pub use read_before_write_sandbox::ReadBeforeWriteSandbox;
pub use sandbox::{
    format_lines_numbered, DirEntry, ExecResult, GrepOptions, Sandbox, SandboxEvent,
    SandboxEventCallback,
};
pub use session::Session;
pub use skills::Skill;
pub use subagent::{SubAgent, SubAgentEventCallback, SubAgentManager, SubAgentResult};
pub use tool_registry::ToolRegistry;
pub use tools::{
    make_edit_file_tool, make_glob_tool, make_grep_tool, make_read_file_tool, make_shell_tool,
    make_shell_tool_with_config, make_write_file_tool, register_core_tools, WebFetchSummarizer,
};
pub use truncation::{
    floor_char_boundary, truncate_lines, truncate_output, truncate_tool_output, TruncationMode,
};
pub use types::{AgentEvent, SessionEvent, SessionState, Turn};

#[cfg(test)]
pub(crate) mod test_support;
