//! 图执行：按 stages 跑节点，把上游输出按 pointer 注入下游参数。
//!
//! 这里不关心命令从哪来（由调用方传入 handler），
//! 所以是纯函数式的、可单测的执行内核——将来接 `executor::spawn` 只需换 handler。

use serde::Serialize;
use serde_json::Value;

use crate::graph::{pick, plan, GraphError, GraphSpec};

#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    Plan(GraphError),
    Failed { node: String, message: String },
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plan(e) => write!(f, "图计划失败: {e}"),
            Self::Failed { node, message } => write!(f, "节点 {node} 执行失败: {message}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NodeRun {
    pub id: String,
    pub output: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunResult {
    pub order: Vec<String>,
    pub stages: Vec<Vec<String>>,
    pub outputs: Vec<NodeRun>,
    pub elapsed_ms: u64,
}

/// 按拓扑分层执行。同层节点串行（demo handler 是同步纯函数）。
pub fn run<H>(graph: &GraphSpec, handler: H) -> Result<RunResult, RunError>
where
    H: Fn(&str, &Value) -> Result<Value, String>,
{
    let plan = plan(graph).map_err(RunError::Plan)?;
    let start = std::time::Instant::now();
    let mut done: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    let mut outputs = Vec::new();

    for stage in &plan.stages {
        for id in stage {
            let node = graph
                .nodes
                .iter()
                .find(|n| &n.id == id)
                .expect("plan 只会产出已存在的节点 id");
            let mut args = node.args.clone();
            if !args.is_object() {
                args = serde_json::json!({});
            }
            for edge in graph.edges.iter().filter(|e| &e.to == id) {
                let src = done.get(&edge.from).cloned().unwrap_or(Value::Null);
                args[&edge.arg] = pick(&src, edge.pointer.as_deref());
            }
            let output = handler(&node.command, &args).map_err(|message| RunError::Failed {
                node: id.clone(),
                message,
            })?;
            done.insert(id.clone(), output.clone());
            outputs.push(NodeRun {
                id: id.clone(),
                output,
            });
        }
    }

    Ok(RunResult {
        order: plan.order.clone(),
        stages: plan.stages.clone(),
        outputs,
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, Node};

    fn graph() -> GraphSpec {
        GraphSpec {
            nodes: vec![
                Node {
                    id: "a".into(),
                    command: "count".into(),
                    args: serde_json::json!({ "steps": 3 }),
                    x: None,
                    y: None,
                },
                Node {
                    id: "b".into(),
                    command: "echo".into(),
                    args: serde_json::json!({}),
                    x: None,
                    y: None,
                },
            ],
            edges: vec![Edge {
                from: "a".into(),
                to: "b".into(),
                arg: "text".into(),
                pointer: Some("/value".into()),
            }],
        }
    }

    fn handler(cmd: &str, args: &Value) -> Result<Value, String> {
        match cmd {
            "count" => Ok(serde_json::json!({ "value": args["steps"] })),
            "echo" => Ok(serde_json::json!({ "text": args["text"] })),
            other => Err(format!("未知命令 {other}")),
        }
    }

    #[test]
    fn pointer_injection_feeds_downstream() {
        let r = run(&graph(), handler).unwrap();
        assert_eq!(r.order, vec!["a", "b"]);
        assert_eq!(r.outputs[1].output["text"], 3);
    }

    #[test]
    fn stage_order_is_preserved() {
        let r = run(&graph(), handler).unwrap();
        assert_eq!(r.stages, vec![vec!["a"], vec!["b"]]);
    }

    #[test]
    fn unknown_command_is_failed_node() {
        let mut g = graph();
        g.nodes[0].command = "nope".into();
        assert_eq!(
            run(&g, handler),
            Err(RunError::Failed {
                node: "a".into(),
                message: "未知命令 nope".into()
            })
        );
    }

    #[test]
    fn cycle_is_reported_before_running() {
        let mut g = graph();
        g.edges.push(Edge {
            from: "b".into(),
            to: "a".into(),
            arg: "steps".into(),
            pointer: None,
        });
        assert!(matches!(run(&g, handler), Err(RunError::Plan(_))));
    }
}
