// Evaluation tests for DF-101 Mew ★ δ

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 101); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Mew ★ δ"); }
    #[test]
    fn eval_attack_1_name() { assert_eq!(ATTACK_1_NAME, "Mimicry"); }
    #[test]
    fn eval_attack_1_damage() { assert_eq!(ATTACK_1_DAMAGE, 0); }
    #[test]
    fn eval_attack_2_name() { assert_eq!(ATTACK_2_NAME, "Rainbow Wave"); }
    #[test]
    fn eval_attack_2_damage() { assert_eq!(ATTACK_2_DAMAGE, 0); }
    #[test] fn eval_attack_1_text() { assert_eq!(ATTACK_1_TEXT, "Choose an attack on 1 of your opponent's Pokémon in play. Mimicry copies that attack. This attack does nothing if Mew Star doesn't have the Energy necessary to use that attack. (You must still do anything else required for that attack.) Mew Star performs that attack."); }
    #[test] fn eval_attack_2_text() { assert_eq!(ATTACK_2_TEXT, "Choose 1 basic Energy card attached to Mew Star. This attack does 20 damage to each of your opponent's Pokémon that is the same type as the basic Energy card that you chose. (Don't apply Weakness and Resistance for Benched Pokémon.)"); }
}

#[cfg(test)]
mod deep_stateful_tests {
    use super::*;
    use tcg_core::{Attack, AttackCost, CardDefId, CardInstance, CardInstanceId, CardMeta, CardMetaMap, GameState, PlayerId, PokemonSlot, Prompt, Stage, Type};
    use tcg_rules_ex::RulesetConfig;

    fn create_pokemon_meta(name: &str, types: Vec<Type>, attacks: Vec<Attack>) -> CardMeta {
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
            is_star: true,
            is_delta: false,
            stage: Stage::Basic,
            types,
            weakness: None,
            resistance: None,
            retreat_cost: Some(1),
            trainer_effect: None,
            evolves_from: None,
            attacks,
        }
    }

    fn create_energy_meta(name: &str, energy_type: Type) -> CardMeta {
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

    fn setup_game() -> (GameState, CardInstanceId, CardInstanceId, CardInstanceId, CardInstanceId, CardDefId) {
        let card_meta = CardMetaMap::new();
        let mut game = GameState::new_with_card_meta(
            Vec::new(),
            Vec::new(),
            12345,
            RulesetConfig::default(),
            card_meta,
        );

        let p1_def = CardDefId::new("P1-ACTIVE".to_string());
        game.card_meta.insert(p1_def.clone(), create_pokemon_meta("Mew ★ δ", vec![Type::Psychic], Vec::new()));
        let p1_card = CardInstance::new(p1_def, PlayerId::P1);
        game.players[0].active = Some(PokemonSlot::new(p1_card));

        let p2_def = CardDefId::new("P2-ACTIVE".to_string());
        game.card_meta.insert(p2_def.clone(), create_pokemon_meta("P2 Active", vec![Type::Fire], Vec::new()));
        let p2_card = CardInstance::new(p2_def, PlayerId::P2);
        let mut p2_active = PokemonSlot::new(p2_card);
        p2_active.types = vec![Type::Fire];
        game.players[1].active = Some(p2_active);

        let bench_match_def = CardDefId::new("P2-BENCH-MATCH".to_string());
        game.card_meta.insert(bench_match_def.clone(), create_pokemon_meta("P2 Bench Match", vec![Type::Fire], Vec::new()));
        let bench_match_card = CardInstance::new(bench_match_def, PlayerId::P2);
        let mut bench_match_slot = PokemonSlot::new(bench_match_card.clone());
        bench_match_slot.types = vec![Type::Fire];
        game.players[1].bench.push(bench_match_slot);

        let bench_other_def = CardDefId::new("P2-BENCH-OTHER".to_string());
        game.card_meta.insert(bench_other_def.clone(), create_pokemon_meta("P2 Bench Other", vec![Type::Water], Vec::new()));
        let bench_other_card = CardInstance::new(bench_other_def, PlayerId::P2);
        let mut bench_other_slot = PokemonSlot::new(bench_other_card.clone());
        bench_other_slot.types = vec![Type::Water];
        game.players[1].bench.push(bench_other_slot);

        let borrowed_attack = Attack {
            name: "Borrowed Attack".to_string(),
            damage: 20,
            attack_type: Type::Colorless,
            cost: AttackCost { total_energy: 0, types: vec![] },
            effect_ast: None,
        };
        let mimic_def = CardDefId::new("P2-MIMIC".to_string());
        game.card_meta.insert(mimic_def.clone(), create_pokemon_meta("Mimic Target", vec![Type::Colorless], vec![borrowed_attack.clone()]));
        let mimic_card = CardInstance::new(mimic_def.clone(), PlayerId::P2);
        let mut mimic_slot = PokemonSlot::new(mimic_card.clone());
        mimic_slot.attacks = vec![borrowed_attack];
        game.players[1].bench.push(mimic_slot);

        game.turn.player = PlayerId::P1;

        let source_id = game.players[0].active.as_ref().unwrap().card.id;
        (game, source_id, bench_match_card.id, bench_other_card.id, mimic_card.id, mimic_def)
    }

    fn attach_energy(game: &mut GameState, source_id: CardInstanceId, energy_type: Type) {
        let def_id = CardDefId::new(format!("ENERGY-{energy_type:?}"));
        game.card_meta.insert(def_id.clone(), create_energy_meta("Energy", energy_type));
        let card = CardInstance::new(def_id, PlayerId::P1);
        if let Some(slot) = game.players[0].find_pokemon_mut(source_id) {
            slot.attached_energy.push(card);
        }
    }

    #[test]
    fn eval_mimicry_full_prompt_and_execute() {
        let (mut game, source_id, _bench_match_id, _bench_other_id, mimic_id, mimic_def) = setup_game();

        assert!(crate::df::runtime::execute_power(&mut game, "Mimicry", source_id));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChoosePokemonInPlay { options, min, max, .. }) => {
                assert_eq!(*min, 1);
                assert_eq!(*max, 1);
                assert!(options.contains(&mimic_id));
            }
            other => panic!("unexpected mimicry target prompt: {other:?}"),
        }

        assert!(crate::df::runtime::resolve_custom_prompt(
            &mut game,
            "DF-101:Mimicry:Target",
            Some(source_id),
            &[mimic_id],
        ));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChoosePokemonAttack { attacks, .. }) => {
                assert_eq!(attacks, &vec!["Borrowed Attack".to_string()]);
            }
            other => panic!("unexpected mimicry attack prompt: {other:?}"),
        }

        let effect_id = format!("DF-101:Mimicry:Attack:{}:Borrowed Attack", mimic_def.as_str());
        assert!(crate::df::runtime::resolve_custom_prompt(
            &mut game,
            &effect_id,
            Some(source_id),
            &[],
        ));
        let defender = game.players[1].active.as_ref().unwrap();
        assert_eq!(defender.damage_counters, 2);
    }

    #[test]
    fn eval_rainbow_wave_hits_matching_types_only() {
        let (mut game, source_id, bench_match_id, bench_other_id, _mimic_id, _mimic_def) = setup_game();
        attach_energy(&mut game, source_id, Type::Fire);

        assert!(crate::df::runtime::execute_power(&mut game, "Rainbow Wave", source_id));
        let active = game.players[1].active.as_ref().unwrap();
        let match_bench = game.players[1].bench.iter().find(|slot| slot.card.id == bench_match_id).unwrap();
        let other_bench = game.players[1].bench.iter().find(|slot| slot.card.id == bench_other_id).unwrap();
        assert_eq!(active.damage_counters, 2);
        assert_eq!(match_bench.damage_counters, 2);
        assert_eq!(other_bench.damage_counters, 0);
    }
}
