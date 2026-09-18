# Task 1 report: advisory plugin update preflight

## Outcome

Implemented the captured catalog assessment and read-only comparison vertical slice. A ready import preview is now schema 2 and carries either `notInCatalog` or an `existingId` assessment cloned from the same post-picker catalog snapshot whose generation is published with the preview. Existing-ID previews are advisory only: the dialog and store both prevent commit, while cancellation continues to release the one-time token.

The frontend strictly parses the closed assessment wire shape, compares same-ID manifests without mutation, and presents metadata, command, common-order, source, management, status, and backend-authored version-precedence information. No frontend semver implementation was added.

## TDD evidence

### Backend RED/GREEN

- RED: `cargo test ... plugin::import::tests::import_result_wire_keys_match_spec` failed because the ready wire still returned schema 1 without `assessment`. GREEN: schema 2 plus literal `{"kind":"notInCatalog"}` passed.
- RED: assessment tests initially failed because `ImportAssessment::from_catalog` did not exist. GREEN: literal wire, all catalog classifications/statuses, and numeric/prerelease/build-precedence cases passed.
- RED: session preview compilation failed after tests required the catalog input. GREEN: `PrepareLease::publish` accepts the catalog slice and serializes an existing item with `samePrecedence` for `1.0.0+old` versus `1.0.0+new`.
- RED (mutation check): the runtime test failed with actual `notInCatalog` versus expected `existingId` when the runtime was temporarily wired with an empty catalog. GREEN: passing `snapshot.plugins` from the existing post-picker snapshot read made assessment and generation agree, with no prepare-time persistence or package promotion.
- GREEN: the existing `duplicate_identity_is_never_overwritten` test still passes; registry duplicate admission was not changed.

### Frontend RED/GREEN

- RED: the manifest-diff test could not resolve the new module. GREEN: six pure tests cover metadata, publisher identity, versions/build strings, v1/v2/v3 commands, action-specific params independent of object-key order, common-ID order, mismatched IDs, and non-mutation.
- RED: valid schema-2 service fixtures were rejected by the schema-1 parser. GREEN: strict exact-key assessment parsing accepts the two variants and rejects arrays, missing/extra keys, unknown relations, wrong schemas, and mismatched current/incoming IDs.
- RED: an existing-ID store preview reached commit state. GREEN: it now returns before setting committing/authority or issuing commit IPC.
- RED: the dialog lacked comparison/unchanged/version-precedence output. GREEN: it renders host-owned labels and bounded native details, disables and independently guards confirm, and labels cancel as Close.
- Real service/store/page integration initially raised `DataCloneError` because `structuredClone` received a Vue reactive proxy; the test now takes an ordinary JSON snapshot. jsdom also lacked native dialog methods, so the test installs a dialog-only polyfill.
- The lifecycle failure was not a sleep-sensitive race: direct `store.clearImportResult()` changed Pinia state while the rendered import button remained disabled until Vue's render update, so its click was correctly ignored and `importPreview` stayed undefined. The test now dismisses through the rendered control, awaits `flushPromises`, asserts the idle state, then clicks the enabled import control. This verifies the actual UI/state boundary without sleeps or weakened assertions.
- One grouped run exposed a mechanical fixture inversion (`validManifest` schema 2 and `readyImport` schema 1); restoring manifest schema 1 and ready schema 2 made the full strict service file green. Validation order remains token/generation before manifest parsing, preserving the prior unknown-result gates.

## Final verification

- Frontend focused group: `vitest run` over `pluginManifestDiff`, `pluginImportAssessment`, `pluginService`, `pluginImportDialog`, `pluginStore`, `pluginMarketplacePage`, and `pluginCommands`: **7 files, 662 tests passed**.
- Rust import/session/fixture group: `cargo test plugin::import::tests --target-dir ...`: **15 passed, 0 failed**.
- Rust pure assessment group: `cargo test plugin::import::assessment::tests --target-dir ...`: **3 passed, 0 failed**.
- Rust same-snapshot runtime test: **1 passed, 0 failed**.
- Rust unchanged duplicate rejection test: **1 passed, 0 failed**.
- `vue-tsc --noEmit`: passed.
- Scoped ESLint over all changed TS/Vue/test files: passed.
- `vite build`: passed, 4,759 modules transformed.
- Scoped `rustfmt --check` over the affected Rust modules/tests: passed.
- `git diff --check`: passed.

Rust emitted the repository's existing warning set (the run summarized 65 library warnings and 36 library-test warnings, 33 duplicated, chiefly dead code plus the Windows linker library-creation message). No warning was introduced or fixed in this task. Vite retained its existing advisory that a minified chunk exceeds 500 kB.

## Changed implementation and tests

- Backend: `src-tauri/src/plugin/import.rs`, `import/assessment.rs`, `import/session.rs`, `import/tests.rs`, `plugin/manifest.rs`, `plugin/runtime/import.rs`, `runtime/import/tests.rs`, and compile-only publish fixture updates in `runtime/removal/tests.rs`.
- Frontend: `src/types/plugin.ts`, `services/pluginService.ts`, `services/pluginManifestDiff.ts`, `stores/plugin.ts`, `components/plugins/PluginImportDialog.vue`, `PluginManifestComparison.vue`, `PluginMarketplacePage.css`, `PluginCard.vue`, and `pluginPresentation.ts`.
- Tests: new `pluginManifestDiff.test.ts` and `pluginImportAssessment.test.ts`, plus ready-preview/integration extensions in plugin service/dialog/store/marketplace/commands tests.

Supporting changes are intentionally narrow. `PluginCatalogItem::manifest()` is a crate-private read-only accessor needed by pure assessment. `pluginStatusLabel` moved into `pluginPresentation.ts` so comparison and `PluginCard` share the existing wording. The two removal tests only add the empty catalog argument required by the new `publish` signature; removal behavior is unchanged. Cargo regenerated seven permission TOMLs as line-ending-only noise; those files were explicitly restored, so there is no ACL change.

## Self-review and boundaries

- Ready preview alone moved to schema 2. Cancel and commit envelopes retain their schemas and exact keys.
- Assessment and generation are captured from one post-picker registry snapshot. No extra discovery/storage read was introduced.
- Existing-ID comparison cannot confirm in the component and cannot commit in the store. Backend duplicate rejection remains the authoritative final guard.
- The comparison is pure and preserves inputs; it rejects unrelated IDs and compares action-specific params structurally.
- No new IPC, ACL, dependency, network, authentication, path, receipt, slot, identity, storage, discovery, removal, or import-commit behavior was added.
- This is intentionally advisory: it does not overwrite, update, roll back, remove, or chain a second import. Existing catalog reconciliation may still write according to its prior contract.
- No native production launch, real AppData/keyring, account/trade call, cross-platform run, full CI run, push, merge, or release was performed.

No unresolved implementation concern remains within Task 1 scope.
