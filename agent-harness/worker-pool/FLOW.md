# End-to-end agent-native production flow

This is the compact P3 handoff recipe for Script -> ScenePlan -> parallel Visual + Voice -> QA -> ProductionPack.

```text
Coordinator
  |
  +-- Content control: commit Script
  |
  +-- ScenePlan control: commit storyboard
  |
  +-- agent_work_graph
  |      |
  |      +-- visual work IDs after ScenePlan
  |      `-- voice work IDs after Content
  |
  +-- agent_work_prepare x ready item
  |
  +-- bounded fan-out
  |      +-- visual worker 1..N
  |      `-- voice worker 1..M
  |
  +-- candidate results + provenance
  |
  +-- agent_work_commit (serialized bounded batch)
  |
  +-- agent_work_graph again
  |
  +-- Review Center / recovery for missing, failed, stale, replaced work
  |
  `-- ProductionPack -> Resolve export
```

## Coordinator invariants

- It is the only harness role that commits parallel-work results.
- It uses only canonical IDs returned by OmniCreator.
- It prepares immediately before dispatch and re-inspects immediately after commit.
- It does not treat worker success, provider success, or file existence as canonical success.
- It rebuilds the dispatch queue from current canonical state after reconnect.

## Worker invariants

- One prepared work descriptor is one worker assignment.
- A worker may call an external provider/tool needed for that assignment.
- A worker returns candidate artifact metadata and truthful provenance to the coordinator.
- A worker never edits OmniCreator canonical storage and never calls `agent_work_commit`.

## Parallelism boundary

Script selection and ScenePlan establishment are dependency gates. Visual scene work and voice-segment work may fan out concurrently only after their respective canonical dependencies are satisfied. Fan-in happens through serialized canonical commits and subsequent QA/recovery, not through a shared worker database.
