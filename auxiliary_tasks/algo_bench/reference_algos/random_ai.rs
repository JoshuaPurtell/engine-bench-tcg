use rand::seq::SliceRandom;
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;

use tcg_core::{Action, Attack, AttackCost, CardInstanceId, GameView, Prompt, Type};

use tcg_ai::traits::AiController;

/// A very simple AI: proposes a randomized list of candidate actions.
///
/// The server is expected to try candidates in order and keep the first one that is accepted by
/// `GameState::apply_action`. If none are accepted, the server should `EndTurn`.
pub struct RandomAi {
    rng: ChaCha8Rng,
}

impl RandomAi {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
        }
    }

    #[allow(dead_code)]
    fn shuffle_actions(&mut self, actions: &mut Vec<Action>) {
        actions.shuffle(&mut self.rng);
    }

    fn dummy_attack() -> Attack {
        Attack {
            name: String::new(),
            damage: 0,
            attack_type: Type::Colorless,
            cost: AttackCost {
                total_energy: 0,
                types: Vec::new(),
            },
            effect_ast: None,
        }
    }

    fn choose_k(&mut self, options: &[CardInstanceId], k: usize) -> Vec<CardInstanceId> {
        let mut ids: Vec<_> = options.to_vec();
        ids.shuffle(&mut self.rng);
        ids.truncate(k);
        ids
    }

    fn choose_one(&mut self, options: &[CardInstanceId]) -> Option<CardInstanceId> {
        options.choose(&mut self.rng).copied()
    }

    fn choose_one_pair(
        &mut self,
        left: &[CardInstanceId],
        right: &[CardInstanceId],
    ) -> Option<(CardInstanceId, CardInstanceId)> {
        Some((self.choose_one(left)?, self.choose_one(right)?))
    }

    fn best_attack(attacks: &[Attack]) -> Option<Attack> {
        // Super-simple heuristic: highest base damage, tie-breaker = lower total energy.
        attacks.iter().cloned().max_by(|a, b| {
            let dmg = a.damage.cmp(&b.damage);
            if dmg != std::cmp::Ordering::Equal {
                return dmg;
            }
            a.cost.total_energy.cmp(&b.cost.total_energy).reverse()
        })
    }
}

impl AiController for RandomAi {
    fn propose_prompt_response(&mut self, view: &GameView, prompt: &Prompt) -> Vec<Action> {
        // These are candidates; the server tries them in order and keeps the first accepted action.
        let mut actions: Vec<Action> = Vec::new();

        // The server will only call this when the prompt is for `view.player_id`.
        // We still keep `player` checks inside prompt variants when present.
        match prompt {
            Prompt::ChooseStartingActive { options } => {
                // Defensive: only select from cards that are actually playable basics.
                // Filter options against action_hints to ensure we never pick Energy.
                let valid_options: Vec<_> = options
                    .iter()
                    .filter(|id| view.action_hints.playable_basic_ids.contains(id))
                    .copied()
                    .collect();
                if let Some(card_id) = valid_options.first().copied() {
                    actions.push(Action::ChooseActive { card_id });
                }
            }
            Prompt::ChooseBenchBasics { options, min, max } => {
                // Defensive: only select from cards that are actually playable basics.
                // Filter options against action_hints to ensure we never pick Energy.
                let valid_options: Vec<_> = options
                    .iter()
                    .filter(|id| view.action_hints.playable_basic_ids.contains(id))
                    .copied()
                    .collect();
                // Always satisfy min. Prefer a small bench (min or min+1) to reduce random noise.
                let required_min = (*min).min(valid_options.len());
                let allowed_max = (*max).min(valid_options.len()).max(required_min);
                let desired = (required_min + 1).min(allowed_max);
                let picked = self.choose_k(&valid_options, desired);
                actions.push(Action::ChooseBench { card_ids: picked });
            }
            Prompt::ChooseAttack { attacks } => {
                // Pick the best attack first; strict to random order.
                if let Some(best) = Self::best_attack(attacks) {
                    actions.push(Action::DeclareAttack { attack: best });
                }
                let mut shuffled = attacks.clone();
                shuffled.shuffle(&mut self.rng);
                for attack in shuffled {
                    actions.push(Action::DeclareAttack { attack });
                }
            }
            Prompt::ChooseCardsFromDeck {
                player,
                count,
                options,
                min,
                max,
                ..
            } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                let required_min = min.unwrap_or(*count);
                let required_max = max.unwrap_or(*count);
                if options.is_empty() && required_min > 0 {
                    // Can't answer a required deck prompt without options (Option A).
                    return vec![Action::EndTurn];
                }
                let take = required_min.min(required_max).min(options.len());
                let picked = self.choose_k(options, take);
                actions.push(Action::TakeCardsFromDeck { card_ids: picked });
            }
            Prompt::ChooseCardsFromDiscard {
                player,
                count,
                options,
                min,
                max,
                ..
            } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                let required_min = min.unwrap_or(*count);
                let required_max = max.unwrap_or(*count);
                if options.is_empty() && required_min > 0 {
                    // Can't satisfy a required selection.
                    return vec![Action::EndTurn];
                }
                let take = required_min.min(required_max).min(options.len());
                let picked = self.choose_k(options, take);
                actions.push(Action::TakeCardsFromDiscard { card_ids: picked });
            }
            Prompt::ChoosePokemonInPlay {
                player,
                options,
                min,
                max,
            } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                let take = (*min).min(*max).min(options.len());
                let picked = self.choose_k(options, take);
                actions.push(Action::ChoosePokemonTargets { target_ids: picked });
            }
            Prompt::ReorderDeckTop { player, options } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                let mut ids = options.clone();
                // random reorder
                ids.shuffle(&mut self.rng);
                actions.push(Action::ReorderDeckTop { card_ids: ids });
            }
            Prompt::ChooseAttachedEnergy {
                player,
                pokemon_id,
                count,
                min,
            } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                let required_min = min.unwrap_or(*count);
                // Use only view-visible energies on that pokemon.
                let mut energies: Vec<CardInstanceId> = Vec::new();
                if let Some(active) = view.my_active.as_ref().filter(|p| p.card.id == *pokemon_id) {
                    energies = active.attached_energy.iter().map(|c| c.id).collect();
                } else if let Some(slot) = view.my_bench.iter().find(|p| p.card.id == *pokemon_id) {
                    energies = slot.attached_energy.iter().map(|c| c.id).collect();
                }
                energies.shuffle(&mut self.rng);
                // If optional (min=0), AI may choose to skip. For now, if we have energy, take some.
                if energies.is_empty() || (required_min == 0 && self.rng.gen_bool(0.5)) {
                    // Skip if no energy or randomly skip optional energy movement
                    actions.push(Action::ChooseAttachedEnergy { energy_ids: vec![] });
                } else {
                    energies.truncate(*count);
                    actions.push(Action::ChooseAttachedEnergy { energy_ids: energies });
                }
            }
            Prompt::ChooseCardsFromHand {
                player,
                count,
                options,
                min,
                max,
                return_to_deck,
            } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                let required_min = min.unwrap_or(*count);
                let required_max = max.unwrap_or(*count);
                let pool: Vec<CardInstanceId> = if options.is_empty() {
                    view.my_hand.iter().map(|c| c.id).collect()
                } else {
                    options.clone()
                };
                if pool.is_empty() && required_min > 0 {
                    return vec![Action::EndTurn];
                }
                let take = required_min.min(required_max).min(pool.len());
                let picked = self.choose_k(&pool, take);
                if *return_to_deck {
                    actions.push(Action::ReturnCardsFromHandToDeck { card_ids: picked });
                } else {
                    actions.push(Action::DiscardCardsFromHand { card_ids: picked });
                }
            }
            Prompt::ChooseCardsInPlay {
                player,
                options,
                min,
                max,
            } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                let take = (*min).min(*max).min(options.len());
                let picked = self.choose_k(options, take);
                actions.push(Action::ChooseCardsInPlay { card_ids: picked });
            }
            Prompt::ChoosePrizeCards {
                player,
                options,
                min,
                max,
            } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                let take = (*min).min(*max).min(options.len());
                let picked = self.choose_k(options, take);
                actions.push(Action::ChoosePrizeCards { card_ids: picked });
            }
            Prompt::ChooseDefenderAttack { player, attacks, .. } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                // Prefer lexicographically "highest" name just to be deterministic-ish; then shuffle strict_paths.
                if let Some(name) = attacks.iter().max().cloned() {
                    actions.push(Action::DeclareAttack {
                        attack: Attack {
                            name,
                            ..Self::dummy_attack()
                        },
                    });
                }
                let mut attacks = attacks.clone();
                attacks.shuffle(&mut self.rng);
                for name in attacks {
                    actions.push(Action::DeclareAttack {
                        attack: Attack {
                            name,
                            ..Self::dummy_attack()
                        },
                    });
                }
            }
            Prompt::ChoosePokemonAttack { player, attacks, .. } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                if let Some(name) = attacks.iter().max().cloned() {
                    actions.push(Action::ChoosePokemonAttack { attack_name: name });
                }
                let mut names = attacks.clone();
                names.shuffle(&mut self.rng);
                for name in names {
                    actions.push(Action::ChoosePokemonAttack { attack_name: name });
                }
            }
            Prompt::ChooseSpecialCondition { player, options } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                if let Some(condition) = options.first().copied() {
                    actions.push(Action::ChooseSpecialCondition { condition });
                }
                let mut choices = options.clone();
                choices.shuffle(&mut self.rng);
                for condition in choices {
                    actions.push(Action::ChooseSpecialCondition { condition });
                }
            }
            Prompt::ChooseNewActive { player, options } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                // Prefer prompt options (engine-provided, should be bench Pokemon IDs).
                // If options are empty (bug/edge-case), fall back to what the AI can see on its bench.
                let mut candidates: Vec<CardInstanceId> = if options.is_empty() {
                    view.my_bench.iter().map(|p| p.card.id).collect()
                } else {
                    options.clone()
                };

                // Pick the first option deterministically, then shuffled strict_paths.
                if let Some(card_id) = candidates.first().copied() {
                    actions.push(Action::ChooseNewActive { card_id });
                }
                candidates.shuffle(&mut self.rng);
                for card_id in candidates {
                    actions.push(Action::ChooseNewActive { card_id });
                }
            }
        }

        actions
    }

    fn propose_free_actions(&mut self, view: &GameView) -> Vec<Action> {
        // Only act on our own turn.
        if view.current_player != view.player_id {
            return Vec::new();
        }
        if view.pending_prompt.is_some() {
            return Vec::new();
        }

        let mut actions: Vec<Action> = Vec::new();

        let hints = &view.action_hints;
        // Prefer attach -> attack -> end turn.
        //
        // Keep the candidate set intentionally small to reduce "random thrash":
        // - at most 1 basic play
        // - at most 1 energy attach attempt
        // - at most 1 attack attempt
        if let Some(card_id) = self.choose_one(&hints.playable_basic_ids) {
            actions.push(Action::PlayBasic { card_id });
        }
        if let Some((energy_id, target_id)) =
            self.choose_one_pair(&hints.playable_energy_ids, &hints.attach_targets)
        {
            actions.push(Action::AttachEnergy { energy_id, target_id });
        }
        if let Some(best) = Self::best_attack(&hints.usable_attacks) {
            actions.push(Action::DeclareAttack { attack: best });
        } else if hints.can_declare_attack {
            // Will trigger ChooseAttack prompt if multiple attacks are available.
            actions.push(Action::DeclareAttack {
                attack: Self::dummy_attack(),
            });
        }

        // Always allow progress.
        if hints.can_end_turn {
            actions.push(Action::EndTurn);
        } else {
            actions.push(Action::EndTurn);
        }

        actions
    }
}
