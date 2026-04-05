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
# geosite_url: download URL (optional)
# geosite_path: local file path (required)
geosite_url = "https://example.com/geosite.dat"
geosite_path = "geosite.dat"
version = 2

[entry-name]
# Entry-specific settings (can override global geosite_url/geosite_path)
# depends: entry to depend on for chain processing (optional)
# geosite_url: override global (optional)
# geosite_path: override global (optional)
# bases: list of geosite bases to export (required)
# output: output JSON path (optional, defaults to <entry-name>.json)
# attr_filters: attribute filters like "has:cn" / "lacks:ads" (optional)
bases = ["geolocation-cn"]
attr_filters = ["lacks:cn"]
output = "rules.json"
```

#### Config Rules

- `[__config__]` - Global config (only these fields: `version`, `geosite_url`, `geosite_path`)
  - `version`: JSON output version, defaults to `2`
  - `geosite_url`: download URL
  - `geosite_path`: local file path
- Entry `geosite_url` + `geosite_path`: download before each run
- Entry `geosite_path` only: read from local file, no download
- Entry can override global `geosite_url` / `geosite_path` for different sources
- `depends` field: chain processing, entry B will merge with entry A's result

#### Chain Processing Example

```toml
[base]
bases = ["geolocation-cn"]
output = "base.json"

[filter-cn]
depends = "base"
attr_filters = ["has:cn"]
output = "cn.json"

[filter-global]
depends = "base"
attr_filters = ["lacks:cn"]
output = "global.json"
```

The processing order will be: `base` → `filter-cn`, `filter-global`

### Run All Entries

```bash
domi-cli --config config.toml
```

### Run Specific Entry(s)

```bash
domi-cli --config config.toml --entry cn
domi-cli --config config.toml --entry cn --entry global
```

### Subcommand: list-attrs

List unique attribute tags from geosite.dat:

```bash
domi-cli list-attrs path/to/geosite.dat
```

## Acknowledgments

Inspired by [chise0713/domi](https://github.com/chise0713/domi)
