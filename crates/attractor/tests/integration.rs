use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use attractor::checkpoint::Checkpoint;
use attractor::context::Context;
use attractor::engine::{PipelineEngine, RunConfig};
use attractor::error::AttractorError;
use attractor::event::EventEmitter;
use attractor::graph::{AttrValue, Edge, Graph, Node};
use attractor::handler::codergen::{CodergenBackend, CodergenHandler, CodergenResult};
use attractor::handler::conditional::ConditionalHandler;
use attractor::handler::exit::ExitHandler;
use attractor::handler::start::StartHandler;
use attractor::handler::wait_human::WaitHumanHandler;
use attractor::handler::{Handler, HandlerRegistry};
use attractor::interviewer::queue::QueueInterviewer;
use attractor::interviewer::{Answer, AnswerValue};
use attractor::outcome::{Outcome, StageStatus};
use attractor::parser::parse;
use attractor::stylesheet::{apply_stylesheet, parse_stylesheet};
use attractor::transform::{StylesheetApplicationTransform, Transform, VariableExpansionTransform};
use attractor::validation::validate_or_raise;

// ---------------------------------------------------------------------------
// 1. Parse and validate all 3 spec examples (Section 2.13)
// ---------------------------------------------------------------------------

#[test]
fn parse_and_validate_simple_linear() {
    let input = r#"digraph Simple {
        graph [goal="Run tests and report"]
        rankdir=LR

        start [shape=Mdiamond, label="Start"]
        exit  [shape=Msquare, label="Exit"]

        run_tests [label="Run Tests", prompt="Run the test suite and report results"]
        report    [label="Report", prompt="Summarize the test results"]

        start -> run_tests -> report -> exit
    }"#;

    let graph = parse(input).expect("parsing should succeed");
    assert_eq!(graph.name, "Simple");
    assert_eq!(graph.goal(), "Run tests and report");
    assert_eq!(graph.nodes.len(), 4);
    assert_eq!(graph.edges.len(), 3);
    assert!(graph.find_start_node().is_some());
    assert!(graph.find_exit_node().is_some());

    let diagnostics = validate_or_raise(&graph, &[]).expect("validation should pass");
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == attractor::validation::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no validation errors");
}

#[test]
fn parse_and_validate_branching_with_conditions() {
    let input = r#"digraph Branch {
        graph [goal="Implement and validate a feature"]
        rankdir=LR
        node [shape=box, timeout="900s"]

        start     [shape=Mdiamond, label="Start"]
        exit      [shape=Msquare, label="Exit"]
        plan      [label="Plan", prompt="Plan the implementation"]
        implement [label="Implement", prompt="Implement the plan"]
        validate  [label="Validate", prompt="Run tests"]
        gate      [shape=diamond, label="Tests passing?"]

        start -> plan -> implement -> validate -> gate
        gate -> exit      [label="Yes", condition="outcome=success"]
        gate -> implement [label="No", condition="outcome!=success"]
    }"#;

    let graph = parse(input).expect("parsing should succeed");
    assert_eq!(graph.name, "Branch");
    assert_eq!(graph.nodes.len(), 6);
    assert_eq!(graph.edges.len(), 6);

    let gate_exit = graph
        .edges
        .iter()
        .find(|e| e.from == "gate" && e.to == "exit")
        .expect("gate -> exit edge should exist");
    assert_eq!(gate_exit.condition(), Some("outcome=success"));

    let gate_impl = graph
        .edges
        .iter()
        .find(|e| e.from == "gate" && e.to == "implement")
        .expect("gate -> implement edge should exist");
    assert_eq!(gate_impl.condition(), Some("outcome!=success"));

    let diagnostics = validate_or_raise(&graph, &[]).expect("validation should pass");
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == attractor::validation::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no validation errors");
}

#[test]
fn parse_and_validate_human_gate() {
    let input = r#"digraph Review {
        rankdir=LR

        start [shape=Mdiamond, label="Start"]
        exit  [shape=Msquare, label="Exit"]

        review_gate [
            shape=hexagon,
            label="Review Changes",
            type="wait.human"
        ]

        start -> review_gate
        review_gate -> ship_it [label="[A] Approve"]
        review_gate -> fixes   [label="[F] Fix"]
        ship_it -> exit
        fixes -> review_gate
    }"#;

    let graph = parse(input).expect("parsing should succeed");
    assert_eq!(graph.name, "Review");
    assert_eq!(graph.nodes.len(), 5);
    assert_eq!(graph.edges.len(), 5);

    let gate = &graph.nodes["review_gate"];
    assert_eq!(gate.node_type(), Some("wait.human"));
    assert_eq!(gate.shape(), "hexagon");
    assert_eq!(gate.label(), "Review Changes");

    let diagnostics = validate_or_raise(&graph, &[]).expect("validation should pass");
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == attractor::validation::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no validation errors");
}

// ---------------------------------------------------------------------------
// 2. End-to-end linear pipeline
// ---------------------------------------------------------------------------

fn make_linear_registry() -> HandlerRegistry {
    let mut registry = HandlerRegistry::new(Box::new(CodergenHandler::new(None)));
    registry.register("start", Box::new(StartHandler));
    registry.register("exit", Box::new(ExitHandler));
    registry.register("codergen", Box::new(CodergenHandler::new(None)));
    registry
}

#[tokio::test]
async fn end_to_end_linear_pipeline() {
    let input = r#"digraph Linear {
        graph [goal="Build the feature"]
        start [shape=Mdiamond]
        exit  [shape=Msquare]
        codergen_step [shape=box, label="Code", prompt="Implement the feature"]
        start -> codergen_step -> exit
    }"#;

    let graph = parse(input).expect("parse should succeed");
    validate_or_raise(&graph, &[]).expect("validation should pass");

    let dir = tempfile::tempdir().unwrap();
    let engine = PipelineEngine::new(make_linear_registry(), EventEmitter::new());
    let config = RunConfig {
        logs_root: dir.path().to_path_buf(),
    };

    let outcome = engine.run(&graph, &config).await.expect("run should succeed");
    assert_eq!(outcome.status, StageStatus::Success);

    // Checkpoint should exist
    let checkpoint_path = dir.path().join("checkpoint.json");
    assert!(checkpoint_path.exists(), "checkpoint.json should exist");

    let checkpoint = Checkpoint::load(&checkpoint_path).expect("checkpoint should load");
    assert!(checkpoint.completed_nodes.contains(&"start".to_string()));
    assert!(checkpoint
        .completed_nodes
        .contains(&"codergen_step".to_string()));

    // Codergen handler writes prompt.md, response.md, status.json
    let stage_dir = dir.path().join("codergen_step");
    assert!(stage_dir.join("prompt.md").exists(), "prompt.md should exist");
    assert!(
        stage_dir.join("response.md").exists(),
        "response.md should exist"
    );
    assert!(
        stage_dir.join("status.json").exists(),
        "status.json should exist"
    );

    let prompt_content = std::fs::read_to_string(stage_dir.join("prompt.md")).unwrap();
    assert_eq!(prompt_content, "Implement the feature");
}

// ---------------------------------------------------------------------------
// 3. End-to-end branching pipeline
// ---------------------------------------------------------------------------

#[tokio::test]
async fn end_to_end_branching_pipeline() {
    // Build a graph:
    //   start -> work -> gate (diamond)
    //   gate -> success_path [condition="outcome=success"]
    //   gate -> fail_path    [condition="outcome=fail"]
    //   success_path -> exit
    //   fail_path -> exit
    //
    // Since work defaults to codergen (shape=box) which returns SUCCESS,
    // the engine should route gate -> success_path via condition match.

    let mut graph = Graph::new("BranchTest");
    graph
        .attrs
        .insert("goal".to_string(), AttrValue::String("Test branching".to_string()));

    let mut start = Node::new("start");
    start
        .attrs
        .insert("shape".to_string(), AttrValue::String("Mdiamond".to_string()));
    graph.nodes.insert("start".to_string(), start);

    let mut exit = Node::new("exit");
    exit.attrs
        .insert("shape".to_string(), AttrValue::String("Msquare".to_string()));
    graph.nodes.insert("exit".to_string(), exit);

    let mut work = Node::new("work");
    work.attrs
        .insert("shape".to_string(), AttrValue::String("box".to_string()));
    work.attrs.insert(
        "prompt".to_string(),
        AttrValue::String("Do work".to_string()),
    );
    graph.nodes.insert("work".to_string(), work);

    let mut gate = Node::new("gate");
    gate.attrs
        .insert("shape".to_string(), AttrValue::String("diamond".to_string()));
    graph.nodes.insert("gate".to_string(), gate);

    graph
        .nodes
        .insert("success_path".to_string(), Node::new("success_path"));
    graph
        .nodes
        .insert("fail_path".to_string(), Node::new("fail_path"));

    graph.edges.push(Edge::new("start", "work"));
    graph.edges.push(Edge::new("work", "gate"));

    let mut gate_success = Edge::new("gate", "success_path");
    gate_success.attrs.insert(
        "condition".to_string(),
        AttrValue::String("outcome=success".to_string()),
    );
    graph.edges.push(gate_success);

    let mut gate_fail = Edge::new("gate", "fail_path");
    gate_fail.attrs.insert(
        "condition".to_string(),
        AttrValue::String("outcome=fail".to_string()),
    );
    graph.edges.push(gate_fail);

    graph.edges.push(Edge::new("success_path", "exit"));
    graph.edges.push(Edge::new("fail_path", "exit"));

    let dir = tempfile::tempdir().unwrap();
    let mut registry = HandlerRegistry::new(Box::new(CodergenHandler::new(None)));
    registry.register("start", Box::new(StartHandler));
    registry.register("exit", Box::new(ExitHandler));
    registry.register("codergen", Box::new(CodergenHandler::new(None)));
    registry.register("conditional", Box::new(ConditionalHandler));

    let engine = PipelineEngine::new(registry, EventEmitter::new());
    let config = RunConfig {
        logs_root: dir.path().to_path_buf(),
    };

    let outcome = engine.run(&graph, &config).await.expect("run should succeed");
    assert_eq!(outcome.status, StageStatus::Success);

    let checkpoint = Checkpoint::load(&dir.path().join("checkpoint.json")).unwrap();
    assert!(
        checkpoint
            .completed_nodes
            .contains(&"success_path".to_string()),
        "should have traversed success_path"
    );
    assert!(
        !checkpoint
            .completed_nodes
            .contains(&"fail_path".to_string()),
        "should NOT have traversed fail_path"
    );
}

// ---------------------------------------------------------------------------
// 4. End-to-end human gate pipeline with QueueInterviewer
// ---------------------------------------------------------------------------

#[tokio::test]
async fn end_to_end_human_gate_pipeline() {
    // Build a graph:
    //   start -> gate (hexagon, type=wait.human)
    //   gate -> approve [label="[A] Approve"]
    //   gate -> reject  [label="[R] Reject"]
    //   approve -> exit
    //   reject -> exit
    //
    // QueueInterviewer pre-filled to select "R" -> should route to reject

    let mut graph = Graph::new("HumanGateTest");

    let mut start = Node::new("start");
    start
        .attrs
        .insert("shape".to_string(), AttrValue::String("Mdiamond".to_string()));
    graph.nodes.insert("start".to_string(), start);

    let mut exit = Node::new("exit");
    exit.attrs
        .insert("shape".to_string(), AttrValue::String("Msquare".to_string()));
    graph.nodes.insert("exit".to_string(), exit);

    let mut gate = Node::new("gate");
    gate.attrs
        .insert("shape".to_string(), AttrValue::String("hexagon".to_string()));
    gate.attrs.insert(
        "type".to_string(),
        AttrValue::String("wait.human".to_string()),
    );
    gate.attrs.insert(
        "label".to_string(),
        AttrValue::String("Review Changes".to_string()),
    );
    graph.nodes.insert("gate".to_string(), gate);

    graph
        .nodes
        .insert("approve".to_string(), Node::new("approve"));
    graph
        .nodes
        .insert("reject".to_string(), Node::new("reject"));

    graph.edges.push(Edge::new("start", "gate"));

    let mut e_approve = Edge::new("gate", "approve");
    e_approve.attrs.insert(
        "label".to_string(),
        AttrValue::String("[A] Approve".to_string()),
    );
    graph.edges.push(e_approve);

    let mut e_reject = Edge::new("gate", "reject");
    e_reject.attrs.insert(
        "label".to_string(),
        AttrValue::String("[R] Reject".to_string()),
    );
    graph.edges.push(e_reject);

    graph.edges.push(Edge::new("approve", "exit"));
    graph.edges.push(Edge::new("reject", "exit"));

    // Pre-fill the queue with an answer selecting "R"
    let answers = VecDeque::from([Answer {
        value: AnswerValue::Selected("R".to_string()),
        selected_option: None,
        text: None,
    }]);
    let interviewer = Arc::new(QueueInterviewer::new(answers));

    let dir = tempfile::tempdir().unwrap();
    let mut registry = HandlerRegistry::new(Box::new(StartHandler));
    registry.register("start", Box::new(StartHandler));
    registry.register("exit", Box::new(ExitHandler));
    registry.register("wait.human", Box::new(WaitHumanHandler::new(interviewer)));

    let engine = PipelineEngine::new(registry, EventEmitter::new());
    let config = RunConfig {
        logs_root: dir.path().to_path_buf(),
    };

    let outcome = engine.run(&graph, &config).await.expect("run should succeed");
    assert_eq!(outcome.status, StageStatus::Success);

    let checkpoint = Checkpoint::load(&dir.path().join("checkpoint.json")).unwrap();
    assert!(
        checkpoint
            .completed_nodes
            .contains(&"reject".to_string()),
        "should have traversed reject path"
    );
    assert!(
        !checkpoint
            .completed_nodes
            .contains(&"approve".to_string()),
        "should NOT have traversed approve path"
    );
}

// ---------------------------------------------------------------------------
// 5. Goal gate enforcement
// ---------------------------------------------------------------------------

/// A custom handler that always returns FAIL for testing goal gate enforcement.
struct AlwaysFailHandler;

#[async_trait::async_trait]
impl Handler for AlwaysFailHandler {
    async fn execute(
        &self,
        node: &Node,
        _context: &attractor::context::Context,
        _graph: &Graph,
        _logs_root: &Path,
    ) -> Result<Outcome, attractor::error::AttractorError> {
        Ok(Outcome::fail(format!("forced failure for {}", node.id)))
    }
}

#[tokio::test]
async fn goal_gate_routes_to_retry_target_on_failure() {
    // Pipeline:
    //   start -> gated_work -> exit
    //   gated_work has goal_gate=true, retry_target=start
    //   gated_work always returns FAIL
    //
    // When engine reaches exit, it checks goal gates and finds gated_work failed.
    // It should route back to retry_target (start).
    //
    // To avoid infinite loops, we set max_retries=0 on gated_work so it fails
    // immediately each time. After looping once (start -> gated_work -> exit -> start
    // -> gated_work -> exit), if goal gate is still unsatisfied and no retry_target
    // changes, we need to limit iterations. The engine itself doesn't limit loops,
    // so we test a simpler scenario: verify the error when retry_target is missing.

    // Test: goal_gate with NO retry_target returns an error
    let mut graph = Graph::new("GoalGateNoRetry");

    let mut start = Node::new("start");
    start
        .attrs
        .insert("shape".to_string(), AttrValue::String("Mdiamond".to_string()));
    graph.nodes.insert("start".to_string(), start);

    let mut exit = Node::new("exit");
    exit.attrs
        .insert("shape".to_string(), AttrValue::String("Msquare".to_string()));
    graph.nodes.insert("exit".to_string(), exit);

    let mut gated_work = Node::new("gated_work");
    gated_work
        .attrs
        .insert("goal_gate".to_string(), AttrValue::Boolean(true));
    gated_work
        .attrs
        .insert("max_retries".to_string(), AttrValue::Integer(0));
    gated_work.attrs.insert(
        "type".to_string(),
        AttrValue::String("always_fail".to_string()),
    );
    graph
        .nodes
        .insert("gated_work".to_string(), gated_work);

    graph.edges.push(Edge::new("start", "gated_work"));
    graph.edges.push(Edge::new("gated_work", "exit"));

    let dir = tempfile::tempdir().unwrap();
    let mut registry = HandlerRegistry::new(Box::new(StartHandler));
    registry.register("start", Box::new(StartHandler));
    registry.register("exit", Box::new(ExitHandler));
    registry.register("always_fail", Box::new(AlwaysFailHandler));

    let engine = PipelineEngine::new(registry, EventEmitter::new());
    let config = RunConfig {
        logs_root: dir.path().to_path_buf(),
    };

    let result = engine.run(&graph, &config).await;
    assert!(result.is_err(), "should fail when goal gate unsatisfied and no retry_target");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("goal gate unsatisfied"),
        "error should mention goal gate, got: {err_msg}"
    );
}

#[tokio::test]
async fn goal_gate_routes_to_retry_target_when_present() {
    // Pipeline:
    //   start -> gated_work -> exit
    //   gated_work has goal_gate=true, retry_target=start
    //   gated_work always fails via AlwaysFailHandler.
    //
    // When engine reaches exit and finds goal gate unsatisfied, it should route
    // to the retry_target. Since AlwaysFailHandler always fails, this creates a
    // loop. However, the gated_work node will emit a FAIL outcome, and the
    // edge gated_work -> exit is unconditional, so it still reaches exit. After
    // the first retry (start -> gated_work -> exit), goal gate is still failed
    // and retry_target is still start, so it loops. To prevent an infinite loop
    // in tests, we use a custom handler that fails the first time and succeeds
    // the second time.

    struct FailThenSucceedHandler {
        call_count: std::sync::atomic::AtomicU32,
    }

    #[async_trait::async_trait]
    impl Handler for FailThenSucceedHandler {
        async fn execute(
            &self,
            _node: &Node,
            _context: &attractor::context::Context,
            _graph: &Graph,
            _logs_root: &Path,
        ) -> Result<Outcome, attractor::error::AttractorError> {
            let count = self
                .call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if count == 0 {
                Ok(Outcome::fail("first attempt fails"))
            } else {
                Ok(Outcome::success())
            }
        }
    }

    let mut graph = Graph::new("GoalGateRetry");

    let mut start = Node::new("start");
    start
        .attrs
        .insert("shape".to_string(), AttrValue::String("Mdiamond".to_string()));
    graph.nodes.insert("start".to_string(), start);

    let mut exit = Node::new("exit");
    exit.attrs
        .insert("shape".to_string(), AttrValue::String("Msquare".to_string()));
    graph.nodes.insert("exit".to_string(), exit);

    let mut gated_work = Node::new("gated_work");
    gated_work
        .attrs
        .insert("goal_gate".to_string(), AttrValue::Boolean(true));
    gated_work
        .attrs
        .insert("max_retries".to_string(), AttrValue::Integer(0));
    gated_work.attrs.insert(
        "retry_target".to_string(),
        AttrValue::String("start".to_string()),
    );
    gated_work.attrs.insert(
        "type".to_string(),
        AttrValue::String("fail_then_succeed".to_string()),
    );
    graph
        .nodes
        .insert("gated_work".to_string(), gated_work);

    graph.edges.push(Edge::new("start", "gated_work"));
    graph.edges.push(Edge::new("gated_work", "exit"));

    let dir = tempfile::tempdir().unwrap();
    let mut registry = HandlerRegistry::new(Box::new(StartHandler));
    registry.register("start", Box::new(StartHandler));
    registry.register("exit", Box::new(ExitHandler));
    registry.register(
        "fail_then_succeed",
        Box::new(FailThenSucceedHandler {
            call_count: std::sync::atomic::AtomicU32::new(0),
        }),
    );

    let engine = PipelineEngine::new(registry, EventEmitter::new());
    let config = RunConfig {
        logs_root: dir.path().to_path_buf(),
    };

    let outcome = engine
        .run(&graph, &config)
        .await
        .expect("run should eventually succeed after retry");
    assert_eq!(outcome.status, StageStatus::Success);

    let checkpoint = Checkpoint::load(&dir.path().join("checkpoint.json")).unwrap();
    // gated_work should appear in completed nodes (at least twice -- first fail, then succeed)
    let gated_work_count = checkpoint
        .completed_nodes
        .iter()
        .filter(|n| *n == "gated_work")
        .count();
    assert!(
        gated_work_count >= 2,
        "gated_work should have been executed at least twice, got {gated_work_count}"
    );
}

// ---------------------------------------------------------------------------
// 6. Variable expansion transform
// ---------------------------------------------------------------------------

#[test]
fn variable_expansion_replaces_goal_in_prompts() {
    let mut graph = Graph::new("test");
    graph.attrs.insert(
        "goal".to_string(),
        AttrValue::String("Fix all bugs".to_string()),
    );

    let mut plan_node = Node::new("plan");
    plan_node.attrs.insert(
        "prompt".to_string(),
        AttrValue::String("Plan to achieve: $goal".to_string()),
    );
    graph.nodes.insert("plan".to_string(), plan_node);

    let mut impl_node = Node::new("implement");
    impl_node.attrs.insert(
        "prompt".to_string(),
        AttrValue::String("Implement $goal now".to_string()),
    );
    graph
        .nodes
        .insert("implement".to_string(), impl_node);

    let mut no_var_node = Node::new("report");
    no_var_node.attrs.insert(
        "prompt".to_string(),
        AttrValue::String("Generate a report".to_string()),
    );
    graph
        .nodes
        .insert("report".to_string(), no_var_node);

    let transform = VariableExpansionTransform;
    transform.apply(&mut graph);

    let plan_prompt = graph.nodes["plan"]
        .attrs
        .get("prompt")
        .and_then(AttrValue::as_str)
        .expect("plan prompt should exist");
    assert_eq!(plan_prompt, "Plan to achieve: Fix all bugs");

    let impl_prompt = graph.nodes["implement"]
        .attrs
        .get("prompt")
        .and_then(AttrValue::as_str)
        .expect("implement prompt should exist");
    assert_eq!(impl_prompt, "Implement Fix all bugs now");

    let report_prompt = graph.nodes["report"]
        .attrs
        .get("prompt")
        .and_then(AttrValue::as_str)
        .expect("report prompt should exist");
    assert_eq!(report_prompt, "Generate a report");
}

// ---------------------------------------------------------------------------
// 7. Stylesheet application
// ---------------------------------------------------------------------------

#[test]
fn stylesheet_application_by_specificity() {
    let stylesheet_text = r#"
        * { llm_model: claude-sonnet-4-5; llm_provider: anthropic; }
        .code { llm_model: claude-opus-4-6; llm_provider: anthropic; }
        #critical_review { llm_model: gpt-5.2; llm_provider: openai; reasoning_effort: high; }
    "#;

    let mut graph = Graph::new("test");
    graph.attrs.insert(
        "model_stylesheet".to_string(),
        AttrValue::String(stylesheet_text.to_string()),
    );

    // plan node: no class, should get universal defaults
    let plan = Node::new("plan");
    graph.nodes.insert("plan".to_string(), plan);

    // implement node: class="code", should get .code overrides
    let mut implement = Node::new("implement");
    implement.classes.push("code".to_string());
    graph
        .nodes
        .insert("implement".to_string(), implement);

    // critical_review node: class="code" AND id="critical_review", id wins
    let mut critical = Node::new("critical_review");
    critical.classes.push("code".to_string());
    graph
        .nodes
        .insert("critical_review".to_string(), critical);

    // explicit node: has explicit llm_model, should NOT be overridden
    let mut explicit = Node::new("explicit_node");
    explicit.attrs.insert(
        "llm_model".to_string(),
        AttrValue::String("my-custom-model".to_string()),
    );
    graph
        .nodes
        .insert("explicit_node".to_string(), explicit);

    let transform = StylesheetApplicationTransform;
    transform.apply(&mut graph);

    // plan: universal -> claude-sonnet-4-5
    assert_eq!(
        graph.nodes["plan"].attrs.get("llm_model"),
        Some(&AttrValue::String("claude-sonnet-4-5".to_string()))
    );
    assert_eq!(
        graph.nodes["plan"].attrs.get("llm_provider"),
        Some(&AttrValue::String("anthropic".to_string()))
    );

    // implement: .code -> claude-opus-4-6
    assert_eq!(
        graph.nodes["implement"].attrs.get("llm_model"),
        Some(&AttrValue::String("claude-opus-4-6".to_string()))
    );
    assert_eq!(
        graph.nodes["implement"].attrs.get("llm_provider"),
        Some(&AttrValue::String("anthropic".to_string()))
    );

    // critical_review: #critical_review -> gpt-5.2 (id overrides class)
    assert_eq!(
        graph.nodes["critical_review"].attrs.get("llm_model"),
        Some(&AttrValue::String("gpt-5.2".to_string()))
    );
    assert_eq!(
        graph.nodes["critical_review"].attrs.get("llm_provider"),
        Some(&AttrValue::String("openai".to_string()))
    );
    assert_eq!(
        graph.nodes["critical_review"]
            .attrs
            .get("reasoning_effort"),
        Some(&AttrValue::String("high".to_string()))
    );

    // explicit_node: explicit attr NOT overridden by universal
    assert_eq!(
        graph.nodes["explicit_node"].attrs.get("llm_model"),
        Some(&AttrValue::String("my-custom-model".to_string()))
    );
}

#[test]
fn stylesheet_application_via_parsed_graph() {
    let input = r#"digraph StyleTest {
        graph [
            goal="Test stylesheet",
            model_stylesheet="* { llm_model: sonnet; }"
        ]
        start [shape=Mdiamond]
        exit  [shape=Msquare]
        work  [shape=box, prompt="Do work"]
        start -> work -> exit
    }"#;

    let mut graph = parse(input).expect("parse should succeed");
    validate_or_raise(&graph, &[]).expect("validation should pass");

    let transform = StylesheetApplicationTransform;
    transform.apply(&mut graph);

    // All nodes without explicit llm_model should get "sonnet"
    assert_eq!(
        graph.nodes["work"].attrs.get("llm_model"),
        Some(&AttrValue::String("sonnet".to_string()))
    );
    assert_eq!(
        graph.nodes["start"].attrs.get("llm_model"),
        Some(&AttrValue::String("sonnet".to_string()))
    );
    assert_eq!(
        graph.nodes["exit"].attrs.get("llm_model"),
        Some(&AttrValue::String("sonnet".to_string()))
    );
}

#[test]
fn stylesheet_parse_and_apply_directly() {
    let stylesheet_text = "* { llm_model: base; } .fast { llm_model: turbo; }";
    let stylesheet = parse_stylesheet(stylesheet_text).expect("stylesheet parse should succeed");
    assert_eq!(stylesheet.rules.len(), 2);

    let mut graph = Graph::new("test");
    let plain = Node::new("a");
    graph.nodes.insert("a".to_string(), plain);

    let mut fast_node = Node::new("b");
    fast_node.classes.push("fast".to_string());
    graph.nodes.insert("b".to_string(), fast_node);

    apply_stylesheet(&stylesheet, &mut graph);

    assert_eq!(
        graph.nodes["a"].attrs.get("llm_model"),
        Some(&AttrValue::String("base".to_string()))
    );
    assert_eq!(
        graph.nodes["b"].attrs.get("llm_model"),
        Some(&AttrValue::String("turbo".to_string()))
    );
}

// ---------------------------------------------------------------------------
// 8. Retry on failure (Gap #35.1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn retry_on_failure_then_succeed() {
    // A handler that fails the first call and succeeds on the second.
    struct RetryHandler {
        call_count: std::sync::atomic::AtomicU32,
    }

    #[async_trait::async_trait]
    impl Handler for RetryHandler {
        async fn execute(
            &self,
            _node: &Node,
            _context: &Context,
            _graph: &Graph,
            _logs_root: &Path,
        ) -> Result<Outcome, AttractorError> {
            let count = self
                .call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if count == 0 {
                Ok(Outcome::retry("transient failure"))
            } else {
                Ok(Outcome::success())
            }
        }
    }

    let mut graph = Graph::new("RetryTest");

    let mut start = Node::new("start");
    start
        .attrs
        .insert("shape".to_string(), AttrValue::String("Mdiamond".to_string()));
    graph.nodes.insert("start".to_string(), start);

    let mut exit = Node::new("exit");
    exit.attrs
        .insert("shape".to_string(), AttrValue::String("Msquare".to_string()));
    graph.nodes.insert("exit".to_string(), exit);

    let mut retry_node = Node::new("work");
    retry_node.attrs.insert(
        "type".to_string(),
        AttrValue::String("retry_handler".to_string()),
    );
    retry_node
        .attrs
        .insert("max_retries".to_string(), AttrValue::Integer(3));
    graph.nodes.insert("work".to_string(), retry_node);

    graph.edges.push(Edge::new("start", "work"));
    graph.edges.push(Edge::new("work", "exit"));

    let dir = tempfile::tempdir().unwrap();
    let mut registry = HandlerRegistry::new(Box::new(StartHandler));
    registry.register("start", Box::new(StartHandler));
    registry.register("exit", Box::new(ExitHandler));
    registry.register(
        "retry_handler",
        Box::new(RetryHandler {
            call_count: std::sync::atomic::AtomicU32::new(0),
        }),
    );

    let engine = PipelineEngine::new(registry, EventEmitter::new());
    let config = RunConfig {
        logs_root: dir.path().to_path_buf(),
    };

    let outcome = engine
        .run(&graph, &config)
        .await
        .expect("should succeed after retry");
    assert_eq!(outcome.status, StageStatus::Success);
}

// ---------------------------------------------------------------------------
// 9. Pipeline with 10+ nodes (Gap #35.2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pipeline_with_many_nodes() {
    // Build a linear pipeline: start -> n1 -> n2 -> ... -> n10 -> exit (12 nodes)
    let mut graph = Graph::new("ManyNodes");
    graph.attrs.insert(
        "goal".to_string(),
        AttrValue::String("Test large pipeline".to_string()),
    );

    let mut start = Node::new("start");
    start
        .attrs
        .insert("shape".to_string(), AttrValue::String("Mdiamond".to_string()));
    graph.nodes.insert("start".to_string(), start);

    let mut exit = Node::new("exit");
    exit.attrs
        .insert("shape".to_string(), AttrValue::String("Msquare".to_string()));
    graph.nodes.insert("exit".to_string(), exit);

    let node_names: Vec<String> = (1..=10).map(|i| format!("step_{i}")).collect();

    for name in &node_names {
        let mut node = Node::new(name.clone());
        node.attrs.insert(
            "shape".to_string(),
            AttrValue::String("box".to_string()),
        );
        node.attrs.insert(
            "prompt".to_string(),
            AttrValue::String(format!("Execute {name}")),
        );
        graph.nodes.insert(name.clone(), node);
    }

    graph.edges.push(Edge::new("start", &node_names[0]));
    for pair in node_names.windows(2) {
        graph.edges.push(Edge::new(&pair[0], &pair[1]));
    }
    graph.edges.push(Edge::new(
        node_names.last().unwrap(),
        "exit",
    ));

    let dir = tempfile::tempdir().unwrap();
    let engine = PipelineEngine::new(make_linear_registry(), EventEmitter::new());
    let config = RunConfig {
        logs_root: dir.path().to_path_buf(),
    };

    let outcome = engine
        .run(&graph, &config)
        .await
        .expect("large pipeline should succeed");
    assert_eq!(outcome.status, StageStatus::Success);

    let checkpoint = Checkpoint::load(&dir.path().join("checkpoint.json")).unwrap();
    // All 10 step nodes should be in completed_nodes
    for name in &node_names {
        assert!(
            checkpoint.completed_nodes.contains(name),
            "{name} should be in completed_nodes"
        );
    }
}

// ---------------------------------------------------------------------------
// 10. Checkpoint save and load round-trip (Gap #35.3)
// ---------------------------------------------------------------------------

#[test]
fn checkpoint_save_and_resume_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("checkpoint.json");

    let ctx = Context::new();
    ctx.set("goal", serde_json::json!("Test checkpoint"));
    ctx.set("progress", serde_json::json!(42));
    ctx.append_log("started");
    ctx.append_log("step_1 completed");

    let mut checkpoint = Checkpoint::from_context(
        &ctx,
        "step_2",
        vec![
            "start".to_string(),
            "step_1".to_string(),
        ],
    );
    checkpoint.node_retries.insert("step_1".to_string(), 1);

    checkpoint.save(&path).expect("save should succeed");

    let loaded = Checkpoint::load(&path).expect("load should succeed");
    assert_eq!(loaded.current_node, "step_2");
    assert_eq!(loaded.completed_nodes.len(), 2);
    assert!(loaded.completed_nodes.contains(&"start".to_string()));
    assert!(loaded.completed_nodes.contains(&"step_1".to_string()));
    assert_eq!(loaded.node_retries.get("step_1"), Some(&1));
    assert_eq!(
        loaded.context_values.get("goal"),
        Some(&serde_json::json!("Test checkpoint"))
    );
    assert_eq!(
        loaded.context_values.get("progress"),
        Some(&serde_json::json!(42))
    );
    assert_eq!(loaded.logs.len(), 2);
}

// ---------------------------------------------------------------------------
// 11. Smoke test with mock CodergenBackend (Gap #36)
// ---------------------------------------------------------------------------

struct MockCodergenBackend;

#[async_trait::async_trait]
impl CodergenBackend for MockCodergenBackend {
    async fn run(
        &self,
        node: &Node,
        prompt: &str,
        _context: &Context,
    ) -> Result<CodergenResult, AttractorError> {
        Ok(CodergenResult::Text(format!(
            "Response for {}: processed prompt '{}'",
            node.id,
            &prompt[..prompt.len().min(50)]
        )))
    }
}

#[tokio::test]
async fn smoke_test_with_mock_codergen_backend() {
    // Pipeline:
    //   start -> plan -> gate (diamond)
    //   gate -> implement [condition="outcome=success"]
    //   gate -> fix       [condition="outcome!=success"]
    //   implement -> exit
    //   fix -> exit
    //
    // codergen nodes use MockCodergenBackend which returns real Text responses.
    // The gate is a conditional node. Since the mock backend returns success,
    // we should route through implement.

    let mut graph = Graph::new("SmokeTest");
    graph.attrs.insert(
        "goal".to_string(),
        AttrValue::String("Build and validate".to_string()),
    );

    let mut start = Node::new("start");
    start
        .attrs
        .insert("shape".to_string(), AttrValue::String("Mdiamond".to_string()));
    graph.nodes.insert("start".to_string(), start);

    let mut exit = Node::new("exit");
    exit.attrs
        .insert("shape".to_string(), AttrValue::String("Msquare".to_string()));
    graph.nodes.insert("exit".to_string(), exit);

    let mut plan = Node::new("plan");
    plan.attrs
        .insert("shape".to_string(), AttrValue::String("box".to_string()));
    plan.attrs.insert(
        "prompt".to_string(),
        AttrValue::String("Plan to achieve: $goal".to_string()),
    );
    graph.nodes.insert("plan".to_string(), plan);

    let mut gate = Node::new("gate");
    gate.attrs
        .insert("shape".to_string(), AttrValue::String("diamond".to_string()));
    graph.nodes.insert("gate".to_string(), gate);

    let mut implement = Node::new("implement");
    implement
        .attrs
        .insert("shape".to_string(), AttrValue::String("box".to_string()));
    implement.attrs.insert(
        "prompt".to_string(),
        AttrValue::String("Implement the plan".to_string()),
    );
    graph
        .nodes
        .insert("implement".to_string(), implement);

    let mut fix = Node::new("fix");
    fix.attrs
        .insert("shape".to_string(), AttrValue::String("box".to_string()));
    fix.attrs.insert(
        "prompt".to_string(),
        AttrValue::String("Fix the issues".to_string()),
    );
    graph.nodes.insert("fix".to_string(), fix);

    graph.edges.push(Edge::new("start", "plan"));
    graph.edges.push(Edge::new("plan", "gate"));

    let mut gate_impl = Edge::new("gate", "implement");
    gate_impl.attrs.insert(
        "condition".to_string(),
        AttrValue::String("outcome=success".to_string()),
    );
    graph.edges.push(gate_impl);

    let mut gate_fix = Edge::new("gate", "fix");
    gate_fix.attrs.insert(
        "condition".to_string(),
        AttrValue::String("outcome!=success".to_string()),
    );
    graph.edges.push(gate_fix);

    graph.edges.push(Edge::new("implement", "exit"));
    graph.edges.push(Edge::new("fix", "exit"));

    let dir = tempfile::tempdir().unwrap();
    let backend = Box::new(MockCodergenBackend);
    let mut registry =
        HandlerRegistry::new(Box::new(CodergenHandler::new(Some(backend))));
    registry.register("start", Box::new(StartHandler));
    registry.register("exit", Box::new(ExitHandler));
    registry.register(
        "codergen",
        Box::new(CodergenHandler::new(Some(Box::new(MockCodergenBackend)))),
    );
    registry.register("conditional", Box::new(ConditionalHandler));

    let engine = PipelineEngine::new(registry, EventEmitter::new());
    let config = RunConfig {
        logs_root: dir.path().to_path_buf(),
    };

    let outcome = engine
        .run(&graph, &config)
        .await
        .expect("smoke test should succeed");
    assert_eq!(outcome.status, StageStatus::Success);

    let checkpoint = Checkpoint::load(&dir.path().join("checkpoint.json")).unwrap();
    assert!(
        checkpoint
            .completed_nodes
            .contains(&"plan".to_string()),
        "plan should have executed"
    );
    assert!(
        checkpoint
            .completed_nodes
            .contains(&"implement".to_string()),
        "should route through implement (success path)"
    );
    assert!(
        !checkpoint
            .completed_nodes
            .contains(&"fix".to_string()),
        "should NOT have traversed fix path"
    );

    // Verify response.md was written by the mock backend
    let plan_response = std::fs::read_to_string(dir.path().join("plan").join("response.md"))
        .expect("plan response should exist");
    assert!(
        plan_response.contains("Response for plan"),
        "mock backend should have written response, got: {plan_response}"
    );

    // Verify prompt.md had $goal expanded by the CodergenHandler
    let plan_prompt = std::fs::read_to_string(dir.path().join("plan").join("prompt.md"))
        .expect("plan prompt should exist");
    assert_eq!(plan_prompt, "Plan to achieve: Build and validate");
}
