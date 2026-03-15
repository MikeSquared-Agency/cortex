# Browser Agent Memory

Cortex gives browser agents a persistent memory for navigating websites, filling forms, and executing web tasks across sessions. The key capability: procedural memory. When an agent learns how to complete a checkout flow on a specific site, that procedure is stored and recalled the next time the agent visits. When the site layout changes, the old procedure is superseded, not lost.

## Configuration

Add a `[briefing.roles]` section to your `cortex.toml` for browser agent workflows:

```toml
[briefing.roles]
identity    = ["agent"]
persistent  = ["procedure", "site-config"]
trackable   = ["task", "workflow"]
temporal    = ["navigation", "interaction"]
reviewable  = ["selector-pattern", "failure-pattern"]
superseding = ["site-layout", "form-structure"]
```

Role mapping for browser agents:

- **persistent** -- step-by-step procedures and site configurations that the agent needs every time it visits a domain.
- **trackable** -- active tasks and multi-step workflows in progress.
- **temporal** -- individual navigations and interactions, providing a session log.
- **reviewable** -- CSS selector patterns and failure patterns that surface for re-evaluation when they start breaking.
- **superseding** -- site layouts and form structures where only the latest version matters. When a site redesigns, the new layout supersedes the old one.

## Procedural Memory

Store step-by-step procedures for specific sites. Tag each procedure with the domain so the briefing engine can surface the right procedure when the agent visits that site:

```python
from cortex_memory import Cortex

cx = Cortex("localhost:9090")

cx.store(
    "procedure",
    "Complete checkout on shop.example.com",
    body="1. Click cart icon (selector: .cart-badge)\n"
         "2. Click 'Proceed to Checkout' (selector: #checkout-btn)\n"
         "3. Fill shipping form: name, address, city, postal code\n"
         "4. Select shipping method (selector: input[name=shipping])\n"
         "5. Click 'Place Order' (selector: .submit-order)",
    importance=0.85,
    source_agent="browser-agent",
    tags=["shop.example.com", "checkout"],
)
```

When the site redesigns and selectors change, store the updated procedure. The auto-linker creates a `supersedes` edge between the old and new versions because they share the same domain tag and high semantic similarity. The briefing shows only the current procedure.

## Cross-Domain Transfer

Techniques learned on one site often apply to others. Cookie consent banners, CAPTCHA flows, and login patterns are structurally similar across domains. The auto-linker discovers this:

```python
cx.store(
    "selector-pattern",
    "Cookie consent dismiss pattern",
    body="Most cookie banners use a button with text 'Accept', "
         "'Accept All', or 'I Agree'. Common selectors: "
         "#accept-cookies, .cookie-accept, [data-action=accept].",
    importance=0.7,
    source_agent="browser-agent",
    tags=["cookie-consent", "cross-domain"],
)
```

When the agent stores a site-specific interaction that matches this pattern, the auto-linker creates an edge linking the specific interaction to the general pattern. The next time the agent encounters a cookie banner on a new site, the briefing surfaces the general pattern as relevant context.

## Failure Patterns

Store failures as reviewable nodes. When the same failure appears across multiple sites, the briefing engine surfaces the pattern so the agent can develop a general workaround:

```python
cx.store(
    "failure-pattern",
    "Dynamic content loading breaks click targets",
    body="On 3 sites, clicking a button immediately after page load "
         "failed because AJAX content shifted the layout. Fix: wait "
         "for network idle or use explicit element-visible checks.",
    importance=0.75,
    source_agent="browser-agent",
    tags=["dynamic-content", "race-condition"],
)
```

The reviewable role means this failure pattern resurfaces periodically in briefings, reminding the agent to apply the workaround proactively rather than waiting for the failure to recur.

## Temporal Validity

Site layouts change without warning. When a selector pattern breaks, mark it with `valid_until` so the briefing engine stops recommending stale procedures:

```python
# Original selector pattern
cx.store(
    "site-layout",
    "shop.example.com navigation structure",
    body="Main nav uses .nav-primary with dropdown menus on hover. "
         "Product pages at /products/{slug}.",
    importance=0.7,
    source_agent="browser-agent",
    tags=["shop.example.com", "navigation"],
    metadata={"valid_from": "2025-11-01"},
)

# When the site redesigns, store the new layout and expire the old one
cx.store(
    "site-layout",
    "shop.example.com navigation structure (2026 redesign)",
    body="Main nav moved to hamburger menu (.nav-mobile). "
         "Product pages now at /shop/{category}/{slug}.",
    importance=0.7,
    source_agent="browser-agent",
    tags=["shop.example.com", "navigation"],
    metadata={"valid_from": "2026-02-15"},
)
```

The briefing engine filters out nodes outside their validity window. The agent sees only the current site layout without manually cleaning up old records.

## Quick Setup

```bash
cortex init --template browser
cortex serve
```

This generates a `cortex.toml` with browser agent roles and a shorter temporal window suited to the fast-changing nature of web content.
