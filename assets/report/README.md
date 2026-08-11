# Report styles

Tailwind v4 + shadcn/ui design tokens for the omen HTML health report.

The report is a single self-contained HTML file. This stylesheet is compiled
offline and committed as `src/report/report.css`, which is embedded into the
binary via `include_str!`. The Rust build never runs Tailwind, so `cargo build`
and `cargo install` need no JavaScript toolchain.

## Regenerate after editing `input.css`

```bash
cd assets/report
bun install   # first time only
bun run build # writes ../../src/report/report.css
```

`input.css` holds the shadcn token system (`:root` light, `[data-theme="dark"]`
dark) and a `@layer components` set of report component classes (`.card`,
`.badge-*`, `.report-table`, `.hero`, `.metric-tile`, `.nav-link`, ...).
