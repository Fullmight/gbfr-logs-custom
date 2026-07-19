use std::ptr::NonNull;

use anyhow::{anyhow, Result};
use protocol::{ActionType, Actor, DamageEvent, Message};
use retour::static_detour;

use crate::{event, hooks::ffi::DamageInstance, process::Process};

use super::{actor_idx, actor_type_id, get_source_parent_instance, read_process_value};

type ProcessDamageEventFunc =
    unsafe extern "system" fn(*const usize, *const usize, *const usize, u8) -> usize;

type ProcessDotEventFunc = unsafe extern "system" fn(*const usize, *const usize) -> usize;

static_detour! {
    static ProcessDamageEvent: unsafe extern "system" fn(*const usize, *const usize, *const usize, u8) -> usize;
    static ProcessPoisonEvent: unsafe extern "system" fn(*const usize, *const usize) -> usize;
    static ProcessBurnEvent: unsafe extern "system" fn(*const usize, *const usize) -> usize;
}

#[derive(Clone)]
pub struct OnProcessDamageHook {
    tx: event::Tx,
}

const PROCESS_DAMAGE_EVENT_SIG: &str = "e8 $ { ' } 66 83 bc 24 ? ? ? ? ?";
const PROCESS_POISON_EVENT_SIG: &str =
    "41 57 41 56 41 55 41 54 56 57 55 53 48 83 ec 78 [1000-1600] c5 fa 2c 87 b4 00 00 00";
const PROCESS_BURN_EVENT_SIG: &str =
    "41 57 41 56 41 55 41 54 56 57 55 53 48 83 ec 78 [800-1400] c5 fa 2c 87 b0 00 00 00 [0-200] c7 46 04 01 00 00 00";
const APPLIED_STUN_VALUE_OFFSET: usize = 0xB90;
const DOT_DAMAGE_RESULT_OFFSET: usize = 0x10;

#[inline(always)]
fn read_applied_stun_value(target: *const usize) -> Option<f32> {
    read_process_value::<f32>(target.wrapping_byte_add(APPLIED_STUN_VALUE_OFFSET).cast())
        .filter(|value| value.is_finite())
}

#[inline(always)]
fn applied_stun_delta(before: Option<f32>, after: Option<f32>) -> Option<f32> {
    match (before, after) {
        (Some(before), Some(after)) if before.is_finite() && after.is_finite() => {
            Some((after - before).max(0.0))
        }
        _ => None,
    }
}

#[inline(always)]
fn stun_value_for_event(
    action_type: &ActionType,
    before: Option<f32>,
    after: Option<f32>,
) -> Option<f32> {
    if matches!(action_type, ActionType::SupplementaryDamage(_)) {
        None
    } else {
        applied_stun_delta(before, after)
    }
}

#[inline(always)]
#[cfg(test)]
fn resolve_source_parent<F>(
    source_type_id: u32,
    source_idx: u32,
    source: *const usize,
    resolver: F,
) -> (u32, u32)
where
    F: FnOnce(u32, *const usize) -> Option<(u32, u32)>,
{
    resolver(source_type_id, source).unwrap_or((source_type_id, source_idx))
}

impl OnProcessDamageHook {
    pub fn new(tx: event::Tx) -> Self {
        OnProcessDamageHook { tx }
    }

    pub fn setup(&self, process: &Process) -> Result<()> {
        let cloned_self = self.clone();

        if let Ok(process_dmg_evt) = process.search_address(PROCESS_DAMAGE_EVENT_SIG) {
            #[cfg(feature = "console")]
            println!("Found process dmg event");

            unsafe {
                let func: ProcessDamageEventFunc = std::mem::transmute(process_dmg_evt);

                ProcessDamageEvent
                    .initialize(func, move |a1, a2, a3, a4| cloned_self.run(a1, a2, a3, a4))?;

                ProcessDamageEvent.enable()?;
            }
        } else {
            return Err(anyhow!("Could not find process_dmg_evt"));
        }

        Ok(())
    }

    fn run(&self, a1: *const usize, a2: *const usize, a3: *const usize, a4: u8) -> usize {
        // Target is the instance of the actor being damaged.
        // For example: Instance of the Em2700 class.
        let target_specified_instance_ptr: usize = unsafe { *(*a1.byte_add(0x08) as *const usize) };
        let previous_stun_value =
            read_applied_stun_value(target_specified_instance_ptr as *const usize);

        super::damage_details::begin_damage(a2);

        let original_value = unsafe { ProcessDamageEvent.call(a1, a2, a3, a4) };
        let current_stun_value =
            read_applied_stun_value(target_specified_instance_ptr as *const usize);

        let damage_details = super::damage_details::finish_damage(a2, original_value);

        // This points to the first Entity instance in the 'a2' entity list.
        let source_entity_ptr = unsafe { (a2.byte_add(0x18) as *const *const usize).read() };

        // @TODO(false): For some reason, online + Ferry's Umlauf skill pet can return a null pointer here.
        // Possible data race with online?
        if source_entity_ptr.is_null() {
            return original_value;
        }

        // entity->m_pSpecifiedInstance, offset 0x70 from entity pointer.
        // Returns the specific class instance of the source entity. (e.g. Instance of Pl1200 / Pl0700Ghost)
        let source_specified_instance_ptr: usize = unsafe { *(source_entity_ptr.byte_add(0x70)) };

        let damage_instance = unsafe { NonNull::new(a2 as *mut DamageInstance).unwrap().as_ref() };
        let damage: i32 = damage_instance.damage;

        if original_value == 0 || damage <= 0 {
            return original_value;
        }

        let flags: u64 = damage_instance.flags;

        let action_type: ActionType = if ((1 << 7 | 1 << 50) & flags) != 0 {
            ActionType::LinkAttack
        } else if ((1 << 13 | 1 << 14) & flags) != 0 {
            ActionType::SBA
        } else if ((1 << 15) & flags) != 0 {
            ActionType::SupplementaryDamage(damage_instance.action_id)
        } else {
            ActionType::Normal(damage_instance.action_id)
        };

        // Get the source actor's type ID.
        let source_type_id = actor_type_id(source_specified_instance_ptr as *const usize);
        let source_idx = actor_idx(source_specified_instance_ptr as *const usize);

        // Resolve known child actors (Ferry's pets, Id's dragon form, summons,
        // etc.) back to their concrete owning player instance. The exact
        // parent actor is also where game 2.0.2 stores the authoritative party
        // identity snapshot used for nickname assignment.
        let (source_parent_type_id, source_parent_idx, identity_actor) =
            if let Some((parent_type, parent_actor)) = get_source_parent_instance(
                source_type_id,
                source_specified_instance_ptr as *const usize,
            ) {
                (parent_type, actor_idx(parent_actor), parent_actor)
            } else {
                (
                    source_type_id,
                    source_idx,
                    source_specified_instance_ptr as *const usize,
                )
            };

        let identities = super::player::identity_events_for_actor(
            identity_actor,
            source_parent_type_id,
            source_parent_idx,
        );
        for identity in identities {
            let _ = self.tx.send(Message::PlayerIdentityEvent(identity));
        }

        let target_type_id: u32 = actor_type_id(target_specified_instance_ptr as *const usize);
        let target_idx = actor_idx(target_specified_instance_ptr as *const usize);
        let stun_value =
            stun_value_for_event(&action_type, previous_stun_value, current_stun_value);

        let event = Message::DamageEvent(DamageEvent {
            source: Actor {
                index: source_idx,
                actor_type: source_type_id,
                parent_index: source_parent_idx,
                parent_actor_type: source_parent_type_id,
            },
            target: Actor {
                index: target_idx,
                actor_type: target_type_id,
                parent_index: target_idx,
                parent_actor_type: target_type_id,
            },
            damage,
            flags,
            action_id: action_type,
            attack_rate: None,
            damage_cap: Some(damage_instance.damage_cap),
            stun_value,
            details: damage_details,
        });

        let _ = self.tx.send(event);

        original_value
    }
}

#[derive(Clone)]
pub struct OnProcessDotHook {
    tx: event::Tx,
}

impl OnProcessDotHook {
    pub fn new(tx: event::Tx) -> Self {
        OnProcessDotHook { tx }
    }

    pub fn setup(&self, process: &Process) -> Result<()> {
        let process_poison_event = process.search_match_address(PROCESS_POISON_EVENT_SIG)?;
        let process_burn_event = process.search_match_address(PROCESS_BURN_EVENT_SIG)?;
        let poison_hook = self.clone();
        let burn_hook = self.clone();

        unsafe {
            let poison_func: ProcessDotEventFunc = std::mem::transmute(process_poison_event);
            ProcessPoisonEvent.initialize(poison_func, move |dot, result| {
                poison_hook.run_poison(dot, result)
            })?;

            let burn_func: ProcessDotEventFunc = std::mem::transmute(process_burn_event);
            ProcessBurnEvent.initialize(burn_func, move |dot, result| {
                burn_hook.run_burn(dot, result)
            })?;

            ProcessPoisonEvent.enable()?;
            ProcessBurnEvent.enable()?;
        }

        Ok(())
    }

    // A1: DoT Instance (StatusPl2300ParalysisArrow)
    // *A1+0x00 -> StatusAilmentPoison : StatusBase
    // A1+0x18->targetEntityInfo : CEntityInfo (Target entity of the DoT, what is being damaged)
    // A1+0x30->sourceEntityInfo : CEntityInfo (Source entity of the DoT, who applied it)
    // A1+0x50->duration : float (How much time is left for the DoT)
    fn run_poison(&self, dot_instance: *const usize, result: *const usize) -> usize {
        let original_value = unsafe { ProcessPoisonEvent.call(dot_instance, result) };
        self.emit_dot_event(dot_instance, result);
        original_value
    }

    fn run_burn(&self, dot_instance: *const usize, result: *const usize) -> usize {
        let original_value = unsafe { ProcessBurnEvent.call(dot_instance, result) };
        self.emit_dot_event(dot_instance, result);
        original_value
    }

    fn emit_dot_event(&self, dot_instance: *const usize, result: *const usize) {
        let Some(damage) = dot_damage_from_result(result) else {
            return;
        };
        let Some(target_info) =
            read_process_value::<*const usize>(dot_instance.wrapping_byte_add(0x18).cast())
        else {
            return;
        };
        let Some(source_info) =
            read_process_value::<*const usize>(dot_instance.wrapping_byte_add(0x30).cast())
        else {
            return;
        };
        let Some(target) =
            read_process_value::<*const usize>(target_info.wrapping_byte_add(0x70).cast())
        else {
            return;
        };
        let Some(source) =
            read_process_value::<*const usize>(source_info.wrapping_byte_add(0x70).cast())
        else {
            return;
        };

        if target.is_null() || source.is_null() {
            return;
        }

        let source_idx = actor_idx(source);
        let source_type_id = actor_type_id(source);

        let (source_parent_type_id, source_parent_idx, identity_actor) =
            if let Some((parent_type, parent_actor)) =
                get_source_parent_instance(source_type_id, source)
            {
                (parent_type, actor_idx(parent_actor), parent_actor)
            } else {
                (source_type_id, source_idx, source)
            };

        let identities = super::player::identity_events_for_actor(
            identity_actor,
            source_parent_type_id,
            source_parent_idx,
        );
        for identity in identities {
            let _ = self.tx.send(Message::PlayerIdentityEvent(identity));
        }

        let target_idx = actor_idx(target);
        let target_type_id = actor_type_id(target);

        let event = Message::DamageEvent(dot_damage_event(
            Actor {
                index: source_idx,
                actor_type: source_type_id,
                parent_index: source_parent_idx,
                parent_actor_type: source_parent_type_id,
            },
            Actor {
                index: target_idx,
                actor_type: target_type_id,
                parent_index: target_idx,
                parent_actor_type: target_type_id,
            },
            damage,
        ));

        let _ = self.tx.send(event);
    }
}

#[inline(always)]
fn dot_damage_from_result(result: *const usize) -> Option<i32> {
    let damage =
        read_process_value::<f32>(result.wrapping_byte_add(DOT_DAMAGE_RESULT_OFFSET).cast())?;
    (damage.is_finite() && damage > 0.0).then_some(damage as i32)
}

fn dot_damage_event(source: Actor, target: Actor, damage: i32) -> DamageEvent {
    DamageEvent {
        source,
        target,
        damage,
        flags: 0,
        action_id: ActionType::DamageOverTime(0),
        attack_rate: None,
        stun_value: None,
        damage_cap: None,
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        applied_stun_delta, dot_damage_event, dot_damage_from_result, resolve_source_parent,
        stun_value_for_event,
    };
    use protocol::{ActionType, Actor};

    const FERRY_GHOST_TYPE: u32 = 0x2AF678E8;
    const FERRY_TYPE: u32 = 0x4C714F77;

    #[test]
    fn ferry_pet_uses_the_resolved_owner() {
        let source = 1usize as *const usize;
        let mut requested_type = None;

        let parent = resolve_source_parent(FERRY_GHOST_TYPE, 41, source, |actor_type, ptr| {
            requested_type = Some(actor_type);
            assert_eq!(ptr, source);
            Some((FERRY_TYPE, 7))
        });

        assert_eq!(requested_type, Some(FERRY_GHOST_TYPE));
        assert_eq!(parent, (FERRY_TYPE, 7));
    }

    #[test]
    fn unresolved_actor_keeps_its_concrete_identity() {
        let parent = resolve_source_parent(0xDEADBEEF, 41, std::ptr::null(), |_, _| None);

        assert_eq!(parent, (0xDEADBEEF, 41));
    }

    #[test]
    fn applied_stun_uses_the_positive_cumulative_delta() {
        let delta = applied_stun_delta(Some(23.03), Some(39.48)).unwrap();
        assert!((delta - 16.45).abs() < 0.001);
    }

    #[test]
    fn applied_stun_does_not_go_negative_when_the_counter_resets() {
        assert_eq!(applied_stun_delta(Some(131.6), Some(0.0)), Some(0.0));
    }

    #[test]
    fn applied_stun_requires_two_finite_reads() {
        assert_eq!(applied_stun_delta(None, Some(10.0)), None);
        assert_eq!(applied_stun_delta(Some(10.0), None), None);
        assert_eq!(applied_stun_delta(Some(f32::NAN), Some(10.0)), None);
        assert_eq!(applied_stun_delta(Some(10.0), Some(f32::INFINITY)), None);
    }

    #[test]
    fn supplementary_damage_never_claims_stun() {
        assert_eq!(
            stun_value_for_event(
                &ActionType::SupplementaryDamage(101),
                Some(23.03),
                Some(39.48),
            ),
            None
        );
    }

    #[test]
    fn reads_game_2_dot_damage_from_the_result_payload() {
        let mut result = [0u8; 0x18];
        unsafe {
            result
                .as_mut_ptr()
                .byte_add(0x10)
                .cast::<f32>()
                .write_unaligned(1_234.75);
        }

        assert_eq!(dot_damage_from_result(result.as_ptr().cast()), Some(1_234));
    }

    #[test]
    fn ignores_non_positive_or_invalid_dot_results() {
        for damage in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let mut result = [0u8; 0x18];
            unsafe {
                result
                    .as_mut_ptr()
                    .byte_add(0x10)
                    .cast::<f32>()
                    .write_unaligned(damage);
            }
            assert_eq!(dot_damage_from_result(result.as_ptr().cast()), None);
        }
    }

    #[test]
    fn dot_results_become_damage_over_time_events() {
        let source = Actor {
            index: 1,
            actor_type: 2,
            parent_index: 3,
            parent_actor_type: 4,
        };
        let target = Actor {
            index: 5,
            actor_type: 6,
            parent_index: 5,
            parent_actor_type: 6,
        };

        let event = dot_damage_event(source.clone(), target.clone(), 777);

        assert_eq!(event.source.index, source.index);
        assert_eq!(event.source.parent_actor_type, source.parent_actor_type);
        assert_eq!(event.target.index, target.index);
        assert_eq!(event.target.actor_type, target.actor_type);
        assert_eq!(event.damage, 777);
        assert_eq!(event.action_id, ActionType::DamageOverTime(0));
    }
}
