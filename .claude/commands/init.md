# Omen Init

Initialize an omen.toml configuration file for this repository.

## Steps

1. Check if `omen.toml` or `.omen/omen.toml` already exists. If so, ask the user if they want to overwrite it.

2. Analyze the repository to detect:
   - Primary languages used (by file extension count)
   - Feature flag providers in use

3. For feature flag provider detection, search for these patterns:
   - **LaunchDarkly**: `boolVariation`, `stringVariation`, `variation` function calls, or `launchdarkly` imports/dependencies
   - **Flipper** (Ruby): `Flipper.enabled?`, `Flipper[`
   - **Split**: `getTreatment`, `get_treatment`
   - **Unleash**: `isEnabled`, `is_enabled` with unleash imports
   - **Generic**: `feature_flag`, `featureFlag`, `is_feature_enabled`, `isFeatureEnabled`
   - **ENV-based**: `ENV["FEATURE_*"]`, `process.env.FEATURE_*`, `os.environ["FEATURE_*"]`

4. Check package files for feature flag dependencies:
   - `package.json`: `launchdarkly-*`, `@split*`, `unleash-*`
   - `Gemfile`/`*.gemspec`: `launchdarkly-*`, `flipper`, `split-*`, `unleash`
   - `Cargo.toml`: `launchdarkly`, `unleash`
   - `go.mod`: `launchdarkly`, `unleash`
   - `requirements.txt`/`pyproject.toml`: `launchdarkly-*`, `split-*`, `unleashclient`

5. Generate the config with:
   - Detected feature flag providers in `feature_flags.providers`
   - If no providers detected, leave `providers = []` with a comment explaining how to add them
   - Sensible defaults for all other settings

6. Write the config to `omen.toml` (or `.omen/omen.toml` if the user prefers)

7. Show the user what was detected and created

## Example Output

```
Analyzing repository...

Detected languages: TypeScript, JavaScript, Python
Detected feature flag providers: launchdarkly

Created omen.toml with:
  - Feature flag providers: ["launchdarkly"]
  - Stale flag threshold: 90 days
  - Default complexity thresholds
  - Standard exclusion patterns

Run `omen flags` to detect feature flags in your codebase.
```
