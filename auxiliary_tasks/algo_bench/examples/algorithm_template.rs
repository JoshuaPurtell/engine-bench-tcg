
use rand::seq::SliceRandom;
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;

use tcg_core::{Action, Attack, CardInstanceId, GameView, Prompt};
use tcg_ai::traits::AiController;

/// TemplateAi - Pokemon TCG AI
/// 
/// Key game concepts available via GameView:
/// - view.my_active: Your active Pokemon (Option<PokemonInPlay>)
/// - view.my_bench: Your bench Pokemon (Vec<PokemonInPlay>)
/// - view.opponent_active: Opponent's active Pokemon
/// - view.opponent_bench: Opponent's bench
/// - view.action_hints: Available actions (playable_basic_ids, playable_energy_ids, 
///   attach_targets, usable_attacks, can_declare_attack, can_retreat, etc.)
/// - view.my_hand: Cards in your hand
/// - view.my_prizes_count / view.opponent_prizes_count: Prize counts remaining
/// - view.current_player / view.player_id: Turn tracking
///
/// PokemonView (my_active, my_bench items, opponent_active, opponent_bench items) has:
/// - card: CardInstance (id, name, card_type, etc.)
/// - hp: Current max HP (u16)
/// - damage_counters: Current damage (10 damage = 1 counter, u16)
/// - attached_energy: Vec<CardInstance> of energy cards
/// - types: Vec<Type> - Pokemon types
/// - special_conditions: Vec of status conditions
/// - weakness/resistance: Type matchup modifiers
pub struct TemplateAi {
    rng: ChaCha8Rng,
}

impl TemplateAi {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
        }
    }
    
    /// Pick best attack by damage, preferring lower energy cost as tiebreaker
    fn best_attack(attacks: &[Attack]) -> Option<Attack> {
        attacks.iter().cloned().max_by(|a, b| {
            let dmg = a.damage.cmp(&b.damage);
            if dmg != std::cmp::Ordering::Equal {
                return dmg;
            }
            a.cost.total_energy.cmp(&b.cost.total_energy).reverse()
        })
    }
}

impl AiController for TemplateAi {
    fn propose_prompt_response(&mut self, view: &GameView, prompt: &Prompt) -> Vec<Action> {
        let mut actions: Vec<Action> = Vec::new();

        match prompt {
            Prompt::ChooseStartingActive { options } => {
                // TODO: Pick the best starter (consider HP, retreat cost, attack potential)
                if let Some(&card_id) = options.first() {
                    actions.push(Action::ChooseActive { card_id });
                }
            }
            Prompt::ChooseBenchBasics { options, min, max } => {
                // TODO: Strategically choose which basics to bench
                let count = (*min).max(1).min(*max).min(options.len());
                let picked: Vec<CardInstanceId> = options.iter().take(count).copied().collect();
                actions.push(Action::ChooseBench { card_ids: picked });
            }
            Prompt::ChooseAttack { attacks } => {
                // Pick highest damage attack
                if let Some(best) = Self::best_attack(attacks) {
                    actions.push(Action::DeclareAttack { attack: best });
                }
                // Strict: try all attacks
                for attack in attacks {
                    actions.push(Action::DeclareAttack { attack: attack.clone() });
                }
            }
            Prompt::ChooseNewActive { player, options } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                // TODO: Pick best replacement (consider HP, energy, matchup)
                let candidates: Vec<CardInstanceId> = if options.is_empty() {
                    view.my_bench.iter().map(|p| p.card.id).collect()
                } else {
                    options.clone()
                };
                if let Some(&card_id) = candidates.first() {
                    actions.push(Action::ChooseNewActive { card_id });
                }
            }
            _ => {
                // Handle other prompts with EndTurn strict
            }
        }

        actions
    }

    fn propose_free_actions(&mut self, view: &GameView) -> Vec<Action> {
        if view.current_player != view.player_id {
            return Vec::new();
        }
        if view.pending_prompt.is_some() {
            return Vec::new();
        }

        let mut actions: Vec<Action> = Vec::new();
        let hints = &view.action_hints;

        // TODO: Improve energy attachment strategy
        // - Prioritize Pokemon that can attack this turn
        // - Consider evolution targets
        if let Some(&energy_id) = hints.playable_energy_ids.first() {
            if let Some(&target_id) = hints.attach_targets.first() {
                actions.push(Action::AttachEnergy { energy_id, target_id });
            }
        }

        // Attack with best available attack
        if let Some(best) = Self::best_attack(&hints.usable_attacks) {
            actions.push(Action::DeclareAttack { attack: best });
        }

        // TODO: Improve bench strategy
        // - Set up evolution lines
        // - Maintain backup attackers
        if let Some(&card_id) = hints.playable_basic_ids.first() {
            actions.push(Action::PlayBasic { card_id });
        }

        // TODO: Consider retreat when advantageous
        // if hints.can_retreat { ... }

        actions.push(Action::EndTurn);
        actions
    }
}

