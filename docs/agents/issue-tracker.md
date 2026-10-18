# Issue tracker: Local Markdown

Issues and specs live as Markdown files in `.scratch/`.

## Conventions

- One feature per directory: `.scratch/<feature-slug>/`.
- Spec: `.scratch/<feature-slug>/spec.md`.
- One file per ticket: `.scratch/<feature-slug>/issues/<NN>-<slug>.md`,
  numbered from 01.
- Record triage state in a `Status:` line near the top.
  Use the values in `triage-labels.md`.
- Append discussion under `## Comments`.

## Publishing and fetching

When publishing, create the corresponding file and directories.
When fetching, read the ticket at the supplied path or resolve its
number within the referenced feature.

## Wayfinder operations

- Map: `.scratch/<effort>/map.md`, containing Notes,
  Decisions-so-far, and Fog.
- Child tickets: `.scratch/<effort>/issues/NN-<slug>.md`.
- Record `Type:` as research, prototype, grilling, or task.
- Record dependencies as `Blocked by: NN, NN`.
- A ticket is unblocked when every dependency is resolved.
- Frontier: select the lowest-numbered open, unblocked,
  unclaimed ticket.
- Before working, save `Status: claimed`.
- To resolve, append `## Answer`, set `Status: resolved`,
  and add a summary with a ticket link to Decisions-so-far.

Wayfinder's claimed/resolved states track execution separately
from the triage vocabulary.
