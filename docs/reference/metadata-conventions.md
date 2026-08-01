# Metadata Conventions

Cortex's `data.metadata` field is an open `HashMap<String, Value>`. This flexibility is intentional — the schema doesn't prescribe what agents store. But without conventions, every agent author invents their own keys.

This document defines **well-known metadata keys**: documented conventions that are not enforced at the schema level but are recognised by Cortex's intelligence layer (auto-linker, briefing engine, trust scoring). Like HTTP headers — anyone can add custom ones, but `Content-Type` means something specific.

## Node metadata

| Key              | Type             | Description                                                                                                       | Used by                                   |
| ---------------- | ---------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| `entity_type`    | string           | What kind of entity this is: `agent`, `company`, `person`, `technology`, `project`, `location`, `product`         | Entity layer, briefing engine             |
| `aliases`        | array of strings | Alternative names for an entity node. Used for entity resolution.                                                 | Auto-linker entity matching               |
| `parent_agent`   | string           | Agent ID that spawned the agent that created this node. Provenance chain.                                         | Trust scoring (source reliability), audit |
| `task_id`        | string           | Orchestrator task ID that prompted this node's creation.                                                          | Audit, task-scoped queries                |
| `source_url`     | string           | External URL where this knowledge originated.                                                                     | Provenance, citation                      |
| `content_type`   | string           | MIME-like type of the body content: `text/plain`, `text/markdown`, `application/json`, `code/rust`, `code/python` | Embedding strategy, display               |
| `language`       | string           | ISO 639-1 language code of the content: `en`, `zh`, `ja`                                                          | Embedding model selection                 |
| `expires_reason` | string           | Why this node has an `expires_at`. `working-memory`, `ttl-policy`, `gdpr-request`                                 | Audit                                     |

## Edge metadata

| Key                  | Type   | Description                                                           | Used by                                          |
| -------------------- | ------ | --------------------------------------------------------------------- | ------------------------------------------------ |
| `entity`             | string | Normalised entity name that links these two nodes.                    | Entity co-occurrence rule, cross-agent discovery |
| `similarity_context` | string | What the similarity was about (title match, body match, tag overlap). | Debugging, trust scoring                         |
| `rule_version`       | string | Version of the rule that created this edge.                           | Migration, debugging                             |

## Entity type

Use on nodes with `kind: "entity"` to classify the entity.

```bash
cortex node create --kind entity --title "Company X" \
    --metadata '{"entity_type": "company", "aliases": ["CompanyX", "company-x"]}'
```

The auto-linker's entity promotion uses `entity_type` to tag promoted nodes.
The briefing engine uses `entity_type` to group entities in cross-agent sections.

Well-known entity types:

| Value        | Description                     |
| ------------ | ------------------------------- |
| `agent`      | An AI agent or bot              |
| `company`    | A company or organisation       |
| `person`     | A human individual              |
| `technology` | A language, framework, or tool  |
| `project`    | A named project or initiative   |
| `location`   | A place (city, region, address) |
| `product`    | A product or service            |

Custom entity types are allowed — these are conventions, not constraints.

## Aliases

Use `aliases` to list alternative names for an entity. The auto-linker's entity matching checks aliases when resolving entity references across nodes.

```bash
cortex node create --kind entity --title "Anthropic" \
    --metadata '{"entity_type": "company", "aliases": ["anthropic-ai", "Anthropic PBC"]}'
```

## Source URL

Use `source_url` to record where knowledge was sourced from. Must be an HTTP or HTTPS URL.

```bash
cortex node create --kind fact --title "GPT-4 context window is 128k" \
    --metadata '{"source_url": "https://platform.openai.com/docs/models"}'
```

## Content type

Use `content_type` to hint at the body format. This may influence embedding strategy in future versions.

```bash
cortex node create --kind fact --title "Rust sort implementation" \
    --body "fn sort(items: &mut [i32]) { items.sort(); }" \
    --metadata '{"content_type": "code/rust"}'
```

## Validation

These conventions are **not enforced** at the storage level. Nodes with non-standard metadata continue to work. However, the `check_conventions()` function (Rust) and the `--check-conventions` CLI flag will emit warnings for metadata that doesn't follow these conventions.

```bash
cortex node create --kind entity --title "Test" --check-conventions
# Warning: entity_type should be a string (missing on entity node)
# Created node abc-123
```

## SDK helpers

### Python

```python
from cortex_memory import Cortex, EntityType

cx = Cortex("localhost:9090")

# Helper that sets entity_type metadata automatically
cx.store_entity("Company X", entity_type=EntityType.COMPANY, aliases=["CompanyX"])

# Equivalent to:
cx.store("entity", "Company X", metadata={
    "entity_type": "company",
    "aliases": "CompanyX",  # serialised as string over gRPC
})
```

### TypeScript

```typescript
import { Cortex, EntityType } from "cortex-memory-client";

const cx = new Cortex("localhost:9090");

await cx.storeEntity("Company X", {
  entityType: EntityType.Company,
  aliases: ["CompanyX"],
});
```

## Backward compatibility

Entirely additive. Existing nodes without well-known metadata keys continue to work. The intelligence layer (auto-linker, briefing engine) checks for these keys but gracefully handles their absence.
