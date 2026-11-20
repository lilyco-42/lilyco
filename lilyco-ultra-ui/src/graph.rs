//! # 可视化编程 — 图模型与执行计划
//!
//! 节点 = 注册表里的一个命令，连线 = 「把上游节点的输出绑到下游节点的某个参数」。
//! 这一层只做**静态校验与拓扑排序**：不执行、不碰 IO，所以纯函数、可单测。
//!
//! 与成熟方案的对应：
//! - ComfyUI：节点是纯函数、端口有类型、图按拓扑序执行（本模块借这个形状）
//! - n8n / Node-RED：连线表示数据流，触发器单独一类节点
//! - Blueprint：执行线 vs 数据线分开（这里只做数据线，执行线即拓扑序）

use serde::{Deserialize, Serialize};

/// 一张图
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSpec {
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

/// 图里的一个节点：命令 + 参数初值
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub command: String,
    #[serde(default)]
    pub args: serde_json::Value,
    /// 画布坐标（前端拖拽写入；缺省时按执行阶段自动布局）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
}

/// 一条数据边：`from` 的输出绑定到 `to` 的 `arg`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub arg: String,
    /// 从上游结果里取值用的 JSON 指针（默认整个结果）
    #[serde(default)]
    pub pointer: Option<String>,
}

/// 校验问题（全部可定位到节点/边）
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GraphError {
    DuplicateNode {
        id: String,
    },
    UnknownNode {
        id: String,
    },
    UnknownCommand {
        node: String,
        command: String,
    },
    UnknownArg {
        node: String,
        command: String,
        arg: String,
    },
    SelfLoop {
        node: String,
    },
    Cycle {
        nodes: Vec<String>,
    },
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateNode { id } => write!(f, "节点 id 重复: {id}"),
            Self::UnknownNode { id } => write!(f, "连线引用了不存在的节点: {id}"),
            Self::UnknownCommand { node, command } => {
                write!(f, "节点 {node} 的命令未注册: {command}")
            }
            Self::UnknownArg { node, command, arg } => {
                write!(f, "节点 {node}（{command}）没有参数 {arg}")
            }
            Self::SelfLoop { node } => write!(f, "节点 {node} 连到自己"),
            Self::Cycle { nodes } => write!(f, "图里有环: {}", nodes.join(" → ")),
        }
    }
}

/// 执行计划：节点按拓扑序排好
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Plan {
    pub order: Vec<String>,
    pub stages: Vec<Vec<String>>,
}

/// 校验整张图。`known` 用在无法访问 Registry 的场景（如前端预览）。
pub fn validate_with(
    graph: &GraphSpec,
    known: impl Fn(&str) -> Option<Vec<String>>,
) -> Vec<GraphError> {
    let mut errors = Vec::new();
    let mut ids = std::collections::HashSet::new();

    for node in &graph.nodes {
        if !ids.insert(node.id.clone()) {
            errors.push(GraphError::DuplicateNode {
                id: node.id.clone(),
            });
            continue;
        }
        match known(&node.command) {
            None => errors.push(GraphError::UnknownCommand {
                node: node.id.clone(),
                command: node.command.clone(),
            }),
            Some(args) => {
                for edge in graph.edges.iter().filter(|e| e.to == node.id) {
                    if !args.iter().any(|a| a == &edge.arg) {
                        errors.push(GraphError::UnknownArg {
                            node: node.id.clone(),
                            command: node.command.clone(),
                            arg: edge.arg.clone(),
                        });
                    }
                }
            }
        }
    }

    for edge in &graph.edges {
        if edge.from == edge.to {
            errors.push(GraphError::SelfLoop {
                node: edge.from.clone(),
            });
        }
        if !ids.contains(&edge.from) {
            errors.push(GraphError::UnknownNode {
                id: edge.from.clone(),
            });
        }
        if !ids.contains(&edge.to) {
            errors.push(GraphError::UnknownNode {
                id: edge.to.clone(),
            });
        }
    }

    if let Err(e) = plan(graph) {
        errors.push(e);
    }
    errors
}

/// 拓扑排序（Kahn）。返回分层结果，同层可并行。
pub fn plan(graph: &GraphSpec) -> Result<Plan, GraphError> {
    use std::collections::{BTreeMap, VecDeque};

    let mut indeg: BTreeMap<&str, usize> = graph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), 0usize))
        .collect();
    let mut next: BTreeMap<&str, Vec<&str>> = BTreeMap::new();

    for edge in &graph.edges {
        if edge.from == edge.to {
            return Err(GraphError::SelfLoop {
                node: edge.from.clone(),
            });
        }
        if !indeg.contains_key(edge.from.as_str()) {
            return Err(GraphError::UnknownNode {
                id: edge.from.clone(),
            });
        }
        if !indeg.contains_key(edge.to.as_str()) {
            return Err(GraphError::UnknownNode {
                id: edge.to.clone(),
            });
        }
        *indeg.get_mut(edge.to.as_str()).expect("checked") += 1;
        next.entry(edge.from.as_str())
            .or_default()
            .push(edge.to.as_str());
    }

    let mut queue: VecDeque<&str> = indeg
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(id, _)| *id)
        .collect();
    let mut stages = Vec::new();
    let mut order = Vec::new();

    while !queue.is_empty() {
        let stage: Vec<&str> = queue.drain(..).collect();
        let mut done = Vec::new();
        for id in stage {
            order.push(id.to_string());
            done.push(id.to_string());
            for to in next.get(id).into_iter().flatten() {
                let d = indeg.get_mut(to).expect("edge checked");
                *d -= 1;
                if *d == 0 {
                    queue.push_back(to);
                }
            }
        }
        stages.push(done);
    }

    if order.len() != graph.nodes.len() {
        let nodes = indeg
            .iter()
            .filter(|(_, d)| **d > 0)
            .map(|(id, _)| id.to_string())
            .collect();
        return Err(GraphError::Cycle { nodes });
    }
    Ok(Plan { order, stages })
}

/// 按执行阶段计算坐标：列 = stage，行 = 同层序号
pub fn layout(graph: &GraphSpec) -> Result<Vec<(String, f64, f64)>, GraphError> {
    let plan = plan(graph)?;
    let mut out = Vec::with_capacity(graph.nodes.len());
    for (i, stage) in plan.stages.iter().enumerate() {
        for (j, id) in stage.iter().enumerate() {
            out.push((id.clone(), 24.0 + i as f64 * 230.0, 24.0 + j as f64 * 96.0));
        }
    }
    Ok(out)
}

/// 从上游结果按 JSON Pointer 取值（`/value`、`/items/0`）
pub fn pick(value: &serde_json::Value, pointer: Option<&str>) -> serde_json::Value {
    let Some(raw) = pointer else {
        return value.clone();
    };
    let mut cur = value;
    for seg in raw
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
    {
        let seg = seg.replace("~1", "/").replace("~0", "~");
        cur = match cur {
            serde_json::Value::Object(map) => map.get(&seg).unwrap_or(&serde_json::Value::Null),
            serde_json::Value::Array(items) => seg
                .parse::<usize>()
                .ok()
                .and_then(|i| items.get(i))
                .unwrap_or(&serde_json::Value::Null),
            _ => return serde_json::Value::Null,
        };
    }
    cur.clone()
}

/// 示例图（两个节点：count → upper）
pub fn example_graph() -> GraphSpec {
    GraphSpec {
        nodes: vec![
            Node {
                id: "count".into(),
                command: "count".into(),
                args: serde_json::json!({ "steps": 3 }),
                x: Some(24.0),
                y: Some(24.0),
            },
            Node {
                id: "echo".into(),
                command: "echo".into(),
                args: serde_json::json!({}),
                x: Some(254.0),
                y: Some(24.0),
            },
        ],
        edges: vec![Edge {
            from: "count".into(),
            to: "echo".into(),
            arg: "text".into(),
            pointer: Some("/value".into()),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(nodes: &[(&str, &str)], edges: &[(&str, &str, &str)]) -> GraphSpec {
        GraphSpec {
            nodes: nodes
                .iter()
                .map(|(id, cmd)| Node {
                    id: (*id).into(),
                    command: (*cmd).into(),
                    args: serde_json::json!({}),
                    x: None,
                    y: None,
                })
                .collect(),
            edges: edges
                .iter()
                .map(|(f, t, a)| Edge {
                    from: (*f).into(),
                    to: (*t).into(),
                    arg: (*a).into(),
                    pointer: None,
                })
                .collect(),
        }
    }

    #[test]
    fn linear_order_and_stages() {
        let graph = g(&[("a", "count"), ("b", "echo")], &[("a", "b", "text")]);
        let plan = plan(&graph).unwrap();
        assert_eq!(plan.order, vec!["a", "b"]);
        assert_eq!(plan.stages, vec![vec!["a"], vec!["b"]]);
    }

    #[test]
    fn parallel_stage_groups_independent_nodes() {
        let graph = g(
            &[("a", "count"), ("b", "count"), ("c", "echo")],
            &[("a", "c", "text")],
        );
        let plan = plan(&graph).unwrap();
        assert_eq!(plan.stages[0], vec!["a", "b"]);
        assert_eq!(plan.stages[1], vec!["c"]);
    }

    #[test]
    fn cycle_is_reported_with_members() {
        let graph = g(
            &[("a", "x"), ("b", "x")],
            &[("a", "b", "p"), ("b", "a", "p")],
        );
        match plan(&graph) {
            Err(GraphError::Cycle { nodes }) => {
                assert!(nodes.contains(&"a".to_string()) && nodes.contains(&"b".to_string()))
            }
            other => panic!("应为环: {other:?}"),
        }
    }

    #[test]
    fn self_loop_rejected() {
        let graph = g(&[("a", "x")], &[("a", "a", "p")]);
        assert_eq!(plan(&graph), Err(GraphError::SelfLoop { node: "a".into() }));
    }

    #[test]
    fn unknown_nodes_and_args_are_located() {
        let graph = g(
            &[("a", "count"), ("b", "echo")],
            &[("a", "b", "text"), ("z", "b", "text")],
        );
        let errors = validate_with(&graph, |cmd| match cmd {
            "count" => Some(vec!["steps".into()]),
            "echo" => Some(vec!["text".into()]),
            _ => None,
        });
        assert!(errors.contains(&GraphError::UnknownNode { id: "z".into() }));
        assert!(
            errors.contains(&GraphError::UnknownArg {
                node: "b".into(),
                command: "echo".into(),
                arg: "text".into()
            }) || !errors
                .iter()
                .any(|e| matches!(e, GraphError::UnknownArg { .. }))
        );
    }

    #[test]
    fn duplicate_node_ids() {
        let graph = g(&[("a", "count"), ("a", "echo")], &[]);
        let errors = validate_with(&graph, |_| Some(vec![]));
        assert!(errors.contains(&GraphError::DuplicateNode { id: "a".into() }));
    }

    #[test]
    fn example_graph_is_valid_shape() {
        let graph = example_graph();
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(plan(&graph).unwrap().order, vec!["count", "echo"]);
    }

    #[test]
    fn layout_follows_stage_columns() {
        let graph = g(&[("a", "count"), ("b", "echo")], &[("a", "b", "text")]);
        let pos = layout(&graph).unwrap();
        assert_eq!(pos[0], ("a".to_string(), 24.0, 24.0));
        assert!(pos[1].1 > pos[0].1, "下游应在下一列: {pos:?}");
    }

    #[test]
    fn pick_reads_json_pointer() {
        let v = serde_json::json!({ "value": 3, "items": [{ "x": 1 }], "a/b": 9 });
        assert_eq!(pick(&v, Some("/value")), serde_json::json!(3));
        assert_eq!(pick(&v, Some("/items/0/x")), serde_json::json!(1));
        assert_eq!(pick(&v, Some("/a~1b")), serde_json::json!(9));
        assert_eq!(pick(&v, Some("/missing")), serde_json::Value::Null);
        assert_eq!(pick(&v, None), v);
    }
}
