# Running in Production

## Docker (recommended)

```bash
docker run -d \
  --name cortex \
  --restart unless-stopped \
  -p 9090:9090 \
  -p 9091:9091 \
  -v /var/cortex/data:/data \
  -e CORTEX_ENCRYPTION_KEY="$(cat /var/cortex/encryption.key)" \
  mikesquared/cortex:latest
```

Generate an encryption key:

```bash
cortex security generate-key
# Outputs a base64-encoded AES-256 key
# Store this key securely — losing it means losing access to your data
```

## Persistence

All data lives in a single `.redb` file in `data_dir`. Back it up regularly:

```bash
# Built-in backup (creates a timestamped copy with SHA-256 checksum)
cortex backup /backups/cortex-$(date +%Y%m%d).redb
```

## Retention

Enable retention to prevent unbounded growth:

```toml
[retention]
enabled = true
max_age_days = 90
max_nodes = 100000
```

## Monitoring

The HTTP health endpoint returns server stats:

```bash
curl http://localhost:9091/health
# {"healthy": true, "version": "0.1.0", "uptime_seconds": 3600, ...}
```

The stats endpoint returns node and edge counts:

```bash
curl http://localhost:9091/stats
```

## Resource Usage

| Metric | Typical range |
|--------|--------------|
| Memory (base) | 150–300 MB |
| Memory (per 10k nodes) | +50–100 MB (HNSW index) |
| Disk (per 10k nodes) | ~20 MB |
| CPU (idle) | <1% |
| CPU (auto-linker cycle) | 5–30% for 1–5 seconds |

## Security

- Enable encryption at rest with `CORTEX_ENCRYPTION_KEY`
- Run behind a firewall — gRPC and HTTP ports are unauthenticated by default
- Use the audit log to track all mutations: `cortex audit`

## Upgrade

Stop the server, replace the binary, restart. Cortex handles schema migrations automatically on startup.

```bash
docker pull mikesquared/cortex:latest
docker stop cortex && docker rm cortex
# Restart with same command as above
```
