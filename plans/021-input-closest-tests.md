# Plan 021: Unit-test the input normalization and closest-match suggestion logic

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat f2a78d0..HEAD -- src/lib/input/mod.rs`
> If the in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: tests
- **Planned at**: commit `f2a78d0`, 2026-09-03

## Why this matters

`src/lib/input/mod.rs` defines how every user-typed enum value is normalized
(`CleanAlphanumeric::clean`) and how a failed parse picks a "Did you mean"
suggestion (`Choices::closest`). The generated `FromStr` of every FML enum
calls both (`crates/codegen/src/common/options/enum/mod.rs:294-305`), so
this module is on the path of every user typo, yet it has zero tests. The
suggestion threshold `SIMILARITY_THRESHOLD = 8` edit operations is generous
and unjustified by any test; today a behavior change there would be invisible.
This plan pins the current behavior with tests. It deliberately changes no
behavior — whether the threshold should shrink is a maintainer decision this
plan surfaces with evidence.

## Current state

`src/lib/input/mod.rs` (62 lines, verified in full at commit `f2a78d0`):

- Lines 6-12: the contract comment. The code generator applies the same
  normalization rule to every enum id, display name, and alias
  (`clean_input_key` in `crates/codegen/src/common/options/enum/mod.rs`),
  and the generated per-enum round-trip tests fail if the two drift. This
  comment is the design record; keep it intact.
- Lines 13-26: `CleanAlphanumeric::clean` — trim, lowercase, keep only ASCII
  alphanumeric plus `.`, `-`, `+`.
- Line 28: `const SIMILARITY_THRESHOLD: usize = 8;`
- Lines 30-52: `trait Choices` with `closest(input: &str) -> Option<String>`:
  lowercases the input, walks `Self::choices()`, keeps the choice with the
  smallest `strsim::damerau_levenshtein` distance, and returns it when the
  best distance is `<= SIMILARITY_THRESHOLD`. The comparison is strict `<`,
  so on a tie the FIRST choice encountered wins.
- Lines 55-62: blanket impl — `Choices` is implemented for every
  `IntoEnumIterator + Display` type, so `choices()` is the `Display` strings
  in declaration order.
- The consumer, generated at
  `crates/codegen/src/common/options/enum/mod.rs:298-303`: an unknown input
  produces `Unknown {Type} '{s}'. Did you mean '{best}'?` when `closest`
  returns `Some`, otherwise `Unknown {Type} '{s}'`.

Useful distance facts for the tests (all verifiable with
`strsim::damerau_levenshtein`): the edit distance between an m-character
string and a 3-character string of fully different characters is
`max(m, 3)` for m ≥ 3 with no transpositions involved, so `"zzzzzzzz"`
(8 z) vs `"abc"` is exactly 8, and `"zzzzzzzzz"` (9 z) vs `"abc"` is exactly 9.

Repo conventions that apply:

- Workspace lints deny `panic`, `unwrap_used`, `todo`, `unimplemented`
  (root `Cargo.toml` lines 15-40). Tests use `.expect("...")`.
- `strum` with the `strum_macros` feature is a production dependency
  (root `Cargo.toml` lines 94-95), so `EnumIter` and `Display` derives are
  available in test code.
- Compiler-backed Cargo commands on the managed Mac run under `build-gate`
  with at most four jobs (`AGENTS.md`); without `build-gate`, run the inner
  command directly.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Focused tests | `build-gate -- cargo test --locked -p fujicli --lib input --jobs 4` | all pass, 9+ new tests |
| Full lib tests | `build-gate -- cargo test --locked -p fujicli --lib --jobs 4` | all pass |
| Formatting | `cargo fmt --all --check` | exit 0 |
| Lint | `build-gate -- cargo clippy --locked -p fujicli --all-targets --jobs 4 -- -D warnings` | exit 0 |

## Scope

**In scope** (the only files you should modify):
- `src/lib/input/mod.rs` — append a `#[cfg(test)] mod tests` block only

**Out of scope** (do NOT touch, even though they look related):
- `SIMILARITY_THRESHOLD` and every line of production logic in the file —
  this plan adds tests only. Changing the threshold changes user-visible
  error text and needs a maintainer decision.
- `crates/codegen/src/common/options/enum/mod.rs` — the generated `FromStr`
  and `clean_input_key` stay as they are; the cross-check between
  `clean_input_key` and `CleanAlphanumeric` is already enforced by the
  generated round-trip tests.
- `fml/`, `tests/`, generated code.

## Git workflow

- Branch: `advisor/021-input-closest-tests`
- One commit: `test(input): pin normalization and closest-suggestion behavior`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add the test module

Append `#[cfg(test)] mod tests` at the end of `src/lib/input/mod.rs`.

Test-local enum (put it inside the tests module):

```rust
#[derive(Debug, PartialEq, Eq, strum_macros::Display, strum_macros::EnumIter)]
enum Probe {
    #[strum(serialize = "aaa")]
    Aaa,
    #[strum(serialize = "bbb")]
    Bbb,
    #[strum(serialize = "abcdefghij")]
    LongName,
}
```

`Choices` is implemented for it through the blanket impl at lines 55-62
(`IntoEnumIterator + Display`); no manual impl is needed.

Write these tests, each asserting the CURRENT behavior:

1. `clean_trims_lowercases_and_keeps_only_allowed_characters` —
   `"  PrOVia!! "` cleans to `"provia"`; `"x-t5.2+beta"` stays
   `"x-t5.2+beta"`; `"日本"` cleans to `""` (non-ASCII is dropped).
2. `closest_returns_the_exact_choice_ignoring_case` — `closest("AAA")` on
   the probe returns `Some("aaa")` (the returned string is the `Display`
   form, not the lowercased input).
3. `closest_suggests_within_small_edit_distance` — `closest("aab")`
   (distance 1 from `"aaa"`) returns `Some("aaa")`.
4. `closest_boundary_at_threshold_returns_some` — add a dedicated probe
   consideration here: use an input that is exactly distance 8 from every
   choice of a two-variant enum `["abc", "abcdefghij"]`. `"zzzzzzzz"` (8 z)
   is distance 8 from `"abc"`. Assert it returns `Some("abc")`.
5. `closest_one_past_threshold_returns_none` — `"zzzzzzzzz"` (9 z) is
   distance 9 from `"abc"`; with a single-variant probe enum
   `#[strum(serialize = "abc")]`, assert `closest` returns `None`.
6. `closest_tie_returns_the_first_declared_choice` — input `"ccc"` is
   distance 3 from both `"aaa"` and `"bbb"`; the strict `<` comparison at
   line 41 keeps the first, so the result is `Some("aaa")`. This pins the
   tie-break; if a maintainer later changes it, this test's name and
   assertion must be updated deliberately.
7. `closest_empty_input_still_suggests` — empty input `""` is distance 3
   from `"abc"`; it returns `Some("abc")`. This documents the practical
   effect of the generous threshold: even a fully stripped input gets a
   suggestion. Leave a `// NOTE:` comment stating this is pinned behavior,
   not an endorsement.
8. `choices_lists_display_strings_in_declaration_order` —
   `<Probe as Choices>::choices()` equals `vec!["aaa", "bbb", "abcdefghij"]`.

Style: import `CleanAlphanumeric` and `Choices` from `super`; use
`strsim::damerau_levenshtein` directly only where a test documents a
distance fact in a comment (do not re-derive distances with helper
functions — keep each expected value literal and visible).

**Verify**: `build-gate -- cargo test --locked -p fujicli --lib input --jobs 4`
→ all pass, 8 new tests.

### Step 2: Confirm the gate

**Verify**: `build-gate -- cargo test --locked -p fujicli --lib --jobs 4` →
all pass.
**Verify**: `cargo fmt --all --check` → exit 0.
**Verify**: `build-gate -- cargo clippy --locked -p fujicli --all-targets --jobs 4 -- -D warnings`
→ exit 0.

## Test plan

- 8 new tests in `src/lib/input/mod.rs` covering: normalization spec
  (trim/lowercase/filter, allowed punctuation, unicode stripping),
  exact-match suggestion, small-distance suggestion, both sides of the
  threshold boundary, first-wins tie-break, empty-input behavior, and
  choices ordering.
- Structural pattern: plain `#[test]` functions with literal expected
  values, matching the style of `src/lib/policy.rs` tests.
- Verification: `build-gate -- cargo test --locked -p fujicli --lib input --jobs 4`
  → all pass, including 8 new tests.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `build-gate -- cargo test --locked -p fujicli --lib input --jobs 4`
      exits 0 with 8 new passing tests
- [ ] `git diff src/lib/input/mod.rs` contains only additions inside a new
      `#[cfg(test)] mod tests` block — no production line changed
- [ ] `cargo fmt --all --check` exits 0
- [ ] `build-gate -- cargo clippy --locked -p fujicli --all-targets --jobs 4 -- -D warnings`
      exits 0
- [ ] `plans/README.md` status row for 021 updated

## STOP conditions

Stop and report back (do not improvise) if:

- Any of the documented behaviors differs from the excerpts — for example
  `closest` no longer uses strict `<`, the threshold constant moved, or the
  blanket impl gained another bound. Report the observed behavior; do not
  write tests against a changed implementation without instruction.
- An expected distance value does not match `strsim::damerau_levenshtein`'s
  actual result. Compute the real value, report it, and wait — the boundary
  tests are only meaningful with verified numbers.
- Making the tests pass requires touching the generated `FromStr` in
  codegen or the threshold constant.

## Maintenance notes

- If the threshold ever changes, tests 4, 5, and 7 must be re-derived from
  the new constant; they are written so the distance facts (8 z vs 9 z
  against `"abc"`) stay valid for any threshold in 3..=8.
- If `clean_input_key` in codegen ever diverges from `CleanAlphanumeric`,
  the generated round-trip tests fail first (per the contract comment at
  lines 6-12); the tests from this plan pin the runtime side of that pair.
- Maintainer decision surfaced, deliberately not executed here: is edit
  distance 8 the right suggestion threshold? Every test result for
  `closest` is within 0..=3 except the boundary pair, so 8 is far above
  anything the current enum vocabulary exercises. Shrinking it (for example
  to 4) would change user-visible error text and deserves its own change
  with `tests/cli_process.rs` coverage.
