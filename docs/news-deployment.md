# News Center deployment

News Center uses one release-bound TG-forwarder source and a bearer token that
is embedded into the desktop release at build time. A correctly built package
starts News Center without any first-run credential provisioning. Trading,
account, chart, and other desktop functions remain independent from News
Center, and completed cached news remains readable during upstream failures.

## Release configuration

Configure these GitHub repository secrets before creating a release:

- `EASIFLUX_NEWS_API_BASE_URL`: the approved HTTPS TG-forwarder base URL.
- `EASIFLUX_NEWS_API_TOKEN`: the raw bearer token, without a `Bearer ` prefix.

Configure this GitHub repository variable:

- `EASIFLUX_NEWS_SOURCE_EPOCH`: a non-empty namespace version for the endpoint
  and delivery stream.

Both release callers pass the two named secrets and the epoch to the reusable
build. The values are scoped to the provisioning-utility and desktop
compilation steps. A release build rejects missing or partial configuration,
an invalid token, an empty epoch, or an invalid/non-HTTPS endpoint. Build
scripts and workflow commands must never print the host or token value.

### Security boundary of an embedded token

GitHub Secrets protect values in repository configuration and mask matching
workflow log output. They do not keep a value secret after it has been compiled
into a distributed desktop binary. Anyone who can inspect the installed
binary, process memory, or authenticated traffic may recover the shared token.
Use only a credential that is explicitly approved for client distribution,
grant it the minimum TG-forwarder scope, apply server-side rate limits, and be
prepared to rotate or revoke it. Never reuse a trading or administrative
credential as the embedded news token.

The release owner is responsible for epoch changes. Changing the endpoint or
rebuilding the upstream delivery database requires incrementing
`EASIFLUX_NEWS_SOURCE_EPOCH` and approving an archive or migration path before
deployment. The application does not automatically reset a database or merge
histories from different source namespaces.

## Runtime credential precedence

The desktop resolves a news credential in this order:

1. A valid same-user Keyring override written by `easiflux-news-provision`.
2. The token embedded in the matching desktop release.

No Keyring entry is the normal default and does not disable News Center.
Keyring unavailability or a locally malformed override falls back to the
embedded token. A syntactically valid override that the upstream rejects is
treated as an invalid credential until the override is replaced or deleted;
the application does not silently switch identities after a server rejection.

The fixed Keyring selector remains:

- service: `easiflux_desktop_tauri_news`
- entry: `news_api_token`

The application never invokes the provisioning utility automatically. The
utility is retained only as an optional future override/rotation mechanism and
is not part of first-run setup.

## Optional provisioning utility

Each platform job continues to publish:

- `easiflux-news-provision-<host-triple>[.exe]`
- `easiflux-news-provision-<host-triple>[.exe].sha256`

Select the utility matching the target computer and verify its checksum before
use. Release jobs do not overwrite an asset with the same name.

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

Run the utility as the same, non-elevated operating-system user who runs
EasiFlux Desktop. `status` reports only whether a Keyring override exists; it
does not report whether the application has an embedded default:

```powershell
.\easiflux-news-provision-<host-triple>.exe status
```

Set or rotate an override interactively:

```powershell
.\easiflux-news-provision-<host-triple>.exe set
```

Enter the raw token at the no-echo prompt. Do not place an override in a
command-line argument, intermediary file, frontend UI, terminal history, or
log. After setting an override, restart the desktop or choose “重新检查”.

Delete the override and return to the embedded default with the two exact
selectors:

```powershell
.\easiflux-news-provision-<host-triple>.exe delete --service easiflux_desktop_tauri_news --entry news_api_token
```

After deletion, restart the desktop or choose “重新检查”.

## Packaging and deployment order

1. Approve a distributable, least-privilege news token and the fixed source.
2. Set the two repository secrets and the source-epoch repository variable.
3. Create the release and confirm all platform builds completed.
4. Confirm desktop, provisioning, and checksum assets were published without
   replacement.
5. Install the matching desktop package; no first-run Token step is required.
6. Verify News Center reaches `live` using the embedded credential.

## Runtime states and troubleshooting

- “新闻数据源未内置，请使用正确的安装包” means the release lacks a valid
  host, epoch, or embedded token. Install a correctly built release.
- “新闻服务凭据无效” means the upstream rejected the active credential. If a
  Keyring override exists, replace or delete it. Otherwise rotate the GitHub
  secret and publish a new release.
- A transient retry or storage warning affects News Center only. Completed
  cached history remains available when the local database can be opened.
- `not-configured` from the provisioning utility only means no Keyring override
  exists; it is not an application error when an embedded token is present.

CI and automated tests must use explicit fake tokens and process-local mock
stores. They must not execute the provisioning binary against a real user
profile or inspect, rotate, or delete a real Keyring credential.
