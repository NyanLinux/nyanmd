# nyanmd

Vim-style markdown notes app. Tauri 2 + Sycamore (Rust, no npm).

Notes live in `~/nyanmd/**/*.md`. Left: file list. Middle: editor. Right: live preview.

## Keys

| key | action |
|-----|--------|
| `h j k l` `w b` `0 $` `gg G` | move |
| `i a A I o O` | enter insert mode |
| `x` `dd` `u` | delete char / line, undo |
| `Esc` | back to normal mode |
| `:e name` | open or create `name.md` |
| `:w` `:q` `:wq` | save / close / both |

`[[note]]` or `[[note|alias]]` in a note renders as a link; clicking it in the preview opens (or creates) `note.md`. The panel under the preview lists notes that link to the current one. Typing `[[` in insert mode opens a note picker: arrows or `Ctrl-n`/`Ctrl-p` move, `Enter`/`Tab` inserts, `Esc` closes.

## Run

```sh
rustup target add wasm32-unknown-unknown
brew install trunk          # or: cargo install trunk
cargo install tauri-cli --version '^2' --locked
cargo tauri dev
```

Without the tauri CLI: `trunk serve` in one terminal, `cargo run -p nyanmd` in another.

Tests for the vim engine: `cargo test -p nyanmd-ui`.
