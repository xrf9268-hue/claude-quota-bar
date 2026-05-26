# Releasing

Releases are automated with [release-plz](https://release-plz.dev) using the
**Release PR** model. You never bump the version, write the changelog, or push
a tag by hand.

## How to cut a release

1. Merge normal PRs to `main` with [Conventional Commit](https://www.conventionalcommits.org)
   subjects (`fix:`, `feat:`, `feat!:` or a `BREAKING CHANGE:` footer for majors).
2. release-plz keeps a **Release PR** titled `chore: release vX.Y.Z` open, and
   updates the version bump + `CHANGELOG.md` as more commits land. Pre-1.0
   semver applies: `feat` → patch, breaking → minor.
3. **Merge the Release PR.** That is the entire release action — it triggers:
   - git tag `vX.Y.Z` + GitHub Release (body = the changelog section)
   - publish to crates.io (OIDC)
   - build the 7 platform binaries and attach them to the release
   - publish the npm umbrella + 7 platform packages (OIDC)

You can edit the Release PR before merging (e.g. polish the changelog) — your
edits are kept.

## What runs where

Everything is one workflow, `.github/workflows/release.yml`, on every push to
`main`:

- **`release-plz`** — maintains the Release PR; on its merge, cuts the
  tag/release and publishes the crate to crates.io. Emits `releases_created`.
- **`build` / `release-assets` / `publish-npm`** — downstream jobs gated on
  `releases_created`, so they run *only* when a release was just cut, in the
  same workflow run. They check out the released tag (not the trigger SHA) so
  binaries and npm packages always match the release exactly.

Keeping the publish jobs in the same run as `release-plz` is deliberate: the
default `GITHUB_TOKEN` cannot trigger a *separate* workflow, so a tag-triggered
design would need a PAT or GitHub App. In-run job dependencies need neither —
no long-lived tokens anywhere.

## One-time setup (already done)

- **crates.io Trusted Publisher** for `claude-quota-bar` — repo
  `xrf9268-hue/claude-quota-bar`, workflow `release.yml`.
- **npm Trusted Publisher** for the umbrella + 7 platform packages — see
  [npm-trusted-publishing.md](./npm-trusted-publishing.md).
- Repo setting **Settings → Actions → General → Workflow permissions →
  "Allow GitHub Actions to create and approve pull requests"** must be enabled,
  or release-plz can't open the Release PR.

crates.io and npm both publish via OIDC, so there are no publish tokens in repo
secrets.

## Troubleshooting

- **Release PR doesn't open / `403 not permitted to create pull requests`** —
  the "Allow GitHub Actions to create and approve pull requests" repo setting is
  off.
- **crates.io publish `403`** — the crates.io Trusted Publisher is missing or
  its owner/repo/workflow fields don't match exactly.
- **npm `403 npm-trusted-publisher-not-configured`** — same, on the npm side
  (per package).
- **Preview the next release locally** — `release-plz update` on a throwaway
  branch writes the version bump + `CHANGELOG.md`; inspect, then revert.
