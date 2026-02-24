#!/usr/bin/env python3
"""Benchmark a new AI implementation against v1-v4.

Usage:
    python benchmark_ai.py --ai-code-file path/to/ai.rs --name MyAI
    python benchmark_ai.py --ai-code "pub struct MyAI { ... }" --name MyAI

The AI code should implement a struct with:
- `pub fn new(seed: u64) -> Self`
- Implementation of `AiController` trait

Reference opponents are loaded from `reference_algos` (v1-v3) plus v4 from `tcg_ai`.

Example AI code:
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
    fn propose_prompt_response(&mut self, view: &GameView, prompt: &Prompt) -> Vec<Action> {
        vec![Action::EndTurn]
    }
    
    fn propose_free_actions(&mut self, view: &GameView) -> Vec<Action> {
        vec![Action::EndTurn]
    }
}
```
"""

import argparse
import json
import re
import subprocess
import tempfile
from pathlib import Path
import sys

# These import prefixes are provided by the benchmark harness and should be filtered
# Any use statement starting with these prefixes will be removed
PROVIDED_IMPORT_PREFIXES = [
    "use rand_chacha::",
    "use rand::",
    "use tcg_core::",
    "use tcg_ai::traits::",
    "use tcg_ai::AiController",
    "use crate::traits::AiController",
]


def filter_duplicate_imports(code: str) -> str:
    """Remove use statements for types already provided by the benchmark harness."""
    lines = code.split("\n")
    filtered_lines = []
    skip_grouped_use = False
    tcg_core_provided = {
        "Action",
        "GameView",
        "Prompt",
        "Attack",
        "AttackCost",
        "CardInstanceId",
        "Type",
    }
    i = 0
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()

        # Handle multi-line grouped imports for tcg_core and keep only non-provided items
        if stripped.startswith("use tcg_core::") and "{" in stripped:
            group_lines = [line]
            j = i
            while "};" not in lines[j]:
                j += 1
                if j >= len(lines):
                    break
                group_lines.append(lines[j])
            group_text = "\n".join(group_lines)
            if "{" in group_text and "}" in group_text:
                items_text = group_text.split("{", 1)[1].rsplit("}", 1)[0]
                items = [item.strip() for item in items_text.replace("\n", " ").split(",")]
                items = [item for item in items if item]
                remaining = [item for item in items if item not in tcg_core_provided]
                if remaining:
                    filtered_lines.append(f"use tcg_core::{{{', '.join(remaining)}}};")
            i = j + 1
            continue

        # Check if this line is a use statement that should be filtered
        if stripped.startswith("use "):
            should_filter = any(
                stripped.startswith(prefix) for prefix in PROVIDED_IMPORT_PREFIXES
            )
            if should_filter:
                if "{" in stripped and "};" not in stripped:
                    # This is a multi-line grouped use; skip until closing "};"
                    skip_grouped_use = True
                i += 1
                continue
        if skip_grouped_use:
            # Skip lines until the grouped use statement ends
            if "};" in stripped:
                skip_grouped_use = False
            i += 1
            continue
        filtered_lines.append(line)
        i += 1
    return "\n".join(filtered_lines)

BASE_DIR = Path(__file__).parent.parent.parent.parent
OVERZEALOUS_DIR = Path("/Users/joshpurtell/Documents/GitHub/overzealous")
REFERENCE_ALGOS_DIR = Path(__file__).parent / "reference_algos"
ALGO_BENCH_DATA_DIR = Path(__file__).parent / "data"


def generate_ai_module(ai_code: str, ai_name: str) -> str:
    """Generate a complete Rust module file for the AI."""
    # Remove common imports from ai_code since they'll be in the module scope
    # These are provided by the include! site in the benchmark binary
    cleaned_code = filter_duplicate_imports(ai_code)
    
    return f"""// Auto-generated AI module for {ai_name}
{cleaned_code}
"""


def discover_reference_algos() -> list[dict]:
    """Discover reference algos from reference_algos directory."""
    algos = []
    if not REFERENCE_ALGOS_DIR.exists():
        return algos

    for path in sorted(REFERENCE_ALGOS_DIR.glob("*.rs")):
        content = path.read_text()
        match = re.search(r"pub struct (\w+)", content)
        if not match:
            continue
        struct_name = match.group(1)
        module_name = f"ref_{path.stem}"
        algos.append(
            {
                "module": module_name,
                "struct": struct_name,
                "path": path,
                "label": reference_label(struct_name),
            }
        )
    return algos


def reference_label(struct_name: str) -> str:
    if struct_name == "RandomAi":
        return "RandomAi (v1)"
    if struct_name == "RandomAiV2":
        return "RandomAiV2 (v2)"
    if struct_name == "RandomAiV3":
        return "RandomAiV3 (v3)"
    return struct_name


def generate_benchmark_binary(ai_name: str, ai_module_path: Path, reference_algos: list[dict]) -> str:
    """Generate a benchmark binary that uses the AI."""
    # Escape paths for Rust string literal
    ai_module_path_str = str(ai_module_path).replace("\\", "\\\\")
    reference_modules = []
    reference_variants = []
    reference_ai_name_arms = []
    reference_build_arms = []
    reference_opponents = []

    for algo in reference_algos:
        module_name = algo["module"]
        struct_name = algo["struct"]
        label = algo["label"]
        path_str = str(algo["path"]).replace("\\", "\\\\")
        variant = f"Ref{struct_name}"

        reference_modules.append(
            f"""mod {module_name} {{
    include!(r#\"{path_str}\"#);
}}"""
        )
        reference_variants.append(f"{variant},")
        reference_ai_name_arms.append(f'AiType::{variant} => "{label}",')
        reference_build_arms.append(
            f"AiType::{variant} => Box::new({module_name}::{struct_name}::new(seed)),"
        )
        reference_opponents.append((variant, label))

    reference_modules_code = "\n\n".join(reference_modules)
    reference_variants_code = "\n    ".join(reference_variants)
    reference_ai_name_arms_code = "\n        ".join(reference_ai_name_arms)
    reference_build_arms_code = "\n        ".join(reference_build_arms)
    if reference_opponents:
        reference_opponents_code = (
            ",\n        ".join(
                [f'(AiType::{variant}, "{label}")' for variant, label in reference_opponents]
            )
            + ",\n        "
        )
    else:
        reference_opponents_code = ""

    return f"""use tcg_ai::{{AiController, RandomAiV4}};
use tcg_core::{{Action, CardInstance, CardMetaMap, GameState, PlayerId, StepResult}};
use tcg_rules_ex::RulesetConfig;

{reference_modules_code}

mod ai_module {{
    use rand_chacha::ChaCha8Rng;
    use rand::SeedableRng;
    use rand::seq::SliceRandom;
    use rand::seq::IteratorRandom;
    use rand::Rng;
    use tcg_core::{{Action, GameView, Prompt, Attack, AttackCost, CardInstanceId, Type}};
    use tcg_ai::traits::AiController;

    include!(r#"{ai_module_path_str}"#);
}}

use ai_module::{ai_name};

#[derive(PartialEq, Clone, Copy, Debug)]
enum AiType {{
    {reference_variants_code}
    RandomAiV4,
    TestAI,
}}

fn ai_name(ai_type: AiType) -> &'static str {{
    match ai_type {{
        {reference_ai_name_arms_code}
        AiType::RandomAiV4 => "RandomAiV4 (v4)",
        AiType::TestAI => "{ai_name}",
    }}
}}

fn build_ai(ai_type: AiType, seed: u64) -> Box<dyn AiController> {{
    match ai_type {{
        {reference_build_arms_code}
        AiType::RandomAiV4 => Box::new(RandomAiV4::new(seed)),
        AiType::TestAI => Box::new({ai_name}::new(seed)),
    }}
}}

fn run_match_loop_with_stats(
    mut game: GameState,
    mut p1_ai: Option<&mut dyn AiController>,
    mut p2_ai: Option<&mut dyn AiController>,
    max_steps: usize,
) -> Option<(PlayerId, GameState, u16, u16)> {{
    let mut steps_left = max_steps;
    let mut actions_budget = 5_000usize;
    let mut p1_evolutions: u16 = 0;
    let mut p2_evolutions: u16 = 0;

    while steps_left > 0 && actions_budget > 0 {{
        match game.step() {{
            StepResult::Event {{ .. }} => {{}}
            StepResult::GameOver {{ winner }} => {{
                return Some((winner, game, p1_evolutions, p2_evolutions));
            }}
            StepResult::Prompt {{ prompt, for_player }} => {{
                let view = game.view_for_player(for_player);
                let mut candidates: Vec<Action> = match for_player {{
                    PlayerId::P1 => {{
                        if let Some(ai) = p1_ai.as_mut() {{
                            ai.propose_prompt_response(&view, &prompt)
                        }} else {{
                            Vec::new()
                        }}
                    }}
                    PlayerId::P2 => {{
                        if let Some(ai) = p2_ai.as_mut() {{
                            ai.propose_prompt_response(&view, &prompt)
                        }} else {{
                            Vec::new()
                        }}
                    }}
                }};
                candidates.push(Action::EndTurn);
                let applied = accepted_action(&mut game, for_player, candidates);
                if let Some(action) = applied {{
                    if matches!(action, Action::EvolveFromHand {{ .. }}) {{
                        if for_player == PlayerId::P1 {{
                            p1_evolutions = p1_evolutions.saturating_add(1);
                        }} else {{
                            p2_evolutions = p2_evolutions.saturating_add(1);
                        }}
                    }}
                }} else {{
                    return None;
                }}
                actions_budget = actions_budget.saturating_sub(1);
            }}
            StepResult::Continue => {{
                let phase = game.turn.phase;
                if matches!(phase, tcg_rules_ex::Phase::Main | tcg_rules_ex::Phase::Attack) {{
                    let current = game.turn.player;
                    let view = game.view_for_player(current);
                    let mut candidates: Vec<Action> = match current {{
                        PlayerId::P1 => {{
                            if let Some(ai) = p1_ai.as_mut() {{
                                ai.propose_free_actions(&view)
                            }} else {{
                                Vec::new()
                            }}
                        }}
                        PlayerId::P2 => {{
                            if let Some(ai) = p2_ai.as_mut() {{
                                ai.propose_free_actions(&view)
                            }} else {{
                                Vec::new()
                            }}
                        }}
                    }};
                    candidates.push(Action::EndTurn);
                    if let Some(action) = accepted_action(&mut game, current, candidates) {{
                        if matches!(action, Action::EvolveFromHand {{ .. }}) {{
                            if current == PlayerId::P1 {{
                                p1_evolutions = p1_evolutions.saturating_add(1);
                            }} else {{
                                p2_evolutions = p2_evolutions.saturating_add(1);
                            }}
                        }}
                    }}
                    actions_budget = actions_budget.saturating_sub(1);
                }}
            }}
        }}
        steps_left -= 1;
    }}

    None
}}

#[derive(Default, Clone)]
struct MatchStats {{
    p1_wins: usize,
    total: usize,
    outcomes: Vec<MatchOutcome>,
}}

#[derive(Clone)]
struct MatchOutcome {{
    tracked_won: bool,
    turns: u32,
    tracked_prizes_taken: u8,
    opponent_prizes_taken: u8,
    tracked_evolutions: u16,
    opponent_evolutions: u16,
}}

fn accepted_action(
    game: &mut GameState,
    player: PlayerId,
    candidates: Vec<Action>,
) -> Option<Action> {{
    for action in candidates {{
        if game.apply_action(player, action.clone()).is_ok() {{
            return Some(action);
        }}
    }}
    None
}}

fn summarize_u32(values: &[u32]) -> serde_json::Value {{
    if values.is_empty() {{
        return serde_json::json!({{"count": 0}});
    }}
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let count = sorted.len();
    let sum: u64 = sorted.iter().map(|v| *v as u64).sum();
    let mean = (sum as f64) / (count as f64);
    let p50 = sorted[(count - 1) / 2];
    let p90 = sorted[((count - 1) * 9) / 10];
    let mut counts = std::collections::BTreeMap::<u32, usize>::new();
    for value in &sorted {{
        *counts.entry(*value).or_insert(0) += 1;
    }}
    serde_json::json!({{
        "count": count,
        "min": sorted[0],
        "max": sorted[count - 1],
        "mean": mean,
        "p50": p50,
        "p90": p90,
        "counts": counts,
    }})
}}

fn summarize_u16(values: &[u16]) -> serde_json::Value {{
    summarize_u32(&values.iter().map(|v| *v as u32).collect::<Vec<_>>())
}}

fn summarize_u8(values: &[u8]) -> serde_json::Value {{
    summarize_u32(&values.iter().map(|v| *v as u32).collect::<Vec<_>>())
}}

fn summarize_match_outcomes(outcomes: &[MatchOutcome]) -> serde_json::Value {{
    let turns: Vec<u32> = outcomes.iter().map(|o| o.turns).collect();
    let tracked_prizes_taken: Vec<u8> = outcomes.iter().map(|o| o.tracked_prizes_taken).collect();
    let opponent_prizes_taken: Vec<u8> = outcomes.iter().map(|o| o.opponent_prizes_taken).collect();
    let tracked_evolutions: Vec<u16> = outcomes.iter().map(|o| o.tracked_evolutions).collect();
    let opponent_evolutions: Vec<u16> = outcomes.iter().map(|o| o.opponent_evolutions).collect();
    let tracked_wins = outcomes.iter().filter(|o| o.tracked_won).count();
    let total = outcomes.len();
    let tracked_win_rate = if total > 0 {{
        (tracked_wins as f64) / (total as f64)
    }} else {{
        0.0
    }};
    serde_json::json!({{
        "tracked_win_rate": tracked_win_rate,
        "tracked_wins": tracked_wins,
        "total_games": total,
        "turns": summarize_u32(&turns),
        "tracked_prizes_taken": summarize_u8(&tracked_prizes_taken),
        "opponent_prizes_taken": summarize_u8(&opponent_prizes_taken),
        "tracked_evolutions": summarize_u16(&tracked_evolutions),
        "opponent_evolutions": summarize_u16(&opponent_evolutions),
    }})
}}

fn run_match_series(
    deck1: &[CardInstance],
    deck2: &[CardInstance],
    p1_ai_type: AiType,
    p2_ai_type: AiType,
    num_matches: usize,
    seed_base: u64,
    count_player: PlayerId,
    card_meta: &CardMetaMap,
) -> MatchStats {{
    let mut stats = MatchStats::default();

    for match_num in 0..num_matches {{
        let seed = seed_base + match_num as u64;
        let game = GameState::new_with_card_meta(
            deck1.to_vec(),
            deck2.to_vec(),
            seed,
            RulesetConfig::default(),
            card_meta.clone(),
        );

        let mut ai1_box = build_ai(p1_ai_type, seed);
        let mut ai2_box = build_ai(p2_ai_type, seed.wrapping_add(9001));

        if let Some((winner, game, p1_evolutions, p2_evolutions)) = run_match_loop_with_stats(
            game,
            Some(ai1_box.as_mut()),
            Some(ai2_box.as_mut()),
            5_000,
        ) {{
            let p1_view = game.view_for_player(PlayerId::P1);
            let p1_prizes_remaining = p1_view.my_prizes_count as u8;
            let p2_prizes_remaining = p1_view.opponent_prizes_count as u8;
            let p1_prizes_taken = 6u8.saturating_sub(p1_prizes_remaining);
            let p2_prizes_taken = 6u8.saturating_sub(p2_prizes_remaining);
            let turns = game.turn.number;
            let (tracked_won, tracked_prizes_taken, opponent_prizes_taken, tracked_evolutions, opponent_evolutions) =
                if count_player == PlayerId::P1 {{
                    (
                        winner == PlayerId::P1,
                        p1_prizes_taken,
                        p2_prizes_taken,
                        p1_evolutions,
                        p2_evolutions,
                    )
                }} else {{
                    (
                        winner == PlayerId::P2,
                        p2_prizes_taken,
                        p1_prizes_taken,
                        p2_evolutions,
                        p1_evolutions,
                    )
                }};

            stats.total += 1;
            if tracked_won {{
                stats.p1_wins += 1;
            }}
            stats.outcomes.push(MatchOutcome {{
                tracked_won,
                turns,
                tracked_prizes_taken,
                opponent_prizes_taken,
                tracked_evolutions,
                opponent_evolutions,
            }});
        }}
    }}

    stats
}}

fn run_blended_matchup(
    deck1: &[CardInstance],
    deck2: &[CardInstance],
    p1_ai_type: AiType,
    p2_ai_type: AiType,
    num_matches: usize,
    seed_base: u64,
    card_meta: &CardMetaMap,
) -> MatchStats {{
    let mut stats = run_match_series(
        deck1,
        deck2,
        p1_ai_type,
        p2_ai_type,
        num_matches,
        seed_base,
        PlayerId::P1,
        card_meta,
    );
    let swapped = run_match_series(
        deck1,
        deck2,
        p2_ai_type,
        p1_ai_type,
        num_matches,
        seed_base + 50_000,
        PlayerId::P2,
        card_meta,
    );
    stats.p1_wins += swapped.p1_wins;
    stats.total += swapped.total;
    stats.outcomes.extend(swapped.outcomes);
    stats
}}

fn load_card_meta(cards_db_path: &str) -> CardMetaMap {{
    use rusqlite::Connection;
    let conn = Connection::open(cards_db_path).ok();
    if let Some(conn) = conn {{
        tcg_db::load_card_meta_map(&conn).unwrap_or_default()
    }} else {{
        CardMetaMap::new()
    }}
}}

#[derive(Clone, serde::Deserialize)]
struct DeckEntry {{
    card_def_id: String,
    count: usize,
}}

#[derive(Clone)]
struct DeckSpec {{
    name: String,
    entries: Vec<DeckEntry>,
}}

fn build_deck_for_player(entries: &[DeckEntry], player: PlayerId) -> Vec<CardInstance> {{
    let mut deck = Vec::new();
    for entry in entries {{
        for _ in 0..entry.count {{
            deck.push(CardInstance::new(
                tcg_core::CardDefId::new(entry.card_def_id.clone()),
                player,
            ));
        }}
    }}
    deck
}}

fn load_public_deck_specs(server_db_path: &str) -> Vec<DeckSpec> {{
    use rusqlite::Connection;
    let conn = match Connection::open(server_db_path) {{
        Ok(c) => c,
        Err(_) => return Vec::new(),
    }};

    let mut stmt = match conn.prepare(
        "SELECT name, cards_json FROM decks WHERE is_public = 1 ORDER BY LOWER(name), deck_id"
    ) {{
        Ok(s) => s,
        Err(_) => return Vec::new(),
    }};

    let rows = match stmt.query_map([], |row| {{
        let name: String = row.get(0)?;
        let cards_json: String = row.get(1)?;
        Ok((name, cards_json))
    }}) {{
        Ok(r) => r,
        Err(_) => return Vec::new(),
    }};

    let mut specs = Vec::new();
    for row in rows {{
        if let Ok((name, cards_json)) = row {{
            if let Ok(entries) = serde_json::from_str::<Vec<DeckEntry>>(&cards_json) {{
                if !entries.is_empty() {{
                    specs.push(DeckSpec {{ name, entries }});
                }}
            }}
        }}
    }}
    specs
}}

fn main() {{
    let args: Vec<String> = std::env::args().collect();
    let server_db_path = args.get(1).map(|s| s.as_str()).unwrap_or("data/server.sqlite");
    let cards_db_path = args.get(2).map(|s| s.as_str()).unwrap_or("data/cards.sqlite");
    let num_matches = args
        .get(3)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50);
    let seed_base = args
        .get(4)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    println!("Benchmarking {ai_name} vs v1-v4");
    println!("==================================");
    println!("Server DB: {{}}", server_db_path);
    println!("Cards DB: {{}}", cards_db_path);
    println!("Matches per pairing: {{}}", num_matches);
    println!("Seed base: {{}}", seed_base);
    println!();

    let card_meta = load_card_meta(cards_db_path);
    println!("Loaded {{}} card definitions", card_meta.len());

    // Load all public decks and deterministically select a non-mirrored deck pair
    // from seed_base. This makes each seed correspond to one deck-combination seed.
    let deck_specs = load_public_deck_specs(server_db_path);
    if deck_specs.len() < 2 {{
        eprintln!("Need at least 2 public decks in server DB; found {{}}", deck_specs.len());
        return;
    }}
    println!("Loaded {{}} public decks", deck_specs.len());
    for spec in &deck_specs {{
        let count: usize = spec.entries.iter().map(|entry| entry.count).sum();
        println!("  - {{}} ({{}} cards)", spec.name, count);
    }}

    let mut deck_pairs: Vec<(usize, usize)> = Vec::new();
    for i in 0..deck_specs.len() {{
        for j in (i + 1)..deck_specs.len() {{
            deck_pairs.push((i, j));
        }}
    }}
    if deck_pairs.is_empty() {{
        eprintln!("No unique non-mirror deck pairs available");
        return;
    }}
    let pair_idx = (seed_base as usize) % deck_pairs.len();
    let (deck1_idx, deck2_idx) = deck_pairs[pair_idx];
    let deck1_spec = &deck_specs[deck1_idx];
    let deck2_spec = &deck_specs[deck2_idx];
    let deck1 = build_deck_for_player(&deck1_spec.entries, PlayerId::P1);
    let deck2 = build_deck_for_player(&deck2_spec.entries, PlayerId::P2);
    println!(
        "Seed-selected deck pair [{{}}/{{}}]: {{}} vs {{}}",
        pair_idx + 1,
        deck_pairs.len(),
        deck1_spec.name,
        deck2_spec.name
    );

    let opponents = [
        {reference_opponents_code}(AiType::RandomAiV4, "RandomAiV4 (v4)"),
    ];

    let mut overall_stats = MatchStats::default();
    let mut opponent_results = Vec::new();

    for (opponent_idx, (opponent_type, opponent_name)) in opponents.iter().enumerate() {{
        println!("\\nOpponent: {{}}", opponent_name);
        let opponent_seed_base = seed_base + (opponent_idx as u64) * 1_000_000;
        let stats = run_blended_matchup(
            &deck1,
            &deck2,
            AiType::TestAI,
            *opponent_type,
            num_matches,
            opponent_seed_base,
            &card_meta,
        );

        let win_rate = if stats.total > 0 {{
            (stats.p1_wins as f64 / stats.total as f64) * 100.0
        }} else {{
            0.0
        }};

        println!("  {ai_name}: {{}} wins / {{}} matches ({{:.1}}%)", 
            stats.p1_wins, stats.total, win_rate);
        println!("  {{}}: {{}} wins / {{}} matches ({{:.1}}%)", 
            opponent_name, stats.total - stats.p1_wins, stats.total, 100.0 - win_rate);

        opponent_results.push((opponent_name.to_string(), stats.clone()));
        overall_stats.p1_wins += stats.p1_wins;
        overall_stats.total += stats.total;
        overall_stats.outcomes.extend(stats.outcomes.clone());
    }}

    let overall_rate = if overall_stats.total > 0 {{
        (overall_stats.p1_wins as f64 / overall_stats.total as f64) * 100.0
    }} else {{
        0.0
    }};

    println!("\\nSummary:");
    println!("{{:18}} {{:>8}} {{:>9}} {{:>8}}", "Opponent", "Wins", "Matches", "Win%");
    for (name, stats) in &opponent_results {{
        let rate = if stats.total > 0 {{
            (stats.p1_wins as f64 / stats.total as f64) * 100.0
        }} else {{
            0.0
        }};
        println!("{{:18}} {{:>8}} {{:>9}} {{:>7.1}}%", name, stats.p1_wins, stats.total, rate);
    }}

    println!("\\nOverall: {{}} wins / {{}} matches ({{:.1}}%)", 
        overall_stats.p1_wins, overall_stats.total, overall_rate);

    let per_opponent_metrics: Vec<serde_json::Value> = opponent_results
        .iter()
        .map(|(name, stats)| {{
            serde_json::json!({{
                "opponent": name,
                "wins": stats.p1_wins,
                "matches": stats.total,
                "win_rate": if stats.total > 0 {{ (stats.p1_wins as f64) / (stats.total as f64) }} else {{ 0.0 }},
                "distributions": summarize_match_outcomes(&stats.outcomes),
            }})
        }})
        .collect();
    let benchmark_metrics = serde_json::json!({{
        "version": "algo_bench_metrics_v1",
        "matches_per_opponent_per_side": num_matches,
        "opponent_count": opponent_results.len(),
        "total_games": overall_stats.total,
        "overall_win_rate": if overall_stats.total > 0 {{
            (overall_stats.p1_wins as f64) / (overall_stats.total as f64)
        }} else {{
            0.0
        }},
        "overall_distributions": summarize_match_outcomes(&overall_stats.outcomes),
        "per_opponent": per_opponent_metrics,
    }});
    println!("BENCHMARK_METRICS_JSON: {{}}", benchmark_metrics);
}}
"""


def write_benchmark_workspace(
    *,
    work_dir: Path,
    ai_code: str,
    ai_name: str,
    reference_algos: list[dict],
    overzealous_dir: Path,
) -> dict[str, Path]:
    """Write benchmark sources/Cargo.toml into work_dir and return important paths."""
    work_dir.mkdir(parents=True, exist_ok=True)
    ai_module_file = work_dir / f"{ai_name.lower()}_ai.rs"
    benchmark_file = work_dir / "benchmark.rs"
    cargo_toml = work_dir / "Cargo.toml"

    ai_module_content = generate_ai_module(ai_code, ai_name)
    ai_module_file.write_text(ai_module_content)

    benchmark_content = generate_benchmark_binary(ai_name, ai_module_file, reference_algos)
    benchmark_file.write_text(benchmark_content)

    overzealous_path = str(overzealous_dir).replace("\\", "/")
    cargo_content = f"""[package]
name = "ai_benchmark"
version = "0.1.0"
edition = "2021"

# Keep this generated package out of parent workspaces.
[workspace]

[[bin]]
name = "benchmark"
path = "benchmark.rs"

[dependencies]
tcg_core = {{ path = "{overzealous_path}/tcg_core" }}
tcg_db = {{ path = "{overzealous_path}/tcg_db" }}
tcg_rules_ex = {{ path = "{overzealous_path}/tcg_rules_ex" }}
tcg_ai = {{ path = "{overzealous_path}/tcg_ai" }}
rusqlite = {{ version = "0.32", features = ["bundled"] }}
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
rand = "0.8"
rand_chacha = "0.3"
"""
    cargo_toml.write_text(cargo_content)
    return {
        "work_dir": work_dir,
        "ai_module_file": ai_module_file,
        "benchmark_file": benchmark_file,
        "cargo_toml": cargo_toml,
        "binary_path": work_dir / "target" / "release" / "benchmark",
    }


def build_benchmark_workspace(work_dir: Path) -> subprocess.CompletedProcess[str]:
    """Compile benchmark binary in an existing workspace."""
    return subprocess.run(
        ["cargo", "build", "--release", "--bin", "benchmark"],
        cwd=work_dir,
        capture_output=True,
        text=True,
    )


def run_benchmark_binary(
    *,
    binary_path: Path,
    server_db: str,
    cards_db: str,
    matches: int,
    seed_base: int,
) -> subprocess.CompletedProcess[str]:
    """Run a pre-built benchmark binary."""
    return subprocess.run(
        [
            str(binary_path),
            server_db,
            cards_db,
            str(matches),
            str(seed_base),
        ],
    )


def main():
    parser = argparse.ArgumentParser(description="Benchmark a new AI against v1-v4")
    parser.add_argument(
        "--mode",
        choices=["single", "build", "run-built"],
        default="single",
        help="single=compile+run in temp dir, build=compile workspace only, run-built=run existing binary",
    )
    parser.add_argument("--ai-code-file", type=Path, help="Path to Rust file with AI implementation")
    parser.add_argument("--ai-code", type=str, help="Rust code string with AI implementation")
    parser.add_argument("--name", type=str, required=True, help="Name of the AI (used for struct name)")
    parser.add_argument("--matches", type=int, default=50, help="Number of matches per opponent")
    parser.add_argument(
        "--seed-base",
        type=int,
        default=0,
        help="Deterministic seed base for matchup generation",
    )
    default_server = str(ALGO_BENCH_DATA_DIR / "server.sqlite") if (ALGO_BENCH_DATA_DIR / "server.sqlite").exists() else "data/server.sqlite"
    default_cards = str(ALGO_BENCH_DATA_DIR / "cards.sqlite") if (ALGO_BENCH_DATA_DIR / "cards.sqlite").exists() else "data/cards.sqlite"
    parser.add_argument("--server-db", type=str, default=default_server, help="Path to server DB")
    parser.add_argument("--cards-db", type=str, default=default_cards, help="Path to cards DB")
    parser.add_argument("--overzealous-dir", type=Path, default=OVERZEALOUS_DIR, help="Path to overzealous repo")
    parser.add_argument("--work-dir", type=Path, help="Workspace dir for --mode build / run-built")
    parser.add_argument("--binary-path", type=Path, help="Path to pre-built benchmark binary for --mode run-built")
    parser.add_argument("--manifest-file", type=Path, help="Optional JSON manifest output path")

    args = parser.parse_args()

    reference_algos = discover_reference_algos()
    if not reference_algos:
        print("Warning: no reference algos found in reference_algos directory.")

    if args.mode in {"single", "build"}:
        if not args.ai_code_file and not args.ai_code:
            parser.error("Must provide either --ai-code-file or --ai-code for --mode single/build")
        ai_code = args.ai_code_file.read_text() if args.ai_code_file else args.ai_code
    else:
        ai_code = ""

    if args.mode == "build":
        if not args.work_dir:
            parser.error("--work-dir is required for --mode build")
        paths = write_benchmark_workspace(
            work_dir=args.work_dir,
            ai_code=ai_code,
            ai_name=args.name,
            reference_algos=reference_algos,
            overzealous_dir=args.overzealous_dir,
        )
        print(f"Compiling benchmark for {args.name}...")
        compile_result = build_benchmark_workspace(paths["work_dir"])
        if compile_result.returncode != 0:
            print("Compilation failed:")
            print(compile_result.stderr)
            sys.exit(1)
        manifest = {
            "name": args.name,
            "work_dir": str(paths["work_dir"]),
            "binary_path": str(paths["binary_path"]),
        }
        if args.manifest_file:
            args.manifest_file.parent.mkdir(parents=True, exist_ok=True)
            args.manifest_file.write_text(json.dumps(manifest, indent=2))
        print(json.dumps(manifest))
        return

    if args.mode == "run-built":
        binary_path = args.binary_path
        if binary_path is None and args.work_dir:
            binary_path = args.work_dir / "target" / "release" / "benchmark"
        if binary_path is None:
            parser.error("--binary-path or --work-dir is required for --mode run-built")
        if not binary_path.exists():
            print(f"Benchmark binary not found: {binary_path}")
            sys.exit(1)
        run_result = run_benchmark_binary(
            binary_path=binary_path,
            server_db=args.server_db,
            cards_db=args.cards_db,
            matches=args.matches,
            seed_base=args.seed_base,
        )
        if run_result.returncode != 0:
            print("Benchmark failed")
            sys.exit(1)
        return

    # mode=single (backward compatible behavior)
    with tempfile.TemporaryDirectory() as tmpdir:
        paths = write_benchmark_workspace(
            work_dir=Path(tmpdir),
            ai_code=ai_code,
            ai_name=args.name,
            reference_algos=reference_algos,
            overzealous_dir=args.overzealous_dir,
        )
        print(f"Compiling benchmark for {args.name}...")
        compile_result = build_benchmark_workspace(paths["work_dir"])
        if compile_result.returncode != 0:
            print("Compilation failed:")
            print(compile_result.stderr)
            sys.exit(1)

        print("Running benchmark...")
        run_result = run_benchmark_binary(
            binary_path=paths["binary_path"],
            server_db=args.server_db,
            cards_db=args.cards_db,
            matches=args.matches,
            seed_base=args.seed_base,
        )
        if run_result.returncode != 0:
            print("Benchmark failed")
            sys.exit(1)


if __name__ == "__main__":
    main()
