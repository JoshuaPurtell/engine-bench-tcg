## EngineBench Harbor Tasks

This directory contains Harbor task folders for EngineBench single-card Rust implementation tasks.

**Total tasks:** 197 cards (DF and HP expansions)

### Environment Setup

**Docker (Colima):**
```bash
colima start
docker context use colima
```

**Daytona:**
```bash
# Set Daytona API key (required)
export DAYTONA_API_KEY="your-api-key"

# Optional: Set Daytona server URL (defaults to localhost)
export DAYTONA_URL="https://your-daytona-server.com"
```

### Generate Tasks

Generate Harbor tasks from EngineBench card data:

```bash
# Generate a single card
python3 generate_tasks.py --card df-007-nidoqueen

# Generate all available cards
python3 generate_tasks.py --all

# Overwrite existing tasks
python3 generate_tasks.py --all --force
```

### Run Tasks

**Using the benchmark script (recommended):**
```bash
# Run 10 tasks in parallel with Docker
./run_benchmark.sh --agent codex --model gpt-5-nano --n-tasks 10 --n-concurrent 4

# Run with Daytona backend (8 concurrent)
./run_benchmark.sh --agent codex --model gpt-5-nano --env daytona --n-concurrent 8

# Run specific card pattern
./run_benchmark.sh --agent codex --model gpt-5-nano --task-pattern "hp-*" --n-tasks 5

# See all options
./run_benchmark.sh --help
```

**Direct Harbor commands:**

Single task:
```bash
harbor run -p "engine-bench-harbor/tasks/df-007-nidoqueen" -a oracle
harbor run -p "engine-bench-harbor/tasks/df-001-ampharos" -a codex -m gpt-5-nano
```

Multiple tasks in parallel:
```bash
# Run all DF cards, 4 concurrent trials
harbor run -p "engine-bench-harbor/tasks" -a codex -m gpt-5-nano -t "df-*" -n 4

# Run with Daytona, 8 concurrent
harbor run -p "engine-bench-harbor/tasks" -a codex -m gpt-5-nano -e daytona -n 8 -t "df-*"

# Limit to first 10 tasks
harbor run -p "engine-bench-harbor/tasks" -a codex -m gpt-5-nano -n 4 -l 10
```

**Available agents:**
- `oracle` - Uses gold solution from `solution/`
- `codex` - OpenAI Codex agent (requires `OPENAI_API_KEY`)
- `opencode` - OpenCode agent (requires `OPENAI_API_KEY`)
- `claude-code` - Claude Code agent
- See `harbor run --help` for full list

**Environment backends:**
- `docker` - Local Docker/Colima (default)
- `daytona` - Daytona cloud sandboxes (requires `DAYTONA_API_KEY`)
- `e2b` - E2B sandboxes
- `modal` - Modal cloud functions

### Task Structure

Each task folder contains:
- `instruction.md` - Task prompt for agents
- `task.toml` - Harbor config (timeouts, metadata, resources)
- `environment/Dockerfile` - Rust 1.82 image with scaffold
- `environment/scaffold/` - Starting workspace (tcg_expansions crate)
- `environment/stubs/{card}.rs` - Stub file with TODOs
- `tests/test.sh` - Verifier script (injects eval tests, runs cargo)
- `tests/{card}_eval.rs` - Eval tests injected at runtime
- `solution/solve.sh` - Oracle script
- `solution/{card}.rs` - Gold implementation (if available)

### Task Runtime Paths

- Working repo: `/app`
- Tests injected by Harbor: `/tests`
- Oracle solution (if enabled): `/solution/`
- Reward output: `/logs/verifier/reward.txt` and `reward.json`

### Scoring

- **0.3** points for successful compilation
- **0.7** points for passing tests (proportional to pass rate)
- **Total:** 0.0 - 1.0

### Environment Variables

Required for different agents/backends:

```bash
# OpenAI agents (codex, opencode)
export OPENAI_API_KEY="sk-..."

# Daytona backend
export DAYTONA_API_KEY="your-api-key"
export DAYTONA_URL="https://your-daytona-server.com"  # Optional

# Claude agents (claude-code)
export ANTHROPIC_API_KEY="sk-ant-..."
```

### Integrations

For detailed documentation on infrastructure providers (Daytona, Kernel), coding agents (Claude Code, Codex, OpenCode), and routing through the Synth interceptor, see **[INTEGRATIONS.md](INTEGRATIONS.md)**.

### Performance Tips

- **Docker:** Good for local testing, limited by local CPU/memory
- **Daytona:** Faster startup with snapshot caching, scales better for large batches
- **Concurrent trials:** Start with `-n 4` for Docker, can go higher with Daytona (8-16)
- **Task filtering:** Use `-t "df-*"` or `-t "hp-*"` to filter by expansion
