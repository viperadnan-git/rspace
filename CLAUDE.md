# CLAUDE.md

## Commits
- Use Conventional Commits (`type(scope): subject`).
- Subject line well under 50 chars; imperative mood, no trailing period.
- Body optional — add only when the *why* is non-obvious.

## Comments
- No narrative or restating-the-code comments.
- Comment only where intent is non-obvious; cite a reference (link/issue) when relevant.

## Engineering
- Simple yet robust ("German engineering"): no over-engineering, no abstraction
  without a present caller, no speculative generality. Solve the real problem
  fully, then stop.

## Fixes
- No patch/band-aid fixes.
- If a proper fix needs a large refactor or design change, stop and propose a brief plan first, then apply the root fix on approval.

## Naming & Structure
- Follow Rust idiomatic naming (RFC 430):
  - `snake_case` — variables, functions, modules, files.
  - `CamelCase` — types, traits, enum variants.
  - `SCREAMING_SNAKE_CASE` — constants, statics.
- Directory and file names: `snake_case`.
- Lay out the repo per standard Cargo conventions.
