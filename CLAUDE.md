# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

This is the `gh-pages` branch containing the Docusaurus 3.9.2 documentation site for Omen. The main Omen source code (Rust CLI) lives on the `main` branch.

## Commands

```bash
npm run start    # Dev server with hot reload (localhost:3000)
npm run build    # Production build to build/ directory
npm run serve    # Serve the production build locally
npm run typecheck # TypeScript type checking
```

The build will fail on broken links (`onBrokenLinks: 'throw'` in docusaurus.config.ts). Fix all broken links before pushing.

## Deployment

Pushes to `gh-pages` trigger `.github/workflows/deploy.yml` which builds and deploys to GitHub Pages via `actions/deploy-pages`. The site is served at `https://panbanda.github.io/omen/`.

GitHub Pages source must be set to **GitHub Actions** (not "Deploy from a branch") in repo settings.

## Structure

- `docs/` -- Markdown documentation pages (MDX)
- `docs/analyzers/` -- One page per analyzer (19 total)
- `docs/integrations/` -- MCP server, CI/CD, Claude Code plugin
- `sidebars.ts` -- Manual sidebar definition (not auto-generated)
- `docusaurus.config.ts` -- Site config; `routeBasePath: '/'` makes docs the landing page (no splash page)
- `static/reports/` -- Standalone HTML report files linked from the examples page
- `src/css/custom.css` -- Theme colors (orange palette)

## MDX Gotchas

Docusaurus uses MDX, not plain Markdown. Angle brackets like `<P80` in table cells will be parsed as JSX and break the build. Use `&lt;` instead. Content inside fenced code blocks is safe.

Curly braces `{}` outside code blocks are treated as JSX expressions.

## Report Links

The examples page (`docs/examples.md`) links to static HTML files in `static/reports/` using the `pathname://` protocol prefix, which tells Docusaurus to skip link validation for those paths.
