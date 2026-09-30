# ugreenapp

Rust CLI for a Linux/Windows port of UGREEN Connect.

Current status:
- real `UG1` control works on `UGREEN HiTune Max5c` and `UGREEN HiTune Max5` over Linux RFCOMM
- verified on Max5c: `query-info`, `query-firmware`, `set-anc`, `set-eq`, `set-prompt-language`, `set-prompt-volume`
- verified on Max5: `info`, `anc`, `eq`, `lang`, `ldac`, `game`, `dual`, `buttons`
  (`vol`, `spatial` and `wear` are refused for Max5: it has none of them and never answers;
  game mode and multipoint are refused by the headset while LDAC is on)
- the main code in this repository was created with AI assistance and then tested/refined manually

## Build

```bash
cargo build --release
```

Binary:

```bash
./target/release/ugreen_core
```

## CLI

Useful commands:
- `scan`
- `connect max5c`
- `info`
- `fw`
- `anc off|transparency|light|medium|deep|adaptive` (Max5: `off|light|medium|anc|transparency`)
- `eq original|classic|jazz|electronic|pop|classical|rock|bass|treble`
- `lang english|chinese`
- `vol 1..15`
- `ldac|game|dual|spatial|wear on|off` (an LDAC change makes the headset reconnect,
  so `ldac` skips writing a value that is already set)
- `buttons <tap> <double> <triple> <long>` with `none|anc|game` or a raw action code

The RFCOMM control channel differs between models: Max5c keeps its verified channel 1,
other models look it up over SDP (`sdptool`, from BlueZ; Max5 answers on 12).
Set `UGREEN_RFCOMM_CHANNEL` to override it.
With no headset connected, `UGREEN_DEMO=1` lists placeholder devices for trying the CLI.

Use `--json` with any command for full state output, for example:

```bash
./target/release/ugreen_core --json query-info
```

Inside the REPL:

```text
scan --json
info --json
```

`factory-reset` is intentionally gated behind `--yes-really-reset-device`.

## Docs

- `docs/earbuds-desktop-bridge.md`
- `docs/desktop-app-shape.md`
- `docs/protocol-layer.md`
