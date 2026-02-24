use rand::seq::SliceRandom;
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;

use tcg_core::{
    Action,
    Attack,
    AttackCost,
    CardInstanceId,
    GameView,
    PokemonView,
    Prompt,
    Type,
};

use tcg_ai::traits::AiController;

pub struct RandomAiV3 {
    rng: ChaCha8Rng,
}

impl RandomAiV3 {
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

    fn find_my_pokemon<'a>(view: &'a GameView, pokemon_id: CardInstanceId) -> Option<&'a PokemonView> {
        if let Some(active) = view.my_active.as_ref().filter(|p| p.card.id == pokemon_id) {
            return Some(active);
        }
        view.my_bench.iter().find(|p| p.card.id == pokemon_id)
    }

    fn find_opponent_pokemon<'a>(
        view: &'a GameView,
        pokemon_id: CardInstanceId,
    ) -> Option<&'a PokemonView> {
        if let Some(active) = view
            .opponent_active
            .as_ref()
            .filter(|p| p.card.id == pokemon_id)
        {
            return Some(active);
        }
        view.opponent_bench.iter().find(|p| p.card.id == pokemon_id)
    }

    fn remaining_hp(view: &PokemonView) -> i32 {
        let damage = view.damage_counters as i32 * 10;
        let remaining = view.hp as i32 - damage;
        remaining.max(0)
    }

    fn opponent_active(view: &GameView) -> Option<&PokemonView> {
        view.opponent_active.as_ref()
    }

    fn expected_damage_vs(&self, attack: &Attack, defender: Option<&PokemonView>) -> i32 {
        let mut damage = attack.damage as i32;
        if let Some(defender) = defender {
            if let Some(weakness) = defender.weakness {
                if weakness.type_ == attack.attack_type {
                    damage = damage.saturating_mul(weakness.multiplier as i32);
                }
            }
            if let Some(resistance) = defender.resistance {
                if resistance.type_ == attack.attack_type {
                    damage = (damage - resistance.value as i32).max(0);
                }
            }
        }
        damage.max(0)
    }

    fn attack_score(&self, attack: &Attack, defender: Option<&PokemonView>) -> i64 {
        let expected_damage = self.expected_damage_vs(attack, defender) as i64;
        let cost = attack.cost.total_energy as i64;
        let effect_bonus = if attack.effect_ast.is_some() { 250 } else { 0 };
        let mut score = expected_damage * 100 - cost * 20 + effect_bonus;
        if let Some(defender) = defender {
            let remaining = Self::remaining_hp(defender) as i64;
            if expected_damage >= remaining {
                score += 100_000;
            }
        }
        score
    }

    fn best_attack_for_view(&self, view: &GameView, attacks: &[Attack]) -> Option<Attack> {
        let defender = Self::opponent_active(view);
        attacks.iter().cloned().max_by(|a, b| {
            self.attack_score(a, defender)
                .cmp(&self.attack_score(b, defender))
                .then_with(|| b.damage.cmp(&a.damage))
                .then_with(|| b.cost.total_energy.cmp(&a.cost.total_energy))
                .then_with(|| b.cost.types.len().cmp(&a.cost.types.len()))
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

    fn active_health_ratio(&self, view: &GameView) -> Option<f32> {
        let active = view.my_active.as_ref()?;
        if active.hp == 0 {
            return None;
        }
        let remaining = Self::remaining_hp(active) as f32;
        Some(remaining / active.hp as f32)
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

        let active_unhealthy = view
            .my_active
            .as_ref()
            .map(|a| !a.special_conditions.is_empty())
            .unwrap_or(false)
            || self
                .active_health_ratio(view)
                .map(|r| r < 0.45)
                .unwrap_or(false);

        if can_attack_now {
            if active_unhealthy {
                if let Some(t) = self.pick_target_by_energy(view, &bench_targets, true) {
                    return Some(t);
                }
            } else {
                if let Some(t) = self.pick_target_by_energy(view, &bench_targets, false) {
                    return Some(t);
                }
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
                return Some(t);
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

        let mut scored: Vec<(usize, i32, CardInstanceId)> = Vec::new();
        for &pid in &candidates {
            let energy = self.attached_energy_count_on(view, pid);
            let remaining = Self::find_my_pokemon(view, pid)
                .map(Self::remaining_hp)
                .unwrap_or(0);
            scored.push((energy, remaining, pid));
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        scored.first().map(|(_, _, pid)| *pid)
    }

    fn should_play_basic_now(&mut self, view: &GameView) -> bool {
        // Aggressive benching early; taper off when full.
        let bench_n = view.my_bench.len();
        if bench_n < 3 {
            return true;
        }
        if bench_n < 5 {
            return self.rng.gen_bool(0.25);
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

    fn pick_opponent_targets_low_hp(
        &mut self,
        view: &GameView,
        options: &[CardInstanceId],
        take: usize,
    ) -> Vec<CardInstanceId> {
        let mut scored: Vec<(i32, usize, CardInstanceId)> = Vec::new();
        for (idx, &pid) in options.iter().enumerate() {
            if let Some(p) = Self::find_opponent_pokemon(view, pid) {
                let remaining = Self::remaining_hp(p);
                scored.push((remaining, idx, pid));
            }
        }
        if scored.is_empty() {
            return self.choose_k(options, take);
        }
        scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        scored.into_iter().take(take).map(|(_, _, id)| id).collect()
    }

    fn pick_own_targets_low_energy(
        &mut self,
        view: &GameView,
        options: &[CardInstanceId],
        take: usize,
    ) -> Vec<CardInstanceId> {
        let mut scored: Vec<(usize, i32, usize, CardInstanceId)> = Vec::new();
        for (idx, &pid) in options.iter().enumerate() {
            let energy = self.attached_energy_count_on(view, pid);
            let remaining = Self::find_my_pokemon(view, pid)
                .map(Self::remaining_hp)
                .unwrap_or(0);
            scored.push((energy, remaining, idx, pid));
        }
        scored.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
        scored.into_iter().take(take).map(|(_, _, _, id)| id).collect()
    }

    fn choose_evolution_action(&mut self, view: &GameView) -> Option<Action> {
        let hints = &view.action_hints;
        if hints.playable_evolution_ids.is_empty() {
            return None;
        }

        let mut best: Option<(i32, CardInstanceId, CardInstanceId)> = None;

        for &card_id in &hints.playable_evolution_ids {
            let targets = match hints.evolve_targets_by_card_id.get(&card_id) {
                Some(targets) => targets,
                None => continue,
            };
            for &target_id in targets {
                let energy = self.attached_energy_count_on(view, target_id) as i32;
                let remaining = Self::find_my_pokemon(view, target_id)
                    .map(Self::remaining_hp)
                    .unwrap_or(0);
                let active_bonus = if view
                    .my_active
                    .as_ref()
                    .map(|a| a.card.id == target_id)
                    .unwrap_or(false)
                {
                    200
                } else {
                    0
                };
                let score = active_bonus + energy * 20 + remaining;
                match best {
                    None => best = Some((score, card_id, target_id)),
                    Some((best_score, _, _)) => {
                        if score > best_score {
                            best = Some((score, card_id, target_id));
                        }
                    }
                }
            }
        }

        best.map(|(_, card_id, target_id)| Action::EvolveFromHand { card_id, target_id })
    }

    fn choose_trainer_action(&mut self, view: &GameView) -> Option<Action> {
        let hints = &view.action_hints;
        let card_id = hints.playable_trainer_ids.choose(&mut self.rng).copied()?;
        Some(Action::PlayTrainer { card_id })
    }

    fn choose_basic_action(&mut self, view: &GameView) -> Option<Action> {
        let hints = &view.action_hints;
        let card_id = hints.playable_basic_ids.choose(&mut self.rng).copied()?;
        Some(Action::PlayBasic { card_id })
    }
}

impl AiController for RandomAiV3 {
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
                if let Some(best) = self.best_attack_for_view(view, attacks) {
                    actions.push(Action::DeclareAttack { attack: best });
                } else if let Some(best) = Self::best_attack_damage_first(attacks) {
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

                // Prefer taking the requested count when allowed.
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
                let take = (*min).min(*max).min(options.len());
                if take == 0 {
                    actions.push(Action::ChoosePokemonTargets { target_ids: vec![] });
                    return actions;
                }

                let any_opponent = options
                    .iter()
                    .any(|id| Self::find_opponent_pokemon(view, *id).is_some());
                let picked = if any_opponent {
                    self.pick_opponent_targets_low_hp(view, options, take)
                } else {
                    self.pick_own_targets_low_energy(view, options, take)
                };
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
        let can_play_trainer = !hints.playable_trainer_ids.is_empty();
        let can_evolve = !hints.playable_evolution_ids.is_empty();

        if can_evolve {
            if let Some(action) = self.choose_evolution_action(view) {
                actions.push(action);
                // StrictPaths
                if can_play_trainer {
                    if let Some(action) = self.choose_trainer_action(view) {
                        actions.push(action);
                    }
                }
                if can_attach_energy {
                    if let Some(target_id) =
                        self.choose_energy_attach_target(view, &hints.attach_targets, can_attack_now)
                    {
                        if let Some(&energy_id) = hints.playable_energy_ids.choose(&mut self.rng) {
                            actions.push(Action::AttachEnergy { energy_id, target_id });
                        }
                    }
                }
                if want_play_basic {
                    if let Some(action) = self.choose_basic_action(view) {
                        actions.push(action);
                    }
                }
                if let Some(best) = self.best_attack_for_view(view, &hints.usable_attacks) {
                    actions.push(Action::DeclareAttack { attack: best });
                } else if hints.can_declare_attack {
                    actions.push(Action::DeclareAttack {
                        attack: Self::dummy_attack(),
                    });
                }
                actions.push(Action::EndTurn);
                return actions;
            }
        }

        if can_play_trainer {
            if let Some(action) = self.choose_trainer_action(view) {
                actions.push(action);
                if can_attach_energy {
                    if let Some(target_id) =
                        self.choose_energy_attach_target(view, &hints.attach_targets, can_attack_now)
                    {
                        if let Some(&energy_id) = hints.playable_energy_ids.choose(&mut self.rng) {
                            actions.push(Action::AttachEnergy { energy_id, target_id });
                        }
                    }
                }
                if want_play_basic {
                    if let Some(action) = self.choose_basic_action(view) {
                        actions.push(action);
                    }
                }
                if let Some(best) = self.best_attack_for_view(view, &hints.usable_attacks) {
                    actions.push(Action::DeclareAttack { attack: best });
                } else if hints.can_declare_attack {
                    actions.push(Action::DeclareAttack {
                        attack: Self::dummy_attack(),
                    });
                }
                actions.push(Action::EndTurn);
                return actions;
            }
        }

        if can_attach_energy {
            if want_play_basic && can_attack_now {
                if let Some(action) = self.choose_basic_action(view) {
                    actions.push(action);
                }
            }

            if let Some(target_id) =
                self.choose_energy_attach_target(view, &hints.attach_targets, can_attack_now)
            {
                if let Some(&energy_id) = hints.playable_energy_ids.choose(&mut self.rng) {
                    actions.push(Action::AttachEnergy { energy_id, target_id });
                }
            }

            if want_play_basic && !can_attack_now {
                if let Some(action) = self.choose_basic_action(view) {
                    actions.push(action);
                }
            }

            if let Some(best) = self.best_attack_for_view(view, &hints.usable_attacks) {
                actions.push(Action::DeclareAttack { attack: best });
            } else if hints.can_declare_attack {
                actions.push(Action::DeclareAttack {
                    attack: Self::dummy_attack(),
                });
            }

            actions.push(Action::EndTurn);
            return actions;
        }

        if want_play_basic {
            if let Some(action) = self.choose_basic_action(view) {
                actions.push(action);
            }
        }

        if let Some(best) = self.best_attack_for_view(view, &hints.usable_attacks) {
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
