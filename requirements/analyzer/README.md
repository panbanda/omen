# Analyzer Porting and Verification Plan

This directory contains individual plans for each analyzer in the Omen codebase. The goal is to ensure each analyzer:

1. Has proper academic grounding and documentation
2. Matches the reference Rust implementation (pmat) in output
3. Performs efficiently at scale

## Analyzers

| Analyzer | Status | Plan File |
|----------|--------|-----------|
| complexity | Done | [complexity.md](complexity.md) |
| churn | Done | [churn.md](churn.md) |
| deadcode | Done | [deadcode.md](deadcode.md) |
| defect | Done | [defect.md](defect.md) |
| duplicates | Done | [duplicates.md](duplicates.md) |
| graph | Done | [graph.md](graph.md) |
| halstead | Done | [halstead.md](halstead.md) |
| satd | Done | [satd.md](satd.md) |
| tdg | Done | [tdg.md](tdg.md) |

## Standard Process for Each Analyzer

### Phase 1: Research and Documentation
- [ ] Academic research on the analyzer's purpose and methodology
- [ ] Document value proposition for dev teams/legacy codebases
- [ ] Add citations to README.md (project root)
- [ ] Create requirements/analyzer/{analyzer}.md plan file

### Phase 2: Code Analysis
- [ ] Review Rust reference implementation
- [ ] Review Go implementation
- [ ] Document any logic differences
- [ ] Match model structure (JSON field names, types, order)
- [ ] Match output format (path prefixes, timestamps, etc.)

### Phase 3: Output Comparison
- [ ] Run pmat on monolith codebase: `pmat analyze {cmd} --format json`
- [ ] Run omen on monolith codebase: `./omen analyze {cmd} -f json`
- [ ] Save outputs to files for comparison
- [ ] Compare outputs and document discrepancies
- [ ] Fix any issues in Go code
- [ ] Port tests from pmat to omen

### Phase 4: Performance
- [ ] Profile with pprof: `./omen analyze {cmd} --pprof`
- [ ] Identify bottlenecks
- [ ] Optimize as needed
- [ ] Document performance characteristics

### Phase 5: PR Creation
- [ ] Create branch: `git checkout -b fix/analyzer-{name}-pmat-parity`
- [ ] Run all tests: `task test`
- [ ] Update requirements/analyzer/README.md status
- [ ] Commit with semantic message
- [ ] Create PR (don't push to origin)

## Important Notes

1. **Use asdf for pmat**: Always set Rust version before running pmat
   ```bash
   echo "rust 1.91.0" > ~/.tool-versions
   asdf reshim rust
   ```

2. **Match pmat exactly**: JSON field names, path formats, and data types must match

3. **Don't remove extra metrics**: Keep additional metrics if they're accurate, just ensure pmat's required fields are present

4. **Author names vs emails**: pmat uses author names from git, not emails

## Reference Repositories

- **pmat (Rust)**: `/reference/paiml-mcp-agent-toolkit`
- **monolith (test corpus)**: `/reference/ms-monolith`
- **pmat book**: https://paiml.github.io/pmat-book/
