# Series SMA executable example

This ready-to-import manifest computes a simple moving average inside the
WebAssembly guest. With values `1,2,3,4,5` and Period `3`, the result is `4`.
The host only supplies the numbers and parameter; the loop and arithmetic are
in [`plugin.wat`](plugin.wat).

The publisher fields are example metadata, not a signature or a trust claim.
Read the WAT and manifest before enabling it. Do not add secrets, account data,
or production inputs to the manifest.

## Verify or rebuild

From the repository root, with the locked Rust dependencies already available:

```powershell
node examples/plugins/series-sma/build.mjs --check
node examples/plugins/series-sma/build.mjs --write
```

`--check` exits nonzero if the canonical padded Base64 in `manifest.json` does
not exactly match compilation of `plugin.wat`. `--write` updates that one field.
Both modes use the exactly pinned `wat` development dependency through the
fixed-path Rust generator in `src-tauri/examples`; they accept no source path,
do not instantiate the module, and need no application secret.

## Try it safely

1. Open **Installed plugins** or **Plugin management** in a build that supports
   manifest v4, and import this directory's `manifest.json`.
2. Check that the preview identifies executable local code, a 264-byte module,
   the `series-f64-v1` ABI, and Period bounds 1–4096.
3. Confirm import. The plugin remains disabled; import does not execute it.
4. Enable it explicitly, open **Compute simple moving average**, enter
   `1,2,3,4,5`, leave Period at `3`, and click **Run**. The result is `4`.
5. Disable the plugin. Its computation command must become unavailable.

Inputs and results are memory-only. Fractional periods or periods larger than
the supplied series deliberately return NaN inside the guest; the host rejects
that as `plugin_compute_invalid_output`. See
[`docs/plugin-compute-runtime.md`](../../../docs/plugin-compute-runtime.md) for
the ABI, lifecycle, limits, error codes, and unsupported features.
