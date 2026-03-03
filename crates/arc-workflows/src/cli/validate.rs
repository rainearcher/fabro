use anyhow::bail;
use arc_util::terminal::Styles;

use crate::workflow::WorkflowBuilder;
use crate::validation::Severity;

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
        styles.bold.apply_to(format!("Parsed workflow: {}", graph.name)),
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
