# Changelog

## [1.4.4](https://github.com/panbanda/omen/compare/omen-v1.4.3...omen-v1.4.4) (2025-12-02)


### Bug Fixes

* **mcp:** use identifier field and shorten description for registry ([0f7e1db](https://github.com/panbanda/omen/commit/0f7e1db6b45277f427a7630cd103391d5673321a))

## [1.4.3](https://github.com/panbanda/omen/compare/omen-v1.4.2...omen-v1.4.3) (2025-12-02)


### Bug Fixes

* **mcp:** migrate manifest to schema version 2025-10-17 ([52e77dd](https://github.com/panbanda/omen/commit/52e77ddd5d04b7f5f252b887f4100b5038d83f8b))

## [1.4.2](https://github.com/panbanda/omen/compare/omen-v1.4.1...omen-v1.4.2) (2025-12-02)


### Bug Fixes

* **release:** strip omen-v prefix from archive names ([015b2be](https://github.com/panbanda/omen/commit/015b2be24d522026b94e5a38ab3652172ff17383))

## [1.4.1](https://github.com/panbanda/omen/compare/omen-v1.4.0...omen-v1.4.1) (2025-12-02)


### Bug Fixes

* **ci:** add --repo flag to gh release download in mcp-registry job ([ffabb4a](https://github.com/panbanda/omen/commit/ffabb4a7ee313a1cfa64399fdce3a8e68637cf49))

## [1.4.0](https://github.com/panbanda/omen/compare/omen-v1.3.2...omen-v1.4.0) (2025-12-02)


### Features

* **mcp:** add --manifest flag to generate server.json dynamically ([4593123](https://github.com/panbanda/omen/commit/4593123a18c2077e05c37720b4bbb6be108a5e73))


### Bug Fixes

* **ci:** use mcp-publisher CLI instead of GoReleaser for MCP registry ([836f293](https://github.com/panbanda/omen/commit/836f2935ea63c850ab02ad0afbf4e8348f41fd22))
* **mcp:** change --manifest flag to manifest subcommand ([31efb9c](https://github.com/panbanda/omen/commit/31efb9c3b5d088a7c160e11ad0a49db936226b1e))

## [1.3.2](https://github.com/panbanda/omen/compare/omen-v1.3.1...omen-v1.3.2) (2025-12-02)


### Bug Fixes

* **ci:** use trimprefix for Docker image version tags ([9ccfcac](https://github.com/panbanda/omen/commit/9ccfcac051ff8e1d869b1847a8c16b77f3aee2a9))

## [1.3.1](https://github.com/panbanda/omen/compare/omen-v1.3.0...omen-v1.3.1) (2025-12-02)


### Bug Fixes

* **ci:** fix MCP and Docker release issues ([a0dcb79](https://github.com/panbanda/omen/commit/a0dcb79607fac78ef78babaf82789531ce6f77b2))

## [1.3.0](https://github.com/panbanda/omen/compare/omen-v1.2.1...omen-v1.3.0) (2025-12-02)


### Features

* **ci:** add MCP registry publishing with Docker images ([535c126](https://github.com/panbanda/omen/commit/535c1267ebf197433caa0208ea7dd56ba39a6a5c))
* **ci:** add MCP registry publishing with Docker images ([03a2bb6](https://github.com/panbanda/omen/commit/03a2bb685d898abedbb4f5008a9f998dd9d9b828))
* **cli:** add init and config commands ([6088cea](https://github.com/panbanda/omen/commit/6088cea6a0ad41b29e24508f421bdd95b289cb7d))


### Bug Fixes

* **cli:** add init and config commands, remove .omen.* config support ([9403df4](https://github.com/panbanda/omen/commit/9403df4e7c9c5b166121e0602edc3b12a9673621))

## [1.2.1](https://github.com/panbanda/omen/compare/omen-v1.2.0...omen-v1.2.1) (2025-12-02)


### Bug Fixes

* **graph:** add Ruby/Python class dependency detection for smells analyzer ([ea33cad](https://github.com/panbanda/omen/commit/ea33cad008ac621bfbb8a117005c47f1e06df8dd))
* **graph:** add Ruby/Python class dependency detection for smells analyzer ([06f5499](https://github.com/panbanda/omen/commit/06f5499aafed08e6aa528c1a7741afb505504a60))

## [1.2.0](https://github.com/panbanda/omen/compare/omen-v1.1.0...omen-v1.2.0) (2025-12-01)


### Features

* **cli:** expand full analysis with 6 additional analyzers ([11c16bb](https://github.com/panbanda/omen/commit/11c16bbf89a03b45a8eaf6b75679ae098c98e5bf))
* **cli:** expand full analysis with 6 additional analyzers ([31666df](https://github.com/panbanda/omen/commit/31666df8c77f3184fbfcebb0a0e25a16f2881f69))

## [1.1.0](https://github.com/panbanda/omen/compare/omen-v1.0.0...omen-v1.1.0) (2025-12-01)


### Features

* **analyzer:** add feature flag detection ([3540b25](https://github.com/panbanda/omen/commit/3540b252bdce2cf9670d50f55d9c58b416b8207b))
* **analyzer:** add feature flag detection ([1aad95c](https://github.com/panbanda/omen/commit/1aad95c342f62559a0dda74ba11226e372c23e9c))
* **mcp:** add flag-audit prompt template ([e54beec](https://github.com/panbanda/omen/commit/e54beecf47740fa99b4e1f078824bc33ada4b603))

## 1.0.0 (2025-12-01)


### ⚠ BREAKING CHANGES

* **analyzer:** Halstead metrics removed from complexity analysis. Use cognitive complexity instead.
* **defect:** Risk levels changed from 4 to 3 (removed RiskCritical). Probability values now use sigmoid transformation producing different output values than previous linear calculation.

### Features

* **analyzer:** add advanced analysis capabilities ([b006d4c](https://github.com/panbanda/omen/commit/b006d4c6ae9d67a272a03956f8b5d4b34fab6244))
* **analyzer:** add advanced analysis capabilities ([78c2a62](https://github.com/panbanda/omen/commit/78c2a620d1d63c2f800cf980533cc5fadfc3fe8b))
* **analyzer:** add PMAT-compatible duplicate detection ([5003425](https://github.com/panbanda/omen/commit/500342590f5a6cf9ebf88d0006e06087167ed27b))
* **analyzer:** add PMAT-compatible duplicate detection ([af09a95](https://github.com/panbanda/omen/commit/af09a9578fdbc929d79f90d09bcd9cd93b01f5bd))
* **analyzer:** add research-backed analysis enhancements ([8795745](https://github.com/panbanda/omen/commit/879574544d2c767f491ec38eae8be21799f8ee04))
* **analyzer:** add research-backed analysis enhancements ([6dfcf29](https://github.com/panbanda/omen/commit/6dfcf294a91ab4ef2ad5a23135f341154d95adf7))
* **analyzer:** add Tarjan SCC cycle detection and enhanced Mermaid output ([6ee8f96](https://github.com/panbanda/omen/commit/6ee8f96ab766ff38b230ba95fe1e30b8b1d5eb5f))
* **analyzer:** add Tarjan SCC cycle detection and enhanced Mermaid output ([8df948e](https://github.com/panbanda/omen/commit/8df948e8eeba4fccae356a5abb32f1ee9063e36a))
* **churn:** add hotspot and stable file detection ([863a6ea](https://github.com/panbanda/omen/commit/863a6ea5950e2399ca69ec527ec2b31008434b68))
* **churn:** add hotspot and stable file detection ([efe13c4](https://github.com/panbanda/omen/commit/efe13c41ec8f754931a57114d214625afd379dc5))
* **ci:** add release-please for automated releases ([40754bd](https://github.com/panbanda/omen/commit/40754bde0af216ea4ea018fc646868562246efe1))
* **complexity:** add Halstead software science metrics ([797399c](https://github.com/panbanda/omen/commit/797399c21c4c723a818b5780fd8f72c5b2037f92))
* **complexity:** add Halstead software science metrics ([ca06165](https://github.com/panbanda/omen/commit/ca061652fba19d7ed8bc6ad8a94f9d73e5edac96))
* **complexity:** add pmat-compatible models and tests ([d980808](https://github.com/panbanda/omen/commit/d98080863e7b13881692680cd31c73aae27dd60f))
* **deadcode:** add call graph and BFS reachability analysis ([1cc52ad](https://github.com/panbanda/omen/commit/1cc52ad713f5a0b106dd2309659d7eeb0b5a0f84))
* **deadcode:** port PMAT architecture with Roaring bitmaps and VTable resolution ([c098ad8](https://github.com/panbanda/omen/commit/c098ad830548c43e0bcf87ebd7da53386086e919))
* **deadcode:** port PMAT architecture with Roaring bitmaps and VTable resolution ([ab49bfd](https://github.com/panbanda/omen/commit/ab49bfd5cadc36ecd94c38a02f62652c922f64bb))
* **defect:** implement PMAT-compatible defect prediction algorithm ([3a3530b](https://github.com/panbanda/omen/commit/3a3530bb790c2997e71d26f250c681bef80f6e43))
* **defect:** PMAT-compatible defect prediction algorithm ([f11337e](https://github.com/panbanda/omen/commit/f11337e8ec261d01a1c7ec1cfb4934cda09f9d73))
* **duplicates:** align duplicate detection with pmat implementation ([0ec0b0f](https://github.com/panbanda/omen/commit/0ec0b0fbb02c21876934886131a178117196e34c))
* **graph:** integrate gonum for graph algorithms ([1ed85d7](https://github.com/panbanda/omen/commit/1ed85d79f79dd016aaa726dbd60f0f87fc8d48a0))
* initial implementation of omen multi-language code analyzer ([832d2bc](https://github.com/panbanda/omen/commit/832d2bcbde956b59e108171bb78a712715cc320d))
* **mcp:** add analyze_changes and analyze_smells tools ([18b266a](https://github.com/panbanda/omen/commit/18b266aad050a768adcb023d1ae3001690c8d8ad))
* **mcp:** add analyze_changes and analyze_smells tools ([88da6cc](https://github.com/panbanda/omen/commit/88da6cce367189914b7e9ce4509f3ee09e004dbe))
* **mcp:** add Go template support for prompts with frontmatter defaults ([287a4d3](https://github.com/panbanda/omen/commit/287a4d3d2f6d4c999ffb12a7f8829e5a6919f2ad))
* **mcp:** add Go template support for prompts with frontmatter defaults ([6d19914](https://github.com/panbanda/omen/commit/6d1991404012b15054bb11f96d0a7f9aa3cdae8d))
* **mcp:** add MCP server for LLM tool integration ([518b378](https://github.com/panbanda/omen/commit/518b3780982949b6d5442239712089584762b62a))
* **mcp:** add MCP server for LLM tool integration ([52c2708](https://github.com/panbanda/omen/commit/52c27088a01eb4b5765e574e64491d1f2039bc80))
* **mcp:** add missing CLI parameters to tool inputs ([d6a9a3c](https://github.com/panbanda/omen/commit/d6a9a3cd29a3252b033a561a15ea9cd566b095da))
* **mcp:** add prompts and skills plugin ([0326af1](https://github.com/panbanda/omen/commit/0326af1da931c83db1fa9aa016051bc425a53d6a))
* **mcp:** add prompts and skills plugin ([bf12602](https://github.com/panbanda/omen/commit/bf12602a949db3e77eb9a51c0c55edd1241ca381))
* **satd:** add PMAT-compatible strict mode, test block tracking, and AST context ([68452db](https://github.com/panbanda/omen/commit/68452db332d8009b77cf0a80f977aace461cea9c))
* **satd:** add severity adjustment, context hash, and file exclusion ([9f4032e](https://github.com/panbanda/omen/commit/9f4032e1a7fd2b83768e5fec1bf7f5912845427a))
* **satd:** add severity adjustment, context hash, and file exclusion ([dddd1f3](https://github.com/panbanda/omen/commit/dddd1f3029d7ab5be7a003ac0e9f48ffb2e8a405))
* **tdg:** port PMAT scoring system with 0-100 scale ([8ce8c02](https://github.com/panbanda/omen/commit/8ce8c02776207a1b1dd8dea5795c64c0b9ed6533))
* **tdg:** port PMAT scoring system with 0-100 scale ([b4e2eb5](https://github.com/panbanda/omen/commit/b4e2eb5d4b539cc4938776a79354e8eeb4392739))
* **tdg:** update scale to 0-5 and add coupling/domain risk ([9d9f743](https://github.com/panbanda/omen/commit/9d9f743a860cb5ce6c060c2472ea3d3b471a9619))
* trigger 0.2.0 release ([525996b](https://github.com/panbanda/omen/commit/525996b8760591070a51682b65c00ec5e00ef084))
* trigger 0.2.0 release ([a24d9bf](https://github.com/panbanda/omen/commit/a24d9bfd0a7cd389cad4327da8b0f666c9b566b4))


### Bug Fixes

* added tip instead of bold ([c0efa24](https://github.com/panbanda/omen/commit/c0efa24ff00dcf54d45a04822addc2bac10891ac))
* address issues from comprehensive code review ([71ccb03](https://github.com/panbanda/omen/commit/71ccb034170f7a7acffd297e6e1ddfac9463c675))
* address issues from comprehensive code review ([d9ad7f7](https://github.com/panbanda/omen/commit/d9ad7f78657d5f9b9383e68056760b8e22e40e91))
* address issues from comprehensive code review ([944bb2a](https://github.com/panbanda/omen/commit/944bb2a4a27dcfe021123a895db9abde20b25a5c))
* address issues from comprehensive code review ([54c193d](https://github.com/panbanda/omen/commit/54c193df859e6088e67eb84a164cf1d6dd0e8ed0))
* address issues from comprehensive code review ([26d6878](https://github.com/panbanda/omen/commit/26d6878e0ff54046f3997b3200106f7ac0307e24))
* **analyzer:** address code review feedback ([6036114](https://github.com/panbanda/omen/commit/6036114821ae78d0822c5c5d48042f3c23455bd3))
* **churn:** align JSON output with pmat reference implementation ([7b80f50](https://github.com/panbanda/omen/commit/7b80f506f10e0af5afdb71dae1ad982121d42147))
* **churn:** align JSON output with pmat reference implementation ([0f75b8d](https://github.com/panbanda/omen/commit/0f75b8d93c7a10906ccf2d9940ba6085b8ab0b9d))
* **ci:** add concurrency control to release workflow ([345fc62](https://github.com/panbanda/omen/commit/345fc62bbc2917d38a2a51f340d76b29241e24f0))
* **ci:** add concurrency control to release workflow ([ada1beb](https://github.com/panbanda/omen/commit/ada1bebeccbe2b09aafbf15d1200c592e8c7c2fa))
* **ci:** add golangci-lint config and fix test data races ([cd1c3b7](https://github.com/panbanda/omen/commit/cd1c3b7d15adbae085ea93e7547f222495498818))
* **ci:** checkout release tag for GoReleaser ([4dadbe3](https://github.com/panbanda/omen/commit/4dadbe3981671dbd0a40f2f9b201538a271e01df))
* **ci:** correct release-please output variable for root package ([a072ec4](https://github.com/panbanda/omen/commit/a072ec46b57d64ec4bde8782b3979207228f5755))
* **ci:** correct release-please output variable for root package ([da83f29](https://github.com/panbanda/omen/commit/da83f295775d3d274453a61c517e060dfa50dbe5))
* **ci:** replace goreleaser homebrew with manual formula generation ([b911b13](https://github.com/panbanda/omen/commit/b911b13b57a472e67a5cb0b3f8686ce5b5f30509))
* **ci:** reset release-please to v1.0.0 ([07e7491](https://github.com/panbanda/omen/commit/07e74912cd044223021904e731895f349ec3e4e9))
* **ci:** reset release-please to v1.0.0 ([e716e09](https://github.com/panbanda/omen/commit/e716e09dd83322856c8e9f80123117bfd2156f9e))
* **ci:** resolve workflow failures ([e7b384a](https://github.com/panbanda/omen/commit/e7b384ae563744754eb43824cd8e71f4950fe705))
* **ci:** simplify to stable Go on Linux and macOS only ([5cb11f5](https://github.com/panbanda/omen/commit/5cb11f53c9e73286dd4b7f7211b053ec5c659697))
* **ci:** use --single-target with per-arch matrix jobs ([0394e6c](https://github.com/panbanda/omen/commit/0394e6c60c7519a5869ec3cca1826b0b2541ac4c))
* **ci:** use builds.skip with Runtime.Goos template ([44538de](https://github.com/panbanda/omen/commit/44538de5e712efcf493ad43f2367b6c44e5f0c0a))
* **ci:** use Go 1.25 across workflow and go.mod ([b3ee14e](https://github.com/panbanda/omen/commit/b3ee14e624d05f0681e5467dcf100613e4ada26a))
* **ci:** use macos-15-intel instead of macos-15-large ([a65dd86](https://github.com/panbanda/omen/commit/a65dd86d2bad265e2f0e5a21b46b1cf94068f75e))
* **ci:** use matrix builds for CGO cross-compilation ([fae3508](https://github.com/panbanda/omen/commit/fae350885da274bc6de57ef68542e37377cc081a))
* **ci:** use matrix builds for CGO cross-compilation ([909af82](https://github.com/panbanda/omen/commit/909af82b3a763d580d6fac757e81f8a227d0ceb7))
* **ci:** use per-target builds with skip conditions ([29c617f](https://github.com/panbanda/omen/commit/29c617f40fc5a78b03980bde621f30c1d5fe3500))
* **ci:** use per-target checksum filenames ([35428d0](https://github.com/panbanda/omen/commit/35428d0eb2f214534983db2275062edb5b2ed08a))
* **ci:** use Runtime.Goos for build filtering ([8c52192](https://github.com/panbanda/omen/commit/8c521924beef9f868b2d0c77d18b6b18260df8a6))
* **ci:** use simple v-prefix for CLI release tags ([41bd100](https://github.com/panbanda/omen/commit/41bd10055f96e16dae4d69ffeceaef12772219b5))
* **ci:** use simple v-prefix for CLI release tags ([485cf0d](https://github.com/panbanda/omen/commit/485cf0d798fae21b9ffc99ffdb331b26fc1f5ba1))
* **cli:** handle trailing flags after positional arguments ([e8ea822](https://github.com/panbanda/omen/commit/e8ea8227b0492da91a1c64ce6d179141b24c3069))
* **cli:** handle trailing flags after positional arguments ([366523b](https://github.com/panbanda/omen/commit/366523b0fbab84660fb4b2c3c2ac555be22de892))
* **deadcode:** restructure JSON output to match pmat format ([6ca4568](https://github.com/panbanda/omen/commit/6ca4568480bcf6daedc2e7efd22ed499a3a64858))
* **deadcode:** restructure JSON output to match pmat format ([b4c68b3](https://github.com/panbanda/omen/commit/b4c68b38d9b33d22fcf15374dabf27caea21ab38))
* **defect:** align JSON output with pmat format ([2d7d417](https://github.com/panbanda/omen/commit/2d7d417992a928f498808f5da8b5fcf90a570d71))
* **defect:** align JSON output with pmat format ([85f5ba3](https://github.com/panbanda/omen/commit/85f5ba398c08b979744e53fa0311297e8eb5504d))
* **duplicates:** align JSON output with pmat format ([68a17d1](https://github.com/panbanda/omen/commit/68a17d1526f3c90c9fc7d5147ffa631becf5adb4))
* **duplicates:** align JSON output with pmat format ([7c7dc2c](https://github.com/panbanda/omen/commit/7c7dc2c7a94286abd154f0e8ff967bcf9c388da3))
* **duplicates:** fix race condition in identifier canonicalization ([3af00f1](https://github.com/panbanda/omen/commit/3af00f1186fa6307235e80fffd2d4fbb4c7b3166))
* final release trigger ([ed5cf8b](https://github.com/panbanda/omen/commit/ed5cf8baf4e9afc65cfe73a429ee98ee8290182c))
* final release trigger ([0e9eb61](https://github.com/panbanda/omen/commit/0e9eb618f00df0e6c00db5c31f4ff4549aa0b602))
* **hotspot:** use geometric mean with CDF normalization for scoring ([594b6e9](https://github.com/panbanda/omen/commit/594b6e916f996b8901e755bc97dfe9134b3bdadd))
* **hotspot:** use geometric mean with CDF normalization for scoring ([06a53a5](https://github.com/panbanda/omen/commit/06a53a509f9c44952d2e26c1d855a40c7306a79f))
* **jit:** correct temporal ordering for state-dependent metrics ([61d6f61](https://github.com/panbanda/omen/commit/61d6f611288488c4d4d81fbcf372482137c80d93))
* **jit:** correct temporal ordering for state-dependent metrics ([f42f810](https://github.com/panbanda/omen/commit/f42f810af2400db753667ec19f19ff0591a16f1b))
* **output:** enable toon format serialization for all model types ([1fda456](https://github.com/panbanda/omen/commit/1fda456b7722653866cbeac91eae3bfa8a696cf6))
* **output:** enable toon format serialization for all model types ([d39c952](https://github.com/panbanda/omen/commit/d39c952d98c322ba523541b7d7f941b3534d0eae))
* **readme:** correct badge URLs to panbanda/omen ([533626f](https://github.com/panbanda/omen/commit/533626f04e3fa8128c456084956835feba8fc72f))
* **readme:** correct badge URLs to panbanda/omen ([d8ea557](https://github.com/panbanda/omen/commit/d8ea5570afcce378906f72c58563ac501cd7aca3))
* **release:** configure GoReleaser to append to existing release ([5ff11cf](https://github.com/panbanda/omen/commit/5ff11cfc5a81ff6edcf0c75a1a3887e8436a3385))
* **release:** reset CLI version to 0.1.0 ([796f5c4](https://github.com/panbanda/omen/commit/796f5c4356fb81cad97717bd0ddb2108ee93aabf))
* **release:** reset CLI versioning to 0.1.0 ([0b090c6](https://github.com/panbanda/omen/commit/0b090c6e080304ef3e01c5463607df739f08863c))
* reset manifest to 4.0.0 ([f1736e3](https://github.com/panbanda/omen/commit/f1736e31828d28b48f4774cf90a4d472c5e48013))
* reset manifest to 4.0.0 ([f72dd84](https://github.com/panbanda/omen/commit/f72dd84ed9bbedd8f411e11f24f207ee5db6382b))
* resolve security, performance, and correctness issues from code review ([cf1ff03](https://github.com/panbanda/omen/commit/cf1ff0366845b60f7906eb0ae3948d15a47e5330))
* retrigger release-please ([83a725c](https://github.com/panbanda/omen/commit/83a725ca2450bffae12f4a87ab42286cd864efb5))
* retrigger release-please ([3949a4f](https://github.com/panbanda/omen/commit/3949a4fa56838cc73ec48ef4b014555cbef97630))
* **satd:** add files_with_satd to summary for pmat compatibility ([4321269](https://github.com/panbanda/omen/commit/432126944b35c552c55988136531fb125c623973))
* **satd:** add files_with_satd to summary for pmat compatibility ([4f2cd4f](https://github.com/panbanda/omen/commit/4f2cd4fb69f8206a115f38670c6f90b56639f26a))
* **tdg:** align JSON output with pmat reference implementation ([d640a07](https://github.com/panbanda/omen/commit/d640a0724ca8eece3cada537543a0d1d7cc1aa43))
* **tdg:** align JSON output with pmat reference implementation ([ae4ad5d](https://github.com/panbanda/omen/commit/ae4ad5d9971b212a4dd6fe539adba38193dce25c))
* trigger release ([4b99bd1](https://github.com/panbanda/omen/commit/4b99bd1f05a556b009e286a55812c12a2909e98b))


### Performance Improvements

* **analyzer:** move map allocations to package level ([f58b2d5](https://github.com/panbanda/omen/commit/f58b2d54a30e5f869a6db793a193180958780a18))
* **analyzer:** optimize context command for large codebases ([6437c83](https://github.com/panbanda/omen/commit/6437c83ba3fa46e50d1b89a5b1db56a561743519))
* **analyzer:** optimize context command for large codebases ([7d072ca](https://github.com/panbanda/omen/commit/7d072ca8064e138b24c7cb9f738669b066d1d2dd))
* **analyzer:** optimize dead code analyzer with single-pass AST traversal ([00203e0](https://github.com/panbanda/omen/commit/00203e0dd18093f1691bbece4188644ea523e1d6))
* **analyzer:** optimize dead code analyzer with single-pass AST traversal ([5e08d83](https://github.com/panbanda/omen/commit/5e08d83894066f0188776423049a7b44530f8209))
* **analyzer:** optimize performance and add PMAT compatibility ([38a4659](https://github.com/panbanda/omen/commit/38a46592dec0c7155946c651f530f1f42d76a032))
* **analyzer:** optimize performance and add PMAT compatibility ([4c77afd](https://github.com/panbanda/omen/commit/4c77afdb8451656997af8281022f9b17a9f248a7))
* **analyzer:** replace xxhash with allocation-free hashing in duplicates ([786e5f5](https://github.com/panbanda/omen/commit/786e5f5bb022ca24a9cfdf149a68421735cc5f03))
* **analyzer:** replace xxhash with allocation-free hashing in duplicates ([1d77a65](https://github.com/panbanda/omen/commit/1d77a65dd301e62d84bf1e4b2be5d16daad9046b))
* **analyzer:** use indexed lookup for function call matching ([0ea2be2](https://github.com/panbanda/omen/commit/0ea2be289fc300baab419b3ca2c35f1793985ce2))
* **ownership:** use native git blame for 100x+ speedup ([0f7cc3c](https://github.com/panbanda/omen/commit/0f7cc3cc01e7941e8febbc1bb78ac4e09f7b0993))
* **ownership:** use native git blame for 100x+ speedup ([7e2a551](https://github.com/panbanda/omen/commit/7e2a5516cd4e8c295feeae78e49b047e4c215e2f))

## [4.3.0](https://github.com/panbanda/omen/compare/v4.2.0...v4.3.0) (2025-12-01)


### Features

* **mcp:** add Go template support for prompts with frontmatter defaults ([287a4d3](https://github.com/panbanda/omen/commit/287a4d3d2f6d4c999ffb12a7f8829e5a6919f2ad))
* **mcp:** add Go template support for prompts with frontmatter defaults ([6d19914](https://github.com/panbanda/omen/commit/6d1991404012b15054bb11f96d0a7f9aa3cdae8d))


### Bug Fixes

* **ci:** add concurrency control to release workflow ([345fc62](https://github.com/panbanda/omen/commit/345fc62bbc2917d38a2a51f340d76b29241e24f0))
* **ci:** add concurrency control to release workflow ([ada1beb](https://github.com/panbanda/omen/commit/ada1bebeccbe2b09aafbf15d1200c592e8c7c2fa))
* **jit:** correct temporal ordering for state-dependent metrics ([61d6f61](https://github.com/panbanda/omen/commit/61d6f611288488c4d4d81fbcf372482137c80d93))
* **jit:** correct temporal ordering for state-dependent metrics ([f42f810](https://github.com/panbanda/omen/commit/f42f810af2400db753667ec19f19ff0591a16f1b))


### Performance Improvements

* **ownership:** use native git blame for 100x+ speedup ([0f7cc3c](https://github.com/panbanda/omen/commit/0f7cc3cc01e7941e8febbc1bb78ac4e09f7b0993))
* **ownership:** use native git blame for 100x+ speedup ([7e2a551](https://github.com/panbanda/omen/commit/7e2a5516cd4e8c295feeae78e49b047e4c215e2f))

## [4.2.0](https://github.com/panbanda/omen/compare/v4.1.0...v4.2.0) (2025-12-01)


### Features

* **mcp:** add analyze_changes and analyze_smells tools ([18b266a](https://github.com/panbanda/omen/commit/18b266aad050a768adcb023d1ae3001690c8d8ad))
* **mcp:** add analyze_changes and analyze_smells tools ([88da6cc](https://github.com/panbanda/omen/commit/88da6cce367189914b7e9ce4509f3ee09e004dbe))

## [4.1.0](https://github.com/panbanda/omen/compare/v4.0.0...v4.1.0) (2025-12-01)


### Features

* trigger 0.2.0 release ([525996b](https://github.com/panbanda/omen/commit/525996b8760591070a51682b65c00ec5e00ef084))
* trigger 0.2.0 release ([a24d9bf](https://github.com/panbanda/omen/commit/a24d9bfd0a7cd389cad4327da8b0f666c9b566b4))


### Bug Fixes

* final release trigger ([ed5cf8b](https://github.com/panbanda/omen/commit/ed5cf8baf4e9afc65cfe73a429ee98ee8290182c))
* final release trigger ([0e9eb61](https://github.com/panbanda/omen/commit/0e9eb618f00df0e6c00db5c31f4ff4549aa0b602))
* **release:** reset CLI version to 0.1.0 ([796f5c4](https://github.com/panbanda/omen/commit/796f5c4356fb81cad97717bd0ddb2108ee93aabf))
* **release:** reset CLI versioning to 0.1.0 ([0b090c6](https://github.com/panbanda/omen/commit/0b090c6e080304ef3e01c5463607df739f08863c))
* reset manifest to 4.0.0 ([f1736e3](https://github.com/panbanda/omen/commit/f1736e31828d28b48f4774cf90a4d472c5e48013))
* reset manifest to 4.0.0 ([f72dd84](https://github.com/panbanda/omen/commit/f72dd84ed9bbedd8f411e11f24f207ee5db6382b))
* retrigger release-please ([83a725c](https://github.com/panbanda/omen/commit/83a725ca2450bffae12f4a87ab42286cd864efb5))
* retrigger release-please ([3949a4f](https://github.com/panbanda/omen/commit/3949a4fa56838cc73ec48ef4b014555cbef97630))

## [4.0.0](https://github.com/panbanda/omen/compare/v3.0.1...v4.0.0) (2025-11-30)


### ⚠ BREAKING CHANGES

* **analyzer:** Halstead metrics removed from complexity analysis. Use cognitive complexity instead.
* **defect:** Risk levels changed from 4 to 3 (removed RiskCritical). Probability values now use sigmoid transformation producing different output values than previous linear calculation.

### Features

* **analyzer:** add advanced analysis capabilities ([b006d4c](https://github.com/panbanda/omen/commit/b006d4c6ae9d67a272a03956f8b5d4b34fab6244))
* **analyzer:** add advanced analysis capabilities ([78c2a62](https://github.com/panbanda/omen/commit/78c2a620d1d63c2f800cf980533cc5fadfc3fe8b))
* **analyzer:** add PMAT-compatible duplicate detection ([5003425](https://github.com/panbanda/omen/commit/500342590f5a6cf9ebf88d0006e06087167ed27b))
* **analyzer:** add PMAT-compatible duplicate detection ([af09a95](https://github.com/panbanda/omen/commit/af09a9578fdbc929d79f90d09bcd9cd93b01f5bd))
* **analyzer:** add research-backed analysis enhancements ([8795745](https://github.com/panbanda/omen/commit/879574544d2c767f491ec38eae8be21799f8ee04))
* **analyzer:** add research-backed analysis enhancements ([6dfcf29](https://github.com/panbanda/omen/commit/6dfcf294a91ab4ef2ad5a23135f341154d95adf7))
* **analyzer:** add Tarjan SCC cycle detection and enhanced Mermaid output ([6ee8f96](https://github.com/panbanda/omen/commit/6ee8f96ab766ff38b230ba95fe1e30b8b1d5eb5f))
* **analyzer:** add Tarjan SCC cycle detection and enhanced Mermaid output ([8df948e](https://github.com/panbanda/omen/commit/8df948e8eeba4fccae356a5abb32f1ee9063e36a))
* **churn:** add hotspot and stable file detection ([863a6ea](https://github.com/panbanda/omen/commit/863a6ea5950e2399ca69ec527ec2b31008434b68))
* **churn:** add hotspot and stable file detection ([efe13c4](https://github.com/panbanda/omen/commit/efe13c41ec8f754931a57114d214625afd379dc5))
* **ci:** add release-please for automated releases ([40754bd](https://github.com/panbanda/omen/commit/40754bde0af216ea4ea018fc646868562246efe1))
* **complexity:** add Halstead software science metrics ([797399c](https://github.com/panbanda/omen/commit/797399c21c4c723a818b5780fd8f72c5b2037f92))
* **complexity:** add Halstead software science metrics ([ca06165](https://github.com/panbanda/omen/commit/ca061652fba19d7ed8bc6ad8a94f9d73e5edac96))
* **complexity:** add pmat-compatible models and tests ([d980808](https://github.com/panbanda/omen/commit/d98080863e7b13881692680cd31c73aae27dd60f))
* **deadcode:** add call graph and BFS reachability analysis ([1cc52ad](https://github.com/panbanda/omen/commit/1cc52ad713f5a0b106dd2309659d7eeb0b5a0f84))
* **deadcode:** port PMAT architecture with Roaring bitmaps and VTable resolution ([c098ad8](https://github.com/panbanda/omen/commit/c098ad830548c43e0bcf87ebd7da53386086e919))
* **deadcode:** port PMAT architecture with Roaring bitmaps and VTable resolution ([ab49bfd](https://github.com/panbanda/omen/commit/ab49bfd5cadc36ecd94c38a02f62652c922f64bb))
* **defect:** implement PMAT-compatible defect prediction algorithm ([3a3530b](https://github.com/panbanda/omen/commit/3a3530bb790c2997e71d26f250c681bef80f6e43))
* **defect:** PMAT-compatible defect prediction algorithm ([f11337e](https://github.com/panbanda/omen/commit/f11337e8ec261d01a1c7ec1cfb4934cda09f9d73))
* **duplicates:** align duplicate detection with pmat implementation ([0ec0b0f](https://github.com/panbanda/omen/commit/0ec0b0fbb02c21876934886131a178117196e34c))
* **graph:** integrate gonum for graph algorithms ([1ed85d7](https://github.com/panbanda/omen/commit/1ed85d79f79dd016aaa726dbd60f0f87fc8d48a0))
* initial implementation of omen multi-language code analyzer ([832d2bc](https://github.com/panbanda/omen/commit/832d2bcbde956b59e108171bb78a712715cc320d))
* **mcp:** add MCP server for LLM tool integration ([518b378](https://github.com/panbanda/omen/commit/518b3780982949b6d5442239712089584762b62a))
* **mcp:** add MCP server for LLM tool integration ([52c2708](https://github.com/panbanda/omen/commit/52c27088a01eb4b5765e574e64491d1f2039bc80))
* **mcp:** add missing CLI parameters to tool inputs ([d6a9a3c](https://github.com/panbanda/omen/commit/d6a9a3cd29a3252b033a561a15ea9cd566b095da))
* **mcp:** add prompts and skills plugin ([0326af1](https://github.com/panbanda/omen/commit/0326af1da931c83db1fa9aa016051bc425a53d6a))
* **mcp:** add prompts and skills plugin ([bf12602](https://github.com/panbanda/omen/commit/bf12602a949db3e77eb9a51c0c55edd1241ca381))
* **satd:** add PMAT-compatible strict mode, test block tracking, and AST context ([68452db](https://github.com/panbanda/omen/commit/68452db332d8009b77cf0a80f977aace461cea9c))
* **satd:** add severity adjustment, context hash, and file exclusion ([9f4032e](https://github.com/panbanda/omen/commit/9f4032e1a7fd2b83768e5fec1bf7f5912845427a))
* **satd:** add severity adjustment, context hash, and file exclusion ([dddd1f3](https://github.com/panbanda/omen/commit/dddd1f3029d7ab5be7a003ac0e9f48ffb2e8a405))
* **tdg:** port PMAT scoring system with 0-100 scale ([8ce8c02](https://github.com/panbanda/omen/commit/8ce8c02776207a1b1dd8dea5795c64c0b9ed6533))
* **tdg:** port PMAT scoring system with 0-100 scale ([b4e2eb5](https://github.com/panbanda/omen/commit/b4e2eb5d4b539cc4938776a79354e8eeb4392739))
* **tdg:** update scale to 0-5 and add coupling/domain risk ([9d9f743](https://github.com/panbanda/omen/commit/9d9f743a860cb5ce6c060c2472ea3d3b471a9619))


### Bug Fixes

* added tip instead of bold ([c0efa24](https://github.com/panbanda/omen/commit/c0efa24ff00dcf54d45a04822addc2bac10891ac))
* address issues from comprehensive code review ([71ccb03](https://github.com/panbanda/omen/commit/71ccb034170f7a7acffd297e6e1ddfac9463c675))
* address issues from comprehensive code review ([d9ad7f7](https://github.com/panbanda/omen/commit/d9ad7f78657d5f9b9383e68056760b8e22e40e91))
* address issues from comprehensive code review ([944bb2a](https://github.com/panbanda/omen/commit/944bb2a4a27dcfe021123a895db9abde20b25a5c))
* address issues from comprehensive code review ([54c193d](https://github.com/panbanda/omen/commit/54c193df859e6088e67eb84a164cf1d6dd0e8ed0))
* address issues from comprehensive code review ([26d6878](https://github.com/panbanda/omen/commit/26d6878e0ff54046f3997b3200106f7ac0307e24))
* **analyzer:** address code review feedback ([6036114](https://github.com/panbanda/omen/commit/6036114821ae78d0822c5c5d48042f3c23455bd3))
* **churn:** align JSON output with pmat reference implementation ([7b80f50](https://github.com/panbanda/omen/commit/7b80f506f10e0af5afdb71dae1ad982121d42147))
* **churn:** align JSON output with pmat reference implementation ([0f75b8d](https://github.com/panbanda/omen/commit/0f75b8d93c7a10906ccf2d9940ba6085b8ab0b9d))
* **ci:** add golangci-lint config and fix test data races ([cd1c3b7](https://github.com/panbanda/omen/commit/cd1c3b7d15adbae085ea93e7547f222495498818))
* **ci:** checkout release tag for GoReleaser ([4dadbe3](https://github.com/panbanda/omen/commit/4dadbe3981671dbd0a40f2f9b201538a271e01df))
* **ci:** correct release-please output variable for root package ([a072ec4](https://github.com/panbanda/omen/commit/a072ec46b57d64ec4bde8782b3979207228f5755))
* **ci:** correct release-please output variable for root package ([da83f29](https://github.com/panbanda/omen/commit/da83f295775d3d274453a61c517e060dfa50dbe5))
* **ci:** replace goreleaser homebrew with manual formula generation ([b911b13](https://github.com/panbanda/omen/commit/b911b13b57a472e67a5cb0b3f8686ce5b5f30509))
* **ci:** resolve workflow failures ([e7b384a](https://github.com/panbanda/omen/commit/e7b384ae563744754eb43824cd8e71f4950fe705))
* **ci:** simplify to stable Go on Linux and macOS only ([5cb11f5](https://github.com/panbanda/omen/commit/5cb11f53c9e73286dd4b7f7211b053ec5c659697))
* **ci:** use --single-target with per-arch matrix jobs ([0394e6c](https://github.com/panbanda/omen/commit/0394e6c60c7519a5869ec3cca1826b0b2541ac4c))
* **ci:** use builds.skip with Runtime.Goos template ([44538de](https://github.com/panbanda/omen/commit/44538de5e712efcf493ad43f2367b6c44e5f0c0a))
* **ci:** use Go 1.25 across workflow and go.mod ([b3ee14e](https://github.com/panbanda/omen/commit/b3ee14e624d05f0681e5467dcf100613e4ada26a))
* **ci:** use macos-15-intel instead of macos-15-large ([a65dd86](https://github.com/panbanda/omen/commit/a65dd86d2bad265e2f0e5a21b46b1cf94068f75e))
* **ci:** use matrix builds for CGO cross-compilation ([fae3508](https://github.com/panbanda/omen/commit/fae350885da274bc6de57ef68542e37377cc081a))
* **ci:** use matrix builds for CGO cross-compilation ([909af82](https://github.com/panbanda/omen/commit/909af82b3a763d580d6fac757e81f8a227d0ceb7))
* **ci:** use per-target builds with skip conditions ([29c617f](https://github.com/panbanda/omen/commit/29c617f40fc5a78b03980bde621f30c1d5fe3500))
* **ci:** use per-target checksum filenames ([35428d0](https://github.com/panbanda/omen/commit/35428d0eb2f214534983db2275062edb5b2ed08a))
* **ci:** use Runtime.Goos for build filtering ([8c52192](https://github.com/panbanda/omen/commit/8c521924beef9f868b2d0c77d18b6b18260df8a6))
* **ci:** use simple v-prefix for CLI release tags ([41bd100](https://github.com/panbanda/omen/commit/41bd10055f96e16dae4d69ffeceaef12772219b5))
* **ci:** use simple v-prefix for CLI release tags ([485cf0d](https://github.com/panbanda/omen/commit/485cf0d798fae21b9ffc99ffdb331b26fc1f5ba1))
* **cli:** handle trailing flags after positional arguments ([e8ea822](https://github.com/panbanda/omen/commit/e8ea8227b0492da91a1c64ce6d179141b24c3069))
* **cli:** handle trailing flags after positional arguments ([366523b](https://github.com/panbanda/omen/commit/366523b0fbab84660fb4b2c3c2ac555be22de892))
* **deadcode:** restructure JSON output to match pmat format ([6ca4568](https://github.com/panbanda/omen/commit/6ca4568480bcf6daedc2e7efd22ed499a3a64858))
* **deadcode:** restructure JSON output to match pmat format ([b4c68b3](https://github.com/panbanda/omen/commit/b4c68b38d9b33d22fcf15374dabf27caea21ab38))
* **defect:** align JSON output with pmat format ([2d7d417](https://github.com/panbanda/omen/commit/2d7d417992a928f498808f5da8b5fcf90a570d71))
* **defect:** align JSON output with pmat format ([85f5ba3](https://github.com/panbanda/omen/commit/85f5ba398c08b979744e53fa0311297e8eb5504d))
* **duplicates:** align JSON output with pmat format ([68a17d1](https://github.com/panbanda/omen/commit/68a17d1526f3c90c9fc7d5147ffa631becf5adb4))
* **duplicates:** align JSON output with pmat format ([7c7dc2c](https://github.com/panbanda/omen/commit/7c7dc2c7a94286abd154f0e8ff967bcf9c388da3))
* **duplicates:** fix race condition in identifier canonicalization ([3af00f1](https://github.com/panbanda/omen/commit/3af00f1186fa6307235e80fffd2d4fbb4c7b3166))
* **hotspot:** use geometric mean with CDF normalization for scoring ([594b6e9](https://github.com/panbanda/omen/commit/594b6e916f996b8901e755bc97dfe9134b3bdadd))
* **hotspot:** use geometric mean with CDF normalization for scoring ([06a53a5](https://github.com/panbanda/omen/commit/06a53a509f9c44952d2e26c1d855a40c7306a79f))
* **output:** enable toon format serialization for all model types ([1fda456](https://github.com/panbanda/omen/commit/1fda456b7722653866cbeac91eae3bfa8a696cf6))
* **output:** enable toon format serialization for all model types ([d39c952](https://github.com/panbanda/omen/commit/d39c952d98c322ba523541b7d7f941b3534d0eae))
* **readme:** correct badge URLs to panbanda/omen ([533626f](https://github.com/panbanda/omen/commit/533626f04e3fa8128c456084956835feba8fc72f))
* **readme:** correct badge URLs to panbanda/omen ([d8ea557](https://github.com/panbanda/omen/commit/d8ea5570afcce378906f72c58563ac501cd7aca3))
* **release:** configure GoReleaser to append to existing release ([5ff11cf](https://github.com/panbanda/omen/commit/5ff11cfc5a81ff6edcf0c75a1a3887e8436a3385))
* resolve security, performance, and correctness issues from code review ([cf1ff03](https://github.com/panbanda/omen/commit/cf1ff0366845b60f7906eb0ae3948d15a47e5330))
* **satd:** add files_with_satd to summary for pmat compatibility ([4321269](https://github.com/panbanda/omen/commit/432126944b35c552c55988136531fb125c623973))
* **satd:** add files_with_satd to summary for pmat compatibility ([4f2cd4f](https://github.com/panbanda/omen/commit/4f2cd4fb69f8206a115f38670c6f90b56639f26a))
* **tdg:** align JSON output with pmat reference implementation ([d640a07](https://github.com/panbanda/omen/commit/d640a0724ca8eece3cada537543a0d1d7cc1aa43))
* **tdg:** align JSON output with pmat reference implementation ([ae4ad5d](https://github.com/panbanda/omen/commit/ae4ad5d9971b212a4dd6fe539adba38193dce25c))
* trigger release ([4b99bd1](https://github.com/panbanda/omen/commit/4b99bd1f05a556b009e286a55812c12a2909e98b))


### Performance Improvements

* **analyzer:** move map allocations to package level ([f58b2d5](https://github.com/panbanda/omen/commit/f58b2d54a30e5f869a6db793a193180958780a18))
* **analyzer:** optimize context command for large codebases ([6437c83](https://github.com/panbanda/omen/commit/6437c83ba3fa46e50d1b89a5b1db56a561743519))
* **analyzer:** optimize context command for large codebases ([7d072ca](https://github.com/panbanda/omen/commit/7d072ca8064e138b24c7cb9f738669b066d1d2dd))
* **analyzer:** optimize dead code analyzer with single-pass AST traversal ([00203e0](https://github.com/panbanda/omen/commit/00203e0dd18093f1691bbece4188644ea523e1d6))
* **analyzer:** optimize dead code analyzer with single-pass AST traversal ([5e08d83](https://github.com/panbanda/omen/commit/5e08d83894066f0188776423049a7b44530f8209))
* **analyzer:** optimize performance and add PMAT compatibility ([38a4659](https://github.com/panbanda/omen/commit/38a46592dec0c7155946c651f530f1f42d76a032))
* **analyzer:** optimize performance and add PMAT compatibility ([4c77afd](https://github.com/panbanda/omen/commit/4c77afdb8451656997af8281022f9b17a9f248a7))
* **analyzer:** replace xxhash with allocation-free hashing in duplicates ([786e5f5](https://github.com/panbanda/omen/commit/786e5f5bb022ca24a9cfdf149a68421735cc5f03))
* **analyzer:** replace xxhash with allocation-free hashing in duplicates ([1d77a65](https://github.com/panbanda/omen/commit/1d77a65dd301e62d84bf1e4b2be5d16daad9046b))
* **analyzer:** use indexed lookup for function call matching ([0ea2be2](https://github.com/panbanda/omen/commit/0ea2be289fc300baab419b3ca2c35f1793985ce2))

## [3.0.1](https://github.com/panbanda/omen/compare/omen-v3.0.0...omen-v3.0.1) (2025-11-30)


### Bug Fixes

* **ci:** correct release-please output variable for root package ([a072ec4](https://github.com/panbanda/omen/commit/a072ec46b57d64ec4bde8782b3979207228f5755))
* **ci:** correct release-please output variable for root package ([da83f29](https://github.com/panbanda/omen/commit/da83f295775d3d274453a61c517e060dfa50dbe5))

## [3.0.0](https://github.com/panbanda/omen/compare/omen-v2.0.0...omen-v3.0.0) (2025-11-30)


### ⚠ BREAKING CHANGES

* **analyzer:** Halstead metrics removed from complexity analysis. Use cognitive complexity instead.

### Features

* **analyzer:** add research-backed analysis enhancements ([8795745](https://github.com/panbanda/omen/commit/879574544d2c767f491ec38eae8be21799f8ee04))
* **analyzer:** add research-backed analysis enhancements ([6dfcf29](https://github.com/panbanda/omen/commit/6dfcf294a91ab4ef2ad5a23135f341154d95adf7))


### Bug Fixes

* **analyzer:** address code review feedback ([6036114](https://github.com/panbanda/omen/commit/6036114821ae78d0822c5c5d48042f3c23455bd3))

## [2.0.0](https://github.com/panbanda/omen/compare/omen-v1.7.2...omen-v2.0.0) (2025-11-30)


### ⚠ BREAKING CHANGES

* **defect:** Risk levels changed from 4 to 3 (removed RiskCritical). Probability values now use sigmoid transformation producing different output values than previous linear calculation.

### Features

* **analyzer:** add advanced analysis capabilities ([b006d4c](https://github.com/panbanda/omen/commit/b006d4c6ae9d67a272a03956f8b5d4b34fab6244))
* **analyzer:** add advanced analysis capabilities ([78c2a62](https://github.com/panbanda/omen/commit/78c2a620d1d63c2f800cf980533cc5fadfc3fe8b))
* **analyzer:** add PMAT-compatible duplicate detection ([5003425](https://github.com/panbanda/omen/commit/500342590f5a6cf9ebf88d0006e06087167ed27b))
* **analyzer:** add PMAT-compatible duplicate detection ([af09a95](https://github.com/panbanda/omen/commit/af09a9578fdbc929d79f90d09bcd9cd93b01f5bd))
* **analyzer:** add Tarjan SCC cycle detection and enhanced Mermaid output ([6ee8f96](https://github.com/panbanda/omen/commit/6ee8f96ab766ff38b230ba95fe1e30b8b1d5eb5f))
* **analyzer:** add Tarjan SCC cycle detection and enhanced Mermaid output ([8df948e](https://github.com/panbanda/omen/commit/8df948e8eeba4fccae356a5abb32f1ee9063e36a))
* **churn:** add hotspot and stable file detection ([863a6ea](https://github.com/panbanda/omen/commit/863a6ea5950e2399ca69ec527ec2b31008434b68))
* **churn:** add hotspot and stable file detection ([efe13c4](https://github.com/panbanda/omen/commit/efe13c41ec8f754931a57114d214625afd379dc5))
* **ci:** add release-please for automated releases ([40754bd](https://github.com/panbanda/omen/commit/40754bde0af216ea4ea018fc646868562246efe1))
* **complexity:** add Halstead software science metrics ([797399c](https://github.com/panbanda/omen/commit/797399c21c4c723a818b5780fd8f72c5b2037f92))
* **complexity:** add Halstead software science metrics ([ca06165](https://github.com/panbanda/omen/commit/ca061652fba19d7ed8bc6ad8a94f9d73e5edac96))
* **complexity:** add pmat-compatible models and tests ([d980808](https://github.com/panbanda/omen/commit/d98080863e7b13881692680cd31c73aae27dd60f))
* **deadcode:** add call graph and BFS reachability analysis ([1cc52ad](https://github.com/panbanda/omen/commit/1cc52ad713f5a0b106dd2309659d7eeb0b5a0f84))
* **deadcode:** port PMAT architecture with Roaring bitmaps and VTable resolution ([c098ad8](https://github.com/panbanda/omen/commit/c098ad830548c43e0bcf87ebd7da53386086e919))
* **deadcode:** port PMAT architecture with Roaring bitmaps and VTable resolution ([ab49bfd](https://github.com/panbanda/omen/commit/ab49bfd5cadc36ecd94c38a02f62652c922f64bb))
* **defect:** implement PMAT-compatible defect prediction algorithm ([3a3530b](https://github.com/panbanda/omen/commit/3a3530bb790c2997e71d26f250c681bef80f6e43))
* **defect:** PMAT-compatible defect prediction algorithm ([f11337e](https://github.com/panbanda/omen/commit/f11337e8ec261d01a1c7ec1cfb4934cda09f9d73))
* **duplicates:** align duplicate detection with pmat implementation ([0ec0b0f](https://github.com/panbanda/omen/commit/0ec0b0fbb02c21876934886131a178117196e34c))
* **graph:** integrate gonum for graph algorithms ([1ed85d7](https://github.com/panbanda/omen/commit/1ed85d79f79dd016aaa726dbd60f0f87fc8d48a0))
* initial implementation of omen multi-language code analyzer ([832d2bc](https://github.com/panbanda/omen/commit/832d2bcbde956b59e108171bb78a712715cc320d))
* **mcp:** add MCP server for LLM tool integration ([518b378](https://github.com/panbanda/omen/commit/518b3780982949b6d5442239712089584762b62a))
* **mcp:** add MCP server for LLM tool integration ([52c2708](https://github.com/panbanda/omen/commit/52c27088a01eb4b5765e574e64491d1f2039bc80))
* **mcp:** add missing CLI parameters to tool inputs ([d6a9a3c](https://github.com/panbanda/omen/commit/d6a9a3cd29a3252b033a561a15ea9cd566b095da))
* **mcp:** add prompts and skills plugin ([0326af1](https://github.com/panbanda/omen/commit/0326af1da931c83db1fa9aa016051bc425a53d6a))
* **mcp:** add prompts and skills plugin ([bf12602](https://github.com/panbanda/omen/commit/bf12602a949db3e77eb9a51c0c55edd1241ca381))
* **satd:** add PMAT-compatible strict mode, test block tracking, and AST context ([68452db](https://github.com/panbanda/omen/commit/68452db332d8009b77cf0a80f977aace461cea9c))
* **satd:** add severity adjustment, context hash, and file exclusion ([9f4032e](https://github.com/panbanda/omen/commit/9f4032e1a7fd2b83768e5fec1bf7f5912845427a))
* **satd:** add severity adjustment, context hash, and file exclusion ([dddd1f3](https://github.com/panbanda/omen/commit/dddd1f3029d7ab5be7a003ac0e9f48ffb2e8a405))
* **tdg:** port PMAT scoring system with 0-100 scale ([8ce8c02](https://github.com/panbanda/omen/commit/8ce8c02776207a1b1dd8dea5795c64c0b9ed6533))
* **tdg:** port PMAT scoring system with 0-100 scale ([b4e2eb5](https://github.com/panbanda/omen/commit/b4e2eb5d4b539cc4938776a79354e8eeb4392739))
* **tdg:** update scale to 0-5 and add coupling/domain risk ([9d9f743](https://github.com/panbanda/omen/commit/9d9f743a860cb5ce6c060c2472ea3d3b471a9619))


### Bug Fixes

* added tip instead of bold ([c0efa24](https://github.com/panbanda/omen/commit/c0efa24ff00dcf54d45a04822addc2bac10891ac))
* address issues from comprehensive code review ([71ccb03](https://github.com/panbanda/omen/commit/71ccb034170f7a7acffd297e6e1ddfac9463c675))
* address issues from comprehensive code review ([d9ad7f7](https://github.com/panbanda/omen/commit/d9ad7f78657d5f9b9383e68056760b8e22e40e91))
* address issues from comprehensive code review ([944bb2a](https://github.com/panbanda/omen/commit/944bb2a4a27dcfe021123a895db9abde20b25a5c))
* address issues from comprehensive code review ([54c193d](https://github.com/panbanda/omen/commit/54c193df859e6088e67eb84a164cf1d6dd0e8ed0))
* address issues from comprehensive code review ([26d6878](https://github.com/panbanda/omen/commit/26d6878e0ff54046f3997b3200106f7ac0307e24))
* **churn:** align JSON output with pmat reference implementation ([7b80f50](https://github.com/panbanda/omen/commit/7b80f506f10e0af5afdb71dae1ad982121d42147))
* **churn:** align JSON output with pmat reference implementation ([0f75b8d](https://github.com/panbanda/omen/commit/0f75b8d93c7a10906ccf2d9940ba6085b8ab0b9d))
* **ci:** add golangci-lint config and fix test data races ([cd1c3b7](https://github.com/panbanda/omen/commit/cd1c3b7d15adbae085ea93e7547f222495498818))
* **ci:** checkout release tag for GoReleaser ([4dadbe3](https://github.com/panbanda/omen/commit/4dadbe3981671dbd0a40f2f9b201538a271e01df))
* **ci:** replace goreleaser homebrew with manual formula generation ([b911b13](https://github.com/panbanda/omen/commit/b911b13b57a472e67a5cb0b3f8686ce5b5f30509))
* **ci:** resolve workflow failures ([e7b384a](https://github.com/panbanda/omen/commit/e7b384ae563744754eb43824cd8e71f4950fe705))
* **ci:** simplify to stable Go on Linux and macOS only ([5cb11f5](https://github.com/panbanda/omen/commit/5cb11f53c9e73286dd4b7f7211b053ec5c659697))
* **ci:** use --single-target with per-arch matrix jobs ([0394e6c](https://github.com/panbanda/omen/commit/0394e6c60c7519a5869ec3cca1826b0b2541ac4c))
* **ci:** use builds.skip with Runtime.Goos template ([44538de](https://github.com/panbanda/omen/commit/44538de5e712efcf493ad43f2367b6c44e5f0c0a))
* **ci:** use Go 1.25 across workflow and go.mod ([b3ee14e](https://github.com/panbanda/omen/commit/b3ee14e624d05f0681e5467dcf100613e4ada26a))
* **ci:** use macos-15-intel instead of macos-15-large ([a65dd86](https://github.com/panbanda/omen/commit/a65dd86d2bad265e2f0e5a21b46b1cf94068f75e))
* **ci:** use matrix builds for CGO cross-compilation ([fae3508](https://github.com/panbanda/omen/commit/fae350885da274bc6de57ef68542e37377cc081a))
* **ci:** use matrix builds for CGO cross-compilation ([909af82](https://github.com/panbanda/omen/commit/909af82b3a763d580d6fac757e81f8a227d0ceb7))
* **ci:** use per-target builds with skip conditions ([29c617f](https://github.com/panbanda/omen/commit/29c617f40fc5a78b03980bde621f30c1d5fe3500))
* **ci:** use per-target checksum filenames ([35428d0](https://github.com/panbanda/omen/commit/35428d0eb2f214534983db2275062edb5b2ed08a))
* **ci:** use Runtime.Goos for build filtering ([8c52192](https://github.com/panbanda/omen/commit/8c521924beef9f868b2d0c77d18b6b18260df8a6))
* **cli:** handle trailing flags after positional arguments ([e8ea822](https://github.com/panbanda/omen/commit/e8ea8227b0492da91a1c64ce6d179141b24c3069))
* **cli:** handle trailing flags after positional arguments ([366523b](https://github.com/panbanda/omen/commit/366523b0fbab84660fb4b2c3c2ac555be22de892))
* **deadcode:** restructure JSON output to match pmat format ([6ca4568](https://github.com/panbanda/omen/commit/6ca4568480bcf6daedc2e7efd22ed499a3a64858))
* **deadcode:** restructure JSON output to match pmat format ([b4c68b3](https://github.com/panbanda/omen/commit/b4c68b38d9b33d22fcf15374dabf27caea21ab38))
* **defect:** align JSON output with pmat format ([2d7d417](https://github.com/panbanda/omen/commit/2d7d417992a928f498808f5da8b5fcf90a570d71))
* **defect:** align JSON output with pmat format ([85f5ba3](https://github.com/panbanda/omen/commit/85f5ba398c08b979744e53fa0311297e8eb5504d))
* **duplicates:** align JSON output with pmat format ([68a17d1](https://github.com/panbanda/omen/commit/68a17d1526f3c90c9fc7d5147ffa631becf5adb4))
* **duplicates:** align JSON output with pmat format ([7c7dc2c](https://github.com/panbanda/omen/commit/7c7dc2c7a94286abd154f0e8ff967bcf9c388da3))
* **duplicates:** fix race condition in identifier canonicalization ([3af00f1](https://github.com/panbanda/omen/commit/3af00f1186fa6307235e80fffd2d4fbb4c7b3166))
* **hotspot:** use geometric mean with CDF normalization for scoring ([594b6e9](https://github.com/panbanda/omen/commit/594b6e916f996b8901e755bc97dfe9134b3bdadd))
* **hotspot:** use geometric mean with CDF normalization for scoring ([06a53a5](https://github.com/panbanda/omen/commit/06a53a509f9c44952d2e26c1d855a40c7306a79f))
* **output:** enable toon format serialization for all model types ([1fda456](https://github.com/panbanda/omen/commit/1fda456b7722653866cbeac91eae3bfa8a696cf6))
* **output:** enable toon format serialization for all model types ([d39c952](https://github.com/panbanda/omen/commit/d39c952d98c322ba523541b7d7f941b3534d0eae))
* **readme:** correct badge URLs to panbanda/omen ([533626f](https://github.com/panbanda/omen/commit/533626f04e3fa8128c456084956835feba8fc72f))
* **readme:** correct badge URLs to panbanda/omen ([d8ea557](https://github.com/panbanda/omen/commit/d8ea5570afcce378906f72c58563ac501cd7aca3))
* **release:** configure GoReleaser to append to existing release ([5ff11cf](https://github.com/panbanda/omen/commit/5ff11cfc5a81ff6edcf0c75a1a3887e8436a3385))
* resolve security, performance, and correctness issues from code review ([cf1ff03](https://github.com/panbanda/omen/commit/cf1ff0366845b60f7906eb0ae3948d15a47e5330))
* **satd:** add files_with_satd to summary for pmat compatibility ([4321269](https://github.com/panbanda/omen/commit/432126944b35c552c55988136531fb125c623973))
* **satd:** add files_with_satd to summary for pmat compatibility ([4f2cd4f](https://github.com/panbanda/omen/commit/4f2cd4fb69f8206a115f38670c6f90b56639f26a))
* **tdg:** align JSON output with pmat reference implementation ([d640a07](https://github.com/panbanda/omen/commit/d640a0724ca8eece3cada537543a0d1d7cc1aa43))
* **tdg:** align JSON output with pmat reference implementation ([ae4ad5d](https://github.com/panbanda/omen/commit/ae4ad5d9971b212a4dd6fe539adba38193dce25c))
* trigger release ([4b99bd1](https://github.com/panbanda/omen/commit/4b99bd1f05a556b009e286a55812c12a2909e98b))


### Performance Improvements

* **analyzer:** move map allocations to package level ([f58b2d5](https://github.com/panbanda/omen/commit/f58b2d54a30e5f869a6db793a193180958780a18))
* **analyzer:** optimize context command for large codebases ([6437c83](https://github.com/panbanda/omen/commit/6437c83ba3fa46e50d1b89a5b1db56a561743519))
* **analyzer:** optimize context command for large codebases ([7d072ca](https://github.com/panbanda/omen/commit/7d072ca8064e138b24c7cb9f738669b066d1d2dd))
* **analyzer:** optimize dead code analyzer with single-pass AST traversal ([00203e0](https://github.com/panbanda/omen/commit/00203e0dd18093f1691bbece4188644ea523e1d6))
* **analyzer:** optimize dead code analyzer with single-pass AST traversal ([5e08d83](https://github.com/panbanda/omen/commit/5e08d83894066f0188776423049a7b44530f8209))
* **analyzer:** optimize performance and add PMAT compatibility ([38a4659](https://github.com/panbanda/omen/commit/38a46592dec0c7155946c651f530f1f42d76a032))
* **analyzer:** optimize performance and add PMAT compatibility ([4c77afd](https://github.com/panbanda/omen/commit/4c77afdb8451656997af8281022f9b17a9f248a7))
* **analyzer:** replace xxhash with allocation-free hashing in duplicates ([786e5f5](https://github.com/panbanda/omen/commit/786e5f5bb022ca24a9cfdf149a68421735cc5f03))
* **analyzer:** replace xxhash with allocation-free hashing in duplicates ([1d77a65](https://github.com/panbanda/omen/commit/1d77a65dd301e62d84bf1e4b2be5d16daad9046b))
* **analyzer:** use indexed lookup for function call matching ([0ea2be2](https://github.com/panbanda/omen/commit/0ea2be289fc300baab419b3ca2c35f1793985ce2))

## [1.7.2](https://github.com/panbanda/omen/compare/v1.7.1...v1.7.2) (2025-11-30)


### Bug Fixes

* **hotspot:** use geometric mean with CDF normalization for scoring ([594b6e9](https://github.com/panbanda/omen/commit/594b6e916f996b8901e755bc97dfe9134b3bdadd))
* **hotspot:** use geometric mean with CDF normalization for scoring ([06a53a5](https://github.com/panbanda/omen/commit/06a53a509f9c44952d2e26c1d855a40c7306a79f))

## [1.7.1](https://github.com/panbanda/omen/compare/v1.7.0...v1.7.1) (2025-11-29)


### Bug Fixes

* added tip instead of bold ([c0efa24](https://github.com/panbanda/omen/commit/c0efa24ff00dcf54d45a04822addc2bac10891ac))
* address issues from comprehensive code review ([71ccb03](https://github.com/panbanda/omen/commit/71ccb034170f7a7acffd297e6e1ddfac9463c675))
* address issues from comprehensive code review ([d9ad7f7](https://github.com/panbanda/omen/commit/d9ad7f78657d5f9b9383e68056760b8e22e40e91))
* address issues from comprehensive code review ([944bb2a](https://github.com/panbanda/omen/commit/944bb2a4a27dcfe021123a895db9abde20b25a5c))
* address issues from comprehensive code review ([54c193d](https://github.com/panbanda/omen/commit/54c193df859e6088e67eb84a164cf1d6dd0e8ed0))
* address issues from comprehensive code review ([26d6878](https://github.com/panbanda/omen/commit/26d6878e0ff54046f3997b3200106f7ac0307e24))
* resolve security, performance, and correctness issues from code review ([cf1ff03](https://github.com/panbanda/omen/commit/cf1ff0366845b60f7906eb0ae3948d15a47e5330))

## [1.7.0](https://github.com/panbanda/omen/compare/v1.6.0...v1.7.0) (2025-11-29)


### Features

* **mcp:** add MCP server for LLM tool integration ([518b378](https://github.com/panbanda/omen/commit/518b3780982949b6d5442239712089584762b62a))
* **mcp:** add MCP server for LLM tool integration ([52c2708](https://github.com/panbanda/omen/commit/52c27088a01eb4b5765e574e64491d1f2039bc80))
* **mcp:** add missing CLI parameters to tool inputs ([d6a9a3c](https://github.com/panbanda/omen/commit/d6a9a3cd29a3252b033a561a15ea9cd566b095da))


### Performance Improvements

* **analyzer:** optimize context command for large codebases ([6437c83](https://github.com/panbanda/omen/commit/6437c83ba3fa46e50d1b89a5b1db56a561743519))
* **analyzer:** optimize context command for large codebases ([7d072ca](https://github.com/panbanda/omen/commit/7d072ca8064e138b24c7cb9f738669b066d1d2dd))
* **analyzer:** replace xxhash with allocation-free hashing in duplicates ([786e5f5](https://github.com/panbanda/omen/commit/786e5f5bb022ca24a9cfdf149a68421735cc5f03))
* **analyzer:** replace xxhash with allocation-free hashing in duplicates ([1d77a65](https://github.com/panbanda/omen/commit/1d77a65dd301e62d84bf1e4b2be5d16daad9046b))
* **analyzer:** use indexed lookup for function call matching ([0ea2be2](https://github.com/panbanda/omen/commit/0ea2be289fc300baab419b3ca2c35f1793985ce2))

## [1.6.0](https://github.com/panbanda/omen/compare/v1.5.3...v1.6.0) (2025-11-29)


### Features

* **analyzer:** add advanced analysis capabilities ([b006d4c](https://github.com/panbanda/omen/commit/b006d4c6ae9d67a272a03956f8b5d4b34fab6244))
* **analyzer:** add advanced analysis capabilities ([78c2a62](https://github.com/panbanda/omen/commit/78c2a620d1d63c2f800cf980533cc5fadfc3fe8b))


### Bug Fixes

* **output:** enable toon format serialization for all model types ([1fda456](https://github.com/panbanda/omen/commit/1fda456b7722653866cbeac91eae3bfa8a696cf6))
* **output:** enable toon format serialization for all model types ([d39c952](https://github.com/panbanda/omen/commit/d39c952d98c322ba523541b7d7f941b3534d0eae))


### Performance Improvements

* **analyzer:** move map allocations to package level ([f58b2d5](https://github.com/panbanda/omen/commit/f58b2d54a30e5f869a6db793a193180958780a18))

## [1.5.3](https://github.com/panbanda/omen/compare/v1.5.2...v1.5.3) (2025-11-29)


### Performance Improvements

* **analyzer:** optimize dead code analyzer with single-pass AST traversal ([00203e0](https://github.com/panbanda/omen/commit/00203e0dd18093f1691bbece4188644ea523e1d6))
* **analyzer:** optimize dead code analyzer with single-pass AST traversal ([5e08d83](https://github.com/panbanda/omen/commit/5e08d83894066f0188776423049a7b44530f8209))

## [1.5.2](https://github.com/panbanda/omen/compare/v1.5.1...v1.5.2) (2025-11-29)


### Bug Fixes

* **cli:** handle trailing flags after positional arguments ([e8ea822](https://github.com/panbanda/omen/commit/e8ea8227b0492da91a1c64ce6d179141b24c3069))
* **cli:** handle trailing flags after positional arguments ([366523b](https://github.com/panbanda/omen/commit/366523b0fbab84660fb4b2c3c2ac555be22de892))

## [1.5.1](https://github.com/panbanda/omen/compare/v1.5.0...v1.5.1) (2025-11-29)


### Bug Fixes

* **churn:** align JSON output with pmat reference implementation ([7b80f50](https://github.com/panbanda/omen/commit/7b80f506f10e0af5afdb71dae1ad982121d42147))
* **churn:** align JSON output with pmat reference implementation ([0f75b8d](https://github.com/panbanda/omen/commit/0f75b8d93c7a10906ccf2d9940ba6085b8ab0b9d))
* **deadcode:** restructure JSON output to match pmat format ([6ca4568](https://github.com/panbanda/omen/commit/6ca4568480bcf6daedc2e7efd22ed499a3a64858))
* **deadcode:** restructure JSON output to match pmat format ([b4c68b3](https://github.com/panbanda/omen/commit/b4c68b38d9b33d22fcf15374dabf27caea21ab38))
* **defect:** align JSON output with pmat format ([2d7d417](https://github.com/panbanda/omen/commit/2d7d417992a928f498808f5da8b5fcf90a570d71))
* **defect:** align JSON output with pmat format ([85f5ba3](https://github.com/panbanda/omen/commit/85f5ba398c08b979744e53fa0311297e8eb5504d))
* **duplicates:** align JSON output with pmat format ([68a17d1](https://github.com/panbanda/omen/commit/68a17d1526f3c90c9fc7d5147ffa631becf5adb4))
* **duplicates:** align JSON output with pmat format ([7c7dc2c](https://github.com/panbanda/omen/commit/7c7dc2c7a94286abd154f0e8ff967bcf9c388da3))
* **duplicates:** fix race condition in identifier canonicalization ([3af00f1](https://github.com/panbanda/omen/commit/3af00f1186fa6307235e80fffd2d4fbb4c7b3166))
* **satd:** add files_with_satd to summary for pmat compatibility ([4321269](https://github.com/panbanda/omen/commit/432126944b35c552c55988136531fb125c623973))
* **satd:** add files_with_satd to summary for pmat compatibility ([4f2cd4f](https://github.com/panbanda/omen/commit/4f2cd4fb69f8206a115f38670c6f90b56639f26a))
* **tdg:** align JSON output with pmat reference implementation ([d640a07](https://github.com/panbanda/omen/commit/d640a0724ca8eece3cada537543a0d1d7cc1aa43))
* **tdg:** align JSON output with pmat reference implementation ([ae4ad5d](https://github.com/panbanda/omen/commit/ae4ad5d9971b212a4dd6fe539adba38193dce25c))


### Performance Improvements

* **analyzer:** optimize performance and add PMAT compatibility ([38a4659](https://github.com/panbanda/omen/commit/38a46592dec0c7155946c651f530f1f42d76a032))
* **analyzer:** optimize performance and add PMAT compatibility ([4c77afd](https://github.com/panbanda/omen/commit/4c77afdb8451656997af8281022f9b17a9f248a7))

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
