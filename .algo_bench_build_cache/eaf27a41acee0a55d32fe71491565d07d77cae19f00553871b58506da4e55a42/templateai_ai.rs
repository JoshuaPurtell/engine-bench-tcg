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
            // Prefer attacks with fewer energy cost if damage equal
            a.cost.total_energy.cmp(&b.cost.total_energy).reverse()
        })
    }
}

impl AiController for TemplateAi {
    fn propose_prompt_response(&mut self, view: &GameView, prompt: &Prompt) -> Vec<Action> {
        let mut actions: Vec<Action> = Vec::new();
        match prompt {
            Prompt::ChooseStartingActive { options } => {
                // Choose the strongest basic Pokemon if available, else first option
                if let Some(&card_id) = options.iter().copied().find(|&id| {
                    view.card_by_id(id).map_or(false, |card| card.is_basic_pokemon())
                }).or_else(|| options.first().copied()) {
                    actions.push(Action::ChooseActive { card_id });
                }
            }
            Prompt::ChooseBenchBasics { options, min, max } => {
                let count = (*min).max(1).min(*max).min(options.len());
                // Prefer basic Pokemon if possible
                let mut basics: Vec<CardInstanceId> = options.iter().copied()
                    .filter(|&id| view.card_by_id(id).map_or(false, |card| card.is_basic_pokemon()))
                    .take(count)
                    .collect();
                if basics.len() < count {
                    basics.extend(options.iter().copied().filter(|id| !basics.contains(id)).take(count - basics.len()));
                }
                actions.push(Action::ChooseBench { card_ids: basics });
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
                if let Some(&card_id) = candidates.first() {
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

        // Attach all available energy to the first available target
        for &energy_id in &hints.playable_energy_ids {
            if let Some(&target_id) = hints.attach_targets.first() {
                actions.push(Action::AttachEnergy { energy_id, target_id });
            }
        }

        // Play all basic Pokemon available
        for &card_id in &hints.playable_basic_ids {
            actions.push(Action::PlayBasic { card_id });
        }

        // Use best attack if possible
        if let Some(best) = Self::best_attack(&hints.usable_attacks) {
            actions.push(Action::DeclareAttack { attack: best.clone() });
        }

        actions.push(Action::EndTurn);
        actions
    }
}

