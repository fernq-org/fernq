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

## Installation

### Quick Install (Recommended)

For quick installation, use the pre-built release package:

```bash
wget https://github.com/fernq-org/fernq/releases/download/v0.1.0/fernq-v0.1.0-linux-x64.tar.gz
tar xzvf fernq-v0.1.0-linux-x64.tar.gz
cd install
./install.sh
```

The install script will:
- Copy binaries to `/usr/local/bin/`
- Set up system service
- Create configuration directories

### From Source

Prerequisites:
- Rust 1.80+ edition 2024
- Linux environment (Unix socket support)

```bash
git clone https://github.com/fernq-org/fernq.git
cd fernq
cargo build --release
```

## Quick Start

### Start Server

**Development Mode:**

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

**Production Mode (as system service):**

```bash
sudo ./target/release/fernqd --config /etc/fernq/config.ini --storage /var/lib/fernq
```

### Manage Rooms

```bash
# Add a new room
fernq add --name myroom --password secret

# List all rooms
fernq list

# List all rooms (show passwords)
fernq list --show-pwd

# Remove a room by UUID
fernq remove --id <uuid>
```

### Server Information

```bash
# Get server listening address
fernq address
```

## CLI Commands

### Global Options

- `-s, --socket <PATH>` - Specify Unix socket path (default: `/run/fernq/fernq.sock`)
- `--dev` - Development mode (uses `/tmp/fernq.sock`)
- `--json` - Output in JSON format
- `--show-pwd` - Show passwords in plain text (default: hidden)

### Commands

**`add`** - Add a new room
```bash
fernq add -n <name> -p <password>
fernq add --name <name> --password <password>
```

**`remove`** - Remove a room by UUID
```bash
fernq remove -i <uuid>
fernq remove --id <uuid>
```

**`list`** - List all rooms
```bash
fernq list
fernq list --show-pwd  # Show passwords
```

**`address`** - Get server listening address
```bash
fernq address
```

**`uninstall`** - Uninstall FernQ (requires root privileges)
```bash
sudo fernq uninstall
```

For more options, use:
```bash
fernq --help
```

**Example output for `fernq list`:**
```
Room ID                               Name                 Password
--------------------------------------------------------------------
550e8400-e29b-41d4-a716-446655440000 myroom              ******
f47ac10b-58cc-4372-a567-0e02b2c3d479 testroom            ******

Total: 2 room(s)
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
