use crate::cli::{grpc_connect, ExportArgs};
use anyhow::Result;
use cortex_proto::*;
use std::io::Write;

pub async fn run(args: ExportArgs, server: &str) -> Result<()> {
    let mut client = grpc_connect(server).await?;

    // Fetch all nodes
    let kind_filter = args.kind.map(|k| vec![k]).unwrap_or_default();
    let nodes_resp = client
        .list_nodes(ListNodesRequest {
            kind_filter,
            limit: 100_000,
            ..Default::default()
        })
        .await?
        .into_inner();

    let nodes = &nodes_resp.nodes;

    // Fetch edges for each node (collect unique edges)
    let mut all_edge_ids = std::collections::HashSet::new();
    let mut all_edges = Vec::new();

    for node in nodes {
        let edges_resp = client
            .get_edges(GetEdgesRequest {
                node_id: node.id.clone(),
                direction: "outgoing".into(),
            })
            .await?
            .into_inner();

        for edge in edges_resp.edges {
            if all_edge_ids.insert(edge.id.clone()) {
                all_edges.push(edge);
            }
        }
    }

    let output = match args.format.as_str() {
        "json" => format_json(nodes, &all_edges)?,
        "jsonl" => format_jsonl(nodes)?,
        "dot" => format_dot(nodes, &all_edges),
        "graphml" => format_graphml(nodes, &all_edges),
        other => anyhow::bail!("Unknown export format: {}", other),
    };

    if let Some(out_path) = args.output {
        std::fs::write(&out_path, &output)?;
        println!("Exported to {}", out_path.display());
    } else {
        std::io::stdout().write_all(output.as_bytes())?;
    }

    Ok(())
}

fn format_json(nodes: &[NodeResponse], edges: &[EdgeResponse]) -> Result<String> {
    let node_vals: Vec<_> = nodes.iter().map(node_to_json).collect();
    let edge_vals: Vec<_> = edges.iter().map(edge_to_json).collect();
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "nodes": node_vals,
        "edges": edge_vals,
    }))?)
}

fn format_jsonl(nodes: &[NodeResponse]) -> Result<String> {
    let mut out = String::new();
    for node in nodes {
        out.push_str(&serde_json::to_string(&node_to_json(node))?);
        out.push('\n');
    }
    Ok(out)
}

fn format_dot(nodes: &[NodeResponse], edges: &[EdgeResponse]) -> String {
    let mut out = String::from("digraph cortex {\n  rankdir=LR;\n");
    for node in nodes {
        let label = node.title.replace('"', "\\\"");
        let id_short = &node.id[..8];
        out.push_str(&format!(
            "  \"{}\" [label=\"{}\\n[{}]\" shape=box];\n",
            id_short, label, node.kind
        ));
    }
    for edge in edges {
        out.push_str(&format!(
            "  \"{}\" -> \"{}\" [label=\"{}\"];\n",
            &edge.from_id[..8],
            &edge.to_id[..8],
            edge.relation
        ));
    }
    out.push_str("}\n");
    out
}

fn format_graphml(nodes: &[NodeResponse], edges: &[EdgeResponse]) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<graphml xmlns="http://graphml.graphdrawing.org/graphml">
  <key id="kind" for="node" attr.name="kind" attr.type="string"/>
  <key id="title" for="node" attr.name="title" attr.type="string"/>
  <key id="importance" for="node" attr.name="importance" attr.type="float"/>
  <key id="relation" for="edge" attr.name="relation" attr.type="string"/>
  <key id="weight" for="edge" attr.name="weight" attr.type="float"/>
  <graph id="cortex" edgedefault="directed">
"#,
    );

    for node in nodes {
        out.push_str(&format!(
            "    <node id=\"{}\">\n      <data key=\"kind\">{}</data>\n      <data key=\"title\">{}</data>\n      <data key=\"importance\">{}</data>\n    </node>\n",
            node.id, xml_escape(&node.kind), xml_escape(&node.title), node.importance
        ));
    }

    for edge in edges {
        out.push_str(&format!(
            "    <edge source=\"{}\" target=\"{}\">\n      <data key=\"relation\">{}</data>\n      <data key=\"weight\">{}</data>\n    </edge>\n",
            edge.from_id, edge.to_id, xml_escape(&edge.relation), edge.weight
        ));
    }

    out.push_str("  </graph>\n</graphml>\n");
    out
}

fn node_to_json(n: &NodeResponse) -> serde_json::Value {
    serde_json::json!({
        "id": n.id,
        "kind": n.kind,
        "title": n.title,
        "body": n.body,
        "importance": n.importance,
        "tags": n.tags,
        "source_agent": n.source_agent,
    })
}

fn edge_to_json(e: &EdgeResponse) -> serde_json::Value {
    serde_json::json!({
        "id": e.id,
        "from_id": e.from_id,
        "to_id": e.to_id,
        "relation": e.relation,
        "weight": e.weight,
    })
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
