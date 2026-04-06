use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use domi::{
    AttrFilter, Entries,
    geosite::proto::{GeoSiteList, domain},
    srs::Rule,
};
use prost::Message;
use serde::Deserialize;

#[derive(Debug, Args, Clone)]
struct ConvertArgs {
    /// Path to the config file
    #[arg(long = "config")]
    config: Option<PathBuf>,

    /// Specific entry to run (can be repeated)
    #[arg(long = "entry")]
    entries: Vec<String>,
}

#[derive(Debug, Args, Clone)]
struct ListAttrsArgs {
    /// Path to a GeoSite file
    #[arg(index = 1)]
    geosite: PathBuf,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// List available attribute tags from a GeoSite file
    ListAttrs(ListAttrsArgs),
}

#[derive(Debug, Parser)]
#[command(name = "domi-cli")]
#[command(
    version,
    about = "Convert geosite.dat into sing-box JSON rule-set files"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    convert: ConvertArgs,
}

/// Global config fields (only for [__config__]).
#[derive(Debug, Default, Clone, Deserialize)]
struct GlobalConfig {
    version: Option<u8>,
    url: Option<String>,
    input: Option<PathBuf>,
}

/// Entry config fields.
#[derive(Debug, Default, Clone, Deserialize)]
struct EntryConfig {
    /// Entry to depend on (chain processing)
    depends: Option<String>,
    /// Override url from global
    url: Option<String>,
    /// Override input from global
    input: Option<PathBuf>,
    /// List of geosite bases to export
    bases: Option<Vec<String>>,
    /// Output JSON path
    output: Option<PathBuf>,
    /// Attribute filters: "has:xxx" / "lacks:xxx"
    attr_filters: Option<Vec<String>>,
}

/// Config file structure:
/// - [__config__] for global defaults (version, url, input)
/// - other tables are entries (bases, output, attr_filters, etc.)
#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    #[serde(rename = "__config__")]
    global: Option<GlobalConfig>,

    #[serde(flatten)]
    entries: HashMap<String, EntryConfig>,
}

#[derive(Debug)]
struct EffectiveConfig {
    entry_name: Option<String>,
    url: Option<String>,
    input: PathBuf,
    download_enabled: bool,
    bases: Vec<String>,
    output: Option<PathBuf>,
    version: u8,
    attr_filters: Vec<String>,
}

#[derive(Debug)]
enum OwnedAttrFilter {
    Has(String),
    Lacks(String),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Some(Commands::ListAttrs(args)) => run_list_attrs(args),
        None => {
            let jobs = resolve_jobs(&cli.convert)?;
            let mut prev_rules: Option<Vec<Rule>> = None;

            for job in jobs {
                prev_rules = run_one(job, prev_rules)?;
            }
            Ok(())
        }
    }
}

fn run_one(config: EffectiveConfig, prev_rules: Option<Vec<Rule>>) -> Result<Option<Vec<Rule>>> {
    // Download geosite if both url and path are configured.
    if config.download_enabled {
        download_geosite(config.url.as_ref().unwrap(), &config.input)?;
    }

    let bytes = fs::read(&config.input).with_context(|| {
        format!(
            "Failed to read geosite file: {}",
            config.input.display()
        )
    })?;

    let geosite =
        GeoSiteList::decode(bytes.as_slice()).context("geosite.dat protobuf decode failed")?;

    let filters = parse_attr_filters(&config.attr_filters)?;
    let mut rules = Vec::new();

    for base in &config.bases {
        let text = build_domi_text_for_base(&geosite, base)
            .with_context(|| format!("Base `{base}` not found in geosite"))?;

        // Use domi's flatten + AttrFilter to ensure consistent behavior.
        let mut entries = Entries::parse(base, text.lines());
        let domi_filters = to_domi_filters(&filters);
        let selected = entries
            .flatten(base, domi_filters.as_deref())
            .with_context(|| format!("Base `{base}` has no available domains after filtering"))?;

        rules.push(Rule::from(selected));
    }

    // Multiple bases are merged into one rule by key. Use BTree* for stable output.
    let final_rules = if let Some(prev) = prev_rules {
        merge_rules(prev, rules)
    } else {
        rules
    };

    let json = build_rule_set_json(&final_rules, config.version)?;

    if let Some(output) = &config.output {
        let json_with_eol = format!("{}\n", json);
        fs::write(output, json_with_eol)
            .with_context(|| format!("Failed to write JSON file: {}", output.display()))?;
    } else {
        if let Some(name) = &config.entry_name {
            println!("# entry: {name}");
        }
        println!("{json}");
    }

    Ok(Some(final_rules))
}

/// Merge previous rules with new rules (chain processing).
fn merge_rules(prev: Vec<Rule>, new: Vec<Rule>) -> Vec<Rule> {
    if prev.is_empty() {
        return new;
    }
    if new.is_empty() {
        return prev;
    }

    // Merge all rules into one by combining their fields.
    let mut merged = Rule::default();

    for rules in [prev, new] {
        for rule in rules {
            if let Some(v) = rule.domain_suffix {
                let existing = merged.domain_suffix.take();
                let mut combined: Vec<Box<str>> =
                    existing.map(|b| b.into_vec()).unwrap_or_default();
                combined.extend(v.into_vec());
                combined.sort();
                combined.dedup();
                merged.domain_suffix = Some(combined.into_boxed_slice());
            }
            if let Some(v) = rule.domain {
                let existing = merged.domain.take();
                let mut combined: Vec<Box<str>> =
                    existing.map(|b| b.into_vec()).unwrap_or_default();
                combined.extend(v.into_vec());
                combined.sort();
                combined.dedup();
                merged.domain = Some(combined.into_boxed_slice());
            }
            if let Some(v) = rule.domain_keyword {
                let existing = merged.domain_keyword.take();
                let mut combined: Vec<Box<str>> =
                    existing.map(|b| b.into_vec()).unwrap_or_default();
                combined.extend(v.into_vec());
                combined.sort();
                combined.dedup();
                merged.domain_keyword = Some(combined.into_boxed_slice());
            }
            if let Some(v) = rule.domain_regex {
                let existing = merged.domain_regex.take();
                let mut combined: Vec<Box<str>> =
                    existing.map(|b| b.into_vec()).unwrap_or_default();
                combined.extend(v.into_vec());
                combined.sort();
                combined.dedup();
                merged.domain_regex = Some(combined.into_boxed_slice());
            }
        }
    }

    vec![merged]
}

fn download_geosite(url: &str, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    let response = reqwest::blocking::get(url)
        .with_context(|| format!("Failed to download geosite: {url}"))?
        .error_for_status()
        .with_context(|| format!("Download geosite returned non-2xx status: {url}"))?;

    let bytes = response
        .bytes()
        .context("Failed to read download response body")?;

    let mut file = fs::File::create(path)
        .with_context(|| format!("Failed to create geosite file: {}", path.display()))?;
    file.write_all(bytes.as_ref())
        .with_context(|| format!("Failed to write geosite file: {}", path.display()))?;

    Ok(())
}

fn load_geosite(path: &Path) -> Result<GeoSiteList> {
    let bytes = fs::read(path)
        .with_context(|| format!("Failed to read geosite file: {}", path.display()))?;
    GeoSiteList::decode(bytes.as_slice()).context("geosite.dat protobuf decode failed")
}

fn load_config(path: &Path) -> Result<ConfigFile> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    let parsed: ConfigFile = toml::from_str(&raw)
        .with_context(|| format!("Failed to parse config TOML: {}", path.display()))?;
    Ok(parsed)
}

fn merge_entry_with_global(
    entry: &EntryConfig,
    global: &Option<GlobalConfig>,
) -> (Option<String>, Option<PathBuf>, bool) {
    let global = match global {
        Some(g) => g,
        None => return (None, None, false),
    };

    let url = entry.url.clone().or(global.url.clone());
    let path = entry.input.clone().or(global.input.clone());

    let download_enabled = url.is_some() && path.is_some();

    (url, path, download_enabled)
}

fn entry_to_effective(
    entry_name: String,
    entry: &EntryConfig,
    global: &Option<GlobalConfig>,
    auto_output: bool,
) -> Result<EffectiveConfig> {
    let (url, input, download_enabled) = merge_entry_with_global(entry, global);

    let input = input.context("Missing input: set in config file")?;

    let bases = entry.bases.clone().unwrap_or_default();
    if bases.is_empty() {
        bail!("Missing bases in entry `{}`: set bases = [...]", entry_name);
    }

    let output = entry
        .output
        .clone()
        .or_else(|| auto_output.then(|| PathBuf::from(format!("{}.json", entry_name))));

    let version = global.as_ref().and_then(|g| g.version).unwrap_or(2);

    let attr_filters = entry.attr_filters.clone().unwrap_or_default();

    Ok(EffectiveConfig {
        entry_name: Some(entry_name),
        url,
        input,
        download_enabled,
        bases,
        output,
        version,
        attr_filters,
    })
}

/// Assemble the actual tasks to execute.
fn resolve_jobs(cli: &ConvertArgs) -> Result<Vec<EffectiveConfig>> {
    let config_path = cli
        .config
        .as_ref()
        .context("Missing --config: please specify a config file")?;

    let cfg = load_config(config_path)?;
    let global = cfg.global.clone();

    // Build job list based on mode.
    let mut job_entries: Vec<(String, EntryConfig)> = if !cli.entries.is_empty() {
        cli.entries
            .iter()
            .map(|name| {
                let entry = cfg
                    .entries
                    .get(name)
                    .cloned()
                    .with_context(|| format!("Entry `{name}` not found in config file"))?;
                Ok((name.clone(), entry))
            })
            .collect::<Result<_>>()?
    } else {
        if cfg.entries.is_empty() {
            bail!("No entries found in config file. Add entries like [cn], [global], etc.")
        }
        cfg.entries.into_iter().collect()
    };

    // Sort by depends order (topological sort).
    sort_by_depends(&mut job_entries)?;

    // Convert to EffectiveConfig.
    let mut jobs = Vec::with_capacity(job_entries.len());
    for (name, entry) in job_entries {
        let job = entry_to_effective(name, &entry, &global, true)?;
        jobs.push(job);
    }

    Ok(jobs)
}

/// Sort entries by depends order (topological sort).
fn sort_by_depends(entries: &mut [(String, EntryConfig)]) -> Result<()> {
    let count = entries.len();
    if count <= 1 {
        return Ok(());
    }

    // Build dependency map.
    let depends_map: HashMap<String, Option<String>> = entries
        .iter()
        .map(|(n, e)| (n.clone(), e.depends.clone()))
        .collect();

    // Detect circular dependency and build in-degree map.
    let mut in_degree: HashMap<String, usize> =
        entries.iter().map(|(n, _)| (n.clone(), 0)).collect();

    for (name, entry) in entries.iter() {
        if let Some(dep) = &entry.depends {
            // Check if dep exists.
            if !depends_map.contains_key(dep) {
                bail!("Entry `{}` depends on non-existent entry `{}`", name, dep);
            }
            // Check for circular dependency.
            let mut visited = HashSet::new();
            let mut current = dep.clone();
            while let Some(next_dep) = depends_map.get(&current).and_then(|d| d.clone()) {
                if next_dep == *name {
                    bail!(
                        "Circular dependency detected: {} -> {} -> {}",
                        name,
                        dep,
                        next_dep
                    );
                }
                if visited.contains(&next_dep) {
                    break;
                }
                visited.insert(current.clone());
                current = next_dep;
            }
            // Increment in-degree of the dependent (the one that has depends).
            *in_degree.get_mut(name).unwrap() += 1;
        }
    }

    // Kahn's algorithm.
    let names: Vec<String> = entries.iter().map(|(n, _)| n.clone()).collect();
    let mut queue: Vec<String> = names
        .iter()
        .filter(|n| in_degree[*n] == 0)
        .cloned()
        .collect();
    let mut sorted = Vec::with_capacity(count);

    while let Some(node) = queue.pop() {
        sorted.push(node.clone());
        for (name, entry) in entries.iter() {
            // If name depends on node, reduce name's in-degree after node is processed.
            if entry.depends.as_ref() == Some(&node) {
                let deg = in_degree.get_mut(name).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    queue.push(name.clone());
                }
            }
        }
    }

    if sorted.len() != count {
        bail!("Circular dependency detected in entries");
    }

    // Reorder entries by sorted order.
    entries.sort_by_key(|(n, _)| sorted.iter().position(|s| s == n).unwrap());
    Ok(())
}

fn run_list_attrs(args: &ListAttrsArgs) -> Result<()> {
    let geosite = load_geosite(&args.geosite)?;
    let mut attrs = BTreeSet::new();

    for site in &geosite.entry {
        for domain in &site.domain {
            for attr in &domain.attribute {
                let key = attr.key.trim();
                if !key.is_empty() {
                    attrs.insert(key.to_string());
                }
            }
        }
    }

    for attr in attrs {
        println!("{attr}");
    }

    Ok(())
}

/// Parse `has:cn` / `lacks:cn` arguments.
fn parse_attr_filters(raw_filters: &[String]) -> Result<Vec<OwnedAttrFilter>> {
    let mut filters = Vec::with_capacity(raw_filters.len());

    for raw in raw_filters {
        let (kind, value) = raw.split_once(':').with_context(|| {
            format!("Invalid attr-filter format: `{raw}`, expected has:xxx or lacks:xxx")
        })?;

        if value.trim().is_empty() {
            bail!("attr-filter value cannot be empty: `{raw}`");
        }

        match kind {
            "has" => filters.push(OwnedAttrFilter::Has(value.to_string())),
            "lacks" => filters.push(OwnedAttrFilter::Lacks(value.to_string())),
            _ => bail!("Unsupported attr-filter type: `{kind}`, only supports has / lacks"),
        }
    }

    Ok(filters)
}

fn to_domi_filters(filters: &[OwnedAttrFilter]) -> Option<Vec<AttrFilter<'_>>> {
    if filters.is_empty() {
        return None;
    }

    Some(
        filters
            .iter()
            .map(|f| match f {
                OwnedAttrFilter::Has(v) => AttrFilter::Has(v.as_str()),
                OwnedAttrFilter::Lacks(v) => AttrFilter::Lacks(v.as_str()),
            })
            .collect(),
    )
}

fn build_domi_text_for_base(geosite: &GeoSiteList, base: &str) -> Result<String> {
    let site = geosite
        .entry
        .iter()
        .find(|s| s.country_code.eq_ignore_ascii_case(base))
        .with_context(|| format!("Base `{base}` does not exist"))?;

    let mut lines = Vec::with_capacity(site.domain.len());

    for d in &site.domain {
        let prefix = match domain::Type::try_from(d.r#type).unwrap_or(domain::Type::Plain) {
            domain::Type::Plain => "keyword",
            domain::Type::Regex => "regexp",
            domain::Type::RootDomain => "domain",
            domain::Type::Full => "full",
        };

        let mut line = format!("{prefix}:{}", d.value.trim());

        for attr in &d.attribute {
            if !attr.key.trim().is_empty() {
                line.push_str(" @");
                line.push_str(attr.key.trim());
            }
        }

        lines.push(line);
    }

    Ok(lines.join("\n"))
}

fn build_rule_set_json(rules: &[Rule], version: u8) -> Result<String> {
    let merged_rule = merge_rules_by_json_keys(rules)?;
    let mut root = serde_json::Map::new();
    root.insert("version".to_string(), serde_json::Value::from(version));
    root.insert(
        "rules".to_string(),
        serde_json::Value::Array(vec![serde_json::Value::Object(merged_rule)]),
    );
    serde_json::to_string_pretty(&serde_json::Value::Object(root))
        .context("JSON serialization failed")
}

fn merge_rules_by_json_keys(rules: &[Rule]) -> Result<serde_json::Map<String, serde_json::Value>> {
    // key -> deduped value set. Use BTree* for stable output.
    let mut merged: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for rule in rules {
        let value = serde_json::to_value(rule).context("Rule serialization failed")?;
        let serde_json::Value::Object(obj) = value else {
            continue;
        };

        for (key, value) in obj {
            let serde_json::Value::Array(arr) = value else {
                continue;
            };
            let bucket = merged.entry(key).or_default();
            for item in arr {
                if let serde_json::Value::String(s) = item {
                    bucket.insert(s);
                }
            }
        }
    }

    let mut out = serde_json::Map::new();
    for (key, values) in merged {
        let arr = values
            .into_iter()
            .map(serde_json::Value::String)
            .collect::<Vec<_>>();
        out.insert(key, serde_json::Value::Array(arr));
    }
    Ok(out)
}
