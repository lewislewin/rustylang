## rustylang

Small, fast Rust CLI to manage i18n locale files and auto-translate missing strings with OpenAI.

### What it does
- Updates/creates strings in your source locale JSON by dot-path (e.g. `general.account`).
- Translates missing (or all) strings from your source locale to other locales asynchronously.
- Works in the current directory with files named `{locale}.json` (e.g. `en-GB.json`, `fr-FR.json`).

### Requirements
- Rust (stable)
- OpenAI API key in `OPENAI_API_KEY` (or a `.env` file in the project root)

### Build
```bash
cargo build --release
# binary at target/release/rustylang
```

### Configuration (rustylang.toml)
Create a `rustylang.toml` in the directory you run the CLI from:
```toml
source_locale = "en-GB"
file_pattern = "{locale}.json"     # files in the current directory
locales = ["fr-FR", "de-DE"]      # defaults for translate (optional)
concurrency = 5                     # parallel requests

[openai]
model = "gpt-4o-mini"              # override with --model if needed

[translate]
overwrite_existing = false          # only fill missing by default
preserve_placeholders = true        # keep {tokens} intact
```

### Usage

Set or update a string in the source file (creates intermediate objects automatically):
```bash
# in a directory containing en-GB.json
rustylang set general.account "Account Name"
rustylang set account.account "Account Name"            # creates nested objects
rustylang set users[0].name "Alice"                     # array index supported
```

Translate missing strings (uses config locales, or pass explicitly):
```bash
# using locales from rustylang.toml
OPENAI_API_KEY=sk-... rustylang translate

# explicit locales
OPENAI_API_KEY=sk-... rustylang translate --locales fr-FR,de-DE

# dry-run preview (no write)
rustylang translate --dry-run

# overwrite existing translations
rustylang translate --overwrite

# concurrency and model overrides
rustylang translate --concurrency 8 --model gpt-4o-mini
```

### Dot-path syntax
- `.` separates object keys; escape literal dots with `\.` (e.g. `labels.some\.`key`).
- Arrays via `[idx]`, e.g. `items[0].name`.
- Only string leaves are translated; non-string values are ignored.

### Notes
- The CLI reads `{locale}.json` files from the current directory.
- `OPENAI_API_KEY` is read from the environment; `.env` is loaded automatically if present.
- Progress bars show per-locale work; errors fall back to the source text.

### Example
```bash
# Start with en-GB.json
{
  "general": {
    "account": "Account"
  }
}

# Add a new key
rustylang set account.account "Account Name"

# Translate to French and German (missing only)
OPENAI_API_KEY=sk-... rustylang translate --locales fr-FR,de-DE
```


