# News Center deployment

News Center uses one release-bound TG-forwarder source and an independent
system Keyring credential. The API endpoint is fixed at build time; operators
provision only the bearer credential after installing the matching desktop
release. A missing or unusable news credential degrades News Center only.
Trading, account, chart, and other desktop functions continue to start and
operate, and already cached news remains readable.

## Fixed-source release variables

Configure these protected, non-secret GitHub repository variables before
creating a release:

- `EASIFLUX_NEWS_API_BASE_URL`: the one approved HTTPS TG-forwarder base URL.
- `EASIFLUX_NEWS_SOURCE_EPOCH`: a non-empty namespace version for that endpoint
  and delivery stream.

Both release callers pass these values to the reusable build. They are scoped
to the two compilation steps and are embedded in the desktop and provisioning
binary. A release build rejects a missing value, an empty epoch, or an invalid
or non-HTTPS endpoint. Never store a news bearer token in GitHub repository
variables, Actions secrets used as build inputs, workflow arguments, artifacts,
or build logs.

The release owner is responsible for epoch changes. Changing the endpoint or
its delivery/database namespace requires incrementing
`EASIFLUX_NEWS_SOURCE_EPOCH` and approving an archive or migration path before
deployment. The application does not automatically reset a database or merge
histories from different source namespaces.

Each platform job publishes two additional assets to the same release tag:

- `easiflux-news-provision-<host-triple>[.exe]`
- `easiflux-news-provision-<host-triple>[.exe].sha256`

Select the utility whose host triple matches the target computer. Release jobs
do not overwrite an asset with the same name; a collision fails the job and
must be investigated instead of silently replacing a published binary.

## Verify the provisioning utility

Download the utility and its sibling checksum from the same release. Verify it
before execution from the directory containing both files.

Windows PowerShell:

```powershell
$expected = (Get-Content .\easiflux-news-provision-<host-triple>.exe.sha256).Split()[0]
$actual = (Get-FileHash -Algorithm SHA256 .\easiflux-news-provision-<host-triple>.exe).Hash
if ($actual.ToLowerInvariant() -ne $expected.ToLowerInvariant()) { throw 'checksum mismatch' }
```

macOS:

```bash
shasum -a 256 -c easiflux-news-provision-<host-triple>.sha256
```

Linux:

```bash
sha256sum -c easiflux-news-provision-<host-triple>.sha256
```

## Provision as the desktop user

The fixed Keyring selector is:

- service: `easiflux_desktop_tauri_news`
- entry: `news_api_token`

It is separate from trading credentials, account IDs, application
configuration, and the news SQLite database. Run the utility as the same,
non-elevated operating-system user who runs EasiFlux Desktop. Administrator,
service-account, or other-user execution can address a different credential
store.

Check the redacted state first:

```powershell
.\easiflux-news-provision-<host-triple>.exe status
```

The command prints only `configured` or `not-configured`; it never reads back or
prints the credential. On macOS or Linux, omit `.exe` in all commands.

Set or rotate the credential interactively:

```powershell
.\easiflux-news-provision-<host-triple>.exe set
```

Enter the value at the no-echo TTY prompt. Approved deployment automation may
send the secret manager value directly to the utility's standard input, with
secret masking enabled. Do not place the credential in a command-line argument,
environment variable, intermediary file, `config.toml`, frontend UI, terminal
history, or log. Re-running `set` securely overwrites the one fixed entry.

Deletion is deliberately guarded and requires both exact selectors:

```powershell
.\easiflux-news-provision-<host-triple>.exe delete --service easiflux_desktop_tauri_news --entry news_api_token
```

Missing or different selectors are rejected before a Keyring operation.

## Packaging and deployment order

1. Approve the fixed source URL and epoch; approve an archive/migration path if
   the endpoint or namespace is changing.
2. Set the protected repository variables and create the release.
3. Confirm all desktop, host-triple provisioning, and checksum assets were
   published without replacement, then verify the downloaded checksum.
4. Install the matching desktop package.
5. As the desktop user, run `status`, then use interactive `set` to provision or
   rotate the credential.
6. Start EasiFlux Desktop and verify News Center reaches its live state. If the
   app was already open, use “重新检查” after provisioning or rotation.

## Runtime states and troubleshooting

- “新闻服务未配置” means the fixed Keyring entry is absent. Verify the same
  operating-system user, run `status`, then provision and choose “重新检查”.
- “新闻服务凭据无效” means the upstream rejected the stored credential. Rotate
  it with interactive `set`, then choose “重新检查”. `status` only confirms that
  an entry exists; it cannot validate the credential with the upstream.
- “新闻数据源未内置，请使用正确的安装包” means the installed release lacks a
  valid fixed source URL or epoch. Install a correctly built package; credential
  changes cannot repair this state.
- A transient retry or storage warning affects News Center only. Cached history
  remains available when the local database can be opened. Never troubleshoot
  by printing, exporting, or copying the credential.

CI and automated tests must use the process-local mock store only. They must
not execute the provisioning binary against a real user profile or inspect,
rotate, or delete a real Keyring credential.
