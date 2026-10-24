# External CLI review fixes

2026-10-24. Integration base: `37415ff`; fix branch:
`codex/external-cli-review-fixes`.

## Findings and resulting behavior

- **P2: exact text from Windows PowerShell 5.** `notify --stdin` reads exact UTF-8
  text through EOF. The installed English skill invokes its bundled
  `scripts/notify.ps1`, which writes UTF-8 bytes directly to redirected stdin,
  without shell argument conversion or an added newline. Both the installer and
  the public installation action include the helper. Help and skill describe the
  stdin mode and uncertainty rules.
- **P2: a saved command reported as failed after unrelated scheduler/adapter
  work.** External dispatch reconciles committed snapshots separately from
  scheduler ticks. Saved results retain success and saved facts, with optional
  `warnings` for desktop display/sound failures. The window is notified after
  every successful command independently of adapter success. Notification
  creation errors explain that a card may already exist.
- **P2: acknowledgement before native initial placement/show completion.** The
  native region/placement tasks remain outside GPUI window updates to avoid
  synchronous resize callback reentry. Their shared completion result is awaited
  before replying. Concurrent requests observe the same initialization result;
  native failure is propagated to the notification response as uncertainty.
- **P3: repeated desktop test lifecycle.** CLI integration tests share
  `tests/support/desktop.rs`: isolated copied binaries, data/discovery, explicit
  stop, and process-handle termination/wait before cleanup or restart.
- **Clippy:** replaced three cloned single-element slices and the existing
  modulo check with their current Rust equivalents.

## Evidence

Red-to-green regressions covered a missing installed helper, absent public
adapter completion boundary, and notification rejection while a due one-time
reminder could not save. The resulting checks exercise:

- A real Windows PowerShell 5 process invokes the publicly installed helper and
  the real CLI against isolated IPC. The application's observed snapshot keeps
  Cyrillic, embedded quotes, backslashes, a newline and the final backslash
  exactly, with no appended newline.
- A real desktop receives a notification while a separate due reminder cannot
  save because its temporary storage path is blocked.
- At the public application/desktop-result boundary, a persisted create remains
  successful with warnings after an injected display failure; restart and shared
  undo retain the committed record/history. A notification adapter failure
  reports uncertainty for an already created card.
- Native window enumeration immediately after notification reply confirms a
  visible toast viewport and no activated main window, without a later list
  request or visibility retry loop masking acknowledgement timing.

Final checks on the fix branch:

- `cargo test --all-targets --locked`: **135 passed, 0 failed, 3 intentionally
  ignored** (private process fixtures and existing manual/native test).
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --all-targets --locked -- -D warnings`: passed.
- Skill format validator (`python -X utf8 .../quick_validate.py
  assets/agent-skill/napomni`): passed.
- `git diff --check`: passed.

The root agent relayed confirmations from both Standards and Spec reviewers:
all reported findings were addressed and no new concrete regressions were
identified. The native initial region/placement completion path was specifically
rechecked. Public adapter boundary tests were added afterward without changing
the reviewed result semantics.

These automated checks do not establish physical audibility or installed
Settings field/button/success/error interaction. Native computer-use capture was
unavailable to the root agent, so those manual checks remain pending. The root
agent will separately evaluate the installed English skill in an isolated
environment after merging these fixes. The installed user instance was not
modified or terminated by these tests.
