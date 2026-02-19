# Phase F — Documentation & Launch

**Duration:** 2 weeks  
**Dependencies:** Phase A-E complete  
**Goal:** Ship it. Make it findable, understandable, and adoptable.

---

## F1. Documentation Site

### Platform

Static site built with `mdBook` (Rust ecosystem convention) or Docusaurus. Hosted on Vercel or GitHub Pages at `docs.cortex.dev` (or `cortex.darlington.dev` initially).

### Structure

```
docs/
├── src/
│   ├── SUMMARY.md
│   ├── getting-started/
│   │   ├── quickstart.md         # Memory for your AI agent in 5 minutes
│   │   ├── installation.md       # Binary, Docker, Homebrew, cargo install
│   │   ├── first-agent.md        # Build a Python agent with Cortex memory
│   │   └── configuration.md      # cortex.toml reference (auto-generated from schema)
│   ├── concepts/
│   │   ├── overview.md           # What is Cortex and why does it exist
│   │   ├── architecture.md       # Internal architecture with diagrams
│   │   ├── graph-model.md        # Nodes, edges, kinds, relations explained
│   │   ├── auto-linker.md        # How relationships are discovered automatically
│   │   ├── briefings.md          # How context synthesis works
│   │   ├── decay-and-memory.md   # How knowledge ages and what survives
│   │   ├── hybrid-search.md      # Vector similarity × graph proximity
│   │   └── vs-alternatives.md    # Cortex vs Mem0 vs Zep vs Chroma vs pgvector
│   ├── guides/
│   │   ├── langchain.md          # LangChain integration guide
│   │   ├── crewai.md             # CrewAI multi-agent guide
│   │   ├── openclaw.md           # OpenClaw integration
│   │   ├── obsidian-import.md    # Turn your Obsidian vault into a knowledge graph
│   │   ├── multi-agent.md        # Shared memory with namespace isolation
│   │   ├── production.md         # Running in production (Docker, monitoring, backup)
│   │   ├── migration.md          # Migrating from other memory solutions
│   │   └── plugins.md            # Writing custom plugins
│   ├── reference/
│   │   ├── cli.md                # Every CLI command documented
│   │   ├── grpc-api.md           # Full gRPC API reference
│   │   ├── http-api.md           # HTTP API reference
│   │   ├── python-sdk.md         # Python cortex-memory reference
│   │   ├── typescript-sdk.md     # TypeScript SDK reference
│   │   ├── go-sdk.md             # Go SDK reference
│   │   ├── rust-sdk.md           # Rust library + client reference
│   │   ├── config.md             # Every config option explained
│   │   ├── metrics.md            # All Prometheus metrics
│   │   └── events.md             # All webhook/SSE event types
│   └── contributing/
│       ├── development.md        # Dev setup, running tests, PR process
│       ├── architecture.md       # Internal architecture for contributors
│       └── plugins.md            # Plugin development guide
```

### Key Pages

**Quickstart (target: 5 minutes to "wow")**

```markdown
# Quick Start

## Install

```bash
# macOS
brew install cortex-memory

# Linux
curl -sSL https://get.cortex.dev | sh

# Docker
docker run -p 9090:9090 -p 9091:9091 mikesquared/cortex

# Cargo
cargo install cortex-server

# From source
git clone https://github.com/MikeSquared-Agency/cortex
cd cortex && cargo build --release
```

## First Use

```bash
# Initialize
cortex init --defaults

# Start server
cortex serve &

# Store some knowledge
cortex node create --kind fact --title "Cortex uses redb" \
    --body "Embedded key-value store with ACID transactions"

cortex node create --kind decision --title "Chose Rust" \
    --body "CPU-bound graph traversal justified the complexity"

# Search
cortex search "database technology"

# See the auto-linker in action
cortex stats  # Watch edge count grow

# Generate a briefing
cortex briefing default

# Explore the graph visually
open http://localhost:9091/viz
```
```

---

## F2. README Rewrite

Public-facing README for GitHub. Not the internal one.

Key sections:
1. **Hero** — name, tagline, badges (CI, crates.io, docs, license)
2. **What is Cortex?** — 3-sentence elevator pitch
3. **Why Cortex?** — comparison table vs alternatives
4. **Quick Start** — 4 commands to running
5. **Features** — bullet list with brief descriptions
6. **Architecture** — clean diagram
7. **SDKs** — code snippets for all 4 languages
8. **Documentation** — link to full docs
9. **Contributing** — link to contributing guide
10. **License** — MIT

### Architecture Diagram (ASCII for README)

```
┌────────────────────────────────────────────────┐
│                 Your AI Agent                   │
│         (Python / TypeScript / Go / Rust)       │
├────────────┬───────────────────┬───────────────┤
│  SDK       │  Library Mode     │  CLI          │
│  (gRPC)    │  (Cortex::open)   │  (cortex ...) │
├────────────┴───────────────────┴───────────────┤
│                cortex-server                    │
│   gRPC API  │  HTTP Debug  │  Event Stream     │
├────────────────────────────────────────────────┤
│                cortex-core                      │
│ ┌──────────┐ ┌──────────┐ ┌──────────────────┐ │
│ │ Storage  │ │  Graph   │ │  Auto-Linker     │ │
│ │ (redb)   │ │  Engine  │ │  (decay/dedup)   │ │
│ └──────────┘ └──────────┘ └──────────────────┘ │
│ ┌──────────┐ ┌──────────┐ ┌──────────────────┐ │
│ │ Vector   │ │ Briefing │ │  Plugins         │ │
│ │ (HNSW)   │ │ Synth    │ │  (WASM)          │ │
│ └──────────┘ └──────────┘ └──────────────────┘ │
├────────────────────────────────────────────────┤
│        Single file on disk (cortex.redb)       │
└────────────────────────────────────────────────┘
```

---

## F3. Repository Hygiene

### Files

```
cortex/
├── LICENSE                    # MIT
├── LICENSE-APACHE             # Apache 2.0 (dual license)
├── CONTRIBUTING.md            # How to contribute
├── CODE_OF_CONDUCT.md         # Contributor Covenant
├── CHANGELOG.md               # Keep-a-changelog format
├── SECURITY.md                # Security policy + vulnerability reporting
├── .github/
│   ├── ISSUE_TEMPLATE/
│   │   ├── bug_report.md
│   │   ├── feature_request.md
│   │   └── plugin_request.md
│   ├── PULL_REQUEST_TEMPLATE.md
│   ├── workflows/
│   │   ├── ci.yml             # Build + test on every PR
│   │   ├── release.yml        # Build binaries + Docker + publish crates
│   │   └── docs.yml           # Deploy docs on merge to main
│   └── FUNDING.yml            # GitHub Sponsors
├── Makefile                    # Common tasks: build, test, bench, lint, docs
└── deny.toml                  # cargo-deny config for license/security audit
```

### CI Pipeline (ci.yml)

```yaml
name: CI
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --all
      - run: cargo clippy -- -D warnings
      - run: cargo fmt --check
      
  build:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release
```

### Release Pipeline (release.yml)

On git tag `v*`:
1. Build binaries for Linux (x86_64, aarch64), macOS (x86_64, arm64), Windows
2. Build Docker image, push to Docker Hub
3. Publish `cortex-core` and `cortex-server` to crates.io
4. Publish Python wheels to PyPI
5. Publish TypeScript package to npm
6. Publish Go module (tag)
7. Create GitHub Release with binaries + changelog

---

## F4. Graph Visualisation SPA

Standalone web app served at `/viz`.

### Tech
- Vanilla HTML + JS (no build step, no npm)
- D3.js for force-directed layout
- Inline in the binary (embedded via `include_str!`)

### Features
- Force-directed layout with configurable physics
- Nodes coloured by kind (legend panel)
- Edge thickness = weight, dashed = auto-generated, solid = manual
- Click node → detail panel (title, body, metadata, edges)
- Search bar → highlights matching nodes, fades others
- Time slider → filter by creation date
- Kind filter → toggle node kinds on/off
- Relation filter → toggle edge types
- Minimap for large graphs
- Export: PNG screenshot, SVG, JSON

### Performance
- WebGL renderer for graphs >1000 nodes (fall back from SVG)
- Lazy loading: only render visible nodes
- Clustering: collapse dense regions into summary nodes

---

## F5. Launch Plan

### Week 1: Soft Launch
- Publish to crates.io, PyPI, npm, Docker Hub
- Post in relevant Discord servers (Rust, AI agents, LangChain)
- Tweet thread from MikeSquared account
- LinkedIn post from Mike's profile
- Submit to Rust weekly newsletter

### Week 2: Public Launch
- **Hacker News** — "Show HN: Cortex — Embedded graph memory for AI agents, written in Rust"
- **Reddit** — r/rust, r/artificial, r/LocalLLaMA, r/ChatGPTCoding
- **Blog post** — "Why we built our own graph memory engine" on darlington.dev
- **Product Hunt** — launch page
- **Dev.to / Hashnode** — cross-post of blog
- **YouTube** — 5-minute demo video (optional but high impact)

### Post-Launch
- Monitor GitHub issues daily for first 2 weeks
- Respond to every issue within 24 hours
- Ship point releases for critical bugs
- Weekly changelog updates
- Monthly blog post on usage patterns / learnings

---

## F6. Success Metrics (90 days)

| Metric | Target | Stretch |
|--------|--------|---------|
| GitHub stars | 500 | 2,000 |
| crates.io downloads | 1,000 | 5,000 |
| PyPI installs | 500 | 2,000 |
| npm installs | 300 | 1,000 |
| Docker pulls | 500 | 2,000 |
| External contributors | 5 | 20 |
| Production users | 10 | 50 |
| Framework integrations | 1 (LangChain or CrewAI) | 3 |
| Blog post views | 5,000 | 20,000 |
| HN upvotes | 100 | 500 |

---

## Deliverables

1. Full documentation site (mdBook) deployed to docs.cortex.dev
2. README rewrite for public audience
3. Repository hygiene (LICENSE, CONTRIBUTING, SECURITY, templates, CI)
4. CI/CD pipeline (test, build, release across all platforms)
5. Graph visualisation SPA with search, filtering, time slider
6. Published packages: crates.io, PyPI, npm, Docker Hub, Homebrew
7. Launch blog post
8. Social media campaign (HN, Reddit, Twitter, LinkedIn, Product Hunt)
