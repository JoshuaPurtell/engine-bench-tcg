// Evaluation tests for DF-011 Togetic δ

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 11); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Togetic δ"); }
    #[test]
    fn eval_attack_1_name() { assert_eq!(ATTACK_1_NAME, "Delta Copy"); }
    #[test]
    fn eval_attack_1_damage() { assert_eq!(ATTACK_1_DAMAGE, 0); }
    #[test]
    fn eval_attack_2_name() { assert_eq!(ATTACK_2_NAME, "Wave Splash"); }
    #[test]
    fn eval_attack_2_damage() { assert_eq!(ATTACK_2_DAMAGE, 30); }
    #[test] fn eval_attack_1_text() { assert_eq!(ATTACK_1_TEXT, "Choose an attack on 1 of your opponent's Pokémon in play that has δ on its card. Delta Copy copies that attack except for its Energy cost. (You must still do anything else required for that attack.) Togetic performs that attack."); }
}

#[cfg(test)]
mod deep_stateful_tests {
    use super::*;
    use tcg_core::{Attack, AttackCost, CardDefId, CardInstance, CardInstanceId, CardMeta, CardMetaMap, GameState, PlayerId, PokemonSlot, Prompt, Stage, Type};
    use tcg_rules_ex::RulesetConfig;

    fn create_pokemon_meta(name: &str, is_delta: bool) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: true,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 60,
            energy_kind: None,
            provides: Vec::new(),
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta,
            stage: Stage::Basic,
            types: vec![Type::Colorless],
            weakness: None,
            resistance: None,
            retreat_cost: Some(1),
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![],
        }
    }

    fn setup_game() -> (GameState, CardInstanceId, CardInstanceId) {
        let card_meta = CardMetaMap::new();
        let mut game = GameState::new_with_card_meta(
            Vec::new(),
            Vec::new(),
            12345,
            RulesetConfig::default(),
            card_meta,
        );

        let p1_def = CardDefId::new("P1-ACTIVE".to_string());
        game.card_meta.insert(p1_def.clone(), create_pokemon_meta("Togetic δ", false));
        let p1_card = CardInstance::new(p1_def, PlayerId::P1);
        game.players[0].active = Some(PokemonSlot::new(p1_card));

        let p2_def = CardDefId::new("P2-ACTIVE".to_string());
        game.card_meta.insert(p2_def.clone(), create_pokemon_meta("P2 Active", false));
        let p2_card = CardInstance::new(p2_def, PlayerId::P2);
        game.players[1].active = Some(PokemonSlot::new(p2_card));

        game.turn.player = PlayerId::P1;

        let source_id = game.players[0].active.as_ref().unwrap().card.id;

        let delta_def = CardDefId::new("P2-DELTA".to_string());
        game.card_meta.insert(delta_def.clone(), create_pokemon_meta("Delta Target", true));
        let delta_card = CardInstance::new(delta_def, PlayerId::P2);
        let mut delta_slot = PokemonSlot::new(delta_card.clone());
        delta_slot.is_delta = true;
        delta_slot.attacks = vec![Attack {
            name: "Borrowed Attack".to_string(),
            damage: 10,
            attack_type: Type::Colorless,
            cost: AttackCost { total_energy: 0, types: vec![] },
            effect_ast: None,
        }];
        game.players[1].bench.push(delta_slot);

        (game, source_id, delta_card.id)
    }

    #[test]
    fn eval_delta_copy_prompts_target_then_attack() {
        let (mut game, source_id, target_id) = setup_game();
        assert!(crate::df::runtime::execute_power(&mut game, "Delta Copy", source_id));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChoosePokemonInPlay { min, max, options, .. }) => {
                assert_eq!(*min, 1);
                assert_eq!(*max, 1);
                assert!(options.contains(&target_id));
            }
            other => panic!("unexpected target prompt: {other:?}"),
        }

        assert!(crate::df::runtime::resolve_custom_prompt(
            &mut game,
            "DF-11:Delta Copy:Target",
            Some(source_id),
            &[target_id],
        ));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChoosePokemonAttack { pokemon_id, attacks, .. }) => {
                assert_eq!(*pokemon_id, target_id);
                assert_eq!(attacks, &vec!["Borrowed Attack".to_string()]);
            }
            other => panic!("unexpected attack prompt: {other:?}"),
        }
    }
}


#[cfg(test)]
mod deep_stateful_tests_2 {
    use super::*;
    use tcg_core::{Attack, AttackCost, CardDefId, CardInstance, CardInstanceId, CardMeta, CardMetaMap, GameState, PlayerId, PokemonSlot, Prompt, Stage, Type};
    use tcg_rules_ex::RulesetConfig;

    fn create_pokemon_meta(name: &str, is_delta: bool, attacks: Vec<Attack>) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: true,
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
            is_delta,
            stage: Stage::Basic,
            types: vec![Type::Colorless],
            weakness: None,
            resistance: None,
            retreat_cost: Some(1),
            trainer_effect: None,
            evolves_from: None,
            attacks,
        }
    }

    fn setup_game() -> (GameState, CardInstanceId, CardInstanceId, CardDefId) {
        let card_meta = CardMetaMap::new();
        let mut game = GameState::new_with_card_meta(
            Vec::new(),
            Vec::new(),
            12345,
            RulesetConfig::default(),
            card_meta,
        );

        let p1_def = CardDefId::new("P1-ACTIVE".to_string());
        game.card_meta.insert(p1_def.clone(), create_pokemon_meta("Togetic δ", false, Vec::new()));
        let p1_card = CardInstance::new(p1_def, PlayerId::P1);
        game.players[0].active = Some(PokemonSlot::new(p1_card));

        let p2_def = CardDefId::new("P2-ACTIVE".to_string());
        game.card_meta.insert(p2_def.clone(), create_pokemon_meta("P2 Active", false, Vec::new()));
        let p2_card = CardInstance::new(p2_def, PlayerId::P2);
        game.players[1].active = Some(PokemonSlot::new(p2_card));

        let borrowed_attack = Attack {
            name: "Borrowed Attack".to_string(),
            damage: 20,
            attack_type: Type::Colorless,
            cost: AttackCost { total_energy: 0, types: vec![] },
            effect_ast: None,
        };
        let delta_def = CardDefId::new("P2-DELTA".to_string());
        game.card_meta.insert(
            delta_def.clone(),
            create_pokemon_meta("Delta Target", true, vec![borrowed_attack.clone()]),
        );
        let delta_card = CardInstance::new(delta_def.clone(), PlayerId::P2);
        let mut delta_slot = PokemonSlot::new(delta_card.clone());
        delta_slot.is_delta = true;
        delta_slot.attacks = vec![borrowed_attack];
        game.players[1].bench.push(delta_slot);

        game.turn.player = PlayerId::P1;

        let source_id = game.players[0].active.as_ref().unwrap().card.id;
        (game, source_id, delta_card.id, delta_def)
    }

    #[test]
    fn eval_delta_copy_executes_copied_attack() {
        let (mut game, source_id, target_id, target_def) = setup_game();
        assert!(crate::df::runtime::execute_power(&mut game, "Delta Copy", source_id));

        assert!(crate::df::runtime::resolve_custom_prompt(
            &mut game,
            "DF-11:Delta Copy:Target",
            Some(source_id),
            &[target_id],
        ));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChoosePokemonAttack { .. }) => {}
            other => panic!("unexpected attack prompt: {other:?}"),
        }

        let effect_id = format!(
            "DF-11:Delta Copy:Attack:{}:Borrowed Attack",
            target_def.as_str()
        );
        assert!(crate::df::runtime::resolve_custom_prompt(
            &mut game,
            &effect_id,
            Some(source_id),
            &[],
        ));
        let defender = game.players[1].active.as_ref().unwrap();
        assert_eq!(defender.damage_counters, 2);
    }
}
