# Changelog

## [4.0.2](https://github.com/panbanda/omen/compare/omen-skills-v4.0.1...omen-skills-v4.0.2) (2026-01-31)


### Bug Fixes

* **graph:** detect Rust imports and module dependencies ([6706dc5](https://github.com/panbanda/omen/commit/6706dc558094aebf45e963bbffe7224e5b6539be))
* **graph:** detect Rust imports and module dependencies ([6edd8b4](https://github.com/panbanda/omen/commit/6edd8b42a939d10244ffe64576e138b459c7d6fd))

## [4.0.1](https://github.com/panbanda/omen/compare/omen-skills-v4.0.0...omen-skills-v4.0.1) (2026-01-30)


### Bug Fixes

* default --since to full history, fix plugin cohesion ([7fb09cb](https://github.com/panbanda/omen/commit/7fb09cbe54333c9c13c6ee47b6f6314258e35beb))
* default --since to full history, fix plugin cohesion issues ([406790b](https://github.com/panbanda/omen/commit/406790b15eb4536bec95109ba0cf29ef7a7a5291))

## [4.0.0](https://github.com/panbanda/omen/compare/omen-skills-v3.0.0...omen-skills-v4.0.0) (2026-01-28)


### ⚠ BREAKING CHANGES

* This release removes the Go implementation entirely and replaces it with a pure Rust implementation.
* **plugins:** plugin/ renamed to plugins/development/

### Features

* complete Rust rewrite of Omen CLI ([#204](https://github.com/panbanda/omen/issues/204)) ([7d26fe2](https://github.com/panbanda/omen/commit/7d26fe2a2ec198fa4306165e3fdc9ec31fa866b4))
* efficiency optimizations, AST abstraction, and LLM prompt improvements ([#202](https://github.com/panbanda/omen/issues/202)) ([c8cabb4](https://github.com/panbanda/omen/commit/c8cabb4ed3e1d7a0433b46037610ff0322487a9a))
* **flags:** implement custom_providers support with tree-sitter queries ([#260](https://github.com/panbanda/omen/issues/260)) ([1076012](https://github.com/panbanda/omen/commit/107601243d8ed08e3f9cf0cba7cc284f04d0dcf7))
* **plugins:** restructure plugins and add research-backed analyst agents ([#179](https://github.com/panbanda/omen/issues/179)) ([696c000](https://github.com/panbanda/omen/commit/696c00094060f5d8034d3a0f4ac4b523b01a10b6))
* **skills:** trigger minor version bump ([#127](https://github.com/panbanda/omen/issues/127)) ([4a72df4](https://github.com/panbanda/omen/commit/4a72df486ab93770bfa27753e381a3f25ff48466))
* trigger 0.2.0 release ([525996b](https://github.com/panbanda/omen/commit/525996b8760591070a51682b65c00ec5e00ef084))
* trigger 0.2.0 release ([a24d9bf](https://github.com/panbanda/omen/commit/a24d9bfd0a7cd389cad4327da8b0f666c9b566b4))


### Bug Fixes

* final release trigger ([ed5cf8b](https://github.com/panbanda/omen/commit/ed5cf8baf4e9afc65cfe73a429ee98ee8290182c))
* final release trigger ([0e9eb61](https://github.com/panbanda/omen/commit/0e9eb618f00df0e6c00db5c31f4ff4549aa0b602))
* **release:** update release-please config for plugins directory ([#314](https://github.com/panbanda/omen/issues/314)) ([274e793](https://github.com/panbanda/omen/commit/274e793fba606a95ad4360ef5539da5b5c3dc5bc))
* retrigger release-please ([83a725c](https://github.com/panbanda/omen/commit/83a725ca2450bffae12f4a87ab42286cd864efb5))
* retrigger release-please ([3949a4f](https://github.com/panbanda/omen/commit/3949a4fa56838cc73ec48ef4b014555cbef97630))
* trigger release ([4b99bd1](https://github.com/panbanda/omen/commit/4b99bd1f05a556b009e286a55812c12a2909e98b))

## [3.0.0](https://github.com/panbanda/omen/compare/omen-skills-v2.2.0...omen-skills-v3.0.0) (2025-12-16)


### BREAKING CHANGES

* **plugins:** plugin/ renamed to plugins/development/

### Features

* **plugins:** restructure plugins and add research-backed analyst agents ([#179](https://github.com/panbanda/omen/issues/179)) ([696c000](https://github.com/panbanda/omen/commit/696c00094060f5d8034d3a0f4ac4b523b01a10b6))

## [2.2.0](https://github.com/panbanda/omen/compare/omen-skills-v2.1.0...omen-skills-v2.2.0) (2025-12-15)


### Features

* **report:** enhance HTML report with datatables, markdown, and generate-report command ([#176](https://github.com/panbanda/omen/issues/176)) ([76c4dd8](https://github.com/panbanda/omen/commit/76c4dd89a3c71dba782afb1f7182dc1da3aeac78))

## [2.1.0](https://github.com/panbanda/omen/compare/omen-skills-v2.0.0...omen-skills-v2.1.0) (2025-12-15)


### Features

* **report:** add HTML health report generation ([#172](https://github.com/panbanda/omen/issues/172)) ([52b1a88](https://github.com/panbanda/omen/commit/52b1a8856c8bf1003f6c0d49201b345667904b68))
