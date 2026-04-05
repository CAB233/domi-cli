use std::{
    collections::HashMap,
    collections::{BTreeMap, BTreeSet},
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
    #[arg(long = "config")]
    config: Option<PathBuf>,

    #[arg(long = "entry")]
    entries: Vec<String>,
}

#[derive(Debug, Args, Clone)]
struct ListAttrsArgs {
    /// geosite.dat 文件路径
    #[arg(index = 1)]
    geosite: PathBuf,
}

#[derive(Debug, Subcommand)]
enum Commands {
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

/// 一组可合并的配置字段。
#[derive(Debug, Default, Clone, Deserialize)]
struct ConfigScope {
    geosite_url: Option<String>,
    geosite_path: Option<PathBuf>,
    bases: Option<Vec<String>>,
    output: Option<PathBuf>,
    set_version: Option<u8>,
    attr_filters: Option<Vec<String>>,
}

/// 配置结构：
/// - [__config__] 为全局默认
/// - 其余的表为 entry
#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    #[serde(rename = "__config__")]
    config: Option<ConfigScope>,

    #[serde(flatten)]
    entries: HashMap<String, ConfigScope>,
}

#[derive(Debug)]
struct EffectiveConfig {
    entry_name: Option<String>,
    geosite_url: Option<String>,
    geosite_path: PathBuf,
    bases: Vec<String>,
    output: Option<PathBuf>,
    set_version: u8,
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
            for job in jobs {
                run_one(job)?;
            }
            Ok(())
        }
    }
}

fn run_one(config: EffectiveConfig) -> Result<()> {
    // 如果提供了 geosite_url，先下载到 geosite_path。
    if let Some(url) = &config.geosite_url {
        download_geosite(url, &config.geosite_path)?;
    }

    let bytes = fs::read(&config.geosite_path)
        .with_context(|| format!("读取 geosite 文件失败: {}", config.geosite_path.display()))?;

    let geosite = GeoSiteList::decode(bytes.as_slice()).context("geosite.dat protobuf 解码失败")?;

    let filters = parse_attr_filters(&config.attr_filters)?;
    let mut rules = Vec::new();

    for base in &config.bases {
        let text = build_domi_text_for_base(&geosite, base)
            .with_context(|| format!("在 geosite 中找不到 base: {base}"))?;

        // 使用 domi 的 flatten + AttrFilter，保证行为一致。
        let mut entries = Entries::parse(base, text.lines());
        let domi_filters = to_domi_filters(&filters);
        let selected = entries
            .flatten(base, domi_filters.as_deref())
            .with_context(|| format!("base `{base}` 经过过滤后没有可用域名"))?;

        rules.push(Rule::from(selected));
    }

    // 多个 base 默认深度合并为一个 rule。按键动态合并，避免写死字段名。
    let json = build_rule_set_json(rules, config.set_version)?;

    if let Some(output) = &config.output {
        fs::write(output, &json)
            .with_context(|| format!("写入 JSON 文件失败: {}", output.display()))?;
    } else {
        if let Some(name) = &config.entry_name {
            println!("# entry: {name}");
        }
        println!("{json}");
    }

    Ok(())
}

fn download_geosite(url: &str, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建 geosite 目录失败: {}", parent.display()))?;
    }

    let response = reqwest::blocking::get(url)
        .with_context(|| format!("下载 geosite 失败: {url}"))?
        .error_for_status()
        .with_context(|| format!("下载 geosite 返回非 2xx 状态: {url}"))?;

    let bytes = response.bytes().context("读取下载响应体失败")?;

    let mut file = fs::File::create(path)
        .with_context(|| format!("创建 geosite 文件失败: {}", path.display()))?;
    file.write_all(bytes.as_ref())
        .with_context(|| format!("写入 geosite 文件失败: {}", path.display()))?;

    Ok(())
}

fn load_geosite(path: &Path) -> Result<GeoSiteList> {
    let bytes =
        fs::read(path).with_context(|| format!("读取 geosite 文件失败: {}", path.display()))?;
    GeoSiteList::decode(bytes.as_slice()).context("geosite.dat protobuf 解码失败")
}

fn load_config(path: &Path) -> Result<ConfigFile> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("读取配置文件失败: {}", path.display()))?;
    let parsed: ConfigFile = toml::from_str(&raw)
        .with_context(|| format!("配置文件 TOML 解析失败: {}", path.display()))?;
    Ok(parsed)
}

fn merge_scope(base: ConfigScope, overlay: ConfigScope) -> ConfigScope {
    ConfigScope {
        geosite_url: overlay.geosite_url.or(base.geosite_url),
        geosite_path: overlay.geosite_path.or(base.geosite_path),
        bases: overlay.bases.or(base.bases),
        output: overlay.output.or(base.output),
        set_version: overlay.set_version.or(base.set_version),
        attr_filters: overlay.attr_filters.or(base.attr_filters),
    }
}

fn apply_convert_override(_scope: ConfigScope, _cli: &ConvertArgs) -> ConfigScope {
    ConfigScope::default()
}

fn scope_to_effective(
    scope: ConfigScope,
    entry_name: Option<String>,
    auto_output_by_entry: bool,
) -> Result<EffectiveConfig> {
    let geosite_path = scope
        .geosite_path
        .context("缺少 geosite_path：请传 --geosite，或在配置里设置 geosite_path")?;

    let bases = scope.bases.unwrap_or_default();
    if bases.is_empty() {
        bail!("缺少 base：请传 --base，或在配置里设置 bases = [..]");
    }

    let output = scope.output.or_else(|| {
        auto_output_by_entry.then(|| {
            entry_name
                .as_ref()
                .map(|name| PathBuf::from(format!("{name}.json")))
        })?
    });

    Ok(EffectiveConfig {
        entry_name,
        geosite_url: scope.geosite_url,
        geosite_path,
        bases,
        output,
        set_version: scope.set_version.unwrap_or(2),
        attr_filters: scope.attr_filters.unwrap_or_default(),
    })
}

/// 组装实际要执行的一组任务。
fn resolve_jobs(cli: &ConvertArgs) -> Result<Vec<EffectiveConfig>> {
    let config_path = cli
        .config
        .as_ref()
        .context("缺少 --config 参数：请指定配置文件")?;

    let cfg = load_config(config_path)?;
    let global_scope = cfg.config.unwrap_or_default();

    let mut jobs = Vec::new();

    // 模式 A：明确指定 --entry，只生成选中的 entry。
    if !cli.entries.is_empty() {
        jobs.reserve(cli.entries.len());
        for name in &cli.entries {
            let entry_scope = cfg
                .entries
                .get(name)
                .cloned()
                .with_context(|| format!("配置文件中不存在 entry `{name}`"))?;
            let merged = merge_scope(global_scope.clone(), entry_scope);
            let merged = apply_convert_override(merged, cli);
            jobs.push(scope_to_effective(merged, Some(name.clone()), false)?);
        }
        return Ok(jobs);
    }

    // 模式 B：只有 --config，生成全部 entry。
    if cfg.entries.is_empty() {
        bail!("配置文件中没有可用 entry。请添加如 [cn]、[global] 这类表")
    }

    jobs.reserve(cfg.entries.len());
    for (name, entry_scope) in cfg.entries {
        let merged = merge_scope(global_scope.clone(), entry_scope);
        let merged = apply_convert_override(merged, cli);
        jobs.push(scope_to_effective(merged, Some(name), true)?);
    }

    Ok(jobs)
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

/// 解析 `has:cn` / `lacks:cn` 这种参数。
fn parse_attr_filters(raw_filters: &[String]) -> Result<Vec<OwnedAttrFilter>> {
    let mut filters = Vec::with_capacity(raw_filters.len());

    for raw in raw_filters {
        let (kind, value) = raw
            .split_once(':')
            .with_context(|| format!("attr-filter 格式错误: `{raw}`，应为 has:xxx 或 lacks:xxx"))?;

        if value.trim().is_empty() {
            bail!("attr-filter 值不能为空: `{raw}`");
        }

        match kind {
            "has" => filters.push(OwnedAttrFilter::Has(value.to_string())),
            "lacks" => filters.push(OwnedAttrFilter::Lacks(value.to_string())),
            _ => bail!("不支持的 attr-filter 类型: `{kind}`，仅支持 has / lacks"),
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
        .with_context(|| format!("base `{base}` 不存在"))?;

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

fn build_rule_set_json(rules: Vec<Rule>, version: u8) -> Result<String> {
    let merged_rule = merge_rules_by_json_keys(&rules)?;
    let mut root = serde_json::Map::new();
    root.insert("version".to_string(), serde_json::Value::from(version));
    root.insert(
        "rules".to_string(),
        serde_json::Value::Array(vec![serde_json::Value::Object(merged_rule)]),
    );
    serde_json::to_string_pretty(&serde_json::Value::Object(root)).context("JSON 序列化失败")
}

fn merge_rules_by_json_keys(rules: &[Rule]) -> Result<serde_json::Map<String, serde_json::Value>> {
    // key -> 去重后的值集合。用 BTree* 保证输出稳定。
    let mut merged: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for rule in rules {
        let value = serde_json::to_value(rule).context("Rule 序列化失败")?;
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
