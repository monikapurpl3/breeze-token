# breeze-token -- notes for working on it

`breeze-core fetch` on its own: a V3 unit's token and key from Midea's cloud,
checked on the unit, handed to any Breeze Core (μcore, native 4.x, Python
3.x) as a config.json or over a server's API. Users' documentation is
`README.md`.

## Layout

| | |
|---|---|
| `src/main.rs` | arguments, and the flow: destination, units, cloud, fetch, hand-over |
| `src/clouds.rs` | breeze-cloud behind a trait, and the `fake-cloud` pretend account |
| `src/lan.rs` | discovery (breeze-device's scan) and the on-unit check |
| `src/config_file.rs` | `--write`, through breeze-store |
| `src/server.rs` | `--to`: pairing (v1 or signed v2), `POST /api/units`, unpairing |
| `src/talk.rs` | `fetch`'s wording helpers |
| `tests/end_to_end.rs` | the whole thing against real servers on breeze-mock units |
| `run.sh`, `run.ps1` | the `curl \| sh` / `irm \| iex` runners: download, check, run once, delete |
| `packaging/build.sh` | the eight release binaries and SHA256SUMS into `packaging/out/` |
| `packaging/macos/` | the CoreFoundation link stub, from breeze-core-native |
| `stage.sh OUT` | what aspic serves under `/breeze-token/` |

## Rules that are not obvious from the code

- **Breeze Core's crates, by tag** (`v4.3.2` of breeze-core-native):
  breeze-cloud, breeze-device, breeze-proto, breeze-store. Never copy their
  code in. The flow in `main.rs` follows `crates/breeze-core/src/cli/fetch.rs`
  word for word where it can, so a change there is a change here.
- **Tokens, keys, passwords and API keys are never printed**, not in errors
  either. `tests/end_to_end.rs` greps every run's output for each token, key
  and the password.
- **Never with real Midea credentials.** The maintainer runs a real account;
  tests use the `fake-cloud` feature. A build with that feature prints that it
  is one, on every run, and is never released.
- **The end-to-end test and any demo use `--subnet` or `--ip` on breeze-mock's
  dummy segment, always.** Without them it sweeps the /24 around the machine,
  which in rootless podman on mrrp is the maintainer's real LAN, real units
  included.
- **Leave no pairing behind.** `Server`'s `Drop` revokes it; the test checks
  `devices.json` after every `--to`. Exit through `main`'s return, never
  `process::exit` from inside `run`, or the drop does not happen.
- **A config.json keeps its owner, group and mode** (`Owner` in
  `config_file.rs`). breeze-store's save replaces the file, so under sudo it
  would come out root:root, and the packages' service runs as `breeze`: it
  could no longer read its configuration. A new file gets its directory's
  owner. The end-to-end test checks both, as root.
- **The runners are POSIX sh and plain-ASCII PowerShell** (5.1 reads a file
  without a BOM as ANSI; `stage.sh` refuses one that is not ASCII). Under
  `curl | sh` stdin is the script, so the binary gets `/dev/tty`; under
  `irm | iex` everything is in a child scope and nothing may `exit`.
- **Breeze Core 3.x cannot take units over its API** (its add asks Midea and
  fails), so `--to` refuses a 3.x or 2.x server and points at `--write`.

## Building and testing

```sh
cargo test --release
cargo clippy --release --all-targets [--features fake-cloud]   # zero warnings, both
```

The end-to-end run (Linux, `CAP_NET_ADMIN`), as it was done on mrrp:

```sh
PATH=/c/Users/Admin/zig/zig-x86_64-windows-0.16.0:$PATH \
  cargo zigbuild --release --target x86_64-unknown-linux-musl --features fake-cloud \
  --bin breeze-token --test end_to_end
# copy breeze-token, end_to_end-<hash>, breeze-core 4.3.2 (amd64), breeze-mock and
# μcore's musl `ucore` to ~/breeze-token-e2e on mrrp, then:
podman run --rm --cap-add NET_ADMIN -v "$PWD":/t:Z docker.io/library/alpine:3 sh -c \
  "apk add -q iproute2 && cd /t && BREEZE_DUMMY_SEGMENTS=1 BREEZE_TOKEN_BIN=/t/breeze-token \
   BREEZE_CORE_BIN=/t/breeze-core BREEZE_MOCK_BIN=/t/breeze-mock UCORE_BIN=/t/ucore ./end_to_end"
```

Build μcore's `ucore` on its own (`--bin ucore` only): built together with
its tests, dev-dependency features end up in it.

## Traps met so far

- Shell heredocs in this environment halve backslashes: write edit scripts
  with the Write tool.

## Publishing

Public at github.com/monikapurpl3/breeze-token since 2026-10-30. Served from
aspic under `/breeze-token/`: `sh packaging/build.sh`, commit, then from
breeze-core-native `./packaging/repo/stage-breeze-token.sh` (which runs
`stage.sh`) and `./site/publish.sh --tree packaging/out/aspic`, carrying the
maintainer's live pages into the tree first (see its release/PRIVATE.md).
Each publish waits for the maintainer's yes.
