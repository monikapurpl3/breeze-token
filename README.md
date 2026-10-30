<div align="center">

# breeze-token

**Your V3 air conditioners' keys, from Midea, once.**

A V3 Midea air conditioner answers nothing without a token and a key, and only
Midea's cloud hands them out. breeze-token fetches them from the account the
units are paired with, checks each one on its unit, and gives them to any
[Breeze Core](https://github.com/monikapurpl3/breeze-core-native):
[Breeze μcore](https://github.com/monikapurpl3/breeze-ucore), Breeze Core 4,
or the Python 3.x line.

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue)](LICENSE)
[![For](https://img.shields.io/badge/for-Breeze%20μcore%20·%20Breeze%20Core%204%20·%20Breeze%20Core%203-e69138)](#why)

</div>

```console
$ breeze-token --to 192.168.1.10
== The server ==
  The key is right. http://192.168.1.10:8420 runs Breeze μcore 4.3.2+u1.0.0.
  Paired as "breeze-token on laptop", approved from here: this machine is on
  the server's network.

== Finding the units ==
  192.168.1.50     id 151732605161920  V3, needs a token and key
  192.168.1.51     id 151732605161921  V3, needs a token and key

== Which of Midea's clouds knows this account ==
  Asking each of them by name only. No password is sent for this.
  MSmartHome       has this account
  ...
== Fetching and checking ==
  OK   Living room accepted it: a V3 handshake and a state read, nothing changed on the unit.
  OK   Bedroom accepted it: a V3 handshake and a state read, nothing changed on the unit.

== Adding them to http://192.168.1.10:8420 ==
  OK   Living room: added.
  OK   Bedroom: added.
  Its pairing with http://192.168.1.10:8420 is removed again.
```

## Why

Breeze Core 4 has `breeze-core fetch`, but it runs on the server. The other
two cannot fetch at all: **Breeze μcore** is a microcontroller, with no TLS
and no cloud, and **Breeze Core 3.x** asks Midea for a token itself whenever a
unit is added, through a request that no longer works, so it cannot add a V3
unit. breeze-token is `fetch` on its own, for any machine on the units'
network, and it keeps `fetch`'s promises:

- **The password goes only to a cloud that has the account.** Which of
  Midea's clouds knows it is asked first, without it. It is read without
  echo, used for one sign-in, and forgotten.
- **Every token is checked on its unit** (a V3 handshake and a state read,
  which change nothing) before it goes anywhere.
- **Tokens are never printed.** They go into a config.json, backed up first,
  or to a running server over its own API, and nowhere else.
- **It is Breeze Core's own code**: its cloud flows, discovery, handshake and
  config.json writer, from the release it follows, not a copy of them.

## Using it

```sh
breeze-token --write config.json        # any Breeze Core reads it at start
breeze-token --to 192.168.1.10          # a running Breeze μcore or Breeze Core 4
```

`--write` makes the file if it is not there, with an API key, and otherwise
adds the units to it (a unit already there keeps its name) after copying it to
`config.json.before-breeze-token`. This is the way for Breeze Core 3.x, and
for a server that is not running yet.

`--to` pairs with the server the way the app does, approving itself if this
machine is on the server's network, adds the units with `POST /api/units`, and
removes its pairing again. The server's API key comes from `BREEZE_API_KEY`
or is asked for.

| | |
|---|---|
| `--ip ADDRESS` | only that unit (again for more); else every unit around this machine (its /24) |
| `--subnet CIDR` | every unit in that private subnet, a /22 at most |
| `--cloud NAME` | `smarthome`, `meiju` or `nethome`: the app the units are in |
| `--account EMAIL` | the account you sign in to that app with |
| `--password-stdin` | the Midea password as one line of standard input, for scripts |

Without `--write` or `--to`, it asks. A unit that already has its token where
the tokens are going is left alone unless it is named with `--ip`.

## Which Midea app

Tokens come only from the account a unit is paired with. As of October 2026:

- **MSmartHome** (outside China) and **美的美居 Meiju** (in it) still issue them.
- **NetHome Plus** signs in and then refuses every unit, its owners'
  included, since August 2026. breeze-token says how to move a unit to
  MSmartHome. A NetHome Plus account is not an MSmartHome one.

A unit never forgets its token, but Midea may stop handing them out
altogether. Keep a copy of the config.json somewhere safe.

## Building

```sh
cargo build --release      # target/release/breeze-token
cargo test
```

The end-to-end test (`tests/end_to_end.rs`) drives breeze-mock's units with
Breeze Core 4.3.2 and Breeze μcore as the servers, and a pretend MSmartHome
account (the `fake-cloud` feature). It needs Linux with `CAP_NET_ADMIN` (a
container is enough), and its header says how to run it. Nothing here is
ever tested against a real Midea account.

## License

AGPL-3.0-or-later, as Breeze Core, whose crates it is built on.
