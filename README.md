# domi-cli

将 geodata 中域名规则转换为 sing-box JSON 格式规则集

构建
```bash
cargo build --release
```

使用示例
```bash
domi-cli --geosite ./geosite.dat --base google --base microsoft --output ./rules.json --attr-filter "lacks:cn"
```

## 使用命令行

### 子命令

- `list-attrs`：输出 `geosite.dat` 中去重后的属性标签

### 命令参数

- `--config <FILE>`：配置文件路径
- `--entry <NAME>`：指定 entry（可重复）
- `--geosite-url <URL>`：指定 geosite 下载链接
- `--geosite <FILE>`：指定 geosite 保存路径
- `--base <BASE>`：指定 `bases`（可重复）
- `--output <FILE>`：文件输出路径
- `--set-version <N>`：指定规则集中的 `version` 值
- `--attr-filter <RULE>`：覆盖 `attr_filters`


## 使用配置文件

- `[__config__]`：全局默认配置
- `[cn]` / `[global]` 等：entry（用户可自定义命名）

每个 entry 对应一个规则任务和一个 JSON 输出文件。同一任务内如果配置了多个 `bases`，会默认深度合并

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
- `geosite_url`：下载链接。设置后会先下载，再读取本地文件
- `geosite_path`：下载保存路径，或本地 geosite 文件路径（必填）
- `bases`：要导出的 geosite base 列表
- `attr_filters`：属性过滤，格式 `has:xxx` / `lacks:xxx`
- `version`：输出规则集 JSON 中的 `version` 字段，默认 `2`
- `output`：JSON 输出路径

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

此时仅执行 entry 内配置，可用于单独调试
