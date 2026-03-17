# FernQ

🇨🇳 [中文文档](./README_CN.md)

---

A high-performance, lightweight message queue and room management system built with Rust.

## Features

- **High Concurrency** - Async I/O with Tokio, designed for thousands of concurrent connections
- **Room Management** - Create, delete, and manage multiple rooms with flexible configuration
- **Custom Protocol** - Binary protocol with frame fragmentation, CRC validation, and heartbeat mechanism
- **CLI Control** - Unix socket-based CLI for easy server management
- **Persistent Storage** - Room data persistence using fjall key-value database
- **Graceful Shutdown** - Clean connection handling and resource cleanup

## Quick Start

### Prerequisites

- Rust 1.80+ edition 2024
- Linux environment (Unix socket support)

### Build from Source

```bash
git clone https://github.com/fernq-org/fernq.git
cd fernq
cargo build --release
```

### Start Server

```bash
# Create config file
cat > config/config.ini << EOF
[server]
host = "0.0.0.0"
port = 8080
EOF

# Start server
./target/release/fernqd --config config/config.ini --storage ./data
```

### Manage Rooms

```bash
# Add a new room
./target/release/fernq add --name myroom --password secret

# List all rooms
./target/release/fernq list

# Remove a room
./target/release/fernq remove --id <uuid>
```

### Get Server Address

```bash
./target/release/fernq address
```

## Architecture

```
fernq/
├── fernq-core/     # Protocol implementation
│   ├── protocol/   # Encoding/decoding, validation
├── fernqd/         # Server daemon
│   ├── server/     # Room management, connection handling
│   └── storage/    # Database operations
└── fernqctl/       # CLI control tool
```

## Protocol

FernQ uses a custom binary protocol designed for high-performance message transmission:

- **Magic Number** - Protocol identification
- **Version** - Compatibility control
- **Frame Length Limit** - 8KB per frame
- **Stream Length Limit** - 8MB per message
- **CRC Checksum** - Data integrity validation
- **Heartbeat** - Connection keepalive
- **Fragmentation** - Support for large message splitting

## Project Status

🚧 **Work in Progress** - This is a protocol layer implementation. Security features (encryption, authentication) should be implemented by users at the application layer.

## License

MIT License - see [LICENSE](./LICENSE) for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
