// Auto-generated AI module for TemplateAi


/// TemplateAi - Pokemon TCG AI (~25% win rate baseline)
pub struct TemplateAi {
    rng: ChaCha8Rng,
}

impl TemplateAi {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
        }
    }

    fn best_attack(attacks: &[Attack]) -> Option<&Attack> {
        attacks.iter().max_by(|a, b| {
            let dmg = a.damage.cmp(&b.damage);
            if dmg != std::cmp::Ordering::Equal {
                return dmg;
            }
            b.cost.total_energy.cmp(&a.cost.total_energy)
        })
    }
}

impl AiController for TemplateAi {
    fn propose_prompt_response(&mut self, view: &GameView, prompt: &Prompt) -> Vec<Action> {
        let mut actions: Vec<Action> = Vec::new();
        match prompt {
            Prompt::ChooseStartingActive { options } => {
                if let Some(&card_id) = options.choose(&mut self.rng) {
                    actions.push(Action::ChooseActive { card_id });
                }
            }
            Prompt::ChooseBenchBasics { options, min, max } => {
                let count = (*min).max(1).min(*max).min(options.len());
                let mut picked: Vec<CardInstanceId> = options.to_vec();
                picked.shuffle(&mut self.rng);
                picked.truncate(count);
                actions.push(Action::ChooseBench { card_ids: picked });
            }
            Prompt::ChooseAttack { attacks } => {
                if let Some(best) = Self::best_attack(attacks) {
                    actions.push(Action::DeclareAttack { attack: best.clone() });
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
                if let Some(&card_id) = candidates.choose(&mut self.rng) {
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
        if let Some(&energy_id) = hints.playable_energy_ids.choose(&mut self.rng) {
            if let Some(&target_id) = hints.attach_targets.choose(&mut self.rng) {
                actions.push(Action::AttachEnergy { energy_id, target_id });
            }
        }
        if let Some(best) = Self::best_attack(&hints.usable_attacks) {
            actions.push(Action::DeclareAttack { attack: best.clone() });
        }
        if let Some(&card_id) = hints.playable_basic_ids.choose(&mut self.rng) {
            actions.push(Action::PlayBasic { card_id });
        }
        actions.push(Action::EndTurn);
        actions
    }
}

