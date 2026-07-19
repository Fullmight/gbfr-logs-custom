use std::collections::HashMap;

use protocol::{ActionType, DamageEvent, DamageModifierKind};
use serde::{Deserialize, Serialize};

use crate::parser::constants::{CharacterType, FerrySkillId};

use super::{skill_state::SkillState, AdjustedDamageInstance};

/// Damage attributed to a detected buff. This is informational and is never
/// added to encounter, player, or skill damage totals.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuffContributionState {
    pub status_name: String,
    pub kind: DamageModifierKind,
    pub category: i32,
    pub active_hits: u32,
    pub contributed_damage: u64,
}

/// Derived stat breakdown for a player
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub index: u32,
    pub character_type: CharacterType,
    pub total_damage: u64,
    pub last_known_pet_skill: Option<ActionType>, // used for Ferry's skills that don't keep track of where they came from
    pub dps: f64,
    pub skill_breakdown: Vec<SkillState>,
    #[serde(default)]
    pub buff_breakdown: Vec<BuffContributionState>,
    pub sba: f64,
    pub total_stun_value: f64,
    pub stun_per_second: f64,
}

impl PlayerState {
    pub fn set_sba(&mut self, sba: f64) {
        self.sba = sba;
    }

    pub fn update_dps(&mut self, now: i64, start_time: i64) {
        self.dps = self.total_damage as f64 / ((now - start_time) as f64 / 1000.0);
        self.stun_per_second = self.total_stun_value / ((now - start_time) as f64 / 1000.0);
    }

    // @todo(false): maybe Ferry specific stuff can be removed/abstracted if some extra flags are found or the attribution is fixed
    pub fn get_action_from_ferry_damage_event(&mut self, event: &DamageEvent) -> ActionType {
        // Ferry needs special handling because the action_id that comes back for pet skills is usually wrong
        // e.g. if you strafe then dodge the action_id for further hits comes back as "dodge"
        let is_ferry_pet =
            CharacterType::Pl0700Ghost == CharacterType::from_hash(event.source.actor_type);
        let is_ferry_pet_skill = is_ferry_pet && (event.flags & (1 << 2) != 0); // pet skills for ferry always have this flag set
        let is_ferry_pet_normal =
            is_ferry_pet && !is_ferry_pet_skill && event.action_id != ActionType::LinkAttack;

        // Umlauf excluded since that uses a separate actor which works correctly
        if is_ferry_pet_skill
            && vec![
                FerrySkillId::BlausGespenst,
                FerrySkillId::Pendel,
                FerrySkillId::Strafe,
            ]
            .into_iter()
            .any(|skill_id| ActionType::Normal(skill_id as u32) == event.action_id)
        {
            self.last_known_pet_skill = Some(event.action_id);
        }

        const PET_NORMAL: ActionType = ActionType::Normal(FerrySkillId::PetNormal as u32);

        if is_ferry_pet_normal {
            // Note technically the pet portion of Onslaught will count as a Pet normal, but I think that's fine since
            // it does exactly as much as a pet normal. Could consider adding Onslaught (pet) as a separate category
            PET_NORMAL
        } else if is_ferry_pet_skill {
            match self.last_known_pet_skill {
                None => PET_NORMAL, // May be good to instead have a separate "pet skill" backup for this case
                Some(skill_id) => skill_id,
            }
        } else {
            event.action_id
        }
    }

    pub fn update_from_damage_event(&mut self, damage_instance: &AdjustedDamageInstance) {
        self.total_damage += damage_instance.event.damage as u64;
        self.total_stun_value += damage_instance.stun_damage;
        self.update_buff_contributions(damage_instance);

        let parent_character_type =
            CharacterType::from_hash(damage_instance.event.source.parent_actor_type);

        // @TODO(false): Collapse all skill IDs from Seofon's avatar into his own.
        let child_character_type = if parent_character_type == CharacterType::Pl2200 {
            parent_character_type
        } else {
            CharacterType::from_hash(damage_instance.event.source.actor_type)
        };

        // for ferry defer to special function to handle the weird way her pets work
        let action = if parent_character_type == CharacterType::Pl0700 {
            self.get_action_from_ferry_damage_event(damage_instance.event)
        } else {
            damage_instance.event.action_id
        };

        // If the skill is already being tracked, update it.
        for skill in self.skill_breakdown.iter_mut() {
            // Aggregate all supplementary damage events into the same skill instance.
            if matches!(
                skill.action_type,
                protocol::ActionType::SupplementaryDamage(_)
            ) && matches!(action, protocol::ActionType::SupplementaryDamage(_))
            {
                skill.update_from_damage_event(damage_instance);
                return;
            }

            // If the skill is already being tracked, update it.
            if skill.action_type == action && skill.child_character_type == child_character_type {
                skill.update_from_damage_event(damage_instance);
                return;
            }
        }

        // Otherwise, create a new skill and track it.
        let mut skill = SkillState::new(action, child_character_type);

        skill.update_from_damage_event(damage_instance);
        self.skill_breakdown.push(skill);
    }

    fn update_buff_contributions(&mut self, damage_instance: &AdjustedDamageInstance) {
        let Some(details) = &damage_instance.event.details else {
            return;
        };

        let mut attack_buffs: HashMap<(String, i32), f32> = HashMap::new();
        for status in &details.statuses {
            if status.kind == DamageModifierKind::Attack && status.value > 0.0 {
                *attack_buffs
                    .entry((status.status_name.clone(), status.category))
                    .or_default() += status.value;
            }
        }

        let total_buff_value: f32 = attack_buffs.values().sum();
        if total_buff_value <= 0.0 || details.formula_multiplier <= 0.0 {
            return;
        }

        let attack_without_buffs = (details.attack_multiplier - total_buff_value).max(0.0);
        let formula_without_buffs = (details.elemental_multiplier * details.amplify_multiplier
            + (details.defense_multiplier * attack_without_buffs - 1.0) / 2.0)
            * details.supplementary_multiplier;

        let fallback_ratio = ((details.formula_multiplier - formula_without_buffs)
            / details.formula_multiplier)
            .clamp(0.0, 1.0);
        let benefit_ratio = if details.uncapped_damage.is_finite()
            && details.uncapped_damage > 0.0
            && details.damage_cap > 0
        {
            let cap = details.damage_cap as f32;
            let buffed_damage = details.uncapped_damage.min(cap);
            let unbuffed_uncapped =
                details.uncapped_damage * formula_without_buffs / details.formula_multiplier;
            let unbuffed_damage = unbuffed_uncapped.max(0.0).min(cap);

            if buffed_damage > 0.0 {
                ((buffed_damage - unbuffed_damage) / buffed_damage).clamp(0.0, 1.0)
            } else {
                fallback_ratio
            }
        } else {
            fallback_ratio
        };

        let total_contribution = damage_instance.event.damage.max(0) as f32 * benefit_ratio;
        if total_contribution < 0.5 {
            return;
        }

        for ((status_name, category), value) in attack_buffs {
            let contributed_damage = (total_contribution * value / total_buff_value).round() as u64;
            if contributed_damage == 0 {
                continue;
            }

            if let Some(buff) = self.buff_breakdown.iter_mut().find(|buff| {
                buff.status_name == status_name
                    && buff.kind == DamageModifierKind::Attack
                    && buff.category == category
            }) {
                buff.active_hits += 1;
                buff.contributed_damage += contributed_damage;
            } else {
                self.buff_breakdown.push(BuffContributionState {
                    status_name,
                    kind: DamageModifierKind::Attack,
                    category,
                    active_hits: 1,
                    contributed_damage,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::parser::v1::{PlayerData, PlayerStats};

    use super::*;

    #[test]
    fn calculates_dps() {
        let mut player_state = PlayerState {
            index: 0,
            character_type: CharacterType::Pl0000,
            total_damage: 100,
            last_known_pet_skill: None,
            dps: 0.0,
            skill_breakdown: vec![],
            buff_breakdown: vec![],
            sba: 0.0,
            total_stun_value: 0.0,
            stun_per_second: 0.0,
        };

        player_state.update_dps(1000, 0);

        assert_eq!(player_state.dps, 100.0);
    }

    #[test]
    fn updates_from_damage_event() {
        let mut player_state = PlayerState {
            index: 0,
            character_type: CharacterType::Pl0000,
            total_damage: 0,
            last_known_pet_skill: None,
            dps: 0.0,
            skill_breakdown: vec![],
            buff_breakdown: vec![],
            sba: 0.0,
            total_stun_value: 0.0,
            stun_per_second: 0.0,
        };

        let damage_event = DamageEvent {
            source: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            target: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            action_id: ActionType::Normal(1),
            damage: 100,
            flags: 0,
            attack_rate: None,
            stun_value: None,
            damage_cap: None,
            details: None,
        };

        player_state.update_from_damage_event(&AdjustedDamageInstance::from_damage_event(
            &damage_event,
            None,
        ));

        assert_eq!(player_state.total_damage, 100);
        assert_eq!(player_state.skill_breakdown.len(), 1);
        assert_eq!(player_state.skill_breakdown[0].total_damage, 100);
    }

    #[test]
    fn same_skill_updates_from_multiple_damage_events() {
        let mut player_state = PlayerState {
            index: 0,
            character_type: CharacterType::Pl0000,
            total_damage: 0,
            last_known_pet_skill: None,
            dps: 0.0,
            skill_breakdown: vec![],
            buff_breakdown: vec![],
            sba: 0.0,
            total_stun_value: 0.0,
            stun_per_second: 0.0,
        };

        let damage_event = DamageEvent {
            source: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            target: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            action_id: ActionType::Normal(1),
            damage: 100,
            flags: 0,
            attack_rate: None,
            stun_value: None,
            damage_cap: None,
            details: None,
        };

        player_state.update_from_damage_event(&AdjustedDamageInstance::from_damage_event(
            &damage_event,
            None,
        ));
        player_state.update_from_damage_event(&AdjustedDamageInstance::from_damage_event(
            &damage_event,
            None,
        ));
        player_state.update_from_damage_event(&AdjustedDamageInstance::from_damage_event(
            &damage_event,
            None,
        ));

        assert_eq!(player_state.total_damage, 300);
        assert_eq!(player_state.skill_breakdown.len(), 1);
        assert_eq!(player_state.skill_breakdown[0].total_damage, 300);
    }

    #[test]
    fn new_skills_are_tracked_separately() {
        let mut player_state = PlayerState {
            index: 0,
            character_type: CharacterType::Pl0000,
            total_damage: 0,
            last_known_pet_skill: None,
            dps: 0.0,
            skill_breakdown: vec![],
            buff_breakdown: vec![],
            sba: 0.0,
            stun_per_second: 0.0,
            total_stun_value: 0.0,
        };

        let skill_one = DamageEvent {
            source: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            target: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            action_id: ActionType::Normal(1),
            damage: 100,
            flags: 0,
            attack_rate: None,
            stun_value: None,
            damage_cap: None,
            details: None,
        };

        let skill_two = DamageEvent {
            source: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            target: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            action_id: ActionType::Normal(2),
            damage: 100,
            flags: 0,
            attack_rate: None,
            stun_value: None,
            damage_cap: None,
            details: None,
        };

        player_state
            .update_from_damage_event(&AdjustedDamageInstance::from_damage_event(&skill_one, None));
        player_state
            .update_from_damage_event(&AdjustedDamageInstance::from_damage_event(&skill_two, None));
        player_state
            .update_from_damage_event(&AdjustedDamageInstance::from_damage_event(&skill_two, None));

        assert_eq!(player_state.total_damage, 300);
        assert_eq!(player_state.skill_breakdown.len(), 2);
        assert_eq!(player_state.skill_breakdown[0].total_damage, 100);
        assert_eq!(player_state.skill_breakdown[1].total_damage, 200);
    }

    #[test]
    fn skills_from_children_are_tracked_separately() {
        let mut player_state = PlayerState {
            index: 0,
            character_type: CharacterType::Pl0000,
            total_damage: 0,
            last_known_pet_skill: None,
            dps: 0.0,
            skill_breakdown: vec![],
            buff_breakdown: vec![],
            sba: 0.0,
            stun_per_second: 0.0,
            total_stun_value: 0.0,
        };

        let parent_skill = DamageEvent {
            source: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            target: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            action_id: ActionType::Normal(1),
            damage: 100,
            flags: 0,
            attack_rate: None,
            stun_value: None,
            damage_cap: None,
            details: None,
        };

        let child_skill = DamageEvent {
            source: protocol::Actor {
                index: 1,
                actor_type: 1,
                parent_actor_type: 0,
                parent_index: 0,
            },
            target: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            action_id: ActionType::Normal(1),
            damage: 100,
            flags: 0,
            attack_rate: None,
            stun_value: None,
            damage_cap: None,
            details: None,
        };

        player_state.update_from_damage_event(&AdjustedDamageInstance::from_damage_event(
            &parent_skill,
            None,
        ));
        player_state.update_from_damage_event(&AdjustedDamageInstance::from_damage_event(
            &child_skill,
            None,
        ));
        player_state.update_from_damage_event(&AdjustedDamageInstance::from_damage_event(
            &child_skill,
            None,
        ));

        assert_eq!(player_state.total_damage, 300);
        assert_eq!(player_state.skill_breakdown.len(), 2);
        assert_eq!(player_state.skill_breakdown[0].total_damage, 100);
        assert_eq!(player_state.skill_breakdown[1].total_damage, 200);
    }

    #[test]
    fn stun_is_tracked_with_player_stats() {
        let mut player_state = PlayerState {
            index: 0,
            character_type: CharacterType::Pl0000,
            total_damage: 0,
            last_known_pet_skill: None,
            dps: 0.0,
            skill_breakdown: vec![],
            buff_breakdown: vec![],
            sba: 0.0,
            total_stun_value: 0.0,
            stun_per_second: 0.0,
        };

        let damage_event = DamageEvent {
            source: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            target: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            action_id: ActionType::Normal(1),
            damage: 100,
            flags: 0,
            attack_rate: None,
            stun_value: Some(5.0),
            damage_cap: None,
            details: None,
        };

        let player_data = PlayerData {
            actor_index: 0,
            character_type: CharacterType::Pl0000,
            display_name: "Test".to_string(),
            character_name: "Test".to_string(),
            sigils: Vec::new(),
            is_online: false,
            weapon_info: None,
            overmastery_info: None,
            player_stats: Some(PlayerStats {
                level: 100,
                total_hp: 10000,
                total_attack: 1000,
                stun_power: 130.0,
                critical_rate: 100.0,
                total_power: 1000,
            }),
        };

        player_state.update_from_damage_event(&AdjustedDamageInstance::from_damage_event(
            &damage_event,
            Some(&player_data),
        ));

        assert_eq!(player_state.total_stun_value, 5.0);
    }

    #[test]
    fn stun_value_without_player_stats() {
        let mut player_state = PlayerState {
            index: 0,
            character_type: CharacterType::Pl0000,
            total_damage: 0,
            last_known_pet_skill: None,
            dps: 0.0,
            skill_breakdown: vec![],
            buff_breakdown: vec![],
            sba: 0.0,
            total_stun_value: 0.0,
            stun_per_second: 0.0,
        };

        let damage_event = DamageEvent {
            source: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            target: protocol::Actor {
                index: 0,
                actor_type: 0,
                parent_actor_type: 0,
                parent_index: 0,
            },
            action_id: ActionType::Normal(1),
            damage: 100,
            flags: 0,
            attack_rate: None,
            stun_value: Some(5.0),
            damage_cap: None,
            details: None,
        };

        player_state.update_from_damage_event(&AdjustedDamageInstance::from_damage_event(
            &damage_event,
            None,
        ));

        assert_eq!(player_state.total_stun_value, 5.0);
    }

    #[test]
    fn attack_buff_damage_is_attributed_without_changing_damage_totals() {
        let mut player_state = PlayerState {
            index: 0,
            character_type: CharacterType::Pl2900,
            total_damage: 0,
            last_known_pet_skill: None,
            dps: 0.0,
            skill_breakdown: vec![],
            buff_breakdown: vec![],
            sba: 0.0,
            total_stun_value: 0.0,
            stun_per_second: 0.0,
        };
        let damage_event = DamageEvent {
            source: protocol::Actor {
                index: 0,
                actor_type: 0x0A58FB4D,
                parent_actor_type: 0x0A58FB4D,
                parent_index: 0,
            },
            target: protocol::Actor {
                index: 1,
                actor_type: 1,
                parent_actor_type: 1,
                parent_index: 1,
            },
            action_id: ActionType::Normal(100),
            damage: 1_100,
            flags: 0,
            attack_rate: None,
            stun_value: None,
            damage_cap: Some(2_000),
            details: Some(protocol::DamageDetails {
                elemental_multiplier: 1.0,
                amplify_multiplier: 1.0,
                defense_multiplier: 1.0,
                attack_multiplier: 1.2,
                supplementary_multiplier: 1.0,
                formula_multiplier: 1.1,
                attack_rate: 1.0,
                uncapped_damage: 1_100.0,
                damage_cap: 2_000,
                damage_limit_multiplier: 1.0,
                statuses: vec![protocol::DamageStatusContribution {
                    status_name: "StatusAttackBuff".to_string(),
                    kind: DamageModifierKind::Attack,
                    category: 7,
                    value: 0.2,
                }],
            }),
        };

        player_state.update_from_damage_event(&AdjustedDamageInstance::from_damage_event(
            &damage_event,
            None,
        ));

        assert_eq!(player_state.total_damage, 1_100);
        assert_eq!(player_state.skill_breakdown[0].total_damage, 1_100);
        assert_eq!(player_state.buff_breakdown.len(), 1);
        assert_eq!(player_state.buff_breakdown[0].contributed_damage, 100);
        assert_eq!(player_state.buff_breakdown[0].active_hits, 1);
    }

    #[test]
    fn attack_buff_is_not_credited_when_the_hit_would_still_be_capped_without_it() {
        let mut player_state = PlayerState {
            index: 0,
            character_type: CharacterType::Pl2900,
            total_damage: 0,
            last_known_pet_skill: None,
            dps: 0.0,
            skill_breakdown: vec![],
            buff_breakdown: vec![],
            sba: 0.0,
            total_stun_value: 0.0,
            stun_per_second: 0.0,
        };
        let damage_event = DamageEvent {
            source: protocol::Actor {
                index: 0,
                actor_type: 0x0A58FB4D,
                parent_actor_type: 0x0A58FB4D,
                parent_index: 0,
            },
            target: protocol::Actor {
                index: 1,
                actor_type: 1,
                parent_actor_type: 1,
                parent_index: 1,
            },
            action_id: ActionType::Normal(100),
            damage: 900,
            flags: 0,
            attack_rate: None,
            stun_value: None,
            damage_cap: Some(900),
            details: Some(protocol::DamageDetails {
                elemental_multiplier: 1.0,
                amplify_multiplier: 1.0,
                defense_multiplier: 1.0,
                attack_multiplier: 1.2,
                supplementary_multiplier: 1.0,
                formula_multiplier: 1.1,
                attack_rate: 1.0,
                uncapped_damage: 1_100.0,
                damage_cap: 900,
                damage_limit_multiplier: 1.0,
                statuses: vec![protocol::DamageStatusContribution {
                    status_name: "StatusAttackBuff".to_string(),
                    kind: DamageModifierKind::Attack,
                    category: 7,
                    value: 0.2,
                }],
            }),
        };

        player_state.update_from_damage_event(&AdjustedDamageInstance::from_damage_event(
            &damage_event,
            None,
        ));

        assert_eq!(player_state.total_damage, 900);
        assert!(player_state.buff_breakdown.is_empty());
    }
}
