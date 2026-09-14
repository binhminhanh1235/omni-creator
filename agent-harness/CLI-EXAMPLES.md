# OmniCreator CLI examples for agents

Use `--json` for deterministic machine-readable output. Replace placeholders with IDs returned by previous commands.

```text
omnicreator --data-root <path> --json workspace status
omnicreator --data-root <path> --json project list
omnicreator --data-root <path> --json project show --project <project-id>
omnicreator --data-root <path> --json workflow status --project <project-id>
omnicreator --data-root <path> --json creator state --project <project-id>
omnicreator --data-root <path> --json review list --project <project-id>
```

Start/resume with no input only for a project that already has the necessary canonical input:

```text
omnicreator --data-root <path> --json creator resume --project <project-id>
```

Start with a topic when an automatic LLM provider is configured:

```text
omnicreator --data-root <path> --llm-provider-config <provider.json> --json creator start --project <project-id> --topic "<topic>"
```

Turn one step AUTO OFF, then inspect before supplying a manual result:

```text
omnicreator --data-root <path> --json workflow set-auto --project <project-id> --step content.prepare --off
omnicreator --data-root <path> --json workflow status --project <project-id>
```

Complex manual/external operations should use the exact typed request DTO through `--stdin` or `--input-file` as documented in `docs/18-cli-control.md`. Do not translate them into direct Data Root writes.

After each mutation run the relevant `project show`, `workflow status`, `creator state`, `review list`, or stage-specific status command before deciding what to do next.
