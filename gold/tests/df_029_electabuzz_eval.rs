// Evaluation tests for DF-029 Electabuzz δ

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 29); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Electabuzz δ"); }
    #[test]
    fn eval_ability_1_name() { assert_eq!(ABILITY_1_NAME, "Power of Evolution"); }
    #[test]
    fn eval_attack_1_name() { assert_eq!(ATTACK_1_NAME, "Swift"); }
    #[test]
    fn eval_attack_1_damage() { assert_eq!(ATTACK_1_DAMAGE, 30); }
    #[test] fn eval_ability_1_text() { assert_eq!(ABILITY_1_TEXT, "Once during your turn (before your attack), if Electabuzz is an Evolved Pokémon, you may draw a card from the bottom of your deck. This power can't be used if Electabuzz is affected by a Special Condition."); }
    #[test] fn eval_attack_1_text() { assert_eq!(ATTACK_1_TEXT, "This attack's damage isn't affected by Weakness, Resistance, Poké-Powers, Poké-Bodies, or any other effects on the Defending Pokémon."); }
}

#[cfg(test)]
mod deep_stateful_tests {
    use super::*;
    use tcg_core::{CardDefId, CardInstance, CardInstanceId, CardMeta, CardMetaMap, GameState, PlayerId, PokemonSlot, Stage, Type};
    use tcg_rules_ex::RulesetConfig;

    fn create_pokemon_meta(name: &str, stage: Stage) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: stage == Stage::Basic,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 80,
            energy_kind: None,
            provides: Vec::new(),
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: true,
            stage,
            types: vec![Type::Lightning],
            weakness: None,
            resistance: None,
            retreat_cost: Some(1),
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![],
        }
    }

    fn setup_game() -> (GameState, CardInstanceId) {
        let card_meta = CardMetaMap::new();
        let mut game = GameState::new_with_card_meta(
            Vec::new(),
            Vec::new(),
            7777,
            RulesetConfig::default(),
            card_meta,
        );

        let buzz_def = CardDefId::new("P1-ACTIVE".to_string());
        game.card_meta.insert(buzz_def.clone(), create_pokemon_meta("Electabuzz δ", Stage::Stage1));
        let buzz_card = CardInstance::new(buzz_def, PlayerId::P1);
        let mut slot = PokemonSlot::new(buzz_card);
        slot.stage = Stage::Stage1;

        let pre_def = CardDefId::new("ELEKID".to_string());
        game.card_meta.insert(pre_def.clone(), create_pokemon_meta("Elekid δ", Stage::Basic));
        let pre_card = CardInstance::new(pre_def, PlayerId::P1);
        slot.evolution_stack.push(pre_card);

        game.players[0].active = Some(slot);
        game.turn.player = PlayerId::P1;

        let source_id = game.players[0].active.as_ref().unwrap().card.id;
        (game, source_id)
    }

    fn add_to_deck_bottom(game: &mut GameState, def_id: &str) -> CardInstanceId {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), create_pokemon_meta(def_id, Stage::Basic));
        let card = CardInstance::new(card_def_id, PlayerId::P1);
        game.players[0].deck.add_to_bottom(card.clone());
        card.id
    }

    #[test]
    fn eval_power_of_evolution_draws_bottom_card() {
        let (mut game, source_id) = setup_game();
        let bottom_id = add_to_deck_bottom(&mut game, "P1-BOTTOM");
        add_to_deck_bottom(&mut game, "P1-OTHER");

        let initial_hand = game.players[0].hand.count();
        assert!(crate::df::runtime::execute_power(&mut game, "Power of Evolution", source_id));
        assert_eq!(game.players[0].hand.count(), initial_hand + 1);
        assert!(game.players[0].hand.order().contains(&bottom_id));
    }
}
