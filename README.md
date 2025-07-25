# Pyro - Nix-like Package Manager for Rust

Pyro is a modern, Nix-inspired package manager that brings declarative package management, reproducible builds, and immutable storage to the Rust ecosystem.

## Features

### 🔒 **Nix-like Architecture**
- **Immutable Store**: Content-addressable package storage
- **Reproducible Builds**: Sandboxed, deterministic builds
- **Declarative Configuration**: Define your system state in `pyro.toml`
- **Atomic Operations**: Safe package installations and rollbacks
- **Garbage Collection**: Automatic cleanup of unused packages

### 📦 **Package Management**
- Install packages from crates.io, Git repositories, or local paths
- Dependency resolution with topological sorting
- Parallel builds with configurable job limits
- Build caching for faster subsequent builds

### 🎨 **User Interface**
- **CLI Mode**: Traditional command-line interface
- **TUI Mode**: Beautiful terminal UI with real-time progress
- Dependency graph visualization
- Build logs and progress tracking

## Installation

```bash
# Clone the repository
git clone https://github.com/pyro-linux/pyro.git
cd pyro

# Build Pyro
cargo build --release

# Install to your PATH
cargo install --path pyro
```

## Quick Start

### 1. Initialize Configuration

```bash
# Create a new pyro.toml configuration
pyro init
```

### 2. Install Packages

```bash
# Install packages from crates.io
pyro install ripgrep@14.1.0 bat fd-find

# Install from Git repository
pyro install git+https://github.com/ogham/exa.git

# Install user packages (no admin required)
pyro install --user bat exa
```

### 3. Manage Your System

```bash
# List installed packages
pyro list

# Update packages
pyro update

# Remove packages
pyro remove old-package

# Garbage collect unused packages
pyro gc

# Show store information
pyro store-info
```

## Configuration

Pyro uses a declarative configuration file (`pyro.toml`) to define your system state:

```toml
# System packages (require admin privileges)
[[system_packages]]
name = "ripgrep"
version = "14.1.0"
source = { Crates = { name = "ripgrep", version = "14.1.0" } }
build_inputs = ["libc"]
runtime_inputs = []
environment = { "RUST_BACKTRACE" = "1" }

# User packages
[[user_packages]]
name = "bat"
version = "0.24.0"
source = { Crates = { name = "bat", version = "0.24.0" } }
build_inputs = ["libgit2-sys", "onig_sys"]
runtime_inputs = ["git"]
environment = { "BAT_THEME" = "GitHub" }

# Build configuration
[build_config]
max_jobs = 4
sandbox = true
pure_builds = true
cache_builds = true

# Store configuration
[store_config]
store_path = "/nix/store"
auto_gc = false
max_store_size = 10737418240  # 10GB
```

## Package Sources

Pyro supports multiple package sources:

### Crates.io
```toml
source = { Crates = { name = "ripgrep", version = "14.1.0" } }
```

### Git Repository
```toml
source = { Git = { url = "https://github.com/ogham/exa.git", rev = "master" } }
```

### Local Path
```toml
source = { Path = { path = "./local-packages/my-tool" } }
```

### URL Archive
```toml
source = { Url = { url = "https://example.com/package.tar.gz", hash = "sha256:abc123..." } }
```

## Advanced Features

### Custom Build Scripts

```toml
[[user_packages]]
name = "custom-tool"
source = { Path = { path = "./custom-tool" } }
build_script = """
#!/bin/bash
echo "Custom build process..."
cargo build --release --features special
cp target/release/custom-tool $out/bin/
"""
```

### Dependency Graphs

```bash
# Show dependency graph in DOT format
pyro graph ripgrep --format dot

# Generate JSON dependency information
pyro graph bat --format json
```

### Build from Specification

```bash
# Build package from specification file
pyro build package-spec.toml
```

### TUI Mode

```bash
# Launch interactive TUI (no arguments)
pyro
```

The TUI provides:
- Real-time build progress
- Dependency tree visualization
- Build logs
- Interactive package management

## Store Management

Pyro uses a content-addressable store similar to Nix:

```
/nix/store/
├── abc123-ripgrep-14.1.0/
│   ├── bin/rg
│   └── share/...
├── def456-bat-0.24.0/
│   ├── bin/bat
│   └── share/...
└── .pyro-store.json  # Store metadata
```

### Store Operations

```bash
# Show store statistics
pyro store-info

# Garbage collect (remove unused packages)
pyro gc

# Dry run garbage collection
pyro gc --dry-run
```

## Architecture

### Core Components

- **Config System** (`config.rs`): Declarative configuration management
- **Store** (`store.rs`): Immutable package storage with content addressing
- **Builder** (`builder.rs`): Sandboxed, reproducible package builds
- **CLI** (`cli.rs`): Command-line interface
- **UI** (`ui.rs`): Terminal user interface

### Key Concepts

1. **Immutability**: Packages are never modified after installation
2. **Content Addressing**: Package paths are derived from their content hash
3. **Reproducibility**: Builds are deterministic and sandboxed
4. **Declarative**: System state is defined in configuration files
5. **Atomic**: Operations either succeed completely or fail safely

## Comparison with Other Package Managers

| Feature | Pyro | Nix | Cargo | apt/yum |
|---------|------|-----|-------|----------|
| Immutable Store | ✅ | ✅ | ❌ | ❌ |
| Reproducible Builds | ✅ | ✅ | ⚠️ | ❌ |
| Declarative Config | ✅ | ✅ | ⚠️ | ❌ |
| Rollbacks | ✅ | ✅ | ❌ | ⚠️ |
| Multiple Versions | ✅ | ✅ | ❌ | ❌ |
| Sandboxed Builds | ✅ | ✅ | ❌ | ❌ |
| Garbage Collection | ✅ | ✅ | ⚠️ | ⚠️ |

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Submit a pull request

## License

MIT License - see [LICENSE](LICENSE) for details.

## Roadmap

- [ ] Package registry integration
- [ ] Binary cache support
- [ ] Cross-compilation support
- [ ] Profile management
- [ ] Flake-like functionality
- [ ] Integration with existing Rust toolchain
- [ ] Windows and macOS support
- [ ] Package signing and verification