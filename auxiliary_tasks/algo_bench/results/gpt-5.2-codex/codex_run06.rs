use rand::seq::SliceRandom;
use rand_chacha::ChaCha8Rng;
use rand::SeedableRng;

use tcg_core::{Action, Attack, CardInstanceId, GameView, Prompt};
use tcg_ai::traits::AiController;

pub struct CandidateAi {
    rng: ChaCha8Rng,
}

impl CandidateAi {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
        }
    }

    fn best_attack(attacks: &[Attack]) -> Option<Attack> {
        attacks.iter().cloned().max_by(|a, b| {
            let dmg = a.damage.cmp(&b.damage);
            if dmg != std::cmp::Ordering::Equal {
                return dmg;
            }
            a.cost.total_energy.cmp(&b.cost.total_energy).reverse()
        })
    }

    fn weighted_choice(&mut self, ids: &[(CardInstanceId, i64)]) -> Option<CardInstanceId> {
        let mut pool: Vec<CardInstanceId> = Vec::new();
        for (id, weight) in ids {
            let w = (*weight).max(0);
            for _ in 0..w {
                pool.push(*id);
            }
        }
        if pool.is_empty() {
            return ids.first().map(|(id, _)| *id);
        }
        pool.choose(&mut self.rng).copied()
    }

    fn remaining_hp(hp: u16, damage_counters: u16) -> i64 {
        let damage = (damage_counters as i64) * 10;
        (hp as i64) - damage
    }

    fn attack_energy_cost(attack: &Attack) -> i64 {
        attack.cost.total_energy as i64
    }

    fn score_starter(hp: u16, damage_counters: u16, energy_count: usize, attacks: &[Attack]) -> i64 {
        let remaining = Self::remaining_hp(hp, damage_counters);
        let mut best_attack_score = 0i64;
        for attack in attacks {
            let cost = Self::attack_energy_cost(attack);
            let damage = attack.damage as i64;
            let can_use = energy_count as i64 >= cost;
            let attack_score = damage * 2 - cost * 3 + if can_use { 15 } else { 0 };
            if attack_score > best_attack_score {
                best_attack_score = attack_score;
            }
        }
        remaining + best_attack_score
    }

    fn select_starter(&mut self, view: &GameView, options: &[CardInstanceId]) -> Option<CardInstanceId> {
        let mut scored: Vec<(CardInstanceId, i64)> = Vec::new();
        for id in options {
            if let Some(p) = view.my_bench.iter().find(|p| p.card.id == *id) {
                let score = Self::score_starter(p.hp, p.damage_counters, p.attached_energy.len(), &p.attacks);
                scored.push((*id, score));
            }
        }
        if scored.is_empty() {
            return options.first().copied();
        }
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        let mut weighted: Vec<(CardInstanceId, i64)> = Vec::new();
        let top_score = scored.first().map(|(_, s)| *s).unwrap_or(0);
        for (id, score) in scored {
            let delta = (top_score - score).abs();
            let weight = (20 - delta).max(1);
            weighted.push((id, weight));
        }
        self.weighted_choice(&weighted)
    }

    fn score_energy_target(active_id: CardInstanceId, target_id: CardInstanceId, attack_costs: &[Attack], target_energy: usize) -> i64 {
        let mut best_attack_score = 0i64;
        for attack in attack_costs {
            let cost = Self::attack_energy_cost(attack);
            let damage = attack.damage as i64;
            let remaining = (cost - target_energy as i64).max(0);
            let attack_score = damage * 2 - remaining * 6 - cost * 2;
            if attack_score > best_attack_score {
                best_attack_score = attack_score;
            }
        }
        let active_bonus = if target_id == active_id { 25 } else { 0 };
        best_attack_score + active_bonus
    }

    fn choose_energy_target(&mut self, view: &GameView) -> Option<CardInstanceId> {
        let active_id = view.my_active.as_ref().map(|p| p.card.id);
        let active_attacks = view.my_active.as_ref().map(|p| p.attacks.clone()).unwrap_or_default();
        let mut scored: Vec<(CardInstanceId, i64)> = Vec::new();
        if let Some(active_id) = active_id {
            let energy_count = view.my_active.as_ref().map(|p| p.attached_energy.len()).unwrap_or(0);
            let score = Self::score_energy_target(active_id, active_id, &active_attacks, energy_count);
            scored.push((active_id, score));
        }
        for p in &view.my_bench {
            let score = Self::score_energy_target(
                active_id.unwrap_or(p.card.id),
                p.card.id,
                &p.attacks,
                p.attached_energy.len(),
            );
            scored.push((p.card.id, score));
        }
        if scored.is_empty() {
            return None;
        }
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        let top_score = scored.first().map(|(_, s)| *s).unwrap_or(0);
        let mut weighted: Vec<(CardInstanceId, i64)> = Vec::new();
        for (id, score) in scored {
            let delta = (top_score - score).abs();
            let weight = (15 - delta).max(1);
            weighted.push((id, weight));
        }
        self.weighted_choice(&weighted)
    }

    fn choose_bench_basics(&mut self, view: &GameView, options: &[CardInstanceId], min: usize, max: usize) -> Vec<CardInstanceId> {
        let count = min.max(1).min(max).min(options.len());
        let mut scored: Vec<(CardInstanceId, i64)> = Vec::new();
        for id in options {
            let mut score = 0i64;
            if let Some(p) = view.my_hand.iter().find(|c| c.id == *id) {
                score += p.name.len() as i64;
            }
            if let Some(p) = view.my_bench.iter().find(|p| p.card.id == *id) {
                score += Self::remaining_hp(p.hp, p.damage_counters);
            }
            scored.push((*id, score));
        }
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.iter().take(count).map(|(id, _)| *id).collect()
    }

    fn choose_new_active(&mut self, view: &GameView, options: &[CardInstanceId]) -> Option<CardInstanceId> {
        let mut scored: Vec<(CardInstanceId, i64)> = Vec::new();
        for id in options {
            if let Some(p) = view.my_bench.iter().find(|p| p.card.id == *id) {
                let score = Self::score_starter(p.hp, p.damage_counters, p.attached_energy.len(), &p.attacks);
                scored.push((*id, score));
            }
        }
        if scored.is_empty() {
            return options.first().copied();
        }
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        let top_score = scored.first().map(|(_, s)| *s).unwrap_or(0);
        let mut weighted: Vec<(CardInstanceId, i64)> = Vec::new();
        for (id, score) in scored {
            let delta = (top_score - score).abs();
            let weight = (25 - delta).max(1);
            weighted.push((id, weight));
        }
        self.weighted_choice(&weighted)
    }
}

impl AiController for CandidateAi {
    fn propose_prompt_response(&mut self, view: &GameView, prompt: &Prompt) -> Vec<Action> {
        let mut actions: Vec<Action> = Vec::new();

        match prompt {
            Prompt::ChooseStartingActive { options } => {
                if let Some(card_id) = self.select_starter(view, options) {
                    actions.push(Action::ChooseActive { card_id });
                }
            }
            Prompt::ChooseBenchBasics { options, min, max } => {
                let picked = self.choose_bench_basics(view, options, *min, *max);
                actions.push(Action::ChooseBench { card_ids: picked });
            }
            Prompt::ChooseAttack { attacks } => {
                if let Some(best) = Self::best_attack(attacks) {
                    actions.push(Action::DeclareAttack { attack: best });
                }
                for attack in attacks {
                    actions.push(Action::DeclareAttack { attack: attack.clone() });
                }
            }
            Prompt::ChooseNewActive { player, options } => {
                if *player != view.player_id {
                    return vec![Action::EndTurn];
                }
                let candidates: Vec<CardInstanceId> = if options.is_empty() {
                    view.my_bench.iter().map(|p| p.card.id).collect()
                } else {
                    options.clone()
                };
                if let Some(card_id) = self.choose_new_active(view, &candidates) {
                    actions.push(Action::ChooseNewActive { card_id });
                }
            }
            _ => {}
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

        if !hints.playable_energy_ids.is_empty() && !hints.attach_targets.is_empty() {
            if let Some(energy_id) = hints.playable_energy_ids.first().copied() {
                let target_id = self.choose_energy_target(view);
                if let Some(target_id) = target_id {
                    actions.push(Action::AttachEnergy { energy_id, target_id });
                } else if let Some(&strict) = hints.attach_targets.first() {
                    actions.push(Action::AttachEnergy { energy_id, target_id: strict });
                }
            }
        }

        if let Some(best) = Self::best_attack(&hints.usable_attacks) {
            actions.push(Action::DeclareAttack { attack: best });
        }

        if let Some(&card_id) = hints.playable_basic_ids.first() {
            actions.push(Action::PlayBasic { card_id });
        }

        actions.push(Action::EndTurn);
        actions
    }
}