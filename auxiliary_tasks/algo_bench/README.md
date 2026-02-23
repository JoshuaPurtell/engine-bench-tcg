# AI Benchmarking Tools

Tools for benchmarking AI implementations against v1-v4.

## Quick Start

### Benchmark Existing AI (Simplest)

To benchmark an existing AI (v1–v3 from `reference_algos`, plus v4 from `tcg_ai`):

```bash
python3 benchmark_existing_ai.py \
  --ai-name RandomAiV4 \
  --matches 50
```

Deck data is bundled in `data/` (server.sqlite + cards.sqlite). If you omit `--server-db` and `--cards-db`, the scripts use `data/server.sqlite` and `data/cards.sqlite` when present.

### Benchmark Custom AI Code

To benchmark a new AI implementation from Rust code:

```bash
python3 benchmark_ai.py \
  --ai-code-file path/to/your_ai.rs \
  --name YourAI \
  --matches 50
```

**Verified example output** (with a simple heuristic AI):
```
Summary:
Opponent               Wins   Matches     Win%
RandomAi (v1)            13        38    34.2%
RandomAiV2 (v2)          11        37    29.7%
RandomAiV3 (v3)           5        35    14.3%
RandomAiV4 (v4)           9        35    25.7%

Overall: 38 wins / 145 matches (26.2%)
```

### Agent-in-the-Loop Benchmark

This benchmark runs an agent inside a sandbox repo with the overzealous crates.
The agent must create a `.txt` file containing Rust code for a new AI. The
script then benchmarks it against v1–v4.

```bash
python3 agent_algo_bench.py \
  --model openai/gpt-4.1-mini \
  --timeout 1800 \
  --matches 50 \
  --ai-name CandidateAi \
  --output results.json
```

**Verified example**:
```bash
# Quick test (nano model may produce compile errors - use a stronger model for real runs)
python3 agent_algo_bench.py \
  --model opencode/gpt-5-nano \
  --timeout 300 \
  --matches 5 \
  --ai-name TestAgentAi \
  --output /tmp/agent_test_result.json

# For better results, use a more capable model:
python3 agent_algo_bench.py \
  --model openai/gpt-4.1-mini \
  --timeout 1800 \
  --matches 50 \
  --ai-name CandidateAi \
  --output results.json
```

**Note**: The agent receives a starter template (`examples/algorithm_template.rs`) with ~25% baseline win rate. The goal is to improve upon this template.

**Results JSON** (default output: `results.json`) includes:
- Agent details (model, timeout, duration, stdout/stderr, usage/cost if available)
- Harness details (command, format)
- Sandbox path and prompt used
- `.env` info (path, whether loaded, and which keys were present)
- Submission code and metadata
- Benchmark output, overall win rate, and exit status

**Using a specific .env file**:
```bash
python3 agent_algo_bench.py \
  --env-file /path/to/.env \
  --model openai/gpt-5-codex \
  --timeout 1800 \
  --matches 10 \
  --ai-name ConfirmAi \
  --output /tmp/agent_results.json
```

Details:
- **Sandbox**: A temporary copy of `overzealous` created under your system temp directory.
- **Agent harness**: `opencode run --model <model> --agent build --format json` (invoked by `agent_algo_bench.py`).
- **Model format**: Use `provider/model` format (e.g., `openai/gpt-4.1-mini`, `opencode/gpt-5-nano`). Run `opencode models` to list available models.
- **Code extraction**: If the agent doesn't write to a file, the script extracts code from the agent's JSON output.
- **Standardized instruction** (sent to the agent):
  - Read `tcg_ai/src/traits.rs` and core engine types in `tcg_core`.
  - Read example cards in `tcg_expansions/src/cg/cards/` and `tcg_expansions/src/df/cards/`.
  - Read the reference v4 algorithm in `tcg_ai/src/random_ai_v4.rs`.
  - Write the strongest algorithm it can within the `GameView` constraints.

Or pass the code directly:

```bash
python3 benchmark_ai.py \
  --ai-code "pub struct MyAI { rng: ChaCha8Rng } ..." \
  --name MyAI \
  --matches 50
```

## AI Code Requirements

Your AI code must:

1. Define a struct (e.g., `pub struct MyAI { ... }`)
2. Implement `pub fn new(seed: u64) -> Self`
3. Implement the `AiController` trait:
   - `fn propose_prompt_response(&mut self, view: &GameView, prompt: &Prompt) -> Vec<Action>`
   - `fn propose_free_actions(&mut self, view: &GameView) -> Vec<Action>`

### Example AI Code

```rust
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;
use tcg_core::{Action, GameView, Prompt};
use tcg_ai::traits::AiController;

pub struct MyAI {
    rng: ChaCha8Rng,
}

impl MyAI {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
        }
    }
}

impl AiController for MyAI {
    fn propose_prompt_response(&mut self, _view: &GameView, _prompt: &Prompt) -> Vec<Action> {
        vec![Action::EndTurn]
    }
    
    fn propose_free_actions(&mut self, _view: &GameView) -> Vec<Action> {
        vec![Action::EndTurn]
    }
}
```

## Output Format

The benchmark runs your AI against:
- RandomAi (v1) from `reference_algos`
- RandomAiV2 (v2) from `reference_algos`
- RandomAiV3 (v3) from `reference_algos`
- RandomAiV4 (v4) from `tcg_ai`

Results show:
- Win rate vs each opponent
- Overall aggregate win rate
- Per-matchup statistics

Example output:
```
Benchmarking RandomAiV4 vs v1-v4
==================================
...

Opponent: RandomAi (v1)
  RandomAiV4: 62 wins / 100 matches (62.0%)
  RandomAi (v1): 38 wins / 100 matches (38.0%)

Summary:
Opponent               Wins   Matches     Win%
RandomAi (v1)            62       100    62.0%
RandomAiV2 (v2)          61       100    61.0%
RandomAiV3 (v3)          43       100    43.0%
RandomAiV4 (v4)          50       100    50.0%

Overall: 216 wins / 400 matches (54.0%)
```

## Files

- `benchmark_existing_ai.py` - Simple script for benchmarking existing AIs from the repo
- `benchmark_ai.py` - Full-featured script that compiles custom AI code
- `agent_algo_bench.py` - Agent-in-the-loop benchmark (creates .txt + runs eval)
- `reference_algos/` - v1–v3 reference implementations (moved from overzealous)
- `examples/` - Example AI implementations:
  - `algorithm_template.rs` - Starter template provided to agents (~25% baseline)
  - `simple_heuristic_ai.rs` - Example heuristic AI (~26% win rate)
- `simulate_deck_matchups.rs` - Original Rust benchmark script (moved from overzealous)
- `v4_vs_previous_results.txt` - Results from benchmarking v4 vs v1-v4

## Notes

- The benchmark uses a "blended" matchup approach: runs matches with your AI as both P1 and P2 to account for first-player advantage
- Default deck is "Overzealous" from the server database
- Games are deterministic based on seeds, so results are reproducible
- Each matchup runs `--matches` games in each direction (total = `--matches * 2` per opponent)

## Benchmark Results: gpt-5.2-codex (10 instances, 500 matches)

### OpenCode Harness

```bash
python3 agent_algo_bench.py \
  --env-file /path/to/.env \
  --harness opencode \
  --model openai/gpt-5.2-codex \
  --timeout 300 \
  --matches 500 \
  --ai-name CandidateAi \
  --instances 10 \
  --parallel 10 \
  --output /tmp/agent_results_opencode.json
```

**Results:**
```
Run Success   Win%  Duration(s)       Cost Error
------------------------------------------------
  1     yes   43.2       238.48   0.021613
  2     yes   62.5       180.25   0.010891
  3     yes   53.6        85.96   0.003720
  4     yes   27.9        21.94   0.003123
  5      no      -        28.92   0.003371 SubmissionFailedCompilation
  6     yes   70.6       180.09   0.006853
  7     yes   72.3       299.24   0.008646
  8      no      -        54.89   0.003535 SubmissionFailedCompilation
  9     yes   54.0       202.91   0.011632
 10      no      -        29.58   0.003458 SubmissionFailedCompilation

Average win rate (successful runs): 54.87%
Average win rate (with 0s for failed runs): 38.41%
Average duration: 172.69s
Average cost: $0.009497
Success rate: 7/10 (70%)
```

### Codex Harness

```bash
python3 agent_algo_bench.py \
  --env-file /path/to/.env \
  --harness codex \
  --model openai/gpt-5.2-codex \
  --timeout 300 \
  --matches 500 \
  --ai-name CandidateAi \
  --instances 10 \
  --parallel 10 \
  --output /tmp/agent_results_codex.json
```

**Results:**
```
Run Success   Win%  Duration(s)       Cost Error
------------------------------------------------
  1      no      -       281.70          - SubmissionFailedCompilation
  2      no      -        34.71          - SubmissionFailedCompilation
  3     yes   44.4        41.81          -
  4     yes   65.7       190.28          -
  5      no      -       179.72          - SubmissionFailedCompilation
  6      no      -       257.54          - SubmissionFailedCompilation
  7     yes   54.0       295.21          -
  8      no      -        46.39          - SubmissionFailedCompilation
  9      no      -        44.05          - SubmissionFailedCompilation
 10     yes   62.5       138.29          -

Average win rate (successful runs): 56.65%
Average win rate (with 0s for failed runs): 22.66%
Average duration: 166.40s
Success rate: 4/10 (40%)
```

### Cursor Harness

```bash
python3 agent_algo_bench.py \
  --env-file /path/to/.env \
  --harness cursor \
  --model openai/gpt-5.2-codex \
  --timeout 300 \
  --matches 500 \
  --ai-name CandidateAi \
  --instances 10 \
  --parallel 10 \
  --output /tmp/agent_results_cursor.json
```

**Results:**
```
Run Success   Win%  Duration(s)       Cost Error
------------------------------------------------
  1     yes   28.7       300.05          -
  2     yes   28.7         5.13          -
  3     yes   70.6       203.29          -
  4     yes   53.0       172.34          -
  5     yes   68.4       226.27          -
  6      no      -        47.81          - SubmissionFailedCompilation
  7      no      -       115.23          - SubmissionFailedCompilation
  8     yes   28.7       300.05          -
  9      no      -        44.73          - SubmissionFailedCompilation
 10     yes   68.9       267.32          -

Average win rate (successful runs): 49.57%
Average win rate (with 0s for failed runs): 34.70%
Average duration: 210.63s
Success rate: 7/10 (70%)
```

### Summary: gpt-5.2-codex Performance by Harness

| Harness  | Win Rate (w/ 0s) | Win Rate (success only) | Avg Duration | Success Rate | Avg Cost |
|----------|------------------|-------------------------|--------------|--------------|----------|
| OpenCode | 38.41%          | 54.87%                  | 172.69s      | 70%          | $0.0095  |
| Codex    | 22.66%          | 56.65%                  | 166.40s      | 40%          | N/A      |
| Cursor   | 34.70%          | 49.57%                  | 210.63s      | 70%          | N/A      |

**Key Findings:**
- OpenCode achieves the best balance: highest overall win rate (38.41% with failed runs imputed as 0) and 70% success rate
- Codex harness has the highest win rate on successful runs (56.65%) but lowest success rate (40%)
- Cursor harness is slower (210.63s avg) but achieves 70% success rate
- Baseline template performance: ~25% win rate (see `examples/algorithm_template.rs`)
- All successful submissions significantly outperform the baseline template