# Report styles

Tailwind v4 and shadcn/ui design tokens for the omen HTML health report.

The report is one self-contained HTML file. The stylesheet is compiled offline and committed as `src/report/report.css`. The `include_str!` macro embeds it in the binary. The Rust build never runs Tailwind. Thus `cargo build` and `cargo install` do not need a JavaScript toolchain.

## Regenerate the stylesheet after you edit `input.css`

```bash
cd assets/report
bun install   # first time only
bun run build # writes ../../src/report/report.css
```

`input.css` holds the shadcn token system (`:root` for light, `[data-theme="dark"]` for dark). It also holds a `@layer components` set of report component classes (`.card`, `.badge-*`, `.report-table`, `.hero`, `.metric-tile`, `.nav-link`, ...).
