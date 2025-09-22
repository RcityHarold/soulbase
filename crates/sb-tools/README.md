# sb-tools

Unified Tool SDK providing Manifest, Registry, Preflight, and Invocation orchestration for the Soul platform.

- Schema-first tool declarations with capabilities, scopes, limits, and safety metadata.
- In-memory registry and preflight service (auth hook + sandbox profile synthesis).
- Invocation pipeline with sandbox execution, JSON-schema validation, idempotency cache, and structured results.

## Development



## Follow-ups

- Integrate with real Auth/QoS backends and persistent registries/idempotency stores.
- Expand capability→ExecOp mapping coverage and output repair strategies.
- Emit structured events/metrics via observe hooks and connect to Evidence sinks.
