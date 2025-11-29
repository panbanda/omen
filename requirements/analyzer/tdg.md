# TDG Analyzer Plan

## Status: Done

## Academic Research

### Technical Debt Gradient

The Technical Debt Gradient (TDG) is a composite metric that quantifies accumulated technical debt in source code. It combines multiple quality dimensions into a single actionable score.

**Components (pmat weights):**
- **Complexity** (30%): Cyclomatic and cognitive complexity
- **Code Churn** (35%): Frequency of changes over time
- **Coupling** (15%): Dependencies between modules
- **Domain Risk** (10%): Critical domain areas (auth, crypto, etc.)
- **Duplication** (10%): Code duplication percentage

**Severity Thresholds:**
- Normal: TDG < 1.5
- Warning: TDG 1.5-2.5
- Critical: TDG > 2.5

### Academic References

1. Cunningham, W. (1992). "The WyCash Portfolio Management System". OOPSLA.
2. Kruchten, P., Nord, R., & Ozkaya, I. (2012). "Technical Debt: From Metaphor to Theory and Practice". IEEE Software.
3. Ernst, N., et al. (2015). "Measure It? Manage It? Ignore It? Software Practitioners and Technical Debt". FSE.

## Implementation Comparison

### pmat Structure

```rust
pub struct TDGSummary {
    pub total_files: usize,
    pub critical_files: usize,
    pub warning_files: usize,
    pub average_tdg: f64,
    pub p95_tdg: f64,
    pub p99_tdg: f64,
    pub estimated_debt_hours: f64,
}

pub struct TDGHotspot {
    pub path: String,
    pub tdg_score: f64,
    pub primary_factor: String,
    pub estimated_hours: f64,
}
```

### omen Changes

- Added `TDGReport` with pmat-compatible JSON structure
- Added `TDGSummary` with total_files, critical_files, warning_files, average_tdg, p95_tdg, p99_tdg, estimated_debt_hours
- Added `TDGHotspot` with path, tdg_score, primary_factor, estimated_hours
- Added `ToTDGReport()` conversion method on `ProjectScore`
- Converts omen's 0-100 scale (higher = better) to pmat's 0-5 scale (higher = more debt)
- Formula: `tdg = (100 - omenScore) / 20`
- Estimated hours uses pmat's formula: `2.0 * 1.8^tdg`

## Parity Checklist

- [x] Match summary fields (total_files, critical_files, warning_files)
- [x] Add average_tdg, p95_tdg, p99_tdg to summary
- [x] Add estimated_debt_hours to summary
- [x] Match hotspot structure (path, tdg_score, primary_factor, estimated_hours)
- [x] Proper severity classification (normal/warning/critical)
- [x] All tests passing
- [x] JSON output verified
