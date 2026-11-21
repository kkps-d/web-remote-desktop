# Design guidelines

How to write code in this repository. [Shared](#shared) rules apply everywhere; [Frontend](#frontend) covers `client/` and [Backend](#backend) covers `agent/`.

## Shared

### Simplicity

- Implement what the current task needs (YAGNI), without speculative features or infrastructure.
- Prefer straightforward code over clever patterns.
- Add an abstraction only when it solves a concrete problem or removes meaningful duplication.
- Add a dependency only when it removes real work.
- Keep changes focused and consistent with the surrounding code.

### Readability

- Use descriptive names and let the structure show intent.
- Format with the project's formatter, never by hand: `cargo fmt` for the agent, `npm run format` (oxfmt) for the client.
- Comment only when needed, to explain why: constraints, workarounds, surprising OS or GStreamer behaviour. Don't narrate what the code does.
- Keep docs and comments concise.

### Client and agent

- Prioritize low latency on the input and media paths.
- Keep wire names, units and meaning aligned between client and agent. Internal names can differ.
- Keep the wire format compatible where practical; ignore unknown input message types instead of failing.
- Update [Architecture](ARCHITECTURE.md) and [Known issues](KNOWN_ISSUES.md) when the behaviour they describe changes.

## Frontend

### Interface

- Design for the user's current task. Avoid controls, decoration or settings without a clear purpose.
- Make the primary action easy to find, with clear, concise labels.
- Use whitespace and visual hierarchy to make content easy to scan.
- Keep interactions predictable and motion subtle.

### Components

- Use shadcn/ui as the component foundation.
- Reuse existing project components before adding new ones; extend or compose them when practical.
- Use the project's theme tokens for colors, spacing, typography, borders and radii.
- Keep variants to actual use cases; don't build a speculative design system.

### React structure

- Split large pages into logical sections, such as a header, filters, results and details.
- Extract a component when it has a clear responsibility, is reused, or makes the parent easier to read.
- Keep related logic together. Keep small sections inline and skip wrappers when they would only add indirection.

### Accessibility and states

- Make layouts work on narrow and wide screens without losing content or actions.
- Use semantic elements, visible focus states, accessible labels and keyboard-friendly interactions.
- Keep contrast readable and don't rely on color alone.
- Show loading, empty, error and success states where they apply.
- Explain failures and disabled controls where the user meets them, with the reason.

### Streaming and input

- Keep WebRTC and high-frequency input outside React rendering, using native listeners.
- Keep the order of keys, buttons and touches when merging movement.
- Treat browser storage as best-effort; handle missing, old or malformed values.
- Fit restored stream settings to the connected agent's limits and capabilities.

## Backend

### Structure

- Keep OS-specific code in `src/platform/<os>/`. Each OS provides the same items, re-exported by name in `platform/mod.rs`, and shared code uses only those.
- A small, commented `cfg` outside `platform` is fine when moving it would add indirection, like the socket option in `http.rs`.
- Keep module dependencies one-way; avoid import cycles.
- Document each `unsafe` block's invariant with a `// SAFETY:` comment.

### Pipelines

- Let GStreamer process media; Rust builds and controls pipelines.
- Keep Rust-side instrumentation on the media path lightweight.
- Run blocking pipeline work (start, change, stop) off Tokio's async workers.
- Fit the starting resolution to the agent's and encoder's limits.

### Sessions and errors

- Reject invalid live settings with HTTP 400 before changing the pipeline.
- Keep streaming when an optional capability is unavailable, and report why.
- Make teardown safe to repeat across disconnects, replacement and errors: stop pipelines, release held input, close capture sessions.

### Logging

- Use `tracing`: `info` for session lifecycle, `debug` for routine detail.
- Keep per-frame and per-input traffic out of `info`.
- Log capability failures that only the agent can see, like missing uinput access.

### Config

- Work with no config file.
- Let command-line options override the file.
- Keep `agent.example.toml` current with every option.

### Tests

- Unit-test pure logic, such as settings validation, SDP handling and key mappings.
- Use live tests for changes that depend on capture, pipelines or OS input injection.
