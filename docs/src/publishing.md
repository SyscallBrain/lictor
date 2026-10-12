# Publishing the book

The repository includes three GitHub Actions workflows:

- `ci.yml` formats, lints, tests, and builds the Rust application.
- `book.yml` builds the book and checks its local links on pull requests and pushes.
- `release.yml` prepares Linux x86_64 binary/checksum artifacts for tags or manual runs.

Actions are pinned by commit, and mdBook by version. Workflows use GitHub's repository token with job-scoped permissions, not a personal access token or a live Herdr connection.

## Local preview

```bash
mdbook build
python3 docs/check_links.py book
mdbook serve --open
```

`book.toml` sets `site-url = "/lictor/"` so navigation and the error page work under a project Pages path. Change this value if the public repository uses another name or a custom domain.

## Enable Pages when ready

After creating the intended repository and reviewing the content:

1. In repository settings, select GitHub Actions as the Pages source.
2. Run the book workflow manually with its deploy input enabled, or set repository variable `PUBLISH_BOOK` to `true` to deploy future `main` builds.
3. Review the `github-pages` deployment environment and its branch restrictions.

Pull requests build and validate; they do not deploy. Deployment needs only `pages: write` and `id-token: write` in the deploy job, in addition to read access for the build. The workflow will not create a repository or enable Pages by itself.

## Release artifacts

The release workflow builds with `Cargo.lock`, packages `lictor` with its license/readme, and writes `SHA256SUMS`. It uploads Actions artifacts for review. It does not create a public GitHub Release automatically.

Before publishing a version, verify the tag matches `Cargo.toml`, run isolated integration checks against supported Herdr versions, and test the archive on the declared Linux platform. Record compatibility and known rendering/sidebar limits in the release notes.
