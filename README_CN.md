# FernQ

🇺🇸 [English](./README.md)

---

基于 Rust 开发的高性能轻量级消息队列和房间管理系统。

## 特性

- **高并发** - 基于 Tokio 异步 I/O，支持数千并发连接
- **房间管理** - 支持创建、删除和灵活配置多个房间
- **自定义协议** - 二进制协议，支持分帧传输、CRC 校验和心跳机制
- **CLI 控制** - 基于 Unix Socket 的命令行工具，便于服务管理
- **持久化存储** - 使用 fjall 键值数据库持久化房间数据
- **优雅关闭** - 完善的连接处理和资源清理机制

## 安装

### 快速安装（推荐）

使用预编译的发布包快速安装：

```bash
wget https://github.com/fernq-org/fernq/releases/download/v0.1.0/fernq-v0.1.0-linux-x64.tar.gz
tar xzvf fernq-v0.1.0-linux-x64.tar.gz
cd install
./install.sh
```

安装脚本会自动完成：
- 将二进制文件复制到 `/usr/local/bin/`
- 设置系统服务
- 创建配置目录

### 从源码构建

前置要求：
- Rust 1.80+ (edition 2024)
- Linux 环境（Unix Socket 支持）

```bash
git clone https://github.com/fernq-org/fernq.git
cd fernq
cargo build --release
```

## 快速开始

### 启动服务器

**开发模式：**

```bash
# 创建配置文件
cat > config/config.ini << EOF
[server]
host = "0.0.0.0"
port = 8080
EOF

# 启动服务
./target/release/fernqd --config config/config.ini --storage ./data
```

**生产模式（作为系统服务）：**

```bash
sudo ./target/release/fernqd --config /etc/fernq/config.ini --storage /var/lib/fernq
```

### 管理房间

```bash
# 添加新房间
fernq add --name myroom --password secret

# 列出所有房间
fernq list

# 列出所有房间（显示密码）
fernq list --show-pwd

# 删除房间（通过 UUID）
fernq remove --id <uuid>
```

### 服务器信息

```bash
# 获取服务器监听地址
fernq address
```

## 命令行工具

### 全局选项

- `-s, --socket <路径>` - 指定 Unix socket 路径（默认：`/run/fernq/fernq.sock`）
- `--dev` - 开发模式（使用 `/tmp/fernq.sock`）
- `--json` - 以 JSON 格式输出
- `--show-pwd` - 显示密码明文（默认隐藏）

### 命令列表

**`add`** - 添加新房间
```bash
fernq add -n <房间名> -p <密码>
fernq add --name <房间名> --password <密码>
```

**`remove`** - 删除房间（通过 UUID）
```bash
fernq remove -i <uuid>
fernq remove --id <uuid>
```

**`list`** - 列出所有房间
```bash
fernq list
fernq list --show-pwd  # 显示密码
```

**`address`** - 获取服务器监听地址
```bash
fernq address
```

**`uninstall`** - 卸载 FernQ（需要 root 权限）
```bash
sudo fernq uninstall
```

查看更多选项：
```bash
fernq --help
```

**`fernq list` 命令输出示例：**
```
Room ID                               Name                 Password
--------------------------------------------------------------------
550e8400-e29b-41d4-a716-446655440000 myroom              ******
f47ac10b-58cc-4372-a567-0e02b2c3d479 testroom            ******

Total: 2 room(s)
```

## 架构设计

```
fernq/
├── fernq-core/     # 协议实现
│   ├── protocol/   # 编解码、校验
├── fernqd/         # 服务器守护进程
│   ├── server/     # 房间管理、连接处理
│   └── storage/    # 数据库操作
└── fernqctl/       # CLI 控制工具
```

## 协议说明

FernQ 使用自定义二进制协议，专为高性能消息传输设计：

- **魔数** - 协议标识
- **版本号** - 兼容性控制
- **帧长度限制** - 单帧最大 8KB
- **流长度限制** - 消息最大 8MB
- **CRC 校验** - 数据完整性验证
- **心跳机制** - 连接保活
- **分帧传输** - 支持大消息拆分

## 项目状态

🚧 **开发中** - 这是一个协议层实现。安全特性（加密、认证）应由用户在应用层自行实现。

## 开源协议

MIT License - 详见 [LICENSE](./LICENSE)

## 贡献

欢迎贡献代码！请随时提交 Pull Request。
