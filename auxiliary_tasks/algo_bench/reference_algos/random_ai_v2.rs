use rand::seq::SliceRandom;
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;

use tcg_core::{Action, Attack, AttackCost, CardInstanceId, GameView, Prompt, Type};

use tcg_ai::traits::AiController;

pub struct RandomAiV2 {
    rng: ChaCha8Rng,
}

impl RandomAiV2 {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
        }
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

    fn best_attack_damage_first(attacks: &[Attack]) -> Option<Attack> {
        // v1-style (often stronger in prize races): max raw damage first.
        // Tie-breakers: lower total energy, then fewer typed reqs, then name.
        attacks.iter().cloned().max_by(|a, b| {
            a.damage
                .cmp(&b.damage)
                .then_with(|| b.cost.total_energy.cmp(&a.cost.total_energy)) // prefer lower energy
                .then_with(|| b.cost.types.len().cmp(&a.cost.types.len())) // prefer fewer typed reqs
                .then_with(|| a.name.cmp(&b.name))
        })
    }

    fn attached_energy_count_on(&self, view: &GameView, pokemon_id: CardInstanceId) -> usize {
        if let Some(active) = view.my_active.as_ref().filter(|p| p.card.id == pokemon_id) {
            return active.attached_energy.len();
        }
        if let Some(slot) = view.my_bench.iter().find(|p| p.card.id == pokemon_id) {
            return slot.attached_energy.len();
        }
        0
    }

    fn pick_target_by_energy(
        &mut self,
        view: &GameView,
        targets: &[CardInstanceId],
        prefer_max: bool,
    ) -> Option<CardInstanceId> {
        if targets.is_empty() {
            return None;
        }

        let mut best_ids: Vec<CardInstanceId> = Vec::new();
        let mut best_val: Option<usize> = None;

        for &tid in targets {
            let e = self.attached_energy_count_on(view, tid);
            match best_val {
                None => {
                    best_val = Some(e);
                    best_ids.clear();
                    best_ids.push(tid);
                }
                Some(v) => {
                    let better = if prefer_max { e > v } else { e < v };
                    if better {
                        best_val = Some(e);
                        best_ids.clear();
                        best_ids.push(tid);
                    } else if e == v {
                        best_ids.push(tid);
                    }
                }
            }
        }

        best_ids.choose(&mut self.rng).copied()
    }

    fn choose_energy_attach_target(
        &mut self,
        view: &GameView,
        attach_targets: &[CardInstanceId],
        can_attack_now: bool,
    ) -> Option<CardInstanceId> {
        if attach_targets.is_empty() {
            return None;
        }

        let active_id = view.my_active.as_ref().map(|p| p.card.id);
        let bench_targets: Vec<CardInstanceId> = attach_targets
            .iter()
            .copied()
            .filter(|id| Some(*id) != active_id)
            .collect();

        // Key change vs your current v2:
        // - If we can already attack this turn, prefer putting energy on bench (backup attacker)
        // - If we cannot yet attack, prioritize active to get online ASAP
        if can_attack_now {
            if let Some(t) = self.pick_target_by_energy(view, &bench_targets, false) {
                return Some(t); // bench with lowest energy -> spread
            }
            if let Some(a) = active_id {
                if attach_targets.contains(&a) {
                    return Some(a);
                }
            }
        } else {
            if let Some(a) = active_id {
                if attach_targets.contains(&a) {
                    return Some(a);
                }
            }
            if let Some(t) = self.pick_target_by_energy(view, &bench_targets, true) {
                return Some(t); // bench with highest energy -> rush one attacker if no active option
            }
        }

        self.choose_one(attach_targets)
    }

    fn choose_new_active_best(
        &mut self,
        view: &GameView,
        options: &[CardInstanceId],
    ) -> Option<CardInstanceId> {
        let candidates: Vec<CardInstanceId> = if !options.is_empty() {
            options.to_vec()
        } else {
            view.my_bench.iter().map(|p| p.card.id).collect()
        };
        if candidates.is_empty() {
            return None;
        }

        // Prefer max energy (tempo), break ties randomly.
        self.pick_target_by_energy(view, &candidates, true)
            .or_else(|| candidates.first().copied())
    }

    fn should_play_basic_now(&mut self, view: &GameView) -> bool {
        // More aggressive than your current v2:
        // - Always bench up to 3 if possible
        // - Then occasionally bench up to 5
        let bench_n = view.my_bench.len();
        if bench_n < 3 {
            return true;
        }
        if bench_n < 5 {
            return self.rng.gen_bool(0.20);
        }
        false
    }

    fn pick_hand_cards_disposable(
        &mut self,
        view: &GameView,
        pool: &[CardInstanceId],
        take: usize,
    ) -> Vec<CardInstanceId> {
        // Only uses information you already have:
        // - avoid discarding playable basics and playable energies when possible
        let hints = &view.action_hints;

        let mut scored: Vec<(i32, CardInstanceId)> = pool
            .iter()
            .copied()
            .map(|id| {
                let mut s: i32 = 0;

                // Prefer discarding non-playables (relative to current hints).
                if !hints.playable_basic_ids.contains(&id) {
                    s += 10;
                } else {
                    s -= 10;
                }

                if !hints.playable_energy_ids.contains(&id) {
                    s += 6;
                } else {
                    s -= 6;
                }

                // If our bench is already healthy, basics are a bit more disposable.
                if view.my_bench.len() >= 3 && hints.playable_basic_ids.contains(&id) {
                    s += 4;
                }

                // Random jitter to avoid brittle tie patterns.
                s += self.rng.gen_range(0..3);

                (s, id)
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().take(take).map(|(_, id)| id).collect()
    }
}

impl AiController for RandomAiV2 {
    fn propose_prompt_response(&mut self, view: &GameView, prompt: &Prompt) -> Vec<Action> {
        let mut actions: Vec<Action> = Vec::new();

        match prompt {
            Prompt::ChooseStartingActive { options } => {
                let valid: Vec<_> = options
                    .iter()
                    .filter(|id| view.action_hints.playable_basic_ids.contains(id))
                    .copied()
                    .collect();

                // Revert to v1's "first valid" (often better than random if engine orders options sanely).
                if let Some(card_id) = valid.first().copied() {
                    actions.push(Action::ChooseActive { card_id });
                }

                let mut rest = valid;
                rest.shuffle(&mut self.rng);
                for card_id in rest {
                    actions.push(Action::ChooseActive { card_id });
                }
            }

            Prompt::ChooseBenchBasics { options, min, max } => {
                let valid: Vec<_> = options
                    .iter()
                    .filter(|id| view.action_hints.playable_basic_ids.contains(id))
                    .copied()
                    .collect();

                let required_min = (*min).min(valid.len());
                let allowed_max = (*max).min(valid.len()).max(required_min);

                // More aggressive benching at setup: try to reach 3 (or max if <3).
                let desired = if allowed_max == 0 {
                    0
                } else {
                    let goal = 3usize;
                    goal.clamp(required_min, allowed_max)
                };

                let picked = self.choose_k(&valid, desired);
                actions.push(Action::ChooseBench { card_ids: picked });

                // Strict: satisfy min exactly.
                if required_min > 0 {
                    let picked_min = self.choose_k(&valid, required_min);
                    actions.push(Action::ChooseBench { card_ids: picked_min });
                }
            }

            Prompt::ChooseAttack { attacks } => {
                if let Some(best) = Self::best_attack_damage_first(attacks) {
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
                    return vec![Action::EndTurn];
                }

                // Improvement: if the prompt provides a count, prefer taking that (clamped),
                // rather than always taking the minimum.
                let desired = (*count).clamp(required_min, required_max);
                let take = desired.min(options.len());
                let picked = self.choose_k(options, take);
                actions.push(Action::TakeCardsFromDeck { card_ids: picked });

                // Strict: satisfy minimum.
                let min_take = required_min.min(options.len());
                if min_take != take {
                    let picked_min = self.choose_k(options, min_take);
                    actions.push(Action::TakeCardsFromDeck { card_ids: picked_min });
                }
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
                    return vec![Action::EndTurn];
                }

                let desired = (*count).clamp(required_min, required_max);
                let take = desired.min(options.len());
                let picked = self.choose_k(options, take);
                actions.push(Action::TakeCardsFromDiscard { card_ids: picked });

                let min_take = required_min.min(options.len());
                if min_take != take {
                    let picked_min = self.choose_k(options, min_take);
                    actions.push(Action::TakeCardsFromDiscard { card_ids: picked_min });
                }
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
                // Still conservative: satisfy minimum required.
                let take = (*min).min(*max).min(options.len());
                let picked = self.choose_k(options, take);
                actions.push(Action::ChoosePokemonTargets { target_ids: picked });
            }

            Prompt::ReorderDeckTop { player, options } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                let mut ids = options.clone();
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
                let mut energies: Vec<CardInstanceId> = Vec::new();

                if let Some(active) = view.my_active.as_ref().filter(|p| p.card.id == *pokemon_id) {
                    energies = active.attached_energy.iter().map(|c| c.id).collect();
                } else if let Some(slot) = view.my_bench.iter().find(|p| p.card.id == *pokemon_id) {
                    energies = slot.attached_energy.iter().map(|c| c.id).collect();
                }

                energies.shuffle(&mut self.rng);

                if energies.is_empty() {
                    actions.push(Action::ChooseAttachedEnergy { energy_ids: vec![] });
                    return actions;
                }

                if required_min == 0 && self.rng.gen_bool(0.75) {
                    // More likely to skip optional energy picks (often avoids self-harm).
                    actions.push(Action::ChooseAttachedEnergy { energy_ids: vec![] });
                } else {
                    let take = (*count).max(required_min).min(energies.len());
                    let mut picked = energies;
                    picked.truncate(take);
                    actions.push(Action::ChooseAttachedEnergy { energy_ids: picked });
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

                let desired = (*count).clamp(required_min, required_max);
                let take = desired.min(pool.len());

                let picked = self.pick_hand_cards_disposable(view, &pool, take);

                if *return_to_deck {
                    actions.push(Action::ReturnCardsFromHandToDeck { card_ids: picked });
                } else {
                    actions.push(Action::DiscardCardsFromHand { card_ids: picked });
                }

                // Strict: satisfy minimum only.
                let min_take = required_min.min(pool.len());
                if min_take != take {
                    let picked_min = self.pick_hand_cards_disposable(view, &pool, min_take);
                    if *return_to_deck {
                        actions.push(Action::ReturnCardsFromHandToDeck { card_ids: picked_min });
                    } else {
                        actions.push(Action::DiscardCardsFromHand { card_ids: picked_min });
                    }
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
                if let Some(name) = attacks.iter().max().cloned() {
                    actions.push(Action::DeclareAttack {
                        attack: Attack {
                            name,
                            ..Self::dummy_attack()
                        },
                    });
                }
                let mut names = attacks.clone();
                names.shuffle(&mut self.rng);
                for name in names {
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

                if let Some(best) = self.choose_new_active_best(view, options) {
                    actions.push(Action::ChooseNewActive { card_id: best });
                }

                let mut candidates: Vec<CardInstanceId> = if options.is_empty() {
                    view.my_bench.iter().map(|p| p.card.id).collect()
                } else {
                    options.clone()
                };

                candidates.shuffle(&mut self.rng);
                for card_id in candidates {
                    actions.push(Action::ChooseNewActive { card_id });
                }
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

        let hints = &view.action_hints;
        let mut actions: Vec<Action> = Vec::new();

        let can_attack_now = hints.can_declare_attack && !hints.usable_attacks.is_empty();
        let can_attach_energy =
            !hints.playable_energy_ids.is_empty() && !hints.attach_targets.is_empty();
        let can_play_basic = !hints.playable_basic_ids.is_empty();
        let want_play_basic = can_play_basic && self.should_play_basic_now(view);

        // Priority ladder tuned to beat v1:
        // 1) If we can attach energy, do it (if already online, attach to bench to build backup)
        // 2) If bench is low and we *can't* attach this moment, fill bench
        // 3) Attack
        //
        // This avoids v2's prior failure mode of over-investing in the active forever.

        if can_attach_energy {
            if let Some(target_id) =
                self.choose_energy_attach_target(view, &hints.attach_targets, can_attack_now)
            {
                // Prefer any playable energy (random).
                if let Some(&energy_id) = hints.playable_energy_ids.choose(&mut self.rng) {
                    actions.push(Action::AttachEnergy { energy_id, target_id });
                }
            }

            // StrictPaths: if attachment fails for some reason, try benching or attacking.
            if want_play_basic {
                if let Some(card_id) = self.choose_one(&hints.playable_basic_ids) {
                    actions.push(Action::PlayBasic { card_id });
                }
            }
            if let Some(best) = Self::best_attack_damage_first(&hints.usable_attacks) {
                actions.push(Action::DeclareAttack { attack: best });
            } else if hints.can_declare_attack {
                actions.push(Action::DeclareAttack {
                    attack: Self::dummy_attack(),
                });
            }

            actions.push(Action::EndTurn);
            return actions;
        }

        // If we can't attach energy right now, bias toward benching before attacking (more board presence).
        if want_play_basic {
            if let Some(card_id) = self.choose_one(&hints.playable_basic_ids) {
                actions.push(Action::PlayBasic { card_id });
            }
        }

        if let Some(best) = Self::best_attack_damage_first(&hints.usable_attacks) {
            actions.push(Action::DeclareAttack { attack: best });
        } else if hints.can_declare_attack {
            actions.push(Action::DeclareAttack {
                attack: Self::dummy_attack(),
            });
        }

        actions.push(Action::EndTurn);
        actions
    }
}
