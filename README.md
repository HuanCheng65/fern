# Fern

Fern is a Minecraft launcher. Its Rust core handles version metadata, file completion, Java runtimes, accounts, launch planning, and game processes. The Tauri application presents that state to the player.

Pearl connectivity is consumed as the `pearl-core` Git dependency. Pearl remains a separate product and repository.

## Workspace

- `fern-meta`: Mojang version metadata models and resolution.
- `fern-download`: content-addressed downloads, checksums, mirrors, and progress events.
- `fern-core`: launcher orchestration and Pearl integration boundary.
- `fern-ui`: Tauri 2 application. Its Rust crate is kept in a separate Cargo workspace because Tauri builds require platform WebView libraries.
- `fern-kit`: the design system, shared as source by the application and the site.
- `fern-site`: the marketing site. It renders the real `fern-kit` components rather than screenshots of them.

## Development

```bash
cargo test --workspace
cargo check -p fern-core
cd fern-ui
pnpm install
pnpm tauri dev
```

Building `fern-ui` needs the platform WebView libraries. On Debian/Ubuntu:

```bash
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev librsvg2-dev \
  libayatana-appindicator3-dev libxdo-dev libssl-dev libdbus-1-dev
```

The implementation roadmap lives in [docs/launcher-core-dev.md](docs/launcher-core-dev.md).
Backup, snapshots and export are designed in
[docs/fern-backup-design.md](docs/fern-backup-design.md).
Conventions and hard-won gotchas are in [AGENTS.md](AGENTS.md).

## CI

Three workflows, split by what they are for.

`Check and package` runs for `main`, pull requests and manual dispatches. It
answers "does this commit still build", and uploads three GitHub Actions
artifacts for anyone who wants to try a build:

- `Fern-Linux-x64`: `.deb` and `.AppImage` packages.
- `Fern-Windows-x64-portable`: a portable `fern-ui.exe` desktop binary.
- `Fern-macOS-universal`: universal Apple Silicon/Intel `.app` and `.dmg` packages.

Cutting a release is one command. It closes the changelog's unreleased
section, writes the version to every file that holds one, commits and tags:

```bash
.github/draft-changelog.py     # gather Release-Note trailers, then read it
.github/release.py 0.2.0       # or 0.2.0-beta.1
git push origin main --follow-tags
```

`Release` runs for `v*` tags. It signs the artifacts with the updater key,
uploads them to R2, and points the channel's `manifest.json` at the new
version — `v0.2.0` goes to `stable`, `v0.2.0-beta.1` to `beta`. The tag must
match the version in `fern-ui/src-tauri/Cargo.toml` or the run stops before
building anything. What it needs configured, and why each piece is shaped the
way it is, is in [docs/fern-update-design.md](docs/fern-update-design.md).

`Deploy the site` runs when `fern-site` or `fern-kit` changes. It builds the
site and uploads it to Cloudflare Workers, configured by
[fern-site/wrangler.jsonc](fern-site/wrangler.jsonc). `fern-kit` is in that list
because the site renders those components for real — a change there changes the
site, and without it the site would quietly sit on an older version of the
design system. It needs `CLOUDFLARE_API_TOKEN` (Account → Workers Scripts →
Edit) and `CLOUDFLARE_ACCOUNT_ID` configured as repository secrets.

## License

Fern is free software under the [GNU General Public License v3.0](LICENSE) or
any later version. Copyright © Astral Studio.

Under section 7(e) of that license, an additional term applies: a modified
version must not carry the Fern name, wordmark, or icon. Rename your fork.
The licence covers the code, not the identity — a re-skinned build passing as
Fern is how launcher users get shipped adware and token stealers.

Fern is not an official Minecraft product. It is not approved by or associated
with Mojang Studios or Microsoft.
