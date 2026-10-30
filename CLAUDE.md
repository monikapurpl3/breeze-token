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

Local repository only. A GitHub repository, release binaries and where the
curl-able runner is served are the maintainer's decision.
