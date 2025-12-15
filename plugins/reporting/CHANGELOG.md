# Changelog

## [1.0.0] - 2024-12-15

### Added

- Initial release of omen-reporting plugin
- `generate-report` command for creating HTML health reports with LLM-generated insights
- 10 specialized analyst agents trained on academic research (prompts inline in generate-report.md):
  - `hotspot-analyst` - Trained on Tornhill, Nagappan & Ball 2005
  - `satd-analyst` - Trained on Potdar & Shihab 2014, Maldonado & Shihab 2015
  - `ownership-analyst` - Trained on Bird et al. 2011, Nagappan et al. 2008
  - `duplicates-analyst` - Trained on Juergens et al. 2009
  - `churn-analyst` - Trained on Nagappan & Ball 2005 relative churn metrics
  - `cohesion-analyst` - Trained on Chidamber & Kemerer 1994, Basili et al. 1996
  - `flags-analyst` - Feature flag lifecycle and cleanup prioritization
  - `trends-analyst` - Score trajectory and inflection point analysis
  - `components-analyst` - Per-component health trend analysis
  - `summary-analyst` - Executive summary synthesis (runs after all others complete)
