# Farkle Rust Web App

This is a local, offline-friendly single-player Farkle game for Termux on Android. Rust runs a small Axum server, and Android Chrome opens the static web app at `http://127.0.0.1:8080`.

## Install Termux Packages

```bash
pkg update
pkg install rust git clang pkg-config nano
```

If you want to search shared Android storage from Termux and storage access is missing, run:

```bash
termux-setup-storage
```

## Run

```bash
cd ~/apps/farkle-rust-webapp
cargo run
```

Then open Android Chrome:

```text
http://127.0.0.1:8080
```

Stop the server with `Ctrl+C` in Termux.

## Tests

```bash
cargo test
```

## Release Build

```bash
cargo build --release
./target/release/farkle-rust-webapp
```

## Save File

The game saves to:

```text
data/save.json
```

Use the in-game `Reset save` button to reset the saved game, or stop the server and remove `data/save.json`.

## Rules

Roll six dice. Tap scoring dice to keep them, then roll again or bank the current turn score. If a roll has no scoring dice, that is a Farkle and the turn score is lost.

Singles:

```text
Each 1 = 100
Each 5 = 50
```

Three of a kind:

```text
Three 1s = 1000
Three 2s = 200
Three 3s = 300
Three 4s = 400
Three 5s = 500
Three 6s = 600
```

Four, five, and six of a kind multiply the three-of-a-kind score by 2, 4, and 8.

Special six-dice scores:

```text
Straight 1-2-3-4-5-6 = 1500
Three pairs = 1500
Two triplets = 2500
Four of a kind plus one pair = 1500
```

If all six dice score in one turn, hot dice lets you roll all six dice again and keep building the same turn score.

## Termux Search Checklist

Before replacing an older copy on your phone, use safe read-only searches:

```bash
find "$HOME" "$HOME/storage" "$HOME/storage/downloads" "$HOME/storage/shared" "$HOME/storage/documents" \
  -type f \( -iname '*farkle*' -o -iname '*farkel*' -o -iname '*dice*' -o -iname 'Cargo.toml' -o -iname 'main.rs' -o -iname 'index.html' \) 2>/dev/null
```

If you find an older version, copy it to `~/apps/farkle-rust-webapp/imported_existing_version/` before replacing or improving it.
