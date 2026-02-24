use tcg_ai::{AiController, RandomAiV4};
use tcg_core::{Action, CardInstance, CardMetaMap, GameState, PlayerId, StepResult};
use tcg_rules_ex::RulesetConfig;

mod ref_random_ai {
    include!(r#"/Users/joshpurtell/Documents/Github/engine-bench-tcg/auxiliary_tasks/algo_bench/reference_algos/random_ai.rs"#);
}

mod ref_random_ai_v2 {
    include!(r#"/Users/joshpurtell/Documents/Github/engine-bench-tcg/auxiliary_tasks/algo_bench/reference_algos/random_ai_v2.rs"#);
}

mod ref_random_ai_v3 {
    include!(r#"/Users/joshpurtell/Documents/Github/engine-bench-tcg/auxiliary_tasks/algo_bench/reference_algos/random_ai_v3.rs"#);
}

mod ai_module {
    use rand_chacha::ChaCha8Rng;
    use rand::SeedableRng;
    use rand::seq::SliceRandom;
    use rand::seq::IteratorRandom;
    use rand::Rng;
    use tcg_core::{Action, GameView, Prompt, Attack, AttackCost, CardInstanceId, Type};
    use tcg_ai::traits::AiController;

    include!(r#"/Users/joshpurtell/Documents/Github/engine-bench-tcg/.algo_bench_build_cache/6006fb0a8131120b46789d6ba6a846fe234316913962863bd3bab88101bc0ba6/templateai_ai.rs"#);
}

use ai_module::TemplateAi;

#[derive(PartialEq, Clone, Copy, Debug)]
enum AiType {
    RefRandomAi,
    RefRandomAiV2,
    RefRandomAiV3,
    RandomAiV4,
    TestAI,
}

fn ai_name(ai_type: AiType) -> &'static str {
    match ai_type {
        AiType::RefRandomAi => "RandomAi (v1)",
        AiType::RefRandomAiV2 => "RandomAiV2 (v2)",
        AiType::RefRandomAiV3 => "RandomAiV3 (v3)",
        AiType::RandomAiV4 => "RandomAiV4 (v4)",
        AiType::TestAI => "TemplateAi",
    }
}

fn build_ai(ai_type: AiType, seed: u64) -> Box<dyn AiController> {
    match ai_type {
        AiType::RefRandomAi => Box::new(ref_random_ai::RandomAi::new(seed)),
        AiType::RefRandomAiV2 => Box::new(ref_random_ai_v2::RandomAiV2::new(seed)),
        AiType::RefRandomAiV3 => Box::new(ref_random_ai_v3::RandomAiV3::new(seed)),
        AiType::RandomAiV4 => Box::new(RandomAiV4::new(seed)),
        AiType::TestAI => Box::new(TemplateAi::new(seed)),
    }
}

fn apply_first_accepted(game: &mut GameState, player: PlayerId, candidates: Vec<Action>) -> bool {
    for action in candidates {
        if game.apply_action(player, action).is_ok() {
            return true;
        }
    }
    false
}

fn run_match_loop_with_stats(
    mut game: GameState,
    mut p1_ai: Option<&mut dyn AiController>,
    mut p2_ai: Option<&mut dyn AiController>,
    max_steps: usize,
) -> Option<(PlayerId, GameState)> {
    let mut steps_left = max_steps;
    let mut actions_budget = 5_000usize;

    while steps_left > 0 && actions_budget > 0 {
        match game.step() {
            StepResult::Event { .. } => {}
            StepResult::GameOver { winner } => return Some((winner, game)),
            StepResult::Prompt { prompt, for_player } => {
                let view = game.view_for_player(for_player);
                let mut candidates: Vec<Action> = match for_player {
                    PlayerId::P1 => {
                        if let Some(ai) = p1_ai.as_mut() {
                            ai.propose_prompt_response(&view, &prompt)
                        } else {
                            Vec::new()
                        }
                    }
                    PlayerId::P2 => {
                        if let Some(ai) = p2_ai.as_mut() {
                            ai.propose_prompt_response(&view, &prompt)
                        } else {
                            Vec::new()
                        }
                    }
                };
                candidates.push(Action::EndTurn);
                let applied = apply_first_accepted(&mut game, for_player, candidates);
                if !applied {
                    return None;
                }
                actions_budget = actions_budget.saturating_sub(1);
            }
            StepResult::Continue => {
                let phase = game.turn.phase;
                if matches!(phase, tcg_rules_ex::Phase::Main | tcg_rules_ex::Phase::Attack) {
                    let current = game.turn.player;
                    let view = game.view_for_player(current);
                    let mut candidates: Vec<Action> = match current {
                        PlayerId::P1 => {
                            if let Some(ai) = p1_ai.as_mut() {
                                ai.propose_free_actions(&view)
                            } else {
                                Vec::new()
                            }
                        }
                        PlayerId::P2 => {
                            if let Some(ai) = p2_ai.as_mut() {
                                ai.propose_free_actions(&view)
                            } else {
                                Vec::new()
                            }
                        }
                    };
                    candidates.push(Action::EndTurn);
                    let _ = apply_first_accepted(&mut game, current, candidates);
                    actions_budget = actions_budget.saturating_sub(1);
                }
            }
        }
        steps_left -= 1;
    }

    None
}

#[derive(Default, Clone)]
struct MatchStats {
    p1_wins: usize,
    total: usize,
}

fn run_match_series(
    deck1: &[CardInstance],
    deck2: &[CardInstance],
    p1_ai_type: AiType,
    p2_ai_type: AiType,
    num_matches: usize,
    seed_base: u64,
    count_player: PlayerId,
    card_meta: &CardMetaMap,
) -> MatchStats {
    let mut stats = MatchStats::default();

    for match_num in 0..num_matches {
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

        if let Some((winner, _)) = run_match_loop_with_stats(
            game,
            Some(ai1_box.as_mut()),
            Some(ai2_box.as_mut()),
            5_000,
        ) {
            stats.total += 1;
            if winner == count_player {
                stats.p1_wins += 1;
            }
        }
    }

    stats
}

fn run_blended_matchup(
    deck1: &[CardInstance],
    deck2: &[CardInstance],
    p1_ai_type: AiType,
    p2_ai_type: AiType,
    num_matches: usize,
    seed_base: u64,
    card_meta: &CardMetaMap,
) -> MatchStats {
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
    stats
}

fn load_card_meta(cards_db_path: &str) -> CardMetaMap {
    use rusqlite::Connection;
    let conn = Connection::open(cards_db_path).ok();
    if let Some(conn) = conn {
        tcg_db::load_card_meta_map(&conn).unwrap_or_default()
    } else {
        CardMetaMap::new()
    }
}

fn load_deck_by_name(server_db_path: &str, deck_name: &str, player: PlayerId) -> Option<Vec<CardInstance>> {
    use rusqlite::Connection;
    let conn = Connection::open(server_db_path).ok()?;
    
    let cards_json: String = conn.query_row(
        "SELECT cards_json FROM decks WHERE LOWER(name) LIKE LOWER(?1) AND is_public = 1 LIMIT 1",
        [&format!("%{}%", deck_name)],
        |row| row.get(0),
    ).ok()?;

    #[derive(serde::Deserialize)]
    struct DeckEntry {
        card_def_id: String,
        count: usize,
    }

    let entries: Vec<DeckEntry> = serde_json::from_str(&cards_json).ok()?;
    let mut deck = Vec::new();
    for entry in entries {
        for _ in 0..entry.count {
            deck.push(CardInstance::new(
                tcg_core::CardDefId::new(entry.card_def_id.clone()),
                player,
            ));
        }
    }

    if deck.is_empty() {
        None
    } else {
        Some(deck)
    }
}

fn main() {
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

    println!("Benchmarking TemplateAi vs v1-v4");
    println!("==================================");
    println!("Server DB: {}", server_db_path);
    println!("Cards DB: {}", cards_db_path);
    println!("Matches per pairing: {}", num_matches);
    println!("Seed base: {}", seed_base);
    println!();

    let card_meta = load_card_meta(cards_db_path);
    println!("Loaded {} card definitions", card_meta.len());

    // Load a test deck
    let deck_name = "Overzealous";
    let deck = match load_deck_by_name(server_db_path, deck_name, PlayerId::P1) {
        Some(d) => d,
        None => {
            eprintln!("Failed to load deck: {}", deck_name);
            return;
        }
    };
    println!("Loaded deck: {} ({} cards)", deck_name, deck.len());

    let opponents = [
        (AiType::RefRandomAi, "RandomAi (v1)"),
        (AiType::RefRandomAiV2, "RandomAiV2 (v2)"),
        (AiType::RefRandomAiV3, "RandomAiV3 (v3)"),
        (AiType::RandomAiV4, "RandomAiV4 (v4)"),
    ];

    let mut overall_stats = MatchStats::default();
    let mut opponent_results = Vec::new();

    for (opponent_idx, (opponent_type, opponent_name)) in opponents.iter().enumerate() {
        println!("\nOpponent: {}", opponent_name);
        let opponent_seed_base = seed_base + (opponent_idx as u64) * 1_000_000;
        let stats = run_blended_matchup(
            &deck,
            &deck,
            AiType::TestAI,
            *opponent_type,
            num_matches,
            opponent_seed_base,
            &card_meta,
        );

        let win_rate = if stats.total > 0 {
            (stats.p1_wins as f64 / stats.total as f64) * 100.0
        } else {
            0.0
        };

        println!("  TemplateAi: {} wins / {} matches ({:.1}%)", 
            stats.p1_wins, stats.total, win_rate);
        println!("  {}: {} wins / {} matches ({:.1}%)", 
            opponent_name, stats.total - stats.p1_wins, stats.total, 100.0 - win_rate);

        opponent_results.push((opponent_name.to_string(), stats.clone()));
        overall_stats.p1_wins += stats.p1_wins;
        overall_stats.total += stats.total;
    }

    let overall_rate = if overall_stats.total > 0 {
        (overall_stats.p1_wins as f64 / overall_stats.total as f64) * 100.0
    } else {
        0.0
    };

    println!("\nSummary:");
    println!("{:18} {:>8} {:>9} {:>8}", "Opponent", "Wins", "Matches", "Win%");
    for (name, stats) in &opponent_results {
        let rate = if stats.total > 0 {
            (stats.p1_wins as f64 / stats.total as f64) * 100.0
        } else {
            0.0
        };
        println!("{:18} {:>8} {:>9} {:>7.1}%", name, stats.p1_wins, stats.total, rate);
    }

    println!("\nOverall: {} wins / {} matches ({:.1}%)", 
        overall_stats.p1_wins, overall_stats.total, overall_rate);
}
