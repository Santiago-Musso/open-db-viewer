# Contributing to Open DB Viewer

First off, thank you for considering contributing to Open DB Viewer! It's people like you that make it a great tool.

## Getting Started
1. Ensure you have Node.js and Rust installed.
2. Clone the repo and run `npm install`.
3. To start the development server, run `npm run tauri dev`.

## Adding a New Driver
Open DB Viewer is designed to be easily extensible. To add a new database driver:
1. Create a new crate in `crates/driver-<name>`.
2. Implement the `Driver` trait defined in `crates/driver-api`.
3. Add your driver to the `ConnectionManager` in `crates/core`.

## Pull Requests
- Keep PRs focused on a single feature or bug fix.
- Ensure all Rust code passes `cargo clippy` and `cargo fmt`.
- Add tests for any new backend functionality.
