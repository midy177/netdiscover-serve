# Netdiscover

[English](README.md) | 简体中文

Netdiscover 是一个命令行工具，用于发现节点的网络信息：主机名（hostname）、内网/底层 IPv4（underlay）以及公网 IPv4/IPv6。典型使用场景是在 Kubernetes 或容器内运行时，发现容器所在节点的公网 IP 与主机名——这通常是配置 VoIP 应用所必需的。

本工具是 Rust（edition 2024）实现，命令行参数与输出格式与原 Go 版本完全兼容。

## 特性

- **零配置**：无需指定云厂商，统一发现引擎自动工作于裸机、虚拟机、容器与各类云平台
- **多路径回退**：公网 IP 通过 STUN 获取（可自定义服务器），失败后自动回退 HTTPS 接口；内网 IP 通过内核路由决策获取，不发送任何报文
- **环境变量直通**：`UNDERLAY_IP` / `PUBLIC_IP` 直接指定结果，适配固定 IP 或复杂路由环境
- **单二进制**：无运行时依赖，musl 静态版本可在任意 Linux 发行版直接运行

## 安装

### 从 Release 下载

在 [Releases](../../releases) 页面下载对应平台的二进制文件（裸文件，无需解压）：

| 文件名 | 平台 |
|---|---|
| `netdiscover-x86_64-unknown-linux-gnu` | Linux x86_64（glibc） |
| `netdiscover-x86_64-unknown-linux-musl` | Linux x86_64（静态链接，任意发行版可用） |
| `netdiscover-aarch64-unknown-linux-gnu` | Linux ARM64（glibc） |
| `netdiscover-aarch64-unknown-linux-musl` | Linux ARM64（静态链接） |
| `netdiscover-x86_64-apple-darwin` | macOS（Intel） |
| `netdiscover-aarch64-apple-darwin` | macOS（Apple Silicon） |

```sh
curl -LO https://github.com/<org>/netdiscover/releases/latest/download/netdiscover-x86_64-unknown-linux-musl
chmod +x netdiscover-x86_64-unknown-linux-musl
./netdiscover-x86_64-unknown-linux-musl -field privatev4
```

> 建议下载 `SHA256SUMS.txt` 校验文件完整性：`sha256sum --ignore-missing -c SHA256SUMS.txt`

### 从源码构建

需要 Rust 1.85+ 工具链：

```sh
cargo install --path .
```

## 快速上手

```console
$ netdiscover -field publicv4          # 只查公网 IPv4
203.0.113.7

$ netdiscover -field privatev4         # 只查内网 IPv4
10.0.0.5

$ netdiscover                          # 全量查询，输出单行 JSON
{"hostname":"node1.example.com","private_ipv4":"10.0.0.5","public_ipv4":"203.0.113.7","public_ipv6":""}

$ netdiscover -debug                   # 显示各项发现的详细过程与失败原因
2026/09/03 12:14:51 underlay: default route device is en0
2026/09/03 12:14:51 underlay: UDP probe selected source address 10.10.148.41
{"hostname":"","private_ipv4":"10.10.148.41","public_ipv4":"203.0.113.7","public_ipv6":""}
```

失败的字段在 JSON 中为空字符串；加 `-debug` 可在 stderr 看到每项失败的具体原因（日志格式与 Go 版一致）。

## 命令行参考

```
Usage of netdiscover:
  -debug
    	debug mode
  -field string
    	return only a single field.  Options are: "hostname", "publicv4", publicv6", "privatev4"
  -provider string
    	provider type.  Options are: "aws", "azure", "do", gcp"
```

| 参数 | 说明 |
|---|---|
| `-field <name>` | 只返回单个字段；省略（或传空串）返回完整 JSON |
| `-debug` | 在 stderr 输出各发现项的失败原因与过程日志 |
| `-provider <name>` | 兼容原 Go CLI 的占位参数，取值被忽略 |
| `-h` / `-help` | 打印用法说明 |

**可用字段：**

| 字段 | JSON 键 | 含义 |
|---|---|---|
| `hostname` | `hostname` | 公网主机名（公网 IPv4 的反向 DNS） |
| `privatev4` | `private_ipv4` | 内网（underlay）IPv4 |
| `publicv4` | `public_ipv4` | 公网 IPv4 |
| `publicv6` | `public_ipv6` | 公网 IPv6 |

**退出码：**

| 码 | 含义 |
|---|---|
| `0` | 成功（含 `-h`） |
| `1` | 所请求字段发现失败，或 `-field` 值非法 |
| `2` | 命令行语法错误 |

## 环境变量

| 变量 | 作用 |
|---|---|
| `UNDERLAY_IP` | 直接以此 IPv4 作为内网 IP，跳过探测 |
| `PUBLIC_IP` | 直接以此 IPv4 作为公网 IP，跳过探测 |
| `STUN_SERVERS` | 自定义 STUN 服务器（逗号分隔，格式 `host[:port]`，默认端口 3478），优先于内置列表 |
| `STUN_SERVER` | `STUN_SERVERS` 的别名 |
| `CLOUD_PROVIDER` | 兼容原 Go 版本而保留，取值被忽略 |

## 工作原理

各项信息按以下优先级链解析，首个成功的方法生效：

**内网 IPv4（underlay）**

1. `UNDERLAY_IP` 环境变量
2. UDP 路由探测：向公网目标发起 UDP connect，**不发送任何报文**，由内核按路由表（含策略路由）选出实际使用的源地址
3. 默认路由设备上的首个全局单播 IPv4
4. 任意非 docker、非回环接口上的首个全局单播 IPv4

**公网 IPv4 / IPv6**

1. `PUBLIC_IP` 环境变量
2. STUN 协议（RFC 5389）：优先使用 `STUN_SERVERS` 配置的服务器，其次内置公共服务器列表
3. HTTPS 接口兜底：`api.ip.sb`、`icanhazip.com`、`ifconfig.me`、`api.ipify.org`（v4）；`api6.ip.sb`、`api64.ipify.org`（v6）

**主机名**

1. 解析公网 IPv4（同上链路）
2. 反向 DNS 查询
3. 过滤不可信结果（过短、无域名点、`.local`），返回首个有效主机名

## 常见问题

- **公网 IP 为空**：公网发现需要能访问外部 STUN 服务器（UDP）或 HTTPS 接口。受限网络下可用 `PUBLIC_IP` 直接指定，或配置可达的 `STUN_SERVERS`
- **主机名为空**：主机名来自公网 IP 的 PTR 记录，云厂商通常需要手动配置反向解析；未配置时该字段为空属正常现象
- **多出口/策略路由环境返回了 VPN 地址**：内网探测返回的是"实际出网路径"的源地址。若需指定接口的地址，用 `UNDERLAY_IP` 直接指定；若需特定出口的公网 IP，在 `STUN_SERVERS` 中指定一个走该出口的 STUN 服务器
- **在容器内运行**：容器内查到的"内网 IP"是容器网络视角的地址。若需要宿主机地址，建议以 `hostNetwork` 方式运行，或通过环境变量注入

## 与 Go 版本的兼容性

- 命令行参数、用法文本、退出码完全一致
- 单字段查询输出纯值 + 换行；全量查询输出单行 JSON，四个键固定顺序、恒存在
- `-provider` 与 `CLOUD_PROVIDER` 仅作兼容占位，不再区分云厂商（统一引擎覆盖原有场景）

## 许可证

Apache License 2.0，详见 [LICENSE](LICENSE)。
