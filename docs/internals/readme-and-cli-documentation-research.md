# README and CLI documentation practices for `fujicli`

Research date: 2026-08-31

## Scope and method

This report asks two questions:

1. What information architecture and presentation practices are supported by
   GitHub's own documentation?
2. Which of those practices recur in maintained, successful CLI projects, and
   which ones fit `fujicli`'s current safety and release model?

The evidence is limited to primary sources: GitHub Docs, Diataxis, the official
source and documentation of GitHub CLI, ripgrep, uv, and clap, plus this
repository. The cross-project conclusions below are explicitly identified as
inferences; the example projects are evidence of workable patterns, not
universal rules.

## Executive conclusion

The current [`README.md`](../../README.md) already has the right overall shape:
it identifies the tool, puts the camera-write boundary before installation,
offers a short read-only start, and routes detailed material into the
[`docs/`](../README.md) hierarchy. GitHub describes the README as the page that
usually introduces what a project does, why it is useful, how to start, where
to get help, and who maintains it; it does not require the README to be the
whole manual ([GitHub, "About the repository README file"](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes)).

The best next improvement is therefore not a larger README. It is a sharper
landing page that makes the first safe success path and the implemented versus
authorized capability boundary scannable, followed by GitHub-recognized
community files and a more explicit CLI documentation contract. This mirrors
the separation visible in GitHub CLI (short README, installation links, hosted
manual), ripgrep (README, user guide, FAQ), and uv (README examples, dedicated
installation and task documentation) ([GitHub CLI README](https://github.com/cli/cli/blob/trunk/README.md),
[ripgrep README](https://github.com/BurntSushi/ripgrep/blob/master/README.md),
[ripgrep guide](https://github.com/BurntSushi/ripgrep/blob/master/GUIDE.md),
[uv README](https://github.com/astral-sh/uv/blob/main/README.md),
[uv installation guide](https://github.com/astral-sh/uv/blob/main/docs/getting-started/installation.md)).

## Evidence-derived practices

### 1. Treat the README as a landing page and router

GitHub says a README is often the first item a visitor sees and names five
typical questions it should answer: what the project does, why it is useful,
how to start, where to get help, and who maintains or contributes to it
([GitHub README documentation](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes)).
GitHub also recommends a README for every repository so people can understand
and navigate the work ([GitHub repository best practices](https://docs.github.com/en/repositories/creating-and-managing-repositories/best-practices-for-repositories)).

The CLI exemplars keep that entry path compact while linking to deeper layers:

- GitHub CLI gives a one-sentence value proposition, platform scope, a manual
  link, contribution route, and per-platform installation routes
  ([GitHub CLI README](https://github.com/cli/cli/blob/trunk/README.md)).
- ripgrep begins with behavior and defaults, then exposes quick links to its
  installation, user guide, FAQ, configuration, completions, and build
  documentation ([ripgrep README](https://github.com/BurntSushi/ripgrep/blob/master/README.md)).
- uv puts the shortest installation path and representative workflows in the
  README, while routing each workflow to a dedicated guide and keeping detailed
  installation alternatives elsewhere ([uv README](https://github.com/astral-sh/uv/blob/main/README.md),
  [uv installation guide](https://github.com/astral-sh/uv/blob/main/docs/getting-started/installation.md)).

**Inference:** a CLI README should optimize time-to-orientation and
time-to-first-success. Full command catalogs, platform edge cases, protocol
internals, and contributor procedures belong in linked documents unless they
are required to execute the first safe workflow.

### 2. Put an executable, observable path near the top

The example projects show commands rather than merely enumerate features.
GitHub CLI links straight to installation and its manual, ripgrep follows its
overview with quick examples, and uv pairs each major feature with a terminal
transcript and a next-step guide ([GitHub CLI README](https://github.com/cli/cli/blob/trunk/README.md),
[ripgrep README](https://github.com/BurntSushi/ripgrep/blob/master/README.md),
[uv README](https://github.com/astral-sh/uv/blob/main/README.md)).

**Inference:** the first example should demonstrate a meaningful result with
the fewest prerequisites, state the expected observable outcome, and avoid a
destructive operation. For hardware CLIs, it should also say what happens when
no device is connected, because that is a likely first-run state.

GitHub's own style guide says not to use screenshots of command-line interfaces
to convey commands and output; it recommends providing the commands directly
instead ([GitHub Docs style guide, "Images"](https://docs.github.com/en/contributing/style-guide-and-content-model/style-guide#images)).
For `fujicli`, copyable text transcripts are therefore preferable to terminal
screenshots. A product image or protocol diagram should be added only if it
communicates information that the surrounding text cannot.

### 3. Make status and limitations as prominent as capability claims

GitHub's README checklist is intentionally about communicating expectations,
not only marketing the project ([GitHub README documentation](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes)).
The exemplar projects qualify important scope directly: GitHub CLI names its
supported GitHub products and operating systems, ripgrep states its default
filtering behavior and platform support, and uv links its production and
platform policies from its README FAQ ([GitHub CLI README](https://github.com/cli/cli/blob/trunk/README.md),
[ripgrep README](https://github.com/BurntSushi/ripgrep/blob/master/README.md),
[uv README](https://github.com/astral-sh/uv/blob/main/README.md)).

**Inference:** for a device-mutating tool, a capability statement is incomplete
unless readers can also find the support, firmware, and safety boundary without
leaving the first screen. This is more important than a feature collage or a
large badge row.

### 4. Use badges as live signals, not decoration

GitHub documents workflow badges as indicators of whether a workflow is
passing or failing and notes that they show the default branch by default;
branch and event filters can make the represented state explicit
([GitHub, "Adding a workflow status badge"](https://docs.github.com/en/actions/how-tos/monitor-workflows/add-a-status-badge)).
Both ripgrep and uv use a small number of badges that link to concrete state,
such as CI or a published package ([ripgrep README](https://github.com/BurntSushi/ripgrep/blob/master/README.md),
[uv README](https://github.com/astral-sh/uv/blob/main/README.md)).

**Inference:** add only signals a visitor can act on. For the current
unreleased `fujicli`, default-branch CI and perhaps the security workflow are
useful; version, downloads, coverage, and package-manager badges would imply
distribution channels or quality claims that do not currently exist. A green
workflow badge must never be described as camera compatibility or hardware
verification.

### 5. Keep repository-local navigation clone-safe and accessible

GitHub recommends relative links for files and images inside a repository
because GitHub resolves them for the current branch and they continue to work
in local clones. The same documentation defines image alt text as a textual
equivalent of the visual information ([GitHub writing and formatting syntax](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax#relative-links)).
GitHub automatically derives a file outline from headings, so a short README
does not need a hand-maintained table of contents ([GitHub README documentation](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes#auto-generated-table-of-contents-for-readme-files)).

GitHub's content style guide additionally requires a logical heading hierarchy,
unique peer headings, meaningful links, and textual equivalents for images
([GitHub Docs style guide](https://docs.github.com/en/contributing/style-guide-and-content-model/style-guide)).
Those are sound repository-documentation defaults even though that style guide
is written for GitHub's own documentation.

### 6. Separate README, task guides, command reference, and help

The example projects converge on multiple documentation surfaces:

- GitHub CLI directs usage to a hosted manual while keeping installation and
  contribution routing in the repository README
  ([GitHub CLI README](https://github.com/cli/cli/blob/trunk/README.md)).
- ripgrep keeps a narrative user guide and a separate FAQ, both linked from
  the README ([ripgrep README](https://github.com/BurntSushi/ripgrep/blob/master/README.md),
  [ripgrep guide](https://github.com/BurntSushi/ripgrep/blob/master/GUIDE.md),
  [ripgrep FAQ](https://github.com/BurntSushi/ripgrep/blob/master/FAQ.md)).
- uv keeps installation, task guides, concepts, and references in a documented
  navigation tree while exposing `uv help` as command-line reference
  ([uv README](https://github.com/astral-sh/uv/blob/main/README.md),
  [uv documentation navigation](https://github.com/astral-sh/uv/blob/main/mkdocs.yml)).

**Inference:** these surfaces answer different questions and should not be
forced into one file:

| Surface | Primary question | Appropriate content |
| --- | --- | --- |
| README | Should I use this, and what is the first safe step? | Promise, status, quick start, key routes |
| Installation | How do I obtain and verify it on my platform? | Supported methods, prerequisites, verification, uninstall |
| Task guide | How do I complete an outcome? | Ordered workflows, examples, recovery and failure paths |
| Command reference | What does every command and option mean? | Generated syntax, arguments, defaults, exit behavior |
| Troubleshooting / FAQ | Why did an expected workflow fail? | Symptom-to-cause diagnostics and recovery |
| Contributor docs | How do I change and validate it? | Architecture, source-of-truth routing, gates, releases |

### 7. Organize longer documentation by user need

Diataxis distinguishes four documentation needs: tutorials provide a guided
learning experience; how-to guides help an already-oriented reader achieve a
goal; reference describes the interface accurately and systematically; and
explanation provides context and understanding
([Diataxis overview](https://diataxis.fr/),
[tutorials](https://diataxis.fr/tutorials/),
[how-to guides](https://diataxis.fr/how-to-guides/),
[reference](https://diataxis.fr/reference/),
[explanation](https://diataxis.fr/explanation/)). uv's maintained navigation
uses a compatible split between getting started, guides, concepts, and
reference ([uv documentation navigation](https://github.com/astral-sh/uv/blob/main/mkdocs.yml)).

**Inference:** classify a page by the reader's immediate need, not merely by
the internal command group. A safe first-camera session is a tutorial; a
backup export or restore is a goal-oriented how-to; flags, JSON, and exit codes
are reference; and the fail-closed evidence model is explanation. A page may
link across these layers, but should not try to perform all four jobs.

### 8. Derive command documentation from the command model

clap supports separate concise and long descriptions: `-h` uses short help and
`--help` uses long help. Its derive interface can populate both from source doc
comments ([clap derive reference](https://docs.rs/clap/latest/clap/_derive/#doc-comments)).
The official companion crates generate shell completions and man pages from a
`clap::Command` model ([clap_complete](https://docs.rs/clap_complete/latest/clap_complete/),
[clap_mangen](https://docs.rs/clap_mangen/latest/clap_mangen/struct.Man.html)).
clap's own examples require narrative Markdown examples to be verified with
`trycmd`, demonstrating a first-party docs-as-tests pattern
([clap examples contributor documentation](https://github.com/clap-rs/clap/blob/master/examples/README.md)).

**Inference:** syntax, option names, defaults, aliases, and command hierarchy
should have one source of truth in the CLI model. Narrative docs should explain
intent, workflow, and safety and link to generated reference rather than copy a
large help tree. Any intentionally copied help or transcript should be checked
for drift in CI.

### 9. Use GitHub-recognized community files for contribution and support routes

GitHub's community profile checks for files including README, LICENSE,
CONTRIBUTING, and CODE_OF_CONDUCT in supported locations
([GitHub community profiles](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/about-community-profiles-for-public-repositories)).
When `CONTRIBUTING.md` is present in the root, `.github`, or `docs`, GitHub
surfaces it while users open issues and pull requests and in the repository's
Contributing tab ([GitHub contribution guidelines](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/setting-guidelines-for-repository-contributors)).

Likewise, `SUPPORT.md` is surfaced when someone creates an issue, and GitHub
recommends linking it from the README
([GitHub support resources](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/adding-support-resources-to-your-project)).
A `SECURITY.md` should state supported versions and how to report a
vulnerability ([GitHub security policy documentation](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/add-security-policy)).
GitHub's private vulnerability reporting gives reporters a structured private
channel when enabled ([GitHub private vulnerability reporting](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/configure-for-a-repository)).

These files are navigation and expectation contracts, not README duplication.
A short recognized file may route to a deeper existing guide.

## Current `fujicli` documentation audit

### What is already strong

- The root [`README.md`](../../README.md) answers what the project is, provides
  a quick start, exposes the experimental/unreleased state, routes user and
  contributor documentation, and links the MIT license. This covers most of
  GitHub's typical README questions
  ([GitHub README documentation](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes)).
- The camera-write warning appears before Quick Start and links directly to the
  [`support matrix`](../users/support.md). That placement matches the risk of a
  CLI that can mutate physical-device state.
- The README's first run is `device list`, and it explicitly recommends
  read-only discovery before state-changing workflows
  ([current README](../../README.md)).
- [`docs/README.md`](../README.md) already provides audience-based navigation
  for users, contributors, schema authors, and runtime maintainers. The
  repository therefore has a clear documentation hub without requiring a
  hosted site.
- Installation, usage, support, contributor workflow, CI, and release
  provenance are already separate documents
  ([installation](../users/installation.md),
  [usage](../users/usage.md), [support](../users/support.md),
  [contributing](../contributors/README.md),
  [CI](../contributors/ci.md), [releasing](../contributors/releasing.md)).
- Short and long clap help, deterministic completion files, and generated man
  pages already follow the single-command-model direction supported by clap
  ([usage](../users/usage.md), [releasing](../contributors/releasing.md),
  [clap derive reference](https://docs.rs/clap/latest/clap/_derive/#doc-comments)).
- Markdown syntax and repository-local anchors are validated in CI with
  markdownlint and offline Lychee checks
  ([local CI documentation](../contributors/ci.md)).

### Gaps and ambiguity

1. **The opening capability sentence is broader than the current authorization
   state.** The README says the tool provides simulation profiles and in-camera
   RAW conversion workflows, while the immediately following warning says X-T5
   simulation and RAW conversion are disabled. Both statements can be true at
   the implementation level, but a first-time reader must resolve the apparent
   contradiction by reading a dense warning and support page
   ([current README](../../README.md), [support matrix](../users/support.md)).

2. **The first run has no stated expected outcome or no-camera outcome.** The
   README shows `device list` and `device info`, but does not tell a new user
   what success looks like or how a missing/busy camera is reported
   ([current README](../../README.md)). The detailed installation guide contains
   platform-specific ownership diagnostics, but they are not routed from the
   first-run block ([installation guide](../users/installation.md)).

3. **No GitHub-recognized contribution, support, or security file exists.** The
   repository has a detailed `docs/contributors/README.md`, but it is not named
   `CONTRIBUTING.md`; there is no `SUPPORT.md`, `SECURITY.md`, or
   `CODE_OF_CONDUCT.md` in a supported location. GitHub therefore cannot expose
   the existing contributor guide through the native Contributing affordances
   described in its documentation
   ([GitHub contribution guidelines](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/setting-guidelines-for-repository-contributors),
   [GitHub community profiles](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/about-community-profiles-for-public-repositories)).

   A live community-profile query on 2026-08-31 reported 42% and recognized
   only README and LICENSE among the relevant community files
   ([GitHub community profile API](https://api.github.com/repos/po4yka/fuji-cli/community/profile)).

4. **The README exposes no live repository-health signal.** CI and scheduled
   security workflows exist, but readers must navigate to Actions to discover
   their state ([CI documentation](../contributors/ci.md)). GitHub supports
   default-branch workflow badges for exactly this signal
   ([GitHub workflow badges](https://docs.github.com/en/actions/how-tos/monitor-workflows/add-a-status-badge)).

5. **Installation and troubleshooting are interleaved.** The installation
   guide contains valuable Linux ACL policy, macOS process ownership, Windows
   driver setup, diagnosis, migration, and revocation. Its depth is appropriate
   for safe USB access, but it makes the ordinary install/verify path harder to
   scan ([installation guide](../users/installation.md)).

6. **The user guide mixes several reader intents.** Its command walkthroughs,
   safety rationale, JSON/stdout/stderr contract, exit-code reference, and
   recovery guidance are all useful, but a reader must traverse one long page
   to find each kind of answer ([usage guide](../users/usage.md)).

7. **The narrative usage guide begins with a copied help block.** The repository
   already generates man pages and completion assets from clap and checks them
   byte-for-byte, but the top-level help copy in the usage guide is an
   additional surface that can drift unless a test covers that exact excerpt
   ([usage guide](../users/usage.md),
   [release documentation](../contributors/releasing.md)).

8. **There is no release-history route yet.** That is acceptable while there
   are no published binaries, but once releases begin, users need a stable way
   to map versions to behavior changes. ripgrep links a changelog from its
   README, while GitHub CLI and uv route users to maintained release/manual
   surfaces ([ripgrep README](https://github.com/BurntSushi/ripgrep/blob/master/README.md),
   [GitHub CLI README](https://github.com/cli/cli/blob/trunk/README.md),
   [uv README](https://github.com/astral-sh/uv/blob/main/README.md)).

## Recommendations for `fujicli`

### P0: clarify the README contract before adding visual polish

Keep the existing short structure, but revise the opening into three explicit
layers:

1. one sentence describing the user problem and transport;
2. a compact "Current status" block that distinguishes implemented behavior,
   currently authorized writes, and physically verified support;
3. the existing safety callout, shortened to the decision a new user must make
   and linked to the full matrix.

A suitable information shape is:

```text
fujicli is an experimental USB/PTP CLI for inspecting Fujifilm cameras and
working with camera-native settings and RAW workflows.

Current production boundary:
- read-only discovery: schema support exists, but physical coverage and USB
  mode requirements vary by model; consult the support matrix;
- backup restore: authorized only for X-T5 firmware 4.31 after preflight;
- X-T5 simulations and RAW conversion: disabled pending physical wire proof.
```

The exact wording must remain synchronized with
[`docs/users/support.md`](../users/support.md); do not duplicate the full model
matrix. This resolves the current implemented-versus-authorized ambiguity while
preserving the risk-first ordering supported by GitHub's expectation-setting
role for READMEs
([GitHub README documentation](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes)).

### P0: make the first safe success path observable

Retain a read-only first run, then add:

- `fujicli --help` as a hardware-independent verification;
- `fujicli device list` as the first hardware check;
- one sentence describing the successful output at a semantic level;
- one direct troubleshooting route for "no device", permission, driver, or
  busy-interface failures.

Do not place backup import, simulation mutation, emulation, or reverse tooling
in the README quick start. Use copyable text, not a terminal screenshot, in
line with GitHub's CLI-image guidance
([GitHub Docs style guide](https://docs.github.com/en/contributing/style-guide-and-content-model/style-guide#images)).
Only add an exact output transcript after it is captured from an authorized,
privacy-reviewed run or generated by a deterministic no-hardware test; label
fixture output as fixture output rather than device evidence.

### P0: add native GitHub contribution and security routes

Add a root or `.github/CONTRIBUTING.md` that briefly routes readers to the
existing [`docs/contributors/README.md`](../contributors/README.md). This gains
GitHub's native discovery without maintaining a second contributor manual
([GitHub contribution guidelines](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/setting-guidelines-for-repository-contributors)).

Add `SECURITY.md` with:

- supported release/version policy;
- a private reporting route;
- what not to include publicly, especially serials, backup artifacts, RAF/JPEG
  files, and unreviewed verbose traces;
- an explicit distinction between a software vulnerability and an unverified
  camera capability.

Enable GitHub private vulnerability reporting if the maintainer can monitor and
respond to it. GitHub recommends supported versions and reporting instructions
in the policy and provides a structured private channel when enabled
([GitHub security policy](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/add-security-policy),
[private vulnerability reporting](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/configure-for-a-repository)).

### P1: expose only truthful live status

Add one default-branch `CI` badge linked to `.github/workflows/ci.yml`; consider
one `Security` badge only if its scope is clear in alt text and link target.
Specify `branch=main` and, if appropriate, an event filter so the displayed
state matches the intended claim
([GitHub workflow badge documentation](https://docs.github.com/en/actions/how-tos/monitor-workflows/add-a-status-badge)).

Do not add package version, download, hardware-support, or coverage badges until
there is a maintained source for each claim. Keep a short text sentence saying
there are no published binaries; that is more truthful than an empty release
badge. Alt text should describe the signal, not say only "badge"
([GitHub writing and formatting syntax](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax#images)).

### P1: create an issue-routing contract

Add `SUPPORT.md` that routes:

- security reports to the private security path;
- compatibility evidence to the existing reporting contract in
  [`support.md`](../users/support.md#reporting-compatibility);
- installation and USB ownership problems to troubleshooting;
- feature requests and reproducible software defects to their corresponding
  issue forms.

GitHub surfaces `SUPPORT.md` in the issue flow and recommends linking it from
the README ([GitHub support resources](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/adding-support-resources-to-your-project)).
Add issue forms only after these categories and required privacy fields are
settled; a generic template that encourages raw `-vvv` dumps would conflict
with the repository's privacy guidance.

Adopt `CODE_OF_CONDUCT.md` only if the maintainer is prepared to enforce it;
GitHub explicitly recommends considering enforcement capacity before adopting
one ([GitHub code-of-conduct guidance](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/adding-a-code-of-conduct-to-your-project)).

### P1: sharpen the documentation layers

Keep [`docs/README.md`](../README.md) as the canonical documentation index, but
make these boundaries explicit in future edits:

- getting started: one guided, hardware-safe first session with expected
  outcomes;
- how-to guides: export and inspect a backup, dry-run and restore a backup, and
  resolve platform-specific USB access;
- reference: generated command syntax plus JSON, stdout/stderr, exit codes, and
  artifact contracts;
- explanation: fail-closed policy, physical-evidence model, integrity versus
  authenticity, and unknown-camera-state semantics;
- troubleshooting: Linux ACL/rule diagnosis, macOS `ptpcamerad`, Windows driver
  recovery, busy-device diagnosis, and revocation;
- `support.md`: model/firmware evidence and mutation authorization.

This applies the Diataxis distinction incrementally; it does not require
renaming every existing directory. The highest-value first split is to move
platform diagnosis out of `installation.md` and process/reference material out
of the outcome-oriented parts of `usage.md`
([Diataxis overview](https://diataxis.fr/)).

This is an incremental split, not a documentation-site migration. File-based
GitHub docs are adequate while the index stays navigable and CI checks links.
The exemplar projects justify a hosted site only when cross-document search,
larger navigation, or versioned reference materially improves the user journey:
GitHub CLI uses a manual site, uv uses a full documentation tree, and ripgrep
remains effective with repository Markdown
([GitHub CLI README](https://github.com/cli/cli/blob/trunk/README.md),
[uv documentation navigation](https://github.com/astral-sh/uv/blob/main/mkdocs.yml),
[ripgrep README](https://github.com/BurntSushi/ripgrep/blob/master/README.md)).

### P1: enforce one command-reference source

Continue generating completions and section 1 man pages from the production
clap command model. Extend the existing asset parity test or a focused docs test
to cover any help excerpt intentionally retained in Markdown. clap's short and
long help model already fits `fujicli`'s compact `-h` and schema-rich `--help`
contract ([clap derive reference](https://docs.rs/clap/latest/clap/_derive/#doc-comments)).

For new narrative examples:

- test no-hardware commands and error paths directly;
- validate hardware command grammar without representing fixture execution as
  a camera run;
- keep expected output semantic unless exact bytes/text are part of the public
  contract;
- regenerate command-derived assets in the same change as CLI grammar.

clap's own verified Markdown examples provide a first-party precedent for
testing executable documentation
([clap examples documentation](https://github.com/clap-rs/clap/blob/master/examples/README.md)).

Expand the generated man-page contract beyond the top-level clap summary when
the required information has a single maintained source. Useful sections are
`EXIT STATUS`, `OUTPUT`, `ENVIRONMENT`, `FILES`, `SAFETY`, and `SEE ALSO`.
In particular, exit status `3` should preserve the operational warning that
camera state is unknown and the mutation must not be retried automatically.

Consider a future `fujicli completion <shell>` command if source and
`cargo install` users need completions without separately copying
`assets/share/`. This is a distribution improvement, not a README requirement;
the current ahead-of-time assets remain valid and are generated from the same
clap model
([clap completion generator documentation](https://docs.rs/clap_complete/latest/clap_complete/)).

### P2: prepare release-facing documentation when releases exist

When the first public binary release is published:

- change Quick Start to prefer an immutable release or package-manager path
  over the moving `main` flake reference;
- link artifact verification and provenance from installation, not only from
  contributor release documentation;
- add a changelog or a clearly maintained release-history route;
- state which documentation applies to the current development branch versus
  the latest release.

GitHub CLI's README makes binary verification discoverable from installation,
and ripgrep exposes its changelog from the README
([GitHub CLI README](https://github.com/cli/cli/blob/trunk/README.md),
[ripgrep README](https://github.com/BurntSushi/ripgrep/blob/master/README.md)).
Until then, the existing "no published binary releases" statement should stay
prominent.

### P2: improve repository discoverability outside the README

GitHub topics help people find and classify projects and should describe the
project's purpose, subject area, community, or language
([GitHub topics documentation](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/classifying-your-repository-with-topics)).
Keep the GitHub About description, Cargo package description, and README opening
semantically aligned, while allowing each surface its appropriate length. Use
topics such as `fujifilm`, `camera-control`, `ptp`, `usb`, `rust`, and
`command-line` only where they accurately describe current scope; topics are
classification, not feature-support claims.

## Proposed root README outline

The target should remain roughly the current length, with detailed material
linked rather than duplicated:

```text
# fujicli

One-sentence user value and transport scope
CI badge [and security badge if scoped]

Current status: implemented / authorized / physically verified boundary
Safety alert linking support matrix

## Quick Start
No published binaries statement
Shortest install path
fujicli --help
fujicli device list
Expected semantic outcome and troubleshooting link

## Documentation
Install | Use | Troubleshoot | Support | Contribute | Architecture

## Development
One-paragraph source-of-truth and dev-shell orientation

## Getting Help / Security
SUPPORT.md | SECURITY.md

## License
MIT
```

GitHub already provides an automatic outline for headings, so no manual table
of contents is needed at this size
([GitHub README documentation](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes#auto-generated-table-of-contents-for-readme-files)).

## Validation checklist for a documentation change

1. Render the README on GitHub or an equivalent GFM preview and inspect both
   narrow and wide layouts.
2. Run the repository's documented Markdown and internal-link checks
   ([CI guide](../contributors/ci.md)).
3. Execute every new hardware-independent command exactly as written.
4. For hardware examples, record model, firmware, host, USB mode, command, and
   observed outcome separately from fixture and CI evidence
   ([support matrix](../users/support.md)).
5. Compare copied CLI syntax against `fujicli -h`, `fujicli --help`, and the
   generated man pages; regenerate command assets when the clap model changes
   ([release guide](../contributors/releasing.md)).
6. Check that every image has meaningful alt text and that no CLI command is
   communicated only through an image
   ([GitHub Docs style guide](https://docs.github.com/en/contributing/style-guide-and-content-model/style-guide#images)).
7. Re-read the opening as three audiences: a new user, a camera owner evaluating
   mutation risk, and a contributor looking for the source-of-truth layer.
8. Confirm that CI, a fixture, another model, or an old trace is not described
   as current physical-camera verification.

## Primary sources

- [GitHub: About the repository README file](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes)
- [GitHub: Best practices for repositories](https://docs.github.com/en/repositories/creating-and-managing-repositories/best-practices-for-repositories)
- [GitHub: Basic writing and formatting syntax](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax)
- [GitHub: Style guide and content model](https://docs.github.com/en/contributing/style-guide-and-content-model/style-guide)
- [GitHub: About community profiles](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/about-community-profiles-for-public-repositories)
- [GitHub: Setting contributor guidelines](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/setting-guidelines-for-repository-contributors)
- [GitHub: Adding support resources](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/adding-support-resources-to-your-project)
- [GitHub: Adding a security policy](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/add-security-policy)
- [GitHub: Configuring private vulnerability reporting](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/configure-for-a-repository)
- [GitHub: Adding a workflow status badge](https://docs.github.com/en/actions/how-tos/monitor-workflows/add-a-status-badge)
- [Diataxis documentation system](https://diataxis.fr/)
- [GitHub CLI README](https://github.com/cli/cli/blob/trunk/README.md)
- [ripgrep README](https://github.com/BurntSushi/ripgrep/blob/master/README.md)
- [ripgrep user guide](https://github.com/BurntSushi/ripgrep/blob/master/GUIDE.md)
- [ripgrep FAQ](https://github.com/BurntSushi/ripgrep/blob/master/FAQ.md)
- [uv README](https://github.com/astral-sh/uv/blob/main/README.md)
- [uv installation guide](https://github.com/astral-sh/uv/blob/main/docs/getting-started/installation.md)
- [uv documentation navigation](https://github.com/astral-sh/uv/blob/main/mkdocs.yml)
- [clap derive documentation](https://docs.rs/clap/latest/clap/_derive/#doc-comments)
- [clap completion generator documentation](https://docs.rs/clap_complete/latest/clap_complete/)
- [clap man-page generator documentation](https://docs.rs/clap_mangen/latest/clap_mangen/struct.Man.html)
- [clap verified example documentation](https://github.com/clap-rs/clap/blob/master/examples/README.md)
