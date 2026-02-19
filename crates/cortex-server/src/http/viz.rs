/// Self-contained D3.js graph visualization
pub const GRAPH_VIZ_HTML: &str = r##"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Cortex Graph Visualization</title>
    <script src="https://d3js.org/d3.v7.min.js"></script>
    <style>
        body {
            margin: 0;
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
            background: #1a1a1a;
            color: #fff;
        }
        #controls {
            position: fixed;
            top: 10px;
            left: 10px;
            background: rgba(0,0,0,0.8);
            padding: 15px;
            border-radius: 8px;
            z-index: 1000;
        }
        #controls h3 {
            margin: 0 0 10px 0;
            font-size: 14px;
        }
        #controls label {
            display: block;
            margin: 5px 0;
            font-size: 12px;
        }
        .node {
            stroke: #fff;
            stroke-width: 1.5px;
            cursor: pointer;
        }
        .link {
            stroke: #999;
            stroke-opacity: 0.6;
        }
        .node-label {
            font-size: 10px;
            pointer-events: none;
            fill: #fff;
            text-shadow: 0 0 3px #000;
        }
        #tooltip {
            position: absolute;
            background: rgba(0,0,0,0.9);
            padding: 10px;
            border-radius: 4px;
            font-size: 12px;
            pointer-events: none;
            display: none;
            max-width: 300px;
        }
    </style>
</head>
<body>
    <div id="controls">
        <h3>Cortex Graph</h3>
        <label>
            <input type="checkbox" id="showLabels" checked> Show Labels
        </label>
        <label>
            Filter by kind:
            <select id="kindFilter">
                <option value="">All</option>
                <option value="Fact">Fact</option>
                <option value="Decision">Decision</option>
                <option value="Event">Event</option>
                <option value="Observation">Observation</option>
                <option value="Pattern">Pattern</option>
                <option value="Identity">Identity</option>
                <option value="Goal">Goal</option>
                <option value="Constraint">Constraint</option>
            </select>
        </label>
    </div>
    <div id="tooltip"></div>
    <svg id="graph"></svg>

    <script>
        const width = window.innerWidth;
        const height = window.innerHeight;

        const svg = d3.select("#graph")
            .attr("width", width)
            .attr("height", height);

        const g = svg.append("g");

        // Zoom behavior
        const zoom = d3.zoom()
            .scaleExtent([0.1, 10])
            .on("zoom", (event) => g.attr("transform", event.transform));
        svg.call(zoom);

        // Color scale for node kinds
        const colorScale = d3.scaleOrdinal()
            .domain(["Fact", "Decision", "Event", "Observation", "Pattern", "Identity", "Goal", "Constraint"])
            .range(["#3b82f6", "#ef4444", "#f59e0b", "#10b981", "#8b5cf6", "#ec4899", "#14b8a6", "#f97316"]);

        let allData = { nodes: [], edges: [] };
        let simulation;

        // Load graph data
        fetch("/graph/export")
            .then(r => r.json())
            .then(response => {
                const data = response.data;
                allData = {
                    nodes: data.nodes.map(n => ({ ...n, x: width/2, y: height/2 })),
                    edges: data.edges
                };
                renderGraph(allData);
            });

        function renderGraph(data) {
            // Clear existing
            g.selectAll("*").remove();

            // Filter nodes
            const kindFilter = document.getElementById("kindFilter").value;
            let nodes = data.nodes;
            if (kindFilter) {
                nodes = nodes.filter(n => n.kind === kindFilter);
            }
            const nodeIds = new Set(nodes.map(n => n.id));
            const edges = data.edges.filter(e => nodeIds.has(e.from) && nodeIds.has(e.to));

            // Create simulation
            simulation = d3.forceSimulation(nodes)
                .force("link", d3.forceLink(edges).id(d => d.id).distance(100))
                .force("charge", d3.forceManyBody().strength(-300))
                .force("center", d3.forceCenter(width / 2, height / 2))
                .force("collision", d3.forceCollide().radius(30));

            // Links
            const link = g.append("g")
                .selectAll("line")
                .data(edges)
                .join("line")
                .attr("class", "link")
                .attr("stroke-width", d => Math.sqrt(d.weight * 3));

            // Nodes
            const node = g.append("g")
                .selectAll("circle")
                .data(nodes)
                .join("circle")
                .attr("class", "node")
                .attr("r", d => 5 + Math.sqrt(d.edge_count) * 2)
                .attr("fill", d => colorScale(d.kind))
                .call(drag(simulation))
                .on("mouseover", showTooltip)
                .on("mouseout", hideTooltip);

            // Labels
            const labels = g.append("g")
                .selectAll("text")
                .data(nodes)
                .join("text")
                .attr("class", "node-label")
                .text(d => d.title.substring(0, 20))
                .style("display", document.getElementById("showLabels").checked ? "block" : "none");

            simulation.on("tick", () => {
                link
                    .attr("x1", d => d.source.x)
                    .attr("y1", d => d.source.y)
                    .attr("x2", d => d.target.x)
                    .attr("y2", d => d.target.y);

                node
                    .attr("cx", d => d.x)
                    .attr("cy", d => d.y);

                labels
                    .attr("x", d => d.x + 10)
                    .attr("y", d => d.y + 3);
            });
        }

        function drag(simulation) {
            function dragstarted(event) {
                if (!event.active) simulation.alphaTarget(0.3).restart();
                event.subject.fx = event.subject.x;
                event.subject.fy = event.subject.y;
            }

            function dragged(event) {
                event.subject.fx = event.x;
                event.subject.fy = event.y;
            }

            function dragended(event) {
                if (!event.active) simulation.alphaTarget(0);
                event.subject.fx = null;
                event.subject.fy = null;
            }

            return d3.drag()
                .on("start", dragstarted)
                .on("drag", dragged)
                .on("end", dragended);
        }

        function showTooltip(event, d) {
            const tooltip = document.getElementById("tooltip");
            tooltip.style.display = "block";
            tooltip.style.left = event.pageX + 10 + "px";
            tooltip.style.top = event.pageY + 10 + "px";
            tooltip.innerHTML = `
                <strong>${d.title}</strong><br>
                Kind: ${d.kind}<br>
                Importance: ${d.importance.toFixed(2)}<br>
                Edges: ${d.edge_count}<br>
                Source: ${d.source_agent}
            `;
        }

        function hideTooltip() {
            document.getElementById("tooltip").style.display = "none";
        }

        // Controls
        document.getElementById("showLabels").addEventListener("change", (e) => {
            g.selectAll(".node-label").style("display", e.target.checked ? "block" : "none");
        });

        document.getElementById("kindFilter").addEventListener("change", () => {
            renderGraph(allData);
        });
    </script>
</body>
</html>
"##;
