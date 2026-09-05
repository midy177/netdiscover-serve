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
| `netdiscover-serve-x86_64-unknown-linux-gnu` | Linux x86_64（glibc） |
| `netdiscover-serve-x86_64-unknown-linux-musl` | Linux x86_64（静态链接，任意发行版可用） |
| `netdiscover-serve-aarch64-unknown-linux-gnu` | Linux ARM64（glibc） |
| `netdiscover-serve-aarch64-unknown-linux-musl` | Linux ARM64（静态链接） |
| `netdiscover-serve-x86_64-apple-darwin` | macOS（Intel） |
| `netdiscover-serve-aarch64-apple-darwin` | macOS（Apple Silicon） |

```sh
curl -LO https://github.com/<org>/<repo>/releases/latest/download/netdiscover-serve-x86_64-unknown-linux-musl
chmod +x netdiscover-serve-x86_64-unknown-linux-musl
./netdiscover-serve-x86_64-unknown-linux-musl -field privatev4
```

> 建议下载 `SHA256SUMS.txt` 校验文件完整性：`sha256sum --ignore-missing -c SHA256SUMS.txt`

### 从源码构建

需要 Rust 1.85+ 工具链：

```sh
cargo install --path .
```

## Docker

```sh
docker run --rm -p 8080:8080/tcp -p 8080:8080/udp ghcr.io/<org>/<repo>:latest
```

镜像默认启动 `netdiscover-serve -serve`。

Linux 主机网络部署：

```sh
docker run -d --name netdiscover-serve --network host --restart unless-stopped ghcr.io/midy177/netdiscover-serve:latest
docker compose -f compose.host-network.yml up -d
```

使用主机网络时，容器会直接监听宿主机的 `0.0.0.0:8080` TCP/UDP；Compose 中不要再配置 `ports`。

本地构建镜像时，先用 zigbuild 编译 Linux 二进制，再复制进 runtime 镜像：

```sh
cargo zigbuild --locked --release --target aarch64-unknown-linux-musl
mkdir -p dist/docker
cp target/aarch64-unknown-linux-musl/release/netdiscover-serve dist/docker/netdiscover-serve-arm64
docker build -t netdiscover-serve:test .
```

## 快速上手

```console
$ netdiscover-serve -field publicv4    # 只查公网 IPv4
203.0.113.7

$ netdiscover-serve -field privatev4   # 只查内网 IPv4
10.0.0.5

$ netdiscover-serve                    # 全量查询，输出单行 JSON
{"hostname":"node1.example.com","private_ipv4":"10.0.0.5","public_ipv4":"203.0.113.7","public_ipv6":"","client_ip":"","client_port":0}

$ netdiscover-serve -debug             # 显示各项发现的详细过程与失败原因
2026/09/03 12:14:51 underlay: default route device is en0
2026/09/03 12:14:51 underlay: UDP probe selected source address 10.10.148.41
{"hostname":"","private_ipv4":"10.10.148.41","public_ipv4":"203.0.113.7","public_ipv6":"","client_ip":"","client_port":0}

$ netdiscover-serve -serve -listen 0.0.0.0:8080 # 同时启动 TCP/UDP 服务
2026/09/03 12:14:51 server: TCP listening on 0.0.0.0:8080
2026/09/03 12:14:51 server: UDP listening on 0.0.0.0:8080
```

失败的字段在 JSON 中为空字符串；加 `-debug` 可在 stderr 看到每项失败的具体原因（日志格式与 Go 版一致）。
服务模式下，主机名、内网 IP、公网 IP 在启动时只发现一次并缓存。请求 payload 为 `discover` 时，TCP 连接或 UDP 报文会收到这份缓存结果，并额外填充本次请求的 `client_ip` 和 `client_port`；其他非空 payload 会被原样 echo。UDP 空包会被忽略。

TCP 请求按行处理：服务端读取到第一个 `\n`、EOF、2 秒读取超时或 4 KiB 请求上限后立即响应并关闭连接。

```sh
printf 'discover\n' | nc 127.0.0.1 8080
printf 'discover\n' | nc -u -w 1 127.0.0.1 8080
```

## 命令行参考

```
Usage of netdiscover-serve:
  -debug
    	debug mode
  -field string
    	return only a single field.  Options are: "hostname", "publicv4", publicv6", "privatev4"
  -provider string
    	provider type.  Options are: "aws", "azure", "do", gcp"
  -serve
    	run TCP and UDP response service
  -listen string
    	listen address for -serve TCP and UDP service (default "0.0.0.0:8080")
  -tcp string
    	run TCP response service on this address
  -udp string
    	run UDP response service on this address
```

| 参数 | 说明 |
|---|---|
| `-field <name>` | 只返回单个字段；省略（或传空串）返回完整 JSON |
| `-debug` | 在 stderr 输出各发现项的失败原因与过程日志 |
| `-provider <name>` | 兼容原 Go CLI 的占位参数，取值被忽略 |
| `-serve` | 同时启动 TCP 和 UDP 服务，默认监听 `0.0.0.0:8080` |
| `-listen <addr>` | `-serve` 模式下 TCP 和 UDP 共用的监听地址 |
| `-tcp <addr>` | 只启动 TCP 服务；与 `-serve` 同用时覆盖 TCP 监听地址 |
| `-udp <addr>` | 只启动 UDP 服务；与 `-serve` 同用时覆盖 UDP 监听地址 |
| `-h` / `-help` | 打印用法说明 |

**可用字段：**

| 字段 | JSON 键 | 含义 |
|---|---|---|
| `hostname` | `hostname` | 系统主机名（`gethostname(2)` 系统调用，不依赖 DNS） |
| `privatev4` | `private_ipv4` | 内网（underlay）IPv4 |
| `publicv4` | `public_ipv4` | 公网 IPv4 |
| `publicv6` | `public_ipv6` | 公网 IPv6 |
| 不适用 | `client_ip` | 服务模式下的请求来源 IP；CLI 模式为空字符串 |
| 不适用 | `client_port` | 服务模式下的请求来源端口；CLI 模式为 `0` |

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

1. 通过 `gethostname(2)` 系统调用直接读取系统主机名（不执行命令、不依赖 DNS），去掉末尾的点

## 常见问题

- **公网 IP 为空**：公网发现需要能访问外部 STUN 服务器（UDP）或 HTTPS 接口。受限网络下可用 `PUBLIC_IP` 直接指定，或配置可达的 `STUN_SERVERS`
- **主机名为空**：主机名来自 `gethostname(2)` 系统调用，仅在系统未设置主机名或主机名会被截断为空时才会为空，正常环境不会出现
- **多出口/策略路由环境返回了 VPN 地址**：内网探测返回的是"实际出网路径"的源地址。若需指定接口的地址，用 `UNDERLAY_IP` 直接指定；若需特定出口的公网 IP，在 `STUN_SERVERS` 中指定一个走该出口的 STUN 服务器
- **在容器内运行**：容器内查到的"内网 IP"是容器网络视角的地址。若需要宿主机地址，建议以 `hostNetwork` 方式运行，或通过环境变量注入

## 与 Go 版本的兼容性

- 命令行参数、用法文本、退出码完全一致
- 单字段查询输出纯值 + 换行；全量查询输出单行 JSON，所有键固定顺序、恒存在
- `-provider` 与 `CLOUD_PROVIDER` 仅作兼容占位，不再区分云厂商（统一引擎覆盖原有场景）

## 许可证

Apache License 2.0，详见 [LICENSE](LICENSE)。
