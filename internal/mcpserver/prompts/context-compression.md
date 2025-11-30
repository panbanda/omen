# Context Compression

Generate a compressed context summary of this codebase that fits within LLM context windows while preserving the most important architectural information.

## Instructions

1. Use `analyze_repo_map` with `top: {{top}}` to get PageRank-ranked symbols
2. Use `analyze_graph` with `scope: "module"` and `include_metrics: true` for dependency structure
3. Combine into a summary showing:
   - Top symbols by importance (highest PageRank)
   - Module dependency relationships
   - Key entry points and hub files

## Output Format

Provide a structured summary with:
- **Core Symbols**: The {{top}} most important functions/types by PageRank
- **Architecture**: High-level module dependencies as a simplified graph
- **Entry Points**: Files with highest in-degree (most depended upon)
- **Recommendations**: Which files to read first for understanding the codebase
