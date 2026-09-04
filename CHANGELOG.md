# Changelog

All notable user-visible changes to `fujicli` are documented here. The project
uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html); while the major
version is zero, a minor release may contain breaking contract changes.

The current `main` branch is unreleased. A release exists only when an annotated
`vMAJOR.MINOR.PATCH` tag completes the protected release workflow.

## [Unreleased]

### Added

- Runtime shell-completion generation for Bash, Zsh, Fish, and PowerShell.
- User documentation organized around safe first use, tasks, reference,
  troubleshooting, and the physical-evidence safety model.

### Changed

- The repository landing page and GitHub community entry points now distinguish
  implemented behavior, authorized camera mutations, and physical-device proof.
- The simulation slot adapter now selects the custom-setting slot once per
  adapter scope instead of before and after every property, cutting a
  24-setting profile read from 25 selector writes to 1 while still verifying
  the selector after every property access.
- A simulation write whose confirmed changes are lost to a failed rollback or
  a failed selector restore now reports the settings that were actually
  written (and, on the rollback path, the settings rolled back), instead of
  leaving the operator to guess what changed on the camera.

### Fixed

- Preflight now treats only the PTP `GeneralError` (0x2002) response as a
  descriptor refusal eligible for the value-shape / static-descriptor
  fallback; a transient `DeviceBusy`, an `AccessDenied`, or an
  unsupported-property `DevicePropNotSupported` fails preflight instead of
  silently widening write authority.

[Unreleased]: https://github.com/po4yka/fuji-cli/compare/f18cbf0d9bd39a768077c9de2d2ad7dcc299e34d...HEAD
