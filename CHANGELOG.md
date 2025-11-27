# Changelog

## [1.5.0](https://github.com/panbanda/omen/compare/v1.4.0...v1.5.0) (2025-11-27)


### Features

* **analyzer:** add PMAT-compatible duplicate detection ([5003425](https://github.com/panbanda/omen/commit/500342590f5a6cf9ebf88d0006e06087167ed27b))
* **defect:** PMAT-compatible defect prediction algorithm ([f11337e](https://github.com/panbanda/omen/commit/f11337e8ec261d01a1c7ec1cfb4934cda09f9d73))
* **duplicates:** align duplicate detection with pmat implementation ([0ec0b0f](https://github.com/panbanda/omen/commit/0ec0b0fbb02c21876934886131a178117196e34c))
* **tdg:** port PMAT scoring system with 0-100 scale ([8ce8c02](https://github.com/panbanda/omen/commit/8ce8c02776207a1b1dd8dea5795c64c0b9ed6533))
* **tdg:** port PMAT scoring system with 0-100 scale ([b4e2eb5](https://github.com/panbanda/omen/commit/b4e2eb5d4b539cc4938776a79354e8eeb4392739))

## [1.4.0](https://github.com/panbanda/omen/compare/v1.3.0...v1.4.0) (2025-11-27)


### Features

* **analyzer:** add Tarjan SCC cycle detection and enhanced Mermaid output ([6ee8f96](https://github.com/panbanda/omen/commit/6ee8f96ab766ff38b230ba95fe1e30b8b1d5eb5f))
* **graph:** integrate gonum for graph algorithms ([1ed85d7](https://github.com/panbanda/omen/commit/1ed85d79f79dd016aaa726dbd60f0f87fc8d48a0))

## [1.3.0](https://github.com/panbanda/omen/compare/v1.2.0...v1.3.0) (2025-11-27)


### Features

* **deadcode:** port PMAT architecture with Roaring bitmaps and VTable resolution ([c098ad8](https://github.com/panbanda/omen/commit/c098ad830548c43e0bcf87ebd7da53386086e919))
* **deadcode:** port PMAT architecture with Roaring bitmaps and VTable resolution ([ab49bfd](https://github.com/panbanda/omen/commit/ab49bfd5cadc36ecd94c38a02f62652c922f64bb))

## [1.2.0](https://github.com/panbanda/omen/compare/v1.1.0...v1.2.0) (2025-11-27)


### Features

* **complexity:** add Halstead software science metrics ([797399c](https://github.com/panbanda/omen/commit/797399c21c4c723a818b5780fd8f72c5b2037f92))
* **complexity:** add pmat-compatible models and tests ([d980808](https://github.com/panbanda/omen/commit/d98080863e7b13881692680cd31c73aae27dd60f))
* **satd:** add PMAT-compatible strict mode, test block tracking, and AST context ([68452db](https://github.com/panbanda/omen/commit/68452db332d8009b77cf0a80f977aace461cea9c))
* **satd:** add severity adjustment, context hash, and file exclusion ([9f4032e](https://github.com/panbanda/omen/commit/9f4032e1a7fd2b83768e5fec1bf7f5912845427a))

## [1.1.0](https://github.com/panbanda/omen/compare/v1.0.8...v1.1.0) (2025-11-26)


### Features

* **churn:** add hotspot and stable file detection ([863a6ea](https://github.com/panbanda/omen/commit/863a6ea5950e2399ca69ec527ec2b31008434b68))
* **churn:** add hotspot and stable file detection ([efe13c4](https://github.com/panbanda/omen/commit/efe13c41ec8f754931a57114d214625afd379dc5))

## [1.0.8](https://github.com/panbanda/omen/compare/v1.0.7...v1.0.8) (2025-11-26)


### Bug Fixes

* **ci:** use macos-15-intel instead of macos-15-large ([a65dd86](https://github.com/panbanda/omen/commit/a65dd86d2bad265e2f0e5a21b46b1cf94068f75e))

## [1.0.7](https://github.com/panbanda/omen/compare/v1.0.6...v1.0.7) (2025-11-26)


### Bug Fixes

* **ci:** replace goreleaser homebrew with manual formula generation ([b911b13](https://github.com/panbanda/omen/commit/b911b13b57a472e67a5cb0b3f8686ce5b5f30509))

## [1.0.6](https://github.com/panbanda/omen/compare/v1.0.5...v1.0.6) (2025-11-26)


### Bug Fixes

* trigger release ([4b99bd1](https://github.com/panbanda/omen/commit/4b99bd1f05a556b009e286a55812c12a2909e98b))

## [1.0.5](https://github.com/panbanda/omen/compare/v1.0.4...v1.0.5) (2025-11-26)


### Bug Fixes

* **ci:** use per-target checksum filenames ([35428d0](https://github.com/panbanda/omen/commit/35428d0eb2f214534983db2275062edb5b2ed08a))

## [1.0.4](https://github.com/panbanda/omen/compare/v1.0.3...v1.0.4) (2025-11-26)


### Bug Fixes

* **ci:** use per-target builds with skip conditions ([29c617f](https://github.com/panbanda/omen/commit/29c617f40fc5a78b03980bde621f30c1d5fe3500))

## [1.0.3](https://github.com/panbanda/omen/compare/v1.0.2...v1.0.3) (2025-11-26)


### Bug Fixes

* **ci:** use --single-target with per-arch matrix jobs ([0394e6c](https://github.com/panbanda/omen/commit/0394e6c60c7519a5869ec3cca1826b0b2541ac4c))

## [1.0.2](https://github.com/panbanda/omen/compare/v1.0.1...v1.0.2) (2025-11-26)


### Bug Fixes

* **ci:** use builds.skip with Runtime.Goos template ([44538de](https://github.com/panbanda/omen/commit/44538de5e712efcf493ad43f2367b6c44e5f0c0a))
* **ci:** use Runtime.Goos for build filtering ([8c52192](https://github.com/panbanda/omen/commit/8c521924beef9f868b2d0c77d18b6b18260df8a6))

## [1.0.1](https://github.com/panbanda/omen/compare/v1.0.0...v1.0.1) (2025-11-26)


### Bug Fixes

* **ci:** use matrix builds for CGO cross-compilation ([fae3508](https://github.com/panbanda/omen/commit/fae350885da274bc6de57ef68542e37377cc081a))
* **ci:** use matrix builds for CGO cross-compilation ([909af82](https://github.com/panbanda/omen/commit/909af82b3a763d580d6fac757e81f8a227d0ceb7))

## 1.0.0 (2025-11-26)


### Features

* **ci:** add release-please for automated releases ([40754bd](https://github.com/panbanda/omen/commit/40754bde0af216ea4ea018fc646868562246efe1))
* initial implementation of omen multi-language code analyzer ([832d2bc](https://github.com/panbanda/omen/commit/832d2bcbde956b59e108171bb78a712715cc320d))


### Bug Fixes

* **ci:** add golangci-lint config and fix test data races ([cd1c3b7](https://github.com/panbanda/omen/commit/cd1c3b7d15adbae085ea93e7547f222495498818))
* **ci:** checkout release tag for GoReleaser ([4dadbe3](https://github.com/panbanda/omen/commit/4dadbe3981671dbd0a40f2f9b201538a271e01df))
* **ci:** resolve workflow failures ([e7b384a](https://github.com/panbanda/omen/commit/e7b384ae563744754eb43824cd8e71f4950fe705))
* **ci:** simplify to stable Go on Linux and macOS only ([5cb11f5](https://github.com/panbanda/omen/commit/5cb11f53c9e73286dd4b7f7211b053ec5c659697))
* **ci:** use Go 1.25 across workflow and go.mod ([b3ee14e](https://github.com/panbanda/omen/commit/b3ee14e624d05f0681e5467dcf100613e4ada26a))
* **readme:** correct badge URLs to panbanda/omen ([533626f](https://github.com/panbanda/omen/commit/533626f04e3fa8128c456084956835feba8fc72f))
* **readme:** correct badge URLs to panbanda/omen ([d8ea557](https://github.com/panbanda/omen/commit/d8ea5570afcce378906f72c58563ac501cd7aca3))
* **release:** configure GoReleaser to append to existing release ([5ff11cf](https://github.com/panbanda/omen/commit/5ff11cfc5a81ff6edcf0c75a1a3887e8436a3385))
