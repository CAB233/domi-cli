# domi-cli

Convert geosite.dat domain rules to sing-box JSON rule-set format.

## Build

```bash
cargo build --release
```

## Usage

### Configuration File

All settings are managed via a config file. Create a configuration file, such as `config.toml`:

```toml
[__config__]
# Global settings (version, geosite source)
# version: rule-set version (default: 2)
# url: download URL (optional)
# input: local file path (required)
url = "https://example.com/geosite.dat"
input = "geosite.dat"
version = 2

[entry-name]
# Entry-specific settings (can override global url/input)
# depends: entries to depend on for merging (optional, array)
# url: override global (optional)
# input: override global (optional)
# bases: list of geosite bases to export (required)
# output: output JSON path (optional, defaults to <entry-name>.json)
# internal: don't output file, only used for depends (optional, boolean)
# attr_filters: attribute filters like "has:cn" / "lacks:ads" (optional)
bases = ["geolocation-cn"]
attr_filters = ["lacks:cn"]
output = "rules.json"
```

#### Config Rules

- `[__config__]` - Global config (only these fields: `version`, `url`, `input`)
  - `version`: JSON output version, defaults to `2`
  - `url`: download URL
  - `input`: local file path
- Entry `url` + `input`: download before each run
- Entry `input` only: read from local file, no download
- Entry can override global `url` / `input` for different sources
- `depends` field: multiple entries supported, array format `depends = ["A", "B"]`
- `internal` field: if true, no output file, but can be referenced by depends
- `output` and `internal` cannot both be specified

#### Dependency Example

```toml
[cn]
bases = ["geolocation-cn"]
internal = true

[global]
bases = ["geolocation-!cn"]
attr_filters = ["lacks:cn"]
output = "global.json"

[merged]
depends = ["cn", "global"]
bases = ["private"]
output = "merged.json"
```

The processing order will be topologically sorted. `merged` will merge results from `cn` and `global` first, then merge with its own `bases`.

### Run All Entries

```bash
domi-cli --config config.toml
```

### Run Specific Entry(s)

```bash
domi-cli --config config.toml --entry cn
domi-cli --config config.toml --entry cn --entry global
```

### Subcommand

List unique attribute tags from geosite.dat:

```bash
domi-cli list-attrs path/to/geosite.dat
```

## Acknowledgments

Inspired by [chise0713/domi](https://github.com/chise0713/domi)
