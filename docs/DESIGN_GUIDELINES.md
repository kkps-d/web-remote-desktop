# Design guidelines

## Simple, purposeful interfaces

- Design for the current user task. Avoid extra controls, decoration, or settings without a clear purpose.
- Make the primary action easy to find and use clear, concise labels.
- Use whitespace and visual hierarchy to make content easy to scan.

## Components and consistency

- Use shadcn/ui components and patterns wherever they fit.
- Check for an existing project component before adding a new one. Extend or compose existing components when practical.
- Create custom components only when existing components do not meet the need.
- Use the project's theme tokens for colors, spacing, typography, borders, and radii. Keep styling consistent across screens.
- Keep variants limited to actual use cases; avoid building a speculative design system.

## React structure

- Organize large pages into logical sections, such as a header, filters, results, and details.
- Extract a component when it has a clear responsibility, is reused, or makes the parent easier to read.
- Keep small, readable sections inline when extraction would add indirection without value.

## Usability and accessibility

- Make layouts work across narrow and wide screens without losing essential content or actions.
- Use semantic elements, visible focus states, accessible labels, and keyboard-friendly interactions.
- Maintain readable contrast and do not rely on color alone to communicate meaning.
- Provide clear loading, empty, error, and success states where applicable.
- Keep interactions predictable and motion subtle and purposeful.
