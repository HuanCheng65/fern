# Fern

Fern is a Minecraft launcher. Its Rust core handles version metadata, file completion, Java runtimes, accounts, launch planning, and game processes. The Tauri application presents that state to the player.

Pearl connectivity is consumed as the `pearl-core` Git dependency. Pearl remains a separate product and repository.

## Workspace

- `fern-meta`: Mojang version metadata models and resolution.
- `fern-download`: content-addressed downloads, checksums, mirrors, and progress events.
- `fern-core`: launcher orchestration and Pearl integration boundary.
- `fern-ui`: Tauri 2 application. Its Rust crate is kept in a separate Cargo workspace because Tauri builds require platform WebView libraries.

## Development

```bash
cargo test --workspace
cargo check -p fern-core
cd fern-ui
pnpm install
pnpm tauri dev
```

The implementation roadmap lives in [docs/launcher-core-dev.md](docs/launcher-core-dev.md).

## CI packages

The `Check and package` workflow runs for `main`, pull requests, version tags,
and manual dispatches. Successful package jobs upload two GitHub Actions
artifacts:

- `Fern-Windows-x64-portable`: a portable `fern-ui.exe` desktop binary.
- `Fern-macOS-universal`: universal Apple Silicon/Intel `.app` and `.dmg` packages.

Create a `v*` tag for a versioned package run, or start the workflow from the
Actions page while iterating on a branch.
