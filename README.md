# domi-cli 使用指南

`domi-cli` 用于把 geosite 规则转换为 sing-box 的 JSON 格式规则集

## 配置结构

配置文件使用两层：
- `[__config__]`：全局默认配置
- `[cn]` / `[global]` 等：entry（用户可自定义命名）

每个 entry 对应一个规则任务和一个 JSON 输出文件。
同一任务内如果配置了多个 `bases`，会默认深度合并成一个 rule（而不是生成多个分散 rule）。

```toml
[__config__]
geosite_url = "https://example.com/geosite.dat"
geosite_path = "./data/geosite.dat"
version = 2

[cn]
bases = ["google", "openai"]
attr_filters = ["has:cn", "lacks:ads"]
output = "./rules-cn.json"

[global]
bases = ["google", "openai", "anthropic"]
attr_filters = ["lacks:cn"]
output = "./rules-global.json"
```

字段说明：
- `geosite_url`：下载链接（可选）。设置后会先下载，再读取本地文件
- `geosite_path`：下载保存路径，或本地 geosite 文件路径（必填）
- `bases`：要导出的 geosite base 列表
- `attr_filters`：属性过滤，格式 `has:xxx` / `lacks:xxx`
- `version`：输出规则集 JSON 中的 `version` 字段，默认 `2`
- `output`：JSON 输出路径

## 运行方式

### 仅传递配置文件路径

```bash
domi-cli --config </path/to/your/config>
```

- 会遍历配置里的所有 entry。
- 若某个 entry 没写 `output`，默认输出 `<entry>.json`（例如 `cn.json`）。

### 传递 entry 参数

```bash
domi-cli --config config.toml --entry cn
```

或

```bash
domi-cli --config config.toml --entry cn --entry global
```

- 此时仅执行 entry 内配置，可用于单独调试

## 命令行模式

```bash
domi-cli --geosite ./geosite.dat --base google --base microsoft --output ./rules.json --attr-filter "lacks:cn"
```

## 命令行参数

- `--config <FILE>`：配置文件路径
- `--entry <NAME>`：只生成指定 entry（可重复）
- `--geosite-url <URL>`：覆盖 `geosite_url`
- `--geosite <FILE>`：覆盖 `geosite_path`
- `--base <BASE>`：覆盖 `bases`（可重复）
- `-o, --output <FILE>`：覆盖 `output`
- `--set-version <N>`：覆盖配置里的 `version`，手动指定规则集中的 `version` 值
- `--attr-filter <RULE>`：覆盖 `attr_filters`
- `-V, --version`：输出版本号

## 覆盖优先级

从高到低：
1. 命令行参数
2. entry（如 `[cn]`）
3. `[__config__]`

## 范围限制

- 只支持域名规则 geodata
- 只输出 sing-box 规则集 JSON
