# Portable Data Root and Device Handoff

## Goal

A user chooses one OmniCreator **Data Root**.

That directory contains all durable data required to continue production work.

The user can:

1. close OmniCreator on Machine A
2. copy the Data Root to another disk/machine, or synchronize it through Google Drive
3. install/open OmniCreator on Machine B
4. choose **Use Existing Data Folder**
5. continue projects with the same step/job/take/artifact state

The machine may use a different physical path.

## Product contract

The workspace is identified by its manifest, not by its absolute folder path.

```text
Machine A:
/Users/kiet/Google Drive/OmniCreatorData

Machine B:
/Volumes/Work/OmniCreatorData

                    same workspace_id
                          |
                    same projects/state
```

## Recommended folder layout

```text
OmniCreatorData/
├── .omnicreator/
│   ├── workspace.json
│   ├── state/
│   │   └── omnicreator.sqlite
│   ├── backups/
│   │   ├── state-r000182.sqlite
│   │   ├── state-r000183.sqlite
│   │   └── state-r000184.sqlite
│   ├── handoff/
│   │   └── latest.json
│   └── plugin-lock.json
│
├── projects/
│   ├── P001/
│   ├── P002/
│   └── ...
│
├── library/
│   └── assets/
│
├── studio-packs/
├── channel-profiles/
├── plugin-data/
├── exports/
└── metadata/
```

The exact layout can evolve through schema migration, but every durable path must stay inside the Data Root unless explicitly marked as an Advanced external reference.

## What must be portable

- project definitions
- scripts and versions
- segment/scene state
- DAG/job/attempt history
- SQLite operational state
- local/downloaded/generated media
- voice takes
- selected takes/assets
- captions and timing artifacts
- asset provenance/license data
- Studio Packs
- channel profiles
- plugin settings
- plugin version requirements
- export source data
- clean state backups

## What should not be portable by default

- plaintext API secrets
- OS Keychain data
- temporary files
- disposable download/preview cache
- platform-specific binaries
- Python virtual environments
- AI model runtime caches unless explicitly configured
- DaVinci application path
- per-device GPU preferences

These are reconstructed/reconfigured on the destination machine.

## Logical path rule

Canonical state must never rely on absolute filesystem paths.

Bad:

```text
/Users/kiet/My Drive/OmniCreatorData/projects/P07/audio/S04.wav
```

Good:

```text
workspace://projects/P07/audio/S04.wav
project://audio/S04.wav
artifact://AUDIO_221
```

A PathResolver maps these logical references to the current Data Root.

### Security

Path resolution must normalize and validate paths so a plugin/project cannot use `../` or symlink tricks to escape its allowed workspace scope.

## Machine-local binding

Each OmniCreator installation stores a minimal local binding:

```json
{
  "data_root": "/current/machine/path/OmniCreatorData",
  "device_id": "device_..."
}
```

This is intentionally not synchronized as workspace state.

Changing machines simply creates a different local binding to the same `workspace_id`.

## Workspace manifest

Example:

```json
{
  "schema": "omnicreator.workspace",
  "schema_version": 1,
  "workspace_id": "ws_...",
  "revision": 184,
  "created_at": "...",
  "updated_at": "...",
  "last_clean_shutdown": true,
  "last_writer_device": "device_..."
}
```

The manifest must remain small and atomically replaceable.

## Device handoff protocol

### Normal close

When the application closes cleanly:

1. stop scheduling new writes
2. allow/resolve local transactions to finish
3. commit SQLite state
4. create a consistent database backup/snapshot
5. calculate snapshot hash
6. atomically write the handoff manifest for that revision
7. release workspace lock
8. mark the workspace clean

### Explicit action

Also expose:

```text
Prepare for Device Handoff
```

This performs the same safety sequence without requiring the user to understand database mechanics.

### New machine open

When **Use Existing Data Folder** is selected:

1. locate `.omnicreator/workspace.json`
2. validate schema/workspace ID
3. check for probable active writer
4. validate latest clean handoff manifest
5. verify referenced snapshot/hash
6. validate/recover SQLite if necessary
7. scan artifact existence/hashes lazily
8. check required plugins
9. check credential references
10. load the existing project board
11. reconcile interrupted jobs
12. continue

## Google Drive synchronization model

Supported:

```text
Machine A writes
-> OmniCreator closes cleanly
-> Google Drive finishes sync
-> Machine B opens
```

Not supported as normal workflow:

```text
Machine A writes  <---->  Machine B writes
       simultaneously through Google Drive
```

File synchronization is not a distributed database.

## Single-writer safety

Use:

- an OS file lock for local access
- a synchronized lease/heartbeat record for best-effort cross-device detection
- workspace/device IDs
- last-writer metadata
- clean-shutdown/handoff revision

The lease is advisory because sync latency means it cannot provide strict distributed locking.

If another recent writer is detected, default UX should offer read-only/check-again instead of unsafe automatic write access.

## SQLite strategy

SQLite remains intentionally simple and local-file based.

For a syncable Data Root:

- do not rely on WAL/SHM files being independently synchronized as the recovery mechanism
- use a cloud-sync-friendly journal/close strategy
- create consistent clean snapshots with SQLite backup facilities
- retain several rotating snapshots
- validate before recovery/open after suspicious shutdown
- never assume an actively-mutating DB copy is a valid handoff

The precise PRAGMA values can be benchmarked during implementation, but the architecture requires clean snapshots and single-writer semantics.

## Artifact write strategy

For large files:

```text
write temp file
-> fsync/close as appropriate
-> calculate hash
-> atomic rename to final path
-> commit Artifact metadata
```

This reduces the chance that a sync service exposes a partially-generated artifact under its final name.

## Online-only cloud files

Google Drive and similar tools may keep files online-only.

OmniCreator should distinguish:

- artifact metadata exists
- file entry exists
- file is locally available/fully hydrated
- file hash verified

For DaVinci editing, active project media should preferably be marked **available offline**.

The app should not regenerate an expensive Kaggle artifact merely because a cloud placeholder has not hydrated yet.

## Plugins on a new machine

Store a workspace plugin lock/config:

```json
{
  "plugins": [
    {"id":"pexels","version":"1.0.0"},
    {"id":"omnivoice","version":"3.2.0"}
  ]
}
```

On Machine B:

```text
Workspace loaded
2 required plugins missing

[ Install Missing Plugins ]
```

Plugin data/settings remain portable. Runtime installation is machine-specific.

## Credentials on a new machine

Store references, not secrets:

```text
credential_ref = pexels/default
```

If the credential is absent in Machine B's Keychain:

```text
Pexels credential required

[ Configure ]
```

Completed project state remains usable.

A later optional feature may export/import an encrypted credential bundle, but plaintext secret synchronization is not part of MVP.

## DaVinci export on another machine

Do not assume a copied FCPXML containing old absolute file URLs remains valid.

The portable Timeline IR references artifact IDs.

On Machine B:

```text
Timeline IR
  -> resolve current Data Root
  -> generate fresh FCPXML
  -> import into DaVinci
```

## Move Data Folder

Provide a safe application action:

```text
Move Data Folder
```

Flow:

1. pause writes
2. create clean snapshot
3. copy/move workspace
4. verify workspace manifest + important hashes
5. update machine-local Data Root binding
6. reopen workspace
7. delete old location only after verification/user confirmation

## Failure scenarios

### Folder copied while OmniCreator is running

Destination may not contain a clean handoff.

Open in recovery/read-only mode and prefer the latest verified clean snapshot.

### Google Drive has not finished syncing

If handoff snapshot/hash is incomplete, show:

```text
Workspace synchronization is incomplete.
Waiting for required files...
```

Do not mutate or regenerate missing expensive artifacts immediately.

### SQLite integrity failure

Restore/rebuild from latest verified clean snapshot, then reconcile artifacts/jobs.

### Missing media file

Use provenance/artifact metadata to:

- wait for cloud hydration
- re-download stock source
- restore from another copy
- regenerate only when necessary

### Missing plugin

Keep projects readable. Block only steps that require that plugin.

## Acceptance tests

Portable workspace v1 is complete when all of these pass:

1. Create projects on Mac A.
2. Generate several TTS/image artifacts.
3. Leave projects in mixed states: DONE, GPU_READY, FAILED, READY_FOR_EDIT.
4. Close OmniCreator cleanly.
5. Copy the entire Data Root to a different path.
6. Bind a fresh OmniCreator installation to that folder.
7. Verify project board/statuses/takes/assets are restored.
8. Retry one failed step without regenerating successful steps.
9. Generate a new DaVinci export using the new physical path.
10. Repeat using a Google Drive synchronized Data Root.
11. Simulate an unclean copy and recover from the latest clean snapshot.
12. Verify API secrets are not present in plaintext inside the Data Root.
