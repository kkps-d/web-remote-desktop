# Agent instructions

These instructions apply throughout this repository. Follow [DESIGN_GUIDELINES.md](./docs/DESIGN_GUIDELINES.md) for UI work.

## Keep it simple

- Follow YAGNI: implement what the current task needs, without speculative features or infrastructure.
- Prefer straightforward code over clever patterns. Introduce abstractions only when they solve a concrete problem or remove meaningful duplication.
- Keep changes focused and consistent with existing code.

## For humans

- Don't talk with unnecessary prose, keep it concise. This applies to documentation as well.

## Write readable code

- Use descriptive names, proper whitespace, and consistent formatting.
- Let the code explain itself through clear structure and explicit intent.
- Add short comments for complex logic, non-obvious decisions, or constraints. Explain why; avoid narrating obvious code.
- Commit using conventional commit messages style

## Agent code

- The agent is the Rust crate in `agent-rs/`. Make all agent changes there.
- The C# agent in `agent/` and `agent.Tests/` is archived for reference. Don't update it, even to keep it in step with `agent-rs/`.

## Build React interfaces

- Use shadcn/ui as the component foundation.
- Reuse existing project components and shadcn/ui components before creating new ones.
- Break large React components into logical components or page sections when doing so makes them easier to understand.
- Keep related logic together. Avoid splitting simple components into unnecessary files or creating wrappers with no clear benefit.

## Verify changes

- Use the project's existing formatting, linting, type checks, and tests when relevant to the change.
- For UI changes, check responsive behavior, accessibility, and applicable interaction states.

## Live tests

- You may do live tests if needed without prompting the user
