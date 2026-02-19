/// Self-contained D3.js graph visualization SPA
pub const GRAPH_VIZ_HTML: &str = r##"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Cortex Graph Visualization</title>
    <script src="https://d3js.org/d3.v7.min.js"></script>
    <style>
        * { box-sizing: border-box; }
        body {
            margin: 0;
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
            background: #1a1a1a;
            color: #e0e0e0;
            overflow: hidden;
        }
        #controls {
            position: fixed;
            top: 10px;
            left: 10px;
            background: rgba(20,20,20,0.92);
            padding: 14px 16px;
            border-radius: 8px;
            border: 1px solid #333;
            z-index: 1000;
            min-width: 200px;
        }
        #controls h3 {
            margin: 0 0 10px 0;
            font-size: 13px;
            color: #fff;
            letter-spacing: 0.05em;
            text-transform: uppercase;
        }
        #controls label {
            display: block;
            margin: 6px 0;
            font-size: 12px;
            color: #ccc;
        }
        #controls input[type=text],
        #controls select {
            width: 100%;
            background: #2a2a2a;
            border: 1px solid #444;
            color: #e0e0e0;
            padding: 4px 8px;
            border-radius: 4px;
            font-size: 12px;
            margin-top: 2px;
        }
        #controls input[type=checkbox] {
            margin-right: 6px;
        }
        .btn-row {
            display: flex;
            gap: 4px;
            margin-top: 10px;
        }
        .btn {
            flex: 1;
            background: #2a2a2a;
            border: 1px solid #444;
            color: #ccc;
            padding: 4px 6px;
            border-radius: 4px;
            font-size: 11px;
            cursor: pointer;
            text-align: center;
        }
        .btn:hover { background: #3a3a3a; color: #fff; }
        .node {
            stroke: #fff;
            stroke-width: 1.5px;
            cursor: pointer;
            transition: opacity 0.2s;
        }
        .node.dimmed { opacity: 0.1; }
        .node.highlighted { stroke: #fff; stroke-width: 2.5px; }
        .link {
            stroke: #999;
            stroke-opacity: 0.5;
        }
        .link.dimmed { stroke-opacity: 0.05; }
        .node-label {
            font-size: 10px;
            pointer-events: none;
            fill: #ddd;
            text-shadow: 0 0 4px #000, 0 0 4px #000;
        }
        #detail {
            position: fixed;
            top: 10px;
            right: 10px;
            width: 280px;
            background: rgba(20,20,20,0.95);
            border: 1px solid #333;
            border-radius: 8px;
            padding: 16px;
            z-index: 1000;
            display: none;
            max-height: calc(100vh - 20px);
            overflow-y: auto;
        }
        #detail h4 {
            margin: 0 0 8px 0;
            font-size: 14px;
            color: #fff;
            line-height: 1.3;
        }
        #detail .close-btn {
            position: absolute;
            top: 10px;
            right: 12px;
            background: none;
            border: none;
            color: #888;
            cursor: pointer;
            font-size: 16px;
            line-height: 1;
            padding: 0;
        }
        #detail .close-btn:hover { color: #fff; }
        .detail-row {
            margin: 6px 0;
            font-size: 12px;
            color: #bbb;
        }
        .detail-row strong {
            color: #888;
            font-size: 10px;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            display: block;
            margin-bottom: 2px;
        }
        .kind-badge {
            display: inline-block;
            padding: 2px 8px;
            border-radius: 12px;
            font-size: 11px;
            font-weight: 600;
        }
        .detail-body {
            margin-top: 8px;
            padding: 8px;
            background: #2a2a2a;
            border-radius: 4px;
            font-size: 11px;
            line-height: 1.5;
            color: #ccc;
            max-height: 120px;
            overflow-y: auto;
        }
        #node-count {
            position: fixed;
            bottom: 10px;
            left: 10px;
            font-size: 11px;
            color: #666;
        }
        #legend {
            position: fixed;
            bottom: 10px;
            right: 10px;
            background: rgba(20,20,20,0.85);
            border: 1px solid #333;
            border-radius: 6px;
            padding: 10px 12px;
            font-size: 11px;
        }
        #legend .legend-item {
            display: flex;
            align-items: center;
            gap: 6px;
            margin: 3px 0;
            color: #bbb;
        }
        .legend-dot {
            width: 10px;
            height: 10px;
            border-radius: 50%;
            flex-shrink: 0;
        }
    </style>
</head>
<body>
    <div id="controls">
        <h3>Cortex Graph</h3>
        <label>
            Search
            <input type="text" id="search" placeholder="filter by title…">
        </label>
        <label>
            Kind
            <select id="kindFilter">
                <option value="">All kinds</option>
            </select>
        </label>
        <label>
            <input type="checkbox" id="showLabels" checked> Labels
        </label>
        <label>
            Min importance: <span id="importanceVal">0.0</span>
            <input type="range" id="importanceFilter" min="0" max="1" step="0.05" value="0"
                   style="width:100%;margin-top:4px;">
        </label>
        <div class="btn-row">
            <div class="btn" onclick="exportSVG()">SVG</div>
            <div class="btn" onclick="exportPNG()">PNG</div>
            <div class="btn" onclick="exportJSON()">JSON</div>
        </div>
    </div>

    <div id="detail">
        <button class="close-btn" onclick="closeDetail()">✕</button>
        <h4 id="detail-title"></h4>
        <div class="detail-row">
            <strong>Kind</strong>
            <span id="detail-kind"></span>
        </div>
        <div class="detail-row">
            <strong>Importance</strong>
            <span id="detail-importance"></span>
        </div>
        <div class="detail-row">
            <strong>Source Agent</strong>
            <span id="detail-agent"></span>
        </div>
        <div class="detail-row" id="detail-tags-row">
            <strong>Tags</strong>
            <span id="detail-tags"></span>
        </div>
        <div class="detail-row" id="detail-body-row">
            <strong>Body</strong>
            <div class="detail-body" id="detail-body"></div>
        </div>
        <div class="detail-row">
            <strong>Connections</strong>
            <span id="detail-edges"></span>
        </div>
        <div class="detail-row">
            <strong>Node ID</strong>
            <span id="detail-id" style="font-size:10px;word-break:break-all;color:#666;"></span>
        </div>
    </div>

    <svg id="graph"></svg>
    <div id="node-count"></div>
    <div id="legend"></div>

    <script>
        const width = window.innerWidth;
        const height = window.innerHeight;

        const svg = d3.select("#graph")
            .attr("width", width)
            .attr("height", height);

        const g = svg.append("g");

        const zoom = d3.zoom()
            .scaleExtent([0.05, 15])
            .on("zoom", (event) => g.attr("transform", event.transform));
        svg.call(zoom);

        // Click on canvas background closes detail
        svg.on("click", (event) => {
            if (event.target === svg.node()) closeDetail();
        });

        const KIND_COLORS = {
            "Fact":        "#3b82f6",
            "Decision":    "#ef4444",
            "Event":       "#f59e0b",
            "Observation": "#10b981",
            "Pattern":     "#8b5cf6",
            "Identity":    "#ec4899",
            "Goal":        "#14b8a6",
            "Constraint":  "#f97316",
            "Agent":       "#6366f1",
        };

        const colorScale = d3.scaleOrdinal()
            .domain(Object.keys(KIND_COLORS))
            .range(Object.values(KIND_COLORS))
            .unknown("#94a3b8");

        function nodeRadius(d) {
            return 4 + (d.importance || 0) * 12;
        }

        let allData = { nodes: [], edges: [] };
        let simulation;
        let linkSel, nodeSel, labelSel;

        fetch("/graph/export")
            .then(r => r.json())
            .then(response => {
                const data = response.data;
                allData = {
                    nodes: data.nodes.map(n => ({ ...n, x: width / 2 + (Math.random() - 0.5) * 200, y: height / 2 + (Math.random() - 0.5) * 200 })),
                    edges: data.edges
                };
                populateKindFilter(allData.nodes);
                buildLegend(allData.nodes);
                renderGraph();
            })
            .catch(err => {
                document.getElementById("node-count").textContent = "Failed to load graph data: " + err;
            });

        function populateKindFilter(nodes) {
            const kinds = [...new Set(nodes.map(n => n.kind))].sort();
            const sel = document.getElementById("kindFilter");
            kinds.forEach(k => {
                const opt = document.createElement("option");
                opt.value = k;
                opt.textContent = k;
                sel.appendChild(opt);
            });
        }

        function buildLegend(nodes) {
            const kinds = [...new Set(nodes.map(n => n.kind))].sort();
            const legend = document.getElementById("legend");
            legend.innerHTML = kinds.map(k => `
                <div class="legend-item">
                    <div class="legend-dot" style="background:${colorScale(k)}"></div>
                    ${k}
                </div>
            `).join("");
        }

        function getFilteredData() {
            const searchTerm = document.getElementById("search").value.toLowerCase().trim();
            const kindFilter = document.getElementById("kindFilter").value;
            const minImportance = parseFloat(document.getElementById("importanceFilter").value);

            let nodes = allData.nodes.filter(n => {
                if (kindFilter && n.kind !== kindFilter) return false;
                if ((n.importance || 0) < minImportance) return false;
                return true;
            });

            const nodeIds = new Set(nodes.map(n => n.id));
            const edges = allData.edges.filter(e => nodeIds.has(e.from) && nodeIds.has(e.to));

            return { nodes, edges, searchTerm };
        }

        function renderGraph() {
            const { nodes, edges, searchTerm } = getFilteredData();

            // Clear
            g.selectAll("*").remove();

            if (simulation) simulation.stop();

            simulation = d3.forceSimulation(nodes)
                .force("link", d3.forceLink(edges).id(d => d.id).distance(80))
                .force("charge", d3.forceManyBody().strength(-300))
                .force("center", d3.forceCenter(width / 2, height / 2))
                .force("collision", d3.forceCollide().radius(d => nodeRadius(d) + 4));

            // Links
            linkSel = g.append("g")
                .selectAll("line")
                .data(edges)
                .join("line")
                .attr("class", "link")
                .attr("stroke-width", d => 0.5 + d.weight * 3);

            // Nodes
            nodeSel = g.append("g")
                .selectAll("circle")
                .data(nodes)
                .join("circle")
                .attr("class", "node")
                .attr("r", nodeRadius)
                .attr("fill", d => colorScale(d.kind))
                .call(dragBehavior(simulation))
                .on("click", (event, d) => {
                    event.stopPropagation();
                    showDetail(d);
                });

            // Labels
            labelSel = g.append("g")
                .selectAll("text")
                .data(nodes)
                .join("text")
                .attr("class", "node-label")
                .text(d => d.title.length > 24 ? d.title.substring(0, 22) + "…" : d.title)
                .style("display", document.getElementById("showLabels").checked ? "block" : "none");

            simulation.on("tick", () => {
                linkSel
                    .attr("x1", d => d.source.x)
                    .attr("y1", d => d.source.y)
                    .attr("x2", d => d.target.x)
                    .attr("y2", d => d.target.y);
                nodeSel
                    .attr("cx", d => d.x)
                    .attr("cy", d => d.y);
                labelSel
                    .attr("x", d => d.x + nodeRadius(d) + 3)
                    .attr("y", d => d.y + 4);
            });

            applySearch(searchTerm, nodes, edges);

            document.getElementById("node-count").textContent =
                `${nodes.length} nodes · ${edges.length} edges`;
        }

        function applySearch(term, nodes, edges) {
            if (!nodeSel) return;
            if (!term) {
                nodeSel.classed("dimmed", false);
                linkSel.classed("dimmed", false);
                return;
            }
            const matched = new Set(
                nodes.filter(n => n.title.toLowerCase().includes(term) ||
                                  (n.body || "").toLowerCase().includes(term) ||
                                  (n.kind || "").toLowerCase().includes(term))
                    .map(n => n.id)
            );
            nodeSel.classed("dimmed", d => !matched.has(d.id));
            linkSel.classed("dimmed", d => !matched.has(d.source.id || d.source) &&
                                           !matched.has(d.target.id || d.target));
        }

        function showDetail(d) {
            document.getElementById("detail-title").textContent = d.title;
            document.getElementById("detail-kind").innerHTML =
                `<span class="kind-badge" style="background:${colorScale(d.kind)}20;color:${colorScale(d.kind)};border:1px solid ${colorScale(d.kind)}40">${d.kind}</span>`;
            document.getElementById("detail-importance").textContent =
                (d.importance || 0).toFixed(3);
            document.getElementById("detail-agent").textContent = d.source_agent || "—";
            document.getElementById("detail-edges").textContent = d.edge_count || 0;
            document.getElementById("detail-id").textContent = d.id;

            const tagsRow = document.getElementById("detail-tags-row");
            const tagsEl = document.getElementById("detail-tags");
            if (d.tags && d.tags.length > 0) {
                tagsEl.textContent = d.tags.join(", ");
                tagsRow.style.display = "";
            } else {
                tagsRow.style.display = "none";
            }

            const bodyRow = document.getElementById("detail-body-row");
            const bodyEl = document.getElementById("detail-body");
            if (d.body) {
                bodyEl.textContent = d.body;
                bodyRow.style.display = "";
            } else {
                bodyRow.style.display = "none";
            }

            document.getElementById("detail").style.display = "block";
        }

        function closeDetail() {
            document.getElementById("detail").style.display = "none";
        }

        function dragBehavior(simulation) {
            return d3.drag()
                .on("start", (event) => {
                    if (!event.active) simulation.alphaTarget(0.3).restart();
                    event.subject.fx = event.subject.x;
                    event.subject.fy = event.subject.y;
                })
                .on("drag", (event) => {
                    event.subject.fx = event.x;
                    event.subject.fy = event.y;
                })
                .on("end", (event) => {
                    if (!event.active) simulation.alphaTarget(0);
                    event.subject.fx = null;
                    event.subject.fy = null;
                });
        }

        // Export handlers
        function exportSVG() {
            const svgEl = document.getElementById("graph");
            const serializer = new XMLSerializer();
            let source = serializer.serializeToString(svgEl);
            if (!source.match(/^<svg[^>]+xmlns="http:\/\/www\.w3\.org\/2000\/svg"/)) {
                source = source.replace(/^<svg/, '<svg xmlns="http://www.w3.org/2000/svg"');
            }
            const blob = new Blob([source], { type: "image/svg+xml;charset=utf-8" });
            downloadBlob(blob, "cortex-graph.svg");
        }

        function exportPNG() {
            const svgEl = document.getElementById("graph");
            const serializer = new XMLSerializer();
            let source = serializer.serializeToString(svgEl);
            const canvas = document.createElement("canvas");
            canvas.width = width;
            canvas.height = height;
            const ctx = canvas.getContext("2d");
            ctx.fillStyle = "#1a1a1a";
            ctx.fillRect(0, 0, width, height);
            const img = new Image();
            img.onload = () => {
                ctx.drawImage(img, 0, 0);
                canvas.toBlob(blob => downloadBlob(blob, "cortex-graph.png"), "image/png");
            };
            img.src = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(source);
        }

        function exportJSON() {
            fetch("/graph/export")
                .then(r => r.json())
                .then(data => {
                    const blob = new Blob([JSON.stringify(data.data, null, 2)],
                                         { type: "application/json" });
                    downloadBlob(blob, "cortex-graph.json");
                });
        }

        function downloadBlob(blob, filename) {
            const url = URL.createObjectURL(blob);
            const a = document.createElement("a");
            a.href = url;
            a.download = filename;
            a.click();
            URL.revokeObjectURL(url);
        }

        // Controls
        document.getElementById("showLabels").addEventListener("change", (e) => {
            if (labelSel) labelSel.style("display", e.target.checked ? "block" : "none");
        });

        document.getElementById("kindFilter").addEventListener("change", renderGraph);
        document.getElementById("importanceFilter").addEventListener("input", (e) => {
            document.getElementById("importanceVal").textContent = parseFloat(e.target.value).toFixed(2);
            renderGraph();
        });

        let searchTimeout;
        document.getElementById("search").addEventListener("input", (e) => {
            clearTimeout(searchTimeout);
            searchTimeout = setTimeout(() => {
                const { nodes, edges } = getFilteredData();
                applySearch(e.target.value.toLowerCase().trim(), nodes, edges);
            }, 150);
        });
    </script>
</body>
</html>
"##;
