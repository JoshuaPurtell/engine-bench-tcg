// Evaluation tests for DF-090 Altaria ex δ

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 90); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Altaria ex δ"); }
    #[test]
    fn eval_ability_1_name() { assert_eq!(ABILITY_1_NAME, "Extra Boost"); }
    #[test]
    fn eval_attack_1_name() { assert_eq!(ATTACK_1_NAME, "Healing Light"); }
    #[test]
    fn eval_attack_1_damage() { assert_eq!(ATTACK_1_DAMAGE, 60); }
    #[test] fn eval_ability_1_text() { assert_eq!(ABILITY_1_TEXT, "Once during your turn (before your attack), you may attach a basic Energy card from your hand to 1 of your Stage 2 Pokémon-ex. This power can't be used if Altaria ex is affected by a Special Condition."); }
    #[test] fn eval_attack_1_text() { assert_eq!(ATTACK_1_TEXT, "Remove 1 damage counter from each of your Pokémon."); }
}

#[cfg(test)]
mod deep_stateful_tests {
    use super::*;
    use tcg_core::{CardDefId, CardInstance, CardInstanceId, CardMeta, CardMetaMap, GameState, Phase, PlayerId, PokemonSlot, Prompt, Stage, Type};
    use tcg_rules_ex::RulesetConfig;

    fn create_pokemon_meta(name: &str, stage: Stage, is_ex: bool) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: stage == Stage::Basic,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 120,
            energy_kind: None,
            provides: Vec::new(),
            trainer_kind: None,
            is_ex,
            is_star: false,
            is_delta: true,
            stage,
            types: vec![Type::Colorless],
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

    fn setup_game() -> (GameState, CardInstanceId, CardInstanceId, CardInstanceId) {
        let card_meta = CardMetaMap::new();
        let mut game = GameState::new_with_card_meta(
            Vec::new(),
            Vec::new(),
            3333,
            RulesetConfig::default(),
            card_meta,
        );

        let source_def = CardDefId::new("P1-ACTIVE".to_string());
        game.card_meta.insert(source_def.clone(), create_pokemon_meta("Altaria ex δ", Stage::Stage1, true));
        let source_card = CardInstance::new(source_def, PlayerId::P1);
        game.players[0].active = Some(PokemonSlot::new(source_card));

        let target_def = CardDefId::new("P1-STAGE2-EX".to_string());
        game.card_meta.insert(target_def.clone(), create_pokemon_meta("Stage2 ex", Stage::Stage2, true));
        let target_card = CardInstance::new(target_def, PlayerId::P1);
        game.players[0].bench.push(PokemonSlot::new(target_card.clone()));

        let energy_def = CardDefId::new("P1-ENERGY".to_string());
        game.card_meta.insert(energy_def.clone(), create_basic_energy_meta("Fire Energy", Type::Fire));
        let energy_card = CardInstance::new(energy_def, PlayerId::P1);
        let energy_id = energy_card.id;
        game.players[0].hand.add(energy_card);

        game.turn.player = PlayerId::P1;

        let source_id = game.players[0].active.as_ref().unwrap().card.id;
        (game, source_id, target_card.id, energy_id)
    }

    #[test]
    fn eval_extra_boost_attaches_energy_and_ends_turn() {
        let (mut game, source_id, target_id, energy_id) = setup_game();

        assert!(crate::df::runtime::execute_power(&mut game, "Extra Boost", source_id));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChooseCardsFromHand { options, min, max, .. }) => {
                assert_eq!(*min, Some(1));
                assert_eq!(*max, Some(1));
                assert!(options.contains(&energy_id));
            }
            other => panic!("unexpected extra boost prompt: {other:?}"),
        }

        assert!(crate::df::runtime::resolve_custom_prompt(
            &mut game,
            "DF-90:Extra Boost",
            Some(source_id),
            &[energy_id],
        ));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChoosePokemonInPlay { options, min, max, .. }) => {
                assert_eq!(*min, 1);
                assert_eq!(*max, 1);
                assert!(options.contains(&target_id));
            }
            other => panic!("unexpected target prompt: {other:?}"),
        }

        assert!(crate::df::runtime::resolve_custom_prompt(
            &mut game,
            "DF-90:Extra Boost",
            Some(source_id),
            &[target_id],
        ));
        let target = game.players[0].bench.iter().find(|slot| slot.card.id == target_id).unwrap();
        assert_eq!(target.attached_energy.len(), 1);
        assert_eq!(game.turn.phase, Phase::EndOfTurn);
    }
}
