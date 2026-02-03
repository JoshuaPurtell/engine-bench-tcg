// Evaluation tests for DF-020 Mantine δ

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 20); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Mantine δ"); }
    #[test]
    fn eval_ability_1_name() { assert_eq!(ABILITY_1_NAME, "Power Circulation"); }
    #[test]
    fn eval_attack_1_name() { assert_eq!(ATTACK_1_NAME, "Spiral Drain"); }
    #[test]
    fn eval_attack_1_damage() { assert_eq!(ATTACK_1_DAMAGE, 10); }
    #[test] fn eval_ability_1_text() { assert_eq!(ABILITY_1_TEXT, "Once during your turn (before your attack), you may search your discard pile for a basic Energy card, show it to your opponent, and put it on top of your deck. If you do, put 1 damage counter on Mantine. This power can't be used if Mantine is affected by a Special Condition."); }
    #[test] fn eval_attack_1_text() { assert_eq!(ATTACK_1_TEXT, "Remove 1 damage counter from Mantine."); }
}

#[cfg(test)]
mod deep_stateful_tests {
    use super::*;
    use tcg_core::{CardDefId, CardInstance, CardInstanceId, CardMeta, CardMetaMap, GameState, PlayerId, PokemonSlot, Prompt, SelectionDestination, Stage, Type};
    use tcg_rules_ex::RulesetConfig;

    fn create_pokemon_meta(name: &str) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: true,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 70,
            energy_kind: None,
            provides: Vec::new(),
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: true,
            stage: Stage::Basic,
            types: vec![Type::Water],
            weakness: None,
            resistance: None,
            retreat_cost: Some(1),
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![],
        }
    }

    fn create_basic_energy_meta(name: &str, energy_type: Type) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: false,
            is_tool: false,
            is_stadium: false,
            is_pokemon: false,
            is_energy: true,
            hp: 0,
            energy_kind: Some("Basic".to_string()),
            provides: vec![energy_type],
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: false,
            stage: Stage::Basic,
            types: Vec::new(),
            weakness: None,
            resistance: None,
            retreat_cost: None,
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![],
        }
    }

    fn create_trainer_meta(name: &str) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: false,
            is_tool: false,
            is_stadium: false,
            is_pokemon: false,
            is_energy: false,
            hp: 0,
            energy_kind: None,
            provides: Vec::new(),
            trainer_kind: Some("Item".to_string()),
            is_ex: false,
            is_star: false,
            is_delta: false,
            stage: Stage::Basic,
            types: Vec::new(),
            weakness: None,
            resistance: None,
            retreat_cost: None,
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
            9001,
            RulesetConfig::default(),
            card_meta,
        );

        let p1_def = CardDefId::new("P1-ACTIVE".to_string());
        game.card_meta.insert(p1_def.clone(), create_pokemon_meta("Mantine δ"));
        let p1_card = CardInstance::new(p1_def, PlayerId::P1);
        game.players[0].active = Some(PokemonSlot::new(p1_card));
        game.turn.player = PlayerId::P1;

        let source_id = game.players[0].active.as_ref().unwrap().card.id;
        (game, source_id)
    }

    fn add_to_discard(game: &mut GameState, def_id: &str, meta: CardMeta) -> CardInstanceId {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), meta);
        let card = CardInstance::new(card_def_id, PlayerId::P1);
        game.players[0].discard.add(card.clone());
        card.id
    }

    #[test]
    fn eval_power_circulation_prompts_energy_and_adds_damage() {
        let (mut game, source_id) = setup_game();
        let energy_id = add_to_discard(&mut game, "P1-BASIC-ENERGY", create_basic_energy_meta("Water Energy", Type::Water));
        let trainer_id = add_to_discard(&mut game, "P1-DISCARD-ITEM", create_trainer_meta("Item"));

        assert!(crate::df::runtime::execute_power(&mut game, "Power Circulation", source_id));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChooseCardsFromDiscard { count, min, max, destination, options, .. }) => {
                assert_eq!(*count, 1);
                assert_eq!(*min, Some(1));
                assert_eq!(*max, Some(1));
                assert_eq!(*destination, SelectionDestination::DeckTop);
                assert!(options.contains(&energy_id));
                assert!(!options.contains(&trainer_id));
            }
            other => panic!("unexpected power circulation prompt: {other:?}"),
        }

        assert!(crate::df::runtime::resolve_custom_prompt(
            &mut game,
            "DF-20:Power Circulation",
            Some(source_id),
            &[],
        ));
        let active = game.players[0].active.as_ref().unwrap();
        assert_eq!(active.damage_counters, 1);
    }
}
