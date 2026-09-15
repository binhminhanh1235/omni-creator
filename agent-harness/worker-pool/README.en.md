# Worker-pool quick summary

Use one coordinator for OmniCreator control and bounded workers for external execution. The coordinator owns `agent_work_graph`, `agent_work_prepare`, and `agent_work_commit`; workers only return candidate results.

External work may run in parallel. Canonical commits remain serialized through OmniCreator's writer lease. Re-inspect after every commit batch, rebuild the queue after reconnect, discard stale candidates, and never treat worker/provider output as canonical success until OmniCreator accepts it.

See `README.md` in this directory for the full operating recipe and `contract.json` for the machine-readable boundary.
