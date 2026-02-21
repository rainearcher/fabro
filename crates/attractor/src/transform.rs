use crate::graph::{AttrValue, Graph};
use crate::stylesheet::{apply_stylesheet, parse_stylesheet};

/// A transform that modifies the pipeline graph after parsing and before validation.
pub trait Transform {
    fn apply(&self, graph: &mut Graph);
}

/// Expands `$goal` in node `prompt` attributes to the graph-level `goal` value.
pub struct VariableExpansionTransform;

impl Transform for VariableExpansionTransform {
    fn apply(&self, graph: &mut Graph) {
        let goal = graph.goal().to_string();
        for node in graph.nodes.values_mut() {
            if let Some(AttrValue::String(prompt)) = node.attrs.get("prompt") {
                if prompt.contains("$goal") {
                    let expanded = prompt.replace("$goal", &goal);
                    node.attrs
                        .insert("prompt".to_string(), AttrValue::String(expanded));
                }
            }
        }
    }
}

/// For nodes whose fidelity is not "full", prepend a context mode preamble to the prompt.
pub struct PreambleTransform;

impl Transform for PreambleTransform {
    fn apply(&self, graph: &mut Graph) {
        let default_fidelity = graph.default_fidelity().unwrap_or("full").to_string();
        for node in graph.nodes.values_mut() {
            let fidelity = node
                .fidelity()
                .unwrap_or(&default_fidelity)
                .to_string();
            if fidelity == "full" {
                continue;
            }
            let preamble = format!("[Context mode: {fidelity}]\n");
            if let Some(AttrValue::String(prompt)) = node.attrs.get("prompt") {
                let new_prompt = format!("{preamble}{prompt}");
                node.attrs
                    .insert("prompt".to_string(), AttrValue::String(new_prompt));
            }
        }
    }
}

/// Applies the `model_stylesheet` graph attribute to resolve LLM properties for each node.
pub struct StylesheetApplicationTransform;

impl Transform for StylesheetApplicationTransform {
    fn apply(&self, graph: &mut Graph) {
        let stylesheet_text = graph.model_stylesheet().to_string();
        if stylesheet_text.is_empty() {
            return;
        }
        let Ok(stylesheet) = parse_stylesheet(&stylesheet_text) else {
            return;
        };
        apply_stylesheet(&stylesheet, graph);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Node;

    #[test]
    fn variable_expansion_replaces_goal() {
        let mut graph = Graph::new("test");
        graph
            .attrs
            .insert("goal".to_string(), AttrValue::String("Fix bugs".to_string()));

        let mut node = Node::new("plan");
        node.attrs.insert(
            "prompt".to_string(),
            AttrValue::String("Achieve: $goal now".to_string()),
        );
        graph.nodes.insert("plan".to_string(), node);

        let transform = VariableExpansionTransform;
        transform.apply(&mut graph);

        let prompt = graph.nodes["plan"]
            .attrs
            .get("prompt")
            .and_then(AttrValue::as_str)
            .unwrap();
        assert_eq!(prompt, "Achieve: Fix bugs now");
    }

    #[test]
    fn variable_expansion_no_goal_variable() {
        let mut graph = Graph::new("test");
        graph
            .attrs
            .insert("goal".to_string(), AttrValue::String("Fix bugs".to_string()));

        let mut node = Node::new("plan");
        node.attrs.insert(
            "prompt".to_string(),
            AttrValue::String("Do something".to_string()),
        );
        graph.nodes.insert("plan".to_string(), node);

        let transform = VariableExpansionTransform;
        transform.apply(&mut graph);

        let prompt = graph.nodes["plan"]
            .attrs
            .get("prompt")
            .and_then(AttrValue::as_str)
            .unwrap();
        assert_eq!(prompt, "Do something");
    }

    #[test]
    fn variable_expansion_empty_goal() {
        let mut graph = Graph::new("test");
        let mut node = Node::new("plan");
        node.attrs.insert(
            "prompt".to_string(),
            AttrValue::String("Goal: $goal".to_string()),
        );
        graph.nodes.insert("plan".to_string(), node);

        let transform = VariableExpansionTransform;
        transform.apply(&mut graph);

        let prompt = graph.nodes["plan"]
            .attrs
            .get("prompt")
            .and_then(AttrValue::as_str)
            .unwrap();
        assert_eq!(prompt, "Goal: ");
    }

    #[test]
    fn variable_expansion_no_prompt() {
        let mut graph = Graph::new("test");
        graph
            .attrs
            .insert("goal".to_string(), AttrValue::String("Fix bugs".to_string()));
        let node = Node::new("plan");
        graph.nodes.insert("plan".to_string(), node);

        let transform = VariableExpansionTransform;
        // Should not panic
        transform.apply(&mut graph);
        assert!(graph.nodes["plan"].attrs.get("prompt").is_none());
    }

    #[test]
    fn stylesheet_transform_empty_stylesheet() {
        let mut graph = Graph::new("test");
        graph.nodes.insert("a".to_string(), Node::new("a"));

        let transform = StylesheetApplicationTransform;
        // Should not panic with empty stylesheet
        transform.apply(&mut graph);
    }

    #[test]
    fn preamble_transform_prepends_for_non_full_fidelity() {
        let mut graph = Graph::new("test");
        let mut node = Node::new("work");
        node.attrs.insert(
            "fidelity".to_string(),
            AttrValue::String("truncate".to_string()),
        );
        node.attrs.insert(
            "prompt".to_string(),
            AttrValue::String("Do the thing".to_string()),
        );
        graph.nodes.insert("work".to_string(), node);

        PreambleTransform.apply(&mut graph);

        let prompt = graph.nodes["work"]
            .attrs
            .get("prompt")
            .and_then(AttrValue::as_str)
            .unwrap();
        assert_eq!(prompt, "[Context mode: truncate]\nDo the thing");
    }

    #[test]
    fn preamble_transform_skips_full_fidelity() {
        let mut graph = Graph::new("test");
        let mut node = Node::new("work");
        node.attrs.insert(
            "fidelity".to_string(),
            AttrValue::String("full".to_string()),
        );
        node.attrs.insert(
            "prompt".to_string(),
            AttrValue::String("Do the thing".to_string()),
        );
        graph.nodes.insert("work".to_string(), node);

        PreambleTransform.apply(&mut graph);

        let prompt = graph.nodes["work"]
            .attrs
            .get("prompt")
            .and_then(AttrValue::as_str)
            .unwrap();
        assert_eq!(prompt, "Do the thing");
    }

    #[test]
    fn preamble_transform_uses_graph_default_fidelity() {
        let mut graph = Graph::new("test");
        graph.attrs.insert(
            "default_fidelity".to_string(),
            AttrValue::String("compact".to_string()),
        );
        let mut node = Node::new("work");
        node.attrs.insert(
            "prompt".to_string(),
            AttrValue::String("Do the thing".to_string()),
        );
        graph.nodes.insert("work".to_string(), node);

        PreambleTransform.apply(&mut graph);

        let prompt = graph.nodes["work"]
            .attrs
            .get("prompt")
            .and_then(AttrValue::as_str)
            .unwrap();
        assert_eq!(prompt, "[Context mode: compact]\nDo the thing");
    }

    #[test]
    fn preamble_transform_no_prompt_skips() {
        let mut graph = Graph::new("test");
        let mut node = Node::new("work");
        node.attrs.insert(
            "fidelity".to_string(),
            AttrValue::String("truncate".to_string()),
        );
        graph.nodes.insert("work".to_string(), node);

        PreambleTransform.apply(&mut graph);

        assert!(graph.nodes["work"].attrs.get("prompt").is_none());
    }
}
