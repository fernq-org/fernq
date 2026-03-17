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

## 快速开始

### 前置要求

- Rust 1.80+ (edition 2024)
- Linux 环境（Unix Socket 支持）

### 从源码构建

```bash
git clone https://github.com/fernq-org/fernq.git
cd fernq
cargo build --release
```

### 启动服务器

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

### 管理房间

```bash
# 添加新房间
./target/release/fernq add --name myroom --password secret

# 列出所有房间
./target/release/fernq list

# 删除房间
./target/release/fernq remove --id <uuid>
```

### 获取服务地址

```bash
./target/release/fernq address
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
