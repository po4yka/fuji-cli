# Versioning and compatibility

`fujicli` uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html) for
tagged releases. The project is currently in the `0.y.z` phase: a minor version
may deliberately break a public contract, while a patch version preserves it.
Every breaking change must be called out in the
[changelog](../../../CHANGELOG.md) and the release notes.

The `main` branch is development state, not a release. Documentation viewed on
`main` describes the next release and can change before a tag is published.
Release documentation is the repository content at the corresponding annotated
`vMAJOR.MINOR.PATCH` tag.

## Public contracts

The following surfaces are versioned contracts for users and automation:

- command and option names, aliases, argument grammar, defaults, and conflicts;
- success and error behavior of stdout and stderr;
- exit statuses;
- JSON field names, types, nesting, and document framing;
- the versioned backup-artifact envelope;
- mutation preflight and camera-support claims;
- packaged completion and man-page locations.

Human-readable prose and diagnostic wording can improve without a version
change unless a documented automation contract requires exact text. Generated
command syntax, JSON shapes, exit meanings, and artifact framing must not drift
silently.

## Hardware support is not SemVer compatibility

A software release, green CI run, schema entry, fixture, or trace from another
camera does not establish physical-device support. The
[camera support matrix](../support.md) records model and firmware evidence, and
the runtime preflight policy determines whether a mutation is authorized.

Adding a newly verified camera or firmware profile can occur in a compatible
release. Removing or narrowing an unsafe support claim can occur in any release
because fail-closed behavior takes precedence over compatibility.

## Release history

User-visible changes accumulate under `Unreleased` in the
[changelog](../../../CHANGELOG.md). Before tagging, maintainers move those
entries into a dated release section, update the Cargo and Nix versions, and
follow the [release procedure](../../contributors/releasing.md).
