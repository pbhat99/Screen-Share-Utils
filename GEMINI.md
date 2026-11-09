# ScreenTools Development Guidelines

## 1. Versioning System (v0.0.0 SemVer)
- All builds and releases must strictly adhere to Semantic Versioning (`vMAJOR.MINOR.PATCH`):
  - **MAJOR** (`vX.0.0`): Breaking architectural rewrites or fundamental redesigns.
  - **MINOR** (`v0.X.0`): Substantial new features, new modes, or settings UI additions.
  - **PATCH** (`v0.0.X`): Bug fixes, fine-tuning, performance improvements, and UI adjustments.
- Update the `version = "X.Y.Z"` field in `Cargo.toml` whenever preparing a release build.

## 2. Changelog Maintenance (`CHANGELOG.md`)
- Maintain `CHANGELOG.md` at the project root adhering to standard "Keep a Changelog" conventions.
- Each release must be prepended under a new version heading with the current ISO date:
  ```markdown
  ## [vX.Y.Z] - YYYY-MM-DD
  ### Added
  - New features or settings
  ### Changed
  - Modified behaviors or UI updates
  ### Fixed
  - Bug fixes or performance resolutions
  ### Removed
  - Deprecated or removed capabilities
  ```
- The changelog must be kept up to date with every build deployment.

