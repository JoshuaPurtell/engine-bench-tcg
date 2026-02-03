// Evaluation tests for DF-021 Quagsire δ

#[cfg(test)]
mod eval_tests {
    use super::*;
    #[test]
    fn eval_set_constant() { assert_eq!(SET, "DF"); }
    #[test]
    fn eval_number_constant() { assert_eq!(NUMBER, 21); }
    #[test]
    fn eval_name_constant() { assert_eq!(NAME, "Quagsire δ"); }
    #[test]
    fn eval_ability_1_name() { assert_eq!(ABILITY_1_NAME, "Dig Up"); }
    #[test]
    fn eval_attack_1_name() { assert_eq!(ATTACK_1_NAME, "Pump Out"); }
    #[test]
    fn eval_attack_1_damage() { assert_eq!(ATTACK_1_DAMAGE, 50); }
    #[test] fn eval_pump_out_damage_false() { assert_eq!(pump_out_damage(false), 50); }
    #[test] fn eval_pump_out_damage_true() { assert_eq!(pump_out_damage(true), 70); }
    #[test] fn eval_ability_1_text() { assert_eq!(ABILITY_1_TEXT, "Once during your turn, when you play Quagsire from your hand to evolve 1 of your Pokémon, you may search your discard pile for up to 2 Pokémon Tool cards, show them to your opponent, and put them into your hand."); }
    #[test] fn eval_attack_1_text() { assert_eq!(ATTACK_1_TEXT, "If Quagsire has a Pokémon Tool card attached to it, this attack does 50 damage plus 20 more damage."); }
}

#[cfg(test)]
mod behavioral_tests {
    #![allow(dead_code)]

    use super::*;
    use tcg_core::{CardDefId, CardInstance, CardInstanceId, CardMeta, CardMetaMap, GameState, PlayerId, PokemonSlot, Stage, Type};
    use tcg_rules_ex::{RulesetConfig, SpecialCondition};

    fn create_pokemon_meta(
        name: &str,
        stage: Stage,
        types: Vec<Type>,
        is_ex: bool,
        is_delta: bool,
        retreat_cost: u8,
    ) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: stage == Stage::Basic,
            is_tool: false,
            is_stadium: false,
            is_pokemon: true,
            is_energy: false,
            hp: 100,
            energy_kind: None,
            provides: Vec::new(),
            trainer_kind: None,
            is_ex,
            is_star: false,
            is_delta,
            stage,
            types,
            weakness: None,
            resistance: None,
            retreat_cost: Some(retreat_cost),
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![],
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

    fn create_trainer_meta(name: &str, trainer_kind: Option<&str>, is_tool: bool) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: false,
            is_tool,
            is_stadium: false,
            is_pokemon: false,
            is_energy: false,
            hp: 0,
            energy_kind: None,
            provides: Vec::new(),
            trainer_kind: trainer_kind.map(|value| value.to_string()),
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

    fn apply_meta(slot: &mut PokemonSlot, meta: &CardMeta) {
        slot.hp = meta.hp as u16;
        slot.is_delta = meta.is_delta;
        slot.is_ex = meta.is_ex;
        slot.is_star = meta.is_star;
        slot.stage = meta.stage;
        slot.types = meta.types.clone();
        slot.retreat_cost = meta.retreat_cost.unwrap_or(0) as u8;
    }

    fn create_basic_test_game() -> (GameState, CardInstanceId, CardInstanceId) {
        let mut card_meta_map = CardMetaMap::new();
        let mut deck1 = Vec::new();
        let mut deck2 = Vec::new();

        let active_def_id = CardDefId::new("P1-ACTIVE".to_string());
        let active_meta = create_pokemon_meta("P1 Active", Stage::Basic, vec![Type::Colorless], false, false, 1);
        card_meta_map.insert(active_def_id.clone(), active_meta.clone());
        let active_card = CardInstance::new(active_def_id, PlayerId::P1);
        let attacker_id = active_card.id;

        let p2_active_def_id = CardDefId::new("P2-ACTIVE".to_string());
        let p2_active_meta = create_pokemon_meta("P2 Active", Stage::Basic, vec![Type::Colorless], false, false, 1);
        card_meta_map.insert(p2_active_def_id.clone(), p2_active_meta.clone());
        let p2_active_card = CardInstance::new(p2_active_def_id, PlayerId::P2);
        let defender_id = p2_active_card.id;

        for i in 0..10 {
            let def_id = CardDefId::new(format!("P1-DECK-{i:03}"));
            card_meta_map.insert(def_id.clone(), create_pokemon_meta("Deck P1", Stage::Basic, vec![Type::Colorless], false, false, 1));
            deck1.push(CardInstance::new(def_id, PlayerId::P1));
        }
        for i in 0..10 {
            let def_id = CardDefId::new(format!("P2-DECK-{i:03}"));
            card_meta_map.insert(def_id.clone(), create_pokemon_meta("Deck P2", Stage::Basic, vec![Type::Colorless], false, false, 1));
            deck2.push(CardInstance::new(def_id, PlayerId::P2));
        }

        let mut game = GameState::new_with_card_meta(
            deck1,
            deck2,
            12345,
            RulesetConfig::default(),
            card_meta_map,
        );

        let mut attacker_slot = PokemonSlot::new(active_card);
        apply_meta(&mut attacker_slot, &active_meta);
        let mut defender_slot = PokemonSlot::new(p2_active_card);
        apply_meta(&mut defender_slot, &p2_active_meta);

        game.players[0].active = Some(attacker_slot);
        game.players[1].active = Some(defender_slot);
        game.turn.player = PlayerId::P1;

        (game, attacker_id, defender_id)
    }

    fn find_slot_mut(game: &mut GameState, card_id: CardInstanceId) -> Option<&mut PokemonSlot> {
        // Use index-based lookup to avoid borrow checker issues
        enum Location { P1Active, P1Bench(usize), P2Active, P2Bench(usize) }
        let loc = {
            if let Some(slot) = game.players[0].active.as_ref() {
                if slot.card.id == card_id { Some(Location::P1Active) } else { None }
            } else { None }
        }.or_else(|| {
            game.players[0].bench.iter().position(|s| s.card.id == card_id).map(Location::P1Bench)
        }).or_else(|| {
            if let Some(slot) = game.players[1].active.as_ref() {
                if slot.card.id == card_id { Some(Location::P2Active) } else { None }
            } else { None }
        }).or_else(|| {
            game.players[1].bench.iter().position(|s| s.card.id == card_id).map(Location::P2Bench)
        });
        match loc {
            Some(Location::P1Active) => game.players[0].active.as_mut(),
            Some(Location::P1Bench(i)) => game.players[0].bench.get_mut(i),
            Some(Location::P2Active) => game.players[1].active.as_mut(),
            Some(Location::P2Bench(i)) => game.players[1].bench.get_mut(i),
            None => None,
        }
    }

    fn find_slot(game: &GameState, card_id: CardInstanceId) -> Option<&PokemonSlot> {
        if let Some(slot) = game.players[0].active.as_ref() {
            if slot.card.id == card_id {
                return Some(slot);
            }
        }
        for slot in &game.players[0].bench {
            if slot.card.id == card_id {
                return Some(slot);
            }
        }
        if let Some(slot) = game.players[1].active.as_ref() {
            if slot.card.id == card_id {
                return Some(slot);
            }
        }
        for slot in &game.players[1].bench {
            if slot.card.id == card_id {
                return Some(slot);
            }
        }
        None
    }

    fn find_def_id(game: &GameState, card_id: CardInstanceId) -> Option<CardDefId> {
        find_slot(game, card_id).map(|slot| slot.card.def_id.clone())
    }

    fn set_active_meta(game: &mut GameState, player: PlayerId, meta: CardMeta) -> CardInstanceId {
        let idx = match player { PlayerId::P1 => 0, PlayerId::P2 => 1 };
        let slot = game.players[idx].active.as_mut().expect("active slot missing");
        let def_id = slot.card.def_id.clone();
        game.card_meta.insert(def_id, meta.clone());
        apply_meta(slot, &meta);
        slot.card.id
    }

    fn add_bench_pokemon(
        game: &mut GameState,
        player: PlayerId,
        def_id: &str,
        meta: CardMeta,
    ) -> CardInstanceId {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), meta.clone());
        let card = CardInstance::new(card_def_id, player);
        let mut slot = PokemonSlot::new(card.clone());
        apply_meta(&mut slot, &meta);
        let idx = match player { PlayerId::P1 => 0, PlayerId::P2 => 1 };
        game.players[idx].bench.push(slot);
        card.id
    }

    fn add_to_hand(game: &mut GameState, player: PlayerId, def_id: &str, meta: CardMeta) -> CardInstanceId {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), meta);
        let card = CardInstance::new(card_def_id, player);
        let idx = match player { PlayerId::P1 => 0, PlayerId::P2 => 1 };
        game.players[idx].hand.add(card.clone());
        card.id
    }

    fn add_to_discard(game: &mut GameState, player: PlayerId, def_id: &str, meta: CardMeta) -> CardInstanceId {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), meta);
        let card = CardInstance::new(card_def_id, player);
        let idx = match player { PlayerId::P1 => 0, PlayerId::P2 => 1 };
        game.players[idx].discard.add(card.clone());
        card.id
    }

    fn add_to_deck(game: &mut GameState, player: PlayerId, def_id: &str, meta: CardMeta) -> CardInstanceId {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), meta);
        let card = CardInstance::new(card_def_id, player);
        let idx = match player { PlayerId::P1 => 0, PlayerId::P2 => 1 };
        game.players[idx].deck.add(card.clone());
        card.id
    }

    fn add_to_prizes(game: &mut GameState, player: PlayerId, def_id: &str, meta: CardMeta) -> CardInstanceId {
        let card_def_id = CardDefId::new(def_id.to_string());
        game.card_meta.insert(card_def_id.clone(), meta);
        let card = CardInstance::new(card_def_id, player);
        let idx = match player { PlayerId::P1 => 0, PlayerId::P2 => 1 };
        game.players[idx].prizes.add(card.clone());
        card.id
    }

    fn set_is_delta(game: &mut GameState, card_id: CardInstanceId, is_delta: bool) {
        if let Some(slot) = find_slot_mut(game, card_id) {
            slot.is_delta = is_delta;
        }
        if let Some(def_id) = find_def_id(game, card_id) {
            if let Some(meta) = game.card_meta.get_mut(&def_id) {
                meta.is_delta = is_delta;
            }
        }
    }

    fn set_is_ex(game: &mut GameState, card_id: CardInstanceId, is_ex: bool) {
        if let Some(slot) = find_slot_mut(game, card_id) {
            slot.is_ex = is_ex;
        }
        if let Some(def_id) = find_def_id(game, card_id) {
            if let Some(meta) = game.card_meta.get_mut(&def_id) {
                meta.is_ex = is_ex;
            }
        }
    }

    fn set_stage(game: &mut GameState, card_id: CardInstanceId, stage: Stage) {
        if let Some(slot) = find_slot_mut(game, card_id) {
            slot.stage = stage;
        }
        if let Some(def_id) = find_def_id(game, card_id) {
            if let Some(meta) = game.card_meta.get_mut(&def_id) {
                meta.stage = stage;
                meta.is_basic = stage == Stage::Basic;
            }
        }
    }

    fn set_types(game: &mut GameState, card_id: CardInstanceId, types: Vec<Type>) {
        if let Some(slot) = find_slot_mut(game, card_id) {
            slot.types = types.clone();
        }
        if let Some(def_id) = find_def_id(game, card_id) {
            if let Some(meta) = game.card_meta.get_mut(&def_id) {
                meta.types = types;
            }
        }
    }

    fn set_retreat_cost(game: &mut GameState, card_id: CardInstanceId, retreat_cost: u8) {
        if let Some(slot) = find_slot_mut(game, card_id) {
            slot.retreat_cost = retreat_cost;
        }
        if let Some(def_id) = find_def_id(game, card_id) {
            if let Some(meta) = game.card_meta.get_mut(&def_id) {
                meta.retreat_cost = Some(retreat_cost);
            }
        }
    }

    fn set_card_name(game: &mut GameState, card_id: CardInstanceId, name: &str) {
        if let Some(def_id) = find_def_id(game, card_id) {
            if let Some(meta) = game.card_meta.get_mut(&def_id) {
                meta.name = name.to_string();
            }
        }
    }

    fn set_damage_counters(game: &mut GameState, card_id: CardInstanceId, counters: u16) {
        if let Some(slot) = find_slot_mut(game, card_id) {
            slot.damage_counters = counters;
        }
    }

    fn add_special_condition(game: &mut GameState, card_id: CardInstanceId, condition: SpecialCondition) {
        if let Some(slot) = find_slot_mut(game, card_id) {
            slot.add_special_condition(condition);
        }
    }

    fn attach_energy(game: &mut GameState, card_id: CardInstanceId, energy_type: Type, count: usize) -> Vec<CardInstanceId> {
        let owner = find_slot(game, card_id).map(|slot| slot.card.owner).unwrap_or(PlayerId::P1);
        let mut ids = Vec::new();
        for i in 0..count {
            let def_id = CardDefId::new(format!("ENERGY-{:?}-{i}", energy_type));
            game.card_meta.insert(def_id.clone(), create_energy_meta("Energy", energy_type));
            let card = CardInstance::new(def_id, owner);
            if let Some(slot) = find_slot_mut(game, card_id) {
                slot.attached_energy.push(card.clone());
            }
            ids.push(card.id);
        }
        ids
    }

    fn set_attached_tool(game: &mut GameState, card_id: CardInstanceId, tool_name: &str) -> CardInstanceId {
        let owner = find_slot(game, card_id).map(|slot| slot.card.owner).unwrap_or(PlayerId::P1);
        let def_id = CardDefId::new(format!("TOOL-{tool_name}"));
        game.card_meta.insert(def_id.clone(), create_trainer_meta(tool_name, Some("Tool"), true));
        let card = CardInstance::new(def_id, owner);
        if let Some(slot) = find_slot_mut(game, card_id) {
            slot.attached_tool = Some(card.clone());
        }
        card.id
    }

    
    #[test]
    fn eval_pump_out_damage_bonus() {
        assert_eq!(pump_out_damage(false), 50);
        assert_eq!(pump_out_damage(true), 70);
    }

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
            hp: 90,
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
            retreat_cost: Some(2),
            trainer_effect: None,
            evolves_from: None,
            attacks: vec![],
        }
    }

    fn create_trainer_meta(name: &str, trainer_kind: &str, is_tool: bool) -> CardMeta {
        CardMeta {
            card_type: String::new(),
            delta_species: false,
            name: name.to_string(),
            is_basic: false,
            is_tool,
            is_stadium: false,
            is_pokemon: false,
            is_energy: false,
            hp: 0,
            energy_kind: None,
            provides: Vec::new(),
            trainer_kind: Some(trainer_kind.to_string()),
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
            1111,
            RulesetConfig::default(),
            card_meta,
        );

        let p1_def = CardDefId::new("P1-ACTIVE".to_string());
        game.card_meta.insert(p1_def.clone(), create_pokemon_meta("Quagsire δ"));
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
    fn eval_dig_up_prompts_for_tools_only() {
        let (mut game, source_id) = setup_game();
        let tool_a = add_to_discard(&mut game, "P1-TOOL-A", create_trainer_meta("Tool A", "Tool", true));
        let tool_b = add_to_discard(&mut game, "P1-TOOL-B", create_trainer_meta("Tool B", "Tool", true));
        let item = add_to_discard(&mut game, "P1-ITEM", create_trainer_meta("Item", "Item", false));

        assert!(crate::df::runtime::execute_power(&mut game, "Dig Up", source_id));
        match game.pending_prompt.as_ref().map(|pending| &pending.prompt) {
            Some(Prompt::ChooseCardsFromDiscard { count, min, max, destination, options, .. }) => {
                assert_eq!(*count, 2);
                assert_eq!(*min, Some(0));
                assert_eq!(*max, Some(2));
                assert_eq!(*destination, SelectionDestination::Hand);
                assert!(options.contains(&tool_a));
                assert!(options.contains(&tool_b));
                assert!(!options.contains(&item));
            }
            other => panic!("unexpected dig up prompt: {other:?}"),
        }
    }
}
