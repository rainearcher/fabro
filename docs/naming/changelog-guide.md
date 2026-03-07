# Changelog Writing Guide

How to write Arc changelog entries, inspired by [Linear's changelog](https://linear.app/changelog).

## Post structure

Each changelog entry follows this structure:

1. **Frontmatter** — title and date
2. **Breaking changes** — `<Warning>` blocks at the top (if any)
3. **Hero features** — 1-3 features with dedicated `##` sections
4. **Progressive disclosure footer** — categorized one-liners inside `<Accordion>` groups

## Hero features

Each major feature gets its own `##` section. Write 2-4 sentences max.

**Lead with the problem or context, then present the solution:**

> Long-running agent sessions used to fail when they hit the context window limit. Now, sessions automatically summarize earlier conversation history when approaching the limit, so long workflows keep running without manual intervention.

**Use second person, present tense:** "You can now...", "Workflows now support..."

**Include a code example** when the feature has a CLI command, config snippet, or DOT syntax.

## Progressive disclosure footer

After the hero features, add a horizontal rule (`---`) followed by categorized one-liners inside `<Accordion>` components. This keeps the post scannable — readers who want details can expand the sections.

### Categories

Use only the categories that apply to a given post. Order them as listed:

| Category | What goes here | Verb tense |
|---|---|---|
| **API** | New/changed endpoints, query params, response shapes | Present: "New `GET /usage` endpoint returns..." |
| **Fixes** | Bug fixes | Past: "Fixed UTF-8 slicing panic when..." |
| **Improvements** | Small enhancements, UI polish, perf wins | Past: "Added Gemini 3.1 Flash Lite to model catalog" |

### One-liner style

- Start with a verb (Added, Fixed, Improved, New, Updated)
- One line per item, no sub-bullets
- Use backticks for code: endpoints, flags, config keys, model names
- No periods at the end

## Template

```mdx
---
title: "Hero feature name"
date: "YYYY-MM-DD"
---

<Warning>
**Breaking change summary.** Brief explanation.

To migrate: Steps to update.
</Warning>

## Hero feature name

Problem or context sentence. Solution sentence with what's new. Optional sentence on how it works or why it matters.

```bash
arc example-command --flag
```

## Second feature

Problem/context. Solution.

---

<Accordion title="API">
- New `GET /endpoint` returns aggregate data broken down by model
- `POST /runs` now accepts `concurrency` parameter
</Accordion>

<Accordion title="Fixes">
- Fixed HTTP 529 responses being misclassified as non-retryable
- Fixed progress display panic when tool calls contain long whitespace
</Accordion>

<Accordion title="Improvements">
- Gemini 3.1 Flash Lite added to model catalog
- MODEL column in CLI tables widened from 24 to 30 characters
- All API error responses now use consistent JSON structure with error codes
</Accordion>
```

## Title conventions

- Name the post after the hero feature: "Time in status", "Form templates"
- For multi-feature posts, list 2-3 top features: "mTLS auth, setup wizard, and arc doctor"
- No version numbers or dates in the title (the frontmatter has the date)

## What NOT to do

- Don't write more than 4 sentences per feature section
- Don't put fixes/improvements inline — they go in the accordion footer
- Don't use marketing superlatives ("revolutionary", "game-changing")
- Don't explain things the reader already knows — assume technical literacy
