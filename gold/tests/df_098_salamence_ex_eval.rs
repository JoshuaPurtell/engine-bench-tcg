// Evaluation tests for DF-098 Salamence ex δ

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 98); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Salamence ex δ"); }
    #[test]
    fn eval_ability_1_name() { assert_eq!(ABILITY_1_NAME, "Type Shift"); }
    #[test]
    fn eval_attack_1_name() { assert_eq!(ATTACK_1_NAME, "Claw Swipe"); }
    #[test]
    fn eval_attack_1_damage() { assert_eq!(ATTACK_1_DAMAGE, 60); }
    #[test]
    fn eval_attack_2_name() { assert_eq!(ATTACK_2_NAME, "Dual Stream"); }
    #[test]
    fn eval_attack_2_damage() { assert_eq!(ATTACK_2_DAMAGE, 80); }
    #[test] fn eval_ability_1_text() { assert_eq!(ABILITY_1_TEXT, "Once during your turn (before your attack), you may use this power. Salamence ex's type is Fire until the end of your turn."); }
    #[test] fn eval_attack_2_text() { assert_eq!(ATTACK_2_TEXT, "You may do up to 40 damage instead of 80 to the Defending Pokémon. If you do, this attack does 40 damage to 1 of your opponent's Benched Pokémon. (Don't apply Weakness and Resistance for Benched Pokémon.)"); }
}

#[cfg(test)]
mod deep_stateful_tests {
    use super::*;
    use tcg_core::{CardDefId, CardInstance, CardInstanceId, CardMeta, CardMetaMap, GameState, PlayerId, PokemonSlot, Prompt, Stage, Type};
    use tcg_rules_ex::RulesetConfig;

    fn create_pokemon_meta(name: &str, hp: u16) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: true,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp,
            energy_kind: None,
            provides: Vec::new(),
            trainer_kind: None,
            is_ex: false,
            is_star: false,
            is_delta: false,
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
        game.card_meta.insert(p1_def.clone(), create_pokemon_meta("Salamence ex δ", 150));
        let p1_card = CardInstance::new(p1_def, PlayerId::P1);
        game.players[0].active = Some(PokemonSlot::new(p1_card));

        let p2_def = CardDefId::new("P2-ACTIVE".to_string());
        game.card_meta.insert(p2_def.clone(), create_pokemon_meta("P2 Active", 80));
        let p2_card = CardInstance::new(p2_def, PlayerId::P2);
        game.players[1].active = Some(PokemonSlot::new(p2_card));

        let bench_def = CardDefId::new("P2-BENCH".to_string());
        game.card_meta.insert(bench_def.clone(), create_pokemon_meta("P2 Bench", 60));
        let bench_card = CardInstance::new(bench_def, PlayerId::P2);
        game.players[1].bench.push(PokemonSlot::new(bench_card.clone()));

        game.turn.player = PlayerId::P1;

        let source_id = game.players[0].active.as_ref().unwrap().card.id;
        (game, source_id, bench_card.id)
    }

    #[test]
    fn eval_dual_stream_prompts_bench_then_marks_reduction() {
        let (mut game, source_id, bench_id) = setup_game();
        assert!(crate::df::runtime::execute_power(&mut game, "Dual Stream", source_id));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChoosePokemonInPlay { options, min, max, .. }) => {
                assert_eq!(*min, 0);
                assert_eq!(*max, 1);
                assert!(options.contains(&bench_id));
            }
            other => panic!("unexpected bench prompt: {other:?}"),
        }

        assert!(crate::df::runtime::resolve_custom_prompt(
            &mut game,
            "DF-98:Dual Stream:Bench",
            Some(source_id),
            &[bench_id],
        ));
        assert!(game.has_marker(source_id, "Dual Stream Reduced"));
        let bench = game.players[1].bench.iter().find(|slot| slot.card.id == bench_id).unwrap();
        assert_eq!(bench.damage_counters, 4);
    }

    #[test]
    fn eval_type_shift_overrides_type() {
        let (mut game, source_id, _) = setup_game();
        if let Some(active) = game.players[0].active.as_mut() {
            active.types = vec![Type::Water];
        }

        assert!(crate::df::runtime::execute_power(&mut game, "Type Shift", source_id));
        let types = game.effective_types_for(source_id);
        assert!(types.contains(&Type::Fire));
        assert!(!types.contains(&Type::Water));
    }
}
