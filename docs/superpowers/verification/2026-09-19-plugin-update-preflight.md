# Plugin update preflight verification

Local verification date: 2026-09-19 (Asia/Shanghai).

- Baseline: `main@b42f45b`, after [PR #37](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/pull/37) passed all nine checks and was merged.
- Branch: `plugin/update-preflight`.
- Initial implementation: `13404be`; identity-context review fix: `8e93d4f`; supporting author documentation and example: `e04813b`.
- Scope: [advisory comparison design](../specs/2026-09-18-plugin-update-preflight-design.md) and [implementation plan](../plans/2026-09-18-plugin-update-preflight.md). This is not an update/rollback transaction.

## Local results

| Check | Result |
| --- | --- |
| Initial related frontend group (`13404be`) | 7 files, 662 tests passed |
| Identity-context fix (`8e93d4f`), dialog + real integration | 2 files, 24 tests passed |
| Rust import/session/example parser tests | 15 passed |
| Rust pure assessment tests | 3 passed |
| Rust same post-picker snapshot test | 1 passed |
| Rust unchanged duplicate-ID rejection test | 1 passed |
| `vue-tsc --noEmit` | Passed |
| Scoped ESLint on changed TS/Vue/test files | Passed |
| Vite production build | Passed; 4,759 modules transformed |
| Scoped Rust formatting and `git diff --check` | Passed |

The local run reused existing Node dependencies and a Rust target cache. No dependency or lockfile change was required. A full local suite and repeated cross-platform runs were intentionally avoided; the existing CI workflow covers the new tests and runs the plugin security selections on Windows, Linux and macOS. The Vue-only review fix reran its 24 covering tests, typecheck, scoped ESLint and Vite build; it did not repeat the earlier 662-test group or unchanged Rust tests.

Equivalent focused commands from the feature worktree:

```text
node node_modules/vitest/vitest.mjs run tests/frontend/pluginManifestDiff.test.ts tests/frontend/pluginImportAssessment.test.ts tests/frontend/pluginService.test.ts tests/frontend/pluginImportDialog.test.ts tests/frontend/pluginStore.test.ts tests/frontend/pluginMarketplacePage.test.ts tests/frontend/pluginCommands.test.ts
node node_modules/vue-tsc/bin/vue-tsc.js --noEmit
node node_modules/vite/bin/vite.js build
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib plugin::import::tests
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib plugin::import::assessment::tests
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib plugin::runtime::import::tests::prepare_assessment_and_generation_come_from_the_same_post_picker_snapshot
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib plugin::runtime::import::tests::duplicate_identity_is_never_overwritten
git diff --check
```

## Failure-first evidence

- The ready wire test first failed on schema 1/missing assessment, then passed with schema 2 and literal closed assessment JSON.
- Assessment and session tests first required the missing API/catalog argument. Their final assertions cover numeric/prerelease/build precedence and all existing catalog classifications.
- A runtime mutation check passing an empty catalog failed the expected `existingId` assertion. Passing the existing post-picker snapshot's plugins bound the assessment to its published generation, without additional prepare-time promotion or persistence.
- The pure diff initially failed because its module did not exist. Six tests now cover metadata, publisher identity, command actions/parameters, common-command ordering, schema variants, build-string changes, mismatched IDs and non-mutation.
- Strict service, store and dialog tests first failed on the old protocol or missing guards/UI. The final integration exercises the real service/store/page, mocking only the native boundary and missing jsdom dialog APIs.
- One integration failure was traced to clearing Pinia result state before Vue had rendered the enabled import button. The test now dismisses through the rendered control, awaits the render/promise boundary, asserts idle and reopens. It does not use sleeps or relax the commit assertions.
- A grouped run exposed an inverted fixture schema; the manifest fixture was restored to schema 1 and its ready envelope to schema 2. Production validation was not weakened.

## Warnings and exclusions

Rust retained the existing dead-code warning set and the Windows linker library-creation output. Vite retained its advisory for a minified chunk larger than 500 kB. These were not hidden or treated as new feature work.

No production/native application launch, real AppData/keyring access, account/trading request, installation-package validation or release was performed. Historical Windows OS 5 behavior is not claimed fixed by this change.

The new comparison has no lifecycle writes; existing catalog initialization/reconciliation may still write under its prior contract. Storage, discovery, removal, import-commit, permissions and dependency files are unchanged. Same-ID comparison does not overwrite, update, roll back, enable or execute a plugin.

## Review and publication

Task-scoped independent review approved `13404be` with no Critical, Important or Minor findings. It verified protocol/snapshot binding, parser and submission guards, pure diff semantics, bounded plain-text presentation and the real integration test's dismissal boundary. The controller separately checked the unchanged backend duplicate-admission guard and preserved root checkout.

Whole-branch independent review identified one Important inspection-context issue: changed-only metadata hid the plugin ID and unchanged name/version/publisher values, and the unverified-publisher notice was absent when publisher identity was unchanged. The controller verified the template branches before dispatching a single fix wave.

The fix always displays the shared plugin ID and both sides' name, version, publisher and publisher ID, with unconditional author-supplied/unverified wording. Changed-only details and all submission guards remain. Before implementation, the identical-manifest and same-version command-only dialog scenarios failed on missing identity context (2 failures among 9 tests). They now pass, including literal markup-looking identity/command strings that must remain text rather than links, HTML or command controls.

Scoped independent re-review approved `8e93d4f`: the identity-context finding and its two regression scenarios are addressed, with no new Critical/Important issues or out-of-scope observations. No source change followed this approved fix; subsequent changes only record verification and handoff.

Remote checks for the new PR are not part of the local results above. The branch is prepared for PR publication, not an installation-package release or automatic merge.
