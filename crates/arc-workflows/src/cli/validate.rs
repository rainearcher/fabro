use anyhow::bail;
use arc_util::terminal::Styles;

use crate::validation::Severity;
use crate::workflow::WorkflowBuilder;

use super::{print_diagnostics, read_dot_file, ValidateArgs};

/// Parse and validate a workflow file without executing it.
///
/// # Errors
///
/// Returns an error if the file cannot be read, parsed, or has validation errors.
pub fn validate_command(args: &ValidateArgs, styles: &Styles) -> anyhow::Result<()> {
    let source = read_dot_file(&args.workflow)?;
    let (graph, diagnostics) = WorkflowBuilder::new().prepare(&source)?;

    eprintln!(
        "{} ({} nodes, {} edges)",
        styles
            .bold
            .apply_to(format!("Workflow: {}", graph.name)),
        graph.nodes.len(),
        graph.edges.len(),
    );

    print_diagnostics(&diagnostics, styles);

    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        bail!("Validation failed");
    }

    eprintln!("Validation: {}", styles.green.apply_to("OK"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    #[test]
    fn validate_valid_workflow() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmp,
            r#"digraph Simple {{
    graph [goal="Run tests and report results"]
    rankdir=LR

    start [shape=Mdiamond, label="Start"]
    exit  [shape=Msquare, label="Exit"]

    run_tests [label="Run Tests", prompt="Run the test suite and report results"]
    report    [label="Report", prompt="Summarize the test results"]

    start -> run_tests -> report -> exit
}}"#
        )
        .unwrap();

        let args = ValidateArgs {
            workflow: tmp.path().to_path_buf(),
        };
        let styles = Styles::new(false);
        let result = validate_command(&args, &styles);
        assert!(result.is_ok(), "expected Ok but got: {result:?}");
    }

    #[test]
    fn validate_invalid_syntax() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "not a valid dot file").unwrap();

        let args = ValidateArgs {
            workflow: tmp.path().to_path_buf(),
        };
        let styles = Styles::new(false);
        let result = validate_command(&args, &styles);
        assert!(result.is_err(), "expected Err for invalid syntax");
    }

    #[test]
    fn validate_missing_file() {
        let args = ValidateArgs {
            workflow: PathBuf::from("/tmp/nonexistent_workflow_12345.dot"),
        };
        let styles = Styles::new(false);
        let result = validate_command(&args, &styles);
        assert!(result.is_err(), "expected Err for missing file");
    }
}
