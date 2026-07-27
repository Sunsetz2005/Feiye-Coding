# Changelog

All notable Sunsetz changes are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `HostCapabilities v2` with versioned capability states and unknown-capability hiding.
- Extracted workbench shell, sidebar navigator, top bar, focus restoration, and responsive resource-pane lifecycle.
- Automated three-theme, three-viewport visual baselines and 200% geometry checks.
- Stable Istanbul coverage baseline with enforced changed-code thresholds.

### Changed

- Rebuilt the project README and repository metadata around the Sunsetz product identity.
- Removed obsolete reference-product screenshots, personal links, QR assets, and superseded planning documents.

### Fixed

- Project and task selection now use neutral states without a coral/orange selection border.
- Sidebar disclosures and task actions use native button semantics with visible keyboard focus.
- Closing a workbench pane restores focus to its trigger and unmounts hidden resource content.
