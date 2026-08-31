# Contributing to fujicli

Thank you for helping improve `fujicli`. The full contributor manual explains
the repository layout, source-of-truth boundaries, test expectations, and
hardware-evidence rules:

- [Contributor guide](docs/contributors/README.md)
- [Local CI](docs/contributors/ci.md)
- [Adding a camera](docs/contributors/adding-cameras.md)
- [Reverse-engineering workflow](docs/contributors/reversing.md)

Before opening a pull request, keep the change focused, add regression coverage
when behavior changes, run the relevant checks, and distinguish local or fixture
evidence from a physical-camera run. Never attach camera serial numbers, backup
artifacts, private RAF/JPEG files, custom-setting names, or unreviewed `-vvv`
output.

Use the repository's issue forms for bugs, feature requests, and physical-camera
compatibility evidence. Report security vulnerabilities through the
[private vulnerability-reporting form](https://github.com/po4yka/fuji-cli/security/advisories/new),
not a public issue. By participating, you agree to follow the
[Code of Conduct](CODE_OF_CONDUCT.md).

Contributions are licensed under the project's existing [MIT License](LICENSE).
