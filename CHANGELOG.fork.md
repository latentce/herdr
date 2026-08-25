# Fork changelog

What this fork adds on top of each upstream herdr release. Every `vX.Y.Z-fork.N`
release is upstream `vX.Y.Z` plus everything listed here; the fork revision `N`
restarts at 1 for each upstream base and increases when fork features land
between upstream releases. Upstream's
own changes live in [CHANGELOG.md](CHANGELOG.md) and the upstream release notes.

## Fork infrastructure

- Fork builds are published as `vX.Y.Z-fork.N` GitHub releases by `.github/workflows/fork-release.yml`, which syncs each upstream stable release, reapplies the fork patches, and uploads binaries for all five upstream platforms (Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64) with a `latest.json` update manifest. Fork builds report their identity as `vX.Y.Z-fork.N` in `herdr --version` and self-update when a newer fork revision of the same upstream base is published.
- Installed binaries self-update from this fork's release manifest instead of upstream's.
- An install script for Linux and macOS is published to GitHub Pages (`install.sh`) pointing at the fork releases. Windows builds are downloaded from the releases page.
