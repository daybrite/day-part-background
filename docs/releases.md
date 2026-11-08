# CI and releases

The shared library workflow runs formatting, Clippy, and Cargo tests on macOS, Linux, and
Windows, plus cross-target Clippy and demo walkthroughs. Its crate publication waits for host
and cross-target checks; demo artifacts have their separate build and walkthrough gates.

Branch pushes and pull requests use Day's main branch. Version tags and ordinary manual runs
use published dependencies. Select **day-main** for a manual compatibility run against main.
The portable walkthroughs require neither sender credentials nor an OS background-task grant.
Native delivery and process-restart tests remain separate device checks.

## Release prerequisites

Day's process-service startup hooks and external bridge source loading must be available on
main before branch CI can use them, then released in the Day framework and CLI before tagging
this package. Local ignored Cargo patches do not supply those changes to GitHub runners.

Enable Actions in `daybrite/day-part-background`. The shared workflow receives the repository's optional
signing secrets through `secrets: inherit`; configure the signing credentials needed for your
demo artifacts. See the [Day Actions guide](https://github.com/daybrite/actions#readme).

## Reserve the crate names

Crates.io requires an initial publication using an API token before trusted publishing can be
configured. The Daybrite helper publishes a `0.0.1` placeholder for each unregistered name; it
leaves the real package version unchanged, skips existing names, and handles rate limits.

From the ecosystem directory containing `actions/` and this package:

```sh
cargo login
python3 actions/.github/actions/publish-crates/crates-io.py \
  --root day-part-background reserve --only day-part-background
```

At the login prompt, enter a crates.io API token permitted to publish new crates. Add
`--dry-run` to the reservation command to build and verify placeholders without uploading.
If a name already exists, confirm that your account owns it before configuring publication.
Demo applications are marked `publish = false` and need no reservations.

## Configure trusted publishing

1. In the GitHub repository, open **Settings → Environments** and create `release`.
   Permit version tags matching `v*`. Required reviewers or wait timers pause publication;
   omit them if tagged releases should run automatically.
2. Open each crate's **Settings → Trusted Publishing** on crates.io and add a GitHub publisher.
   Use these exact values:

| Field | Value |
| --- | --- |
| Repository owner | `daybrite` |
| Repository name | `day-part-background` |
| Workflow filename | `ci.yml` |
| Environment | `release` |

The filename is the calling workflow in this repository, not `daylib.yml` in `daybrite/actions`.
The publishing job already requests `id-token: write` and uses
`rust-lang/crates-io-auth-action@v1`. It obtains a short-lived token; no persistent
`CARGO_REGISTRY_TOKEN` GitHub secret is needed for later releases.

See the [crates.io trusted-publishing documentation](https://crates.io/docs/trusted-publishing)
for the registry setup and initial-publication requirement.

## Publish a release

Update the package and demo versions, then run CI with published Day dependencies. Push a
`vX.Y.Z` tag matching those versions. The publisher skips a version that is already on crates.io,
so rerunning an interrupted release can finish the remaining work.

The shared demo workflow builds and attaches demo artifacts to the GitHub release. Store
submissions require their separate app identifiers, signing, and store configuration.
