# Rust migration contract

Scope: replace the Python production executable with a locally compiled Rust binary named kubectl-db3. Preserve the existing CLI, JSON support commands, role log selection, output filenames/manifests, exit statuses, private permissions, failure continuation, timeout handling, and token stdin transport/redaction. Keep the existing Python black-box suite as migration/parity evidence; Python is test tooling only, not a production runtime dependency. Keep all code local; no GitHub action.

## Rust safety and ownership

The implementation uses safe Rust only, forbids unsafe code, and introduces no FFI, raw pointers, custom lock-free primitives, or asynchronous runtime. The undefined-behavior escalation is not triggered by the planned project code. Third-party dependencies are not thereby audited or claimed unsafe-free.

The parsed CLI owns options and paths; collection helpers borrow those options. Command arguments and subprocess output are owned for the duration of a collection step. Manifest records own their strings/values beyond each subprocess call. Necessary copies for subprocess handles and persistent manifest fields must be explicit; no shared mutable ownership, Arc, or interior-mutability wrappers are needed.

Expected failures are returned as Result or recorded as per-command manifest errors. The binary converts local failures to exit 1 and usage failures to exit 2; no production unwrap/expect/panic is acceptable. Assertions in tests are intentional. A killed local kubectl process does not guarantee termination of its remote counterpart.

## Acceptance gates

- cargo fmt --check
- cargo clippy --all-targets -- -D warnings
- cargo test --locked
- cargo test --locked --release
- cargo doc --locked --no-deps
- cargo build --locked --release
- DB3_BINARY="$PWD/target/debug/kubectl-db3" python3 -m unittest discover -s tests -v
- DB3_BINARY="$PWD/target/release/kubectl-db3" python3 -m unittest discover -s tests -v
- Confirm the release artifact is native code, replace the project-root executable atomically, and verify kubectl db3 support logs --help through the existing ~/.local/bin symlink.

Live Kubernetes/Enterprise execution remains separate from fixture tests. Selection by role labels remains a documented convention and is not live discovery evidence.

## Observed verification

- Formatting, Clippy across all targets with warnings denied, locked debug/release native tests, locked documentation build, and locked release build passed.
- 14 native tests passed in each profile (3 unit, 11 integration).
- All 20 existing black-box compatibility tests passed against the debug binary, release binary, and installed project-root executable, including real kubectl plugin dispatch without cluster access.
- The project-root executable is now the compiled Linux x86-64 release ELF. The existing ~/.local/bin/kubectl-db3 symlink resolves to it; actual kubectl help dispatch passed.
- Python remains only in the compatibility-test harness and temporary test substitutes; it is not used to execute the plugin.
- Anonymous private temporary files back token stdin and captures. Tokens may touch temporary storage, though no credentials are intentionally included in bundles or command arguments. Documentation records this limitation.
- Rust 1.97.1 was used for verification; the declared Rust 1.85 minimum and macOS target were not independently exercised. No live cluster, publication, commit, or GitHub push was performed.

Ownership review: options live in main, collectors borrow them, command vectors and captures own per-operation strings, and manifest JSON takes ownership of persisted records. String copies deliberately supply owned subprocess arguments and persistent output records; file-handle duplication shares capture handles with the child. There is no application unsafe code, async runtime, Arc, or interior mutability. Production unwrap_or/unwrap_or_default choose explicit fallback values and do not panic; no panicking unwrap()/expect()/panic! calls remain. Test assertions/unwraps report test failures only.
