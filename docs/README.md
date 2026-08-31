# fujicli Documentation

Start with the [first safe session](users/getting-started/first-safe-session.md),
then check the [camera support matrix](users/support.md) before any
state-changing command.

Command visibility and schema support do not establish physical-device
authorization.

## For Users

### Getting Started

- [User guide](users/usage.md) - task map and current safety boundary.
- [Installation](users/installation.md) - build methods and prerequisites.
- [First safe session](users/getting-started/first-safe-session.md) - verify the
  CLI, discover a camera, and interpret the first result.

### How-to Guides

- [Export and inspect a backup](users/how-to/export-and-inspect-backup.md)
- [Dry-run and restore a backup](users/how-to/dry-run-and-restore-backup.md)
- [Linux USB access](users/how-to/linux-usb-access.md)
- [macOS camera access](users/how-to/macos-camera-access.md)
- [Windows driver setup](users/how-to/windows-driver.md)
- [Troubleshoot device access](users/how-to/troubleshoot-device-access.md)

### Reference

- [CLI](users/reference/cli.md) - commands, aliases, and argument rules.
- [Output and JSON](users/reference/output-and-json.md) - streams, document
  framing, files, and atomic publication.
- [Exit codes](users/reference/exit-codes.md) - retry and unknown-state meaning.
- [Versioning](users/reference/versioning.md) - release compatibility contract.
- [Camera support](users/support.md) - the physical-device support matrix.

### Explanation

- [Fail-closed safety model](users/explanation/fail-closed-safety-model.md)
- [Physical-evidence model](users/explanation/physical-evidence-model.md)

## For Contributors

- [Contributing](contributors/README.md) - workflow, branches, formatting.
- [Continuous Integration](contributors/ci.md) - hosted checks and local
  reproduction.
- [Releasing](contributors/releasing.md) - tag policy, artifacts, and
  provenance.
- [Adding a Camera](contributors/adding-cameras.md) - the most common
  contribution.
- [Reversing](contributors/reversing.md) - capturing PTP traffic from Fujifilm's
  official tools.

## FML Reference

- [Overview](fml/README.md) - what FML is, how the pieces fit together.
- [Options](fml/options.md) - typed scalars (`integer`, `float`, `string`,
  `enum`) with encoding rules.
- [Cameras](fml/cameras.md) - per-model spec: USB IDs, feature blocks.
- [Generations](fml/generations.md) - shared capability presets.
- [Grammar](fml/grammar.md) - predicates and assignments.
- [Rules and Transformations](fml/rules.md) - validation, repair, aliasing
  semantics.

## Internals

- [Architecture](internals/README.md) - the pipeline at a glance.
- [Codegen](internals/codegen.md) - what each emitter produces.
- [Analyses](internals/analyses.md) - DNF, alias substitution, the presence DAG,
  repair, and inverse transformations.
- [Runtime](internals/runtime.md) - how the generated modules are consumed.

Historical implementation plans live in [`plans/`](../plans/README.md). They
record past decisions and remaining hardware-validation gaps; the user and
contributor guides above describe the current supported contract.
