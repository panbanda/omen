# Changelog

## [1.0.0] - 2024-12-15

### Added

- Initial release of omen-reporting plugin
- `generate-report` command for creating HTML health reports with LLM-generated insights
- 9 specialized analyst skills trained on academic research:
  - `hotspot-analyst` - Trained on Tornhill, Nagappan & Ball 2005, Graves et al. 2000
  - `complexity-analyst` - Trained on McCabe 1976, SonarSource cognitive complexity
  - `satd-analyst` - Trained on Potdar & Shihab 2014, Maldonado & Shihab 2015
  - `duplicates-analyst` - Trained on Juergens et al. 2009
  - `ownership-analyst` - Trained on Bird et al. 2011, Nagappan et al. 2008
  - `churn-analyst` - Trained on Nagappan & Ball 2005 relative churn metrics
  - `cohesion-analyst` - Trained on Chidamber & Kemerer 1994, Basili et al. 1996
  - `flags-analyst` - Feature flag lifecycle and cleanup prioritization
  - `trends-analyst` - Score trajectory and inflection point analysis
  - `components-analyst` - Per-component health trend analysis
  - `summary-analyst` - Executive summary synthesis
