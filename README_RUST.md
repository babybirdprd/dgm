# Darwin Gödel Machine (Rust Implementation)

This is a Rust implementation of the Darwin Gödel Machine (DGM), a self-improving AI system that iteratively modifies its own code to improve performance on coding benchmarks.

## Status

🚧 **Work in Progress** 🚧

This Rust implementation is currently a foundational conversion from the original Python codebase. The core architecture and data structures have been implemented, but many components are still placeholders.

### Completed Components

- ✅ **Core Architecture**: Main DGM runner, archive management, evolution strategy
- ✅ **Configuration System**: API keys, Docker settings, evaluation parameters
- ✅ **CLI Interface**: Command-line argument parsing with clap
- ✅ **Utilities**: Common functions, file operations, JSON handling
- ✅ **Async Foundation**: Tokio-based async runtime
- ✅ **Error Handling**: Comprehensive error types with anyhow

### TODO Components

- 🔄 **LLM Integration**: Claude and OpenAI API clients
- 🔄 **Tools System**: Bash execution and file editing tools
- 🔄 **Docker Integration**: Container management with bollard
- 🔄 **Git Operations**: Repository management with git2
- 🔄 **Evaluation Harnesses**: SWE-bench and Polyglot evaluation
- 🔄 **Self-Improvement Logic**: Code generation and patching
- 🔄 **Agent System**: Coding agent implementation
- 🔄 **Prompt Management**: Template system for LLM prompts

## Setup

### Prerequisites

- Rust 1.70+ (install from [rustup.rs](https://rustup.rs/))
- System dependencies:
  ```bash
  # Ubuntu/Debian
  sudo apt install pkg-config libssl-dev
  
  # macOS
  brew install pkg-config openssl
  
  # Fedora/RHEL
  sudo dnf install pkg-config openssl-devel
  ```

### Installation

1. Clone the repository:
   ```bash
   git clone <repository-url>
   cd dgm
   ```

2. Build the project:
   ```bash
   cargo build --release
   ```

3. Set up environment variables:
   ```bash
   export ANTHROPIC_API_KEY='your-key-here'
   export OPENAI_API_KEY='your-key-here'
   ```

## Usage

### Basic Usage

```bash
# Run with default settings
cargo run --release

# Show help
cargo run -- --help

# Run with custom parameters
cargo run -- --max-generation 10 --selfimprove-size 3 --polyglot
```

### Command Line Options

- `--max-generation <N>`: Maximum number of evolution iterations (default: 80)
- `--selfimprove-size <N>`: Number of self-improvement attempts per generation (default: 2)
- `--selfimprove-workers <N>`: Number of parallel workers (default: 2)
- `--choose-selfimproves-method <METHOD>`: Selection method (default: score_child_prop)
- `--continue-from <DIR>`: Continue from previous run
- `--update-archive <METHOD>`: Archive update method (default: keep_all)
- `--polyglot`: Use Polyglot benchmark instead of SWE-bench
- `--shallow-eval`: Run shallow evaluation only
- `--eval-noise <F>`: Noise leeway for evaluation (default: 0.1)

### Environment Variables

- `ANTHROPIC_API_KEY`: Anthropic Claude API key
- `OPENAI_API_KEY`: OpenAI API key
- `AWS_REGION`: AWS region for Bedrock
- `AWS_ACCESS_KEY_ID`: AWS access key
- `AWS_SECRET_ACCESS_KEY`: AWS secret key
- `RUST_LOG`: Logging level (debug, info, warn, error)

## Architecture

The Rust implementation follows a modular architecture:

```
src/
├── main.rs              # CLI entry point
├── lib.rs               # Library root
├── config/              # Configuration management
├── dgm/                 # Core DGM logic
│   ├── runner.rs        # Main DGM runner
│   ├── archive.rs       # Archive management
│   └── evolution.rs     # Evolution strategies
├── llm/                 # LLM client abstractions
├── tools/               # Tool system (bash, edit)
├── agent/               # Coding agent
├── evaluation/          # Evaluation harnesses
├── prompts/             # Prompt management
└── utils/               # Utility functions
```

## Development

### Running Tests

```bash
cargo test
```

### Code Formatting

```bash
cargo fmt
```

### Linting

```bash
cargo clippy
```

### Development Mode

```bash
# Run with debug logging
RUST_LOG=debug cargo run -- --max-generation 1

# Check compilation without running
cargo check

# Watch for changes and rebuild
cargo watch -x check
```

## Performance

The Rust implementation is designed for:

- **Memory Safety**: No segfaults or memory leaks
- **Performance**: Zero-cost abstractions and efficient async I/O
- **Concurrency**: Safe parallel processing with Tokio
- **Error Handling**: Comprehensive error propagation

## Contributing

1. Implement missing components (see TODO list above)
2. Add comprehensive tests
3. Improve error handling and logging
4. Optimize performance bottlenecks
5. Add documentation and examples

## Differences from Python Version

- **Type Safety**: Compile-time guarantees prevent many runtime errors
- **Performance**: Significantly faster execution and lower memory usage
- **Concurrency**: Native async/await with Tokio instead of threading
- **Dependencies**: Fewer runtime dependencies, faster startup
- **Error Handling**: Structured error types instead of exceptions

## License

Apache 2.0 (same as original Python implementation)
