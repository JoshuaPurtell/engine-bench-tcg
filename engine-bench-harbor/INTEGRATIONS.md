# Integrations Guide

This guide covers how to use EngineBench with different sandbox infrastructure providers and coding agents, including how to route inference through the Synth interceptor for tracing and prompt optimization.

---

## Table of Contents

- [Infrastructure Providers](#infrastructure-providers)
  - [Daytona](#daytona)
  - [Kernel](#kernel)
- [Coding Agents](#coding-agents)
  - [Claude Code](#claude-code)
  - [Codex](#codex)
  - [OpenCode](#opencode)
- [Synth Interceptor](#synth-interceptor)
  - [Overview](#overview)
  - [Routing Claude Code through the Interceptor](#routing-claude-code-through-the-interceptor)
  - [Supported API Formats](#supported-api-formats)
  - [Trace Capture](#trace-capture)

---

## Infrastructure Providers

EngineBench tasks run inside isolated sandbox environments. Harbor supports several backends; the two primary ones for production use are **Daytona** (cloud sandboxes) and **Kernel** (browser VMs for web-based tasks).

### Daytona

Cloud sandboxes optimized for coding tasks. Provides fast, reproducible Rust build environments.

**When to use:** Code-generation benchmarks (the default for EngineBench).

**Setup:**
```bash
export DAYTONA_API_KEY="dtn_..."
```

**How it works:**
1. Harbor builds a Docker image per task (Rust 1.82 + scaffold code + stub files).
2. Daytona provisions a sandbox from that image.
3. The agent runs inside the sandbox with access to the workspace at `/app`.
4. After the agent finishes, the verifier injects eval tests and runs `cargo test`.
5. The sandbox is destroyed after scoring.

**Snapshot caching:** Daytona caches base images as snapshots. The first run for a task type builds the full image (~100s). Subsequent runs start from the snapshot (~10s).

**Resource defaults per sandbox:**

| Resource | Default |
|----------|---------|
| CPUs     | 2       |
| Memory   | 8 GB    |
| Storage  | 10 GB   |

Override via Harbor flags:
```bash
harbor run -e daytona --override-cpus 4 --override-memory 16384 -a claude-code -p tasks/
```

**Optional environment variables:**

| Variable | Default | Description |
|----------|---------|-------------|
| `DAYTONA_API_KEY` | (required) | API key for authentication |
| `DAYTONA_API_URL` | `https://app.daytona.io/api` | API endpoint |
| `DAYTONA_TARGET` | `us` | Target region |
| `DAYTONA_USE_SNAPSHOT_CACHE` | `1` | Enable snapshot caching |

**Example:**
```bash
# Run 20 tasks, 8 concurrent, on Daytona
harbor run -a claude-code -e daytona -n 8 -l 20 -p tasks/
```

### Kernel

Cloud browser VMs managed via the [Kernel](https://onkernel.com) API. Provides Chrome instances with persistent profiles for web automation tasks.

**When to use:** Browser-based benchmarks (LinkedIn tasks, web scraping). Not used for EngineBench code tasks directly.

**Setup:**
```bash
export KERNEL_API_KEY="..."
```

**How it works:**
1. A browser pool is created with a named profile (e.g., `linkedin`).
2. For each task, a browser session is acquired from the pool.
3. Claude Code runs with `agent-browser` CLI installed in the VM.
4. After completion, the browser is released back to the pool with `reuse=True`.

**Key features:**
- **Pool reuse:** Tool installations (Claude Code, agent-browser) persist across runs, eliminating reinstallation overhead.
- **Live view URLs:** Each session provides a browser live view URL for debugging.
- **Profile persistence:** Browser cookies and login state survive across pool reuses.

**Example (from browser-agent-gepa):**
```python
from kernel import AsyncKernel

client = AsyncKernel(api_key=os.environ["KERNEL_API_KEY"])
result = await client.browser_pools.acquire("agent-gepa")
session_id = result.session_id
# ... run task ...
await client.browser_pools.release("agent-gepa", session_id=session_id, reuse=True)
```

---

## Coding Agents

Harbor has built-in adapters for several coding agents. Each agent receives the same `instruction.md` prompt and sandbox environment. The key differences are in how they authenticate, accept model configuration, and support proxy base URLs.

### Claude Code

Anthropic's CLI coding agent. Supports extended thinking, custom base URLs, and ATIF trajectory export.

**Required env:**
```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

**Run:**
```bash
# Default model
harbor run -a claude-code -e daytona -p tasks/df-001-ampharos

# Specific model
harbor run -a claude-code -m claude-sonnet-4-20250514 -e daytona -p tasks/

# With extended thinking
harbor run -a claude-code --ak max_thinking_tokens=10000 -e daytona -p tasks/
```

**Custom base URL (for interceptor routing):**
```bash
export ANTHROPIC_BASE_URL="https://your-proxy.example.com/v1"
harbor run -a claude-code -e daytona -p tasks/
```

When `ANTHROPIC_BASE_URL` is set, the adapter also pins all sub-agent model aliases (`ANTHROPIC_DEFAULT_SONNET_MODEL`, `ANTHROPIC_DEFAULT_OPUS_MODEL`, `ANTHROPIC_DEFAULT_HAIKU_MODEL`, `CLAUDE_CODE_SUBAGENT_MODEL`) to the specified model, ensuring all inference routes through the proxy.

**Agent kwargs:**

| Kwarg | Type | Default | Description |
|-------|------|---------|-------------|
| `max_thinking_tokens` | int | None | Extended thinking token budget |
| `version` | str | latest | Claude Code CLI version to install |

**CLI invocation inside sandbox:**
```
claude --verbose --output-format stream-json -p <instruction> \
  --allowedTools Bash Edit Write Read Glob Grep LS WebFetch \
  NotebookEdit NotebookRead TodoRead TodoWrite Agent Skill \
  SlashCommand Task WebSearch
```

**Telemetry:** Disabled in sandbox (`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`).

### Codex

OpenAI's Codex CLI agent. Supports reasoning effort tuning and ATIF trajectory export.

**Required env:**
```bash
export OPENAI_API_KEY="sk-..."
```

**Run:**
```bash
# Default (reasoning_effort=high)
harbor run -a codex -m gpt-5-nano -e daytona -p tasks/

# Tune reasoning effort
harbor run -a codex -m o1 --ak reasoning_effort=medium -e daytona -p tasks/
```

**Agent kwargs:**

| Kwarg | Type | Default | Description |
|-------|------|---------|-------------|
| `reasoning_effort` | str | `high` | Reasoning effort level (`low`, `medium`, `high`) |
| `version` | str | latest | @openai/codex npm package version |

**CLI invocation inside sandbox:**
```
codex exec --dangerously-bypass-approvals-and-sandbox \
  --skip-git-repo-check --model <model> --json \
  --enable unified_exec \
  -c model_reasoning_effort=<effort> \
  -- <instruction>
```

**Auth handling:** Codex uses a JSON auth file (`auth.json`) rather than a direct env var. The adapter creates this file automatically and cleans it up on exit.

**Base URL support:** Codex does not currently support custom base URLs. It always calls the OpenAI API directly.

### OpenCode

Multi-provider coding agent that supports OpenAI, Anthropic, Google, Groq, and other providers through a unified interface.

**Required env (varies by provider):**
```bash
# For OpenAI models
export OPENAI_API_KEY="sk-..."

# For Anthropic models
export ANTHROPIC_API_KEY="sk-ant-..."

# For Google models
export GEMINI_API_KEY="..."
```

**Run:**
```bash
# OpenAI provider
harbor run -a opencode -m openai/gpt-5-nano -e daytona -p tasks/

# Anthropic provider
harbor run -a opencode -m anthropic/claude-sonnet-4 -e daytona -p tasks/

# Google provider
harbor run -a opencode -m google/gemini-2.5-flash -e daytona -p tasks/
```

**Model format:** OpenCode requires `provider/model_name` format (e.g., `anthropic/claude-sonnet-4`).

**Agent kwargs:**

| Kwarg | Type | Default | Description |
|-------|------|---------|-------------|
| `version` | str | latest | opencode-ai npm package version |

**CLI invocation inside sandbox:**
```
opencode --model <provider/model> run --format=json <instruction>
```

**Base URL support:** OpenCode does not support custom base URLs. It routes directly to each provider's default endpoint.

### Agent Comparison

| Feature | Claude Code | Codex | OpenCode |
|---------|------------|-------|----------|
| Provider | Anthropic | OpenAI | Multi-provider |
| Base URL override | Yes (`ANTHROPIC_BASE_URL`) | No | No |
| Interceptor support | Yes | No | No |
| ATIF trajectories | Yes | Yes | No |
| Model specification | Optional (has defaults) | Required (`-m`) | Required (`-m provider/model`) |
| Extended thinking | Yes (`max_thinking_tokens`) | Via `reasoning_effort` | No |
| Streaming output | Yes (stream-json) | Yes (json) | Yes (json) |

---

## Synth Interceptor

### Overview

The Synth interceptor is a transparent API proxy that sits between coding agents and LLM providers. It captures request/response traces, tracks token usage, and enables prompt optimization via Synth's GEPA system.

```
Agent  -->  Interceptor  -->  Provider API
              |
              v
         Trace Store
         (Redis/S3)
```

**Supported providers:**

| Provider | Upstream URL | Auth mechanism |
|----------|-------------|----------------|
| OpenAI | `api.openai.com/v1` | `Authorization: Bearer` |
| Anthropic | `api.anthropic.com/v1/messages` | `x-api-key` + `anthropic-version` |
| Google (Gemini) | `generativelanguage.googleapis.com` | `x-goog-api-key` |
| Groq | `api.groq.com/openai/v1` | `Authorization: Bearer` |

**Endpoint routes:**

| Route pattern | Description |
|---------------|-------------|
| `/:trial_id/chat/completions` | OpenAI Chat Completions |
| `/:trial_id/responses` | OpenAI Responses API |
| `/:trial_id/v1/messages` | Anthropic Messages API |
| `/:trial_id/:correlation_id/v1/messages` | Anthropic with correlation tracking |

The interceptor auto-detects the provider from the model name. Models starting with `claude-` route to Anthropic; `gpt-*`, `o1-*`, `o3-*` route to OpenAI; `gemini-*` to Google; and so on.

### Routing Claude Code through the Interceptor

Claude Code is the only agent that currently supports custom base URLs, making it the primary agent for interceptor-based tracing and optimization.

**Step 1: Register a trial**

Trials tell the interceptor which job a request belongs to and what prompt transformations to apply (if any). For passthrough tracing with no transformations:

```bash
INTERCEPTOR_BASE="https://infra-api.usesynth.ai/api/interceptor/v1"

curl -X POST "${INTERCEPTOR_BASE}/debug/register_trial/my-trial-001" \
  -H "Content-Type: application/json" \
  -d '{
    "job_id": "my-eval-job",
    "seed": 0,
    "stage_key": {"pipeline_id": "eval", "stage_id": "passthrough"},
    "baseline_messages": [],
    "deltas": {},
    "ttl_seconds": 7200
  }'
```

**Step 2: Point Claude Code at the interceptor**

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
export ANTHROPIC_BASE_URL="${INTERCEPTOR_BASE}/my-trial-001"
```

Claude Code will call `${ANTHROPIC_BASE_URL}/v1/messages`, which the interceptor proxies to `api.anthropic.com/v1/messages`.

**Step 3: Run via Harbor**

```bash
harbor run -a claude-code -e daytona -p tasks/df-001-ampharos
```

All inference traffic is now captured by the interceptor.

**Local development:**

To test against a local interceptor, expose it with a tunnel:

```bash
# Terminal 1: Start the Rust backend
cd rust_backend
PORT=8090 cargo run --bin synth-rust-backend

# Terminal 2: Expose via Cloudflare tunnel
cloudflared tunnel --url http://localhost:8090

# Terminal 3: Run Harbor with the tunnel URL
export ANTHROPIC_BASE_URL="https://<tunnel>.trycloudflare.com/api/interceptor/v1/my-trial"
harbor run -a claude-code -e daytona -p tasks/df-001-ampharos
```

### Supported API Formats

**Anthropic Messages API** (used by Claude Code):
- Non-streaming: `POST /v1/messages` returns JSON with `content[]` and `usage`
- Streaming: `POST /v1/messages` with `"stream": true` returns SSE with `event: message_start`, `content_block_delta`, `message_delta`
- Both formats are proxied transparently

**OpenAI Chat Completions** (used by Codex, OpenCode):
- `POST /chat/completions` or `/v1/chat/completions`
- Streaming via `"stream": true` with SSE `data:` lines

**Usage tracking:**

The interceptor extracts token usage from responses:

| Provider | Input tokens field | Output tokens field | Cached tokens field |
|----------|-------------------|--------------------|--------------------|
| OpenAI | `usage.prompt_tokens` | `usage.completion_tokens` | `usage.prompt_tokens_details.cached_tokens` |
| Anthropic | `usage.input_tokens` | `usage.output_tokens` | `usage.cache_read_input_tokens` |
| Gemini | `usageMetadata.promptTokenCount` | `usageMetadata.candidatesTokenCount` | N/A |

For streaming responses, the interceptor passes through SSE chunks in real-time and captures the full response buffer for trace storage. Usage metrics from streaming responses are not counted in the global metrics (only trace storage).

### Trace Capture

When a `correlation_id` is provided (either in the URL path or as a `?cid=` query parameter), the interceptor stores a trace record containing:

- The full request body (JSON or base64-encoded)
- The full response body (JSON or base64-encoded for streaming)
- HTTP status code
- Timestamp
- Trial ID and correlation ID

**Fetching traces:**

```bash
# By correlation ID
curl "${INTERCEPTOR_BASE}/trace/by-correlation/my-corr-id"

# Batch hydration (returns parsed request/response with model, messages, etc.)
curl -X POST "${INTERCEPTOR_BASE}/trace/hydrate-batch" \
  -H "Content-Type: application/json" \
  -d '{"correlation_ids": ["corr-001", "corr-002"]}'
```

**In GEPA optimization flows**, the Synth backend automatically:
1. Assigns trial IDs and correlation IDs to each rollout
2. Routes inference through the interceptor via `inference_url` in `policy_config`
3. Fetches traces after rollout completion for analysis
4. Uses trace data to optimize prompts and agent behavior
