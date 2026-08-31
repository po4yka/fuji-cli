# Security Policy

## Supported Versions

`fujicli` has not published binary releases yet. Security fixes are applied to
the current `main` branch only.

| Version                       | Security support |
| ----------------------------- | ---------------- |
| `main` (unreleased)           | Yes              |
| Earlier commits and forks     | No               |
| Tagged or packaged releases   | None published   |

This table will be updated when the project begins publishing releases.

## Report a Vulnerability Privately

Use GitHub's
[private vulnerability-reporting form](https://github.com/po4yka/fuji-cli/security/advisories/new).
Do not open a public issue, discussion, or pull request for a suspected
vulnerability.

Include the affected commit or build, operating system, impact, and the minimum
steps needed to reproduce the problem. Redact secrets and identifiers. Do not
attach camera serial numbers, backup artifacts, RAF/JPEG files, custom-setting
names, or unreviewed `-vvv` traces. Share sensitive artifacts only if the
maintainer requests a narrowly scoped sample in the private advisory.

The maintainer will coordinate through the private advisory. There is currently
no guaranteed response or disclosure timeline.

## Security Versus Compatibility

An unverified camera capability, unsupported model or firmware, or a command
that correctly fails closed is not by itself a security vulnerability. Report
physical-device compatibility evidence through the
[compatibility form](https://github.com/po4yka/fuji-cli/issues/new?template=compatibility_report.yml)
and reproducible software defects through the
[bug form](https://github.com/po4yka/fuji-cli/issues/new?template=bug_report.yml).

If a camera command bypasses a safety preflight, mutates state outside the
documented boundary, exposes private camera data, or permits unintended code
execution, report it privately as a vulnerability.
