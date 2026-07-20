//! Read-only, pass-through hooks for the game 2.0.2 Endless Mode lifecycle.
//!
//! Every game-memory access uses `ReadProcessMemory` through
//! `read_process_value`; failed reads simply suppress an event. The detours do
//! not write process memory (apart from retour's installation), perform no
//! network I/O, and always call the original function exactly once.

use anyhow::{anyhow, Result};
use retour::static_detour;

use crate::{event, process::Process};

const ON_RECEPTION_FLOW_DISPATCH_SIG: &str =
    "c3 cc cc ' 55 56 57 53 48 83 ec 38 48 8d 6c 24 30 48 c7 45 00 fe ff ff ff 48 89 ce c1 ea 14";
const ON_ENDLESS_BUFF_INSTALL_SIG: &str =
    "c3 cc cc cc ' 56 57 48 83 ec 68 48 89 ce 48 8d 91 c0 00 00 00 48 8d 05";
const ON_ENDLESS_MGR_DTOR_SIG: &str =
    "cc cc cc cc ' 55 41 57 41 56 41 55 41 54 56 57 53 48 81 ec 78 04 00 00 48 8d ac 24 80 00 00 00 c5 f8 29 bd e0 03 00 00";

const ENDLESS_FLOW_TYPE: u32 = 0x887A_E0B0;
const FLOW_SLOT_OFFSET: usize = 0x210;
const FLOW_TYPE_OFFSET: usize = 0x7C8;
const QUEST_ID_OFFSET: usize = 0x1D8;
const ENDLESS_BUFF_FIRST_SLOT: usize = 0xC0;

type ReceptionFlowDispatchFunc = unsafe extern "system" fn(*const usize, u32) -> usize;
type EndlessBuffInstallFunc = unsafe extern "system" fn(*const usize) -> usize;
type EndlessManagerDestructorFunc = unsafe extern "system" fn(*const usize) -> usize;

static_detour! {
    static ReceptionFlowDispatch: unsafe extern "system" fn(*const usize, u32) -> usize;
    static EndlessBuffInstall: unsafe extern "system" fn(*const usize) -> usize;
    static EndlessManagerDestructor: unsafe extern "system" fn(*const usize) -> usize;
}

fn read_at<T: Copy>(base: *const usize, offset: usize) -> Option<T> {
    if base.is_null() {
        return None;
    }
    super::read_process_value(base.wrapping_byte_add(offset).cast())
}

fn flow_type(manager: *const usize) -> Option<u32> {
    let flow: *const usize = read_at(manager, FLOW_SLOT_OFFSET)?;
    read_at(flow, FLOW_TYPE_OFFSET)
}

#[derive(Clone)]
pub struct OnReceptionFlowDispatchHook {
    tx: event::Tx,
}

impl OnReceptionFlowDispatchHook {
    pub fn new(tx: event::Tx) -> Self {
        Self { tx }
    }

    pub fn setup(&self, process: &Process) -> Result<()> {
        let address = process
            .search_address(ON_RECEPTION_FLOW_DISPATCH_SIG)
            .map_err(|_| anyhow!("Could not find Endless Mode reception dispatcher"))?;
        let hook = self.clone();
        unsafe {
            let function: ReceptionFlowDispatchFunc = std::mem::transmute(address);
            ReceptionFlowDispatch.initialize(function, move |manager, packed_type| {
                hook.run(manager, packed_type)
            })?;
            ReceptionFlowDispatch.enable()?;
        }
        Ok(())
    }

    fn run(&self, manager: *const usize, packed_type: u32) -> usize {
        let before = flow_type(manager);
        let result = unsafe { ReceptionFlowDispatch.call(manager, packed_type) };
        let after = flow_type(manager);

        if before != Some(ENDLESS_FLOW_TYPE) && after == Some(ENDLESS_FLOW_TYPE) {
            let quest_id = read_at(manager, QUEST_ID_OFFSET).unwrap_or(0);
            let _ = self.tx.send(protocol::Message::ConfluxRoomEnter(
                protocol::ConfluxRoomEnterEvent {
                    quest_id,
                    manager_ptr: manager as u64,
                },
            ));
        }
        result
    }
}

#[derive(Clone)]
pub struct OnEndlessBuffInstallHook {
    tx: event::Tx,
}

impl OnEndlessBuffInstallHook {
    pub fn new(tx: event::Tx) -> Self {
        Self { tx }
    }

    pub fn setup(&self, process: &Process) -> Result<()> {
        let address = process
            .search_address(ON_ENDLESS_BUFF_INSTALL_SIG)
            .map_err(|_| anyhow!("Could not find Endless Mode buff install"))?;
        let hook = self.clone();
        unsafe {
            let function: EndlessBuffInstallFunc = std::mem::transmute(address);
            EndlessBuffInstall.initialize(function, move |buff| hook.run(buff))?;
            EndlessBuffInstall.enable()?;
        }
        Ok(())
    }

    fn run(&self, buff: *const usize) -> usize {
        if let Some(buff_id) = read_at::<u32>(buff, ENDLESS_BUFF_FIRST_SLOT).filter(|id| *id != 0) {
            let _ = self.tx.send(protocol::Message::ConfluxBuffAcquired(
                protocol::ConfluxBuffAcquiredEvent { buff_id },
            ));
        }
        unsafe { EndlessBuffInstall.call(buff) }
    }
}

#[derive(Clone)]
pub struct OnEndlessMgrDtorHook {
    tx: event::Tx,
}

impl OnEndlessMgrDtorHook {
    pub fn new(tx: event::Tx) -> Self {
        Self { tx }
    }

    pub fn setup(&self, process: &Process) -> Result<()> {
        let address = process
            .search_address(ON_ENDLESS_MGR_DTOR_SIG)
            .map_err(|_| anyhow!("Could not find Endless Mode manager destructor"))?;
        let hook = self.clone();
        unsafe {
            let function: EndlessManagerDestructorFunc = std::mem::transmute(address);
            EndlessManagerDestructor.initialize(function, move |manager| hook.run(manager))?;
            EndlessManagerDestructor.enable()?;
        }
        Ok(())
    }

    fn run(&self, manager: *const usize) -> usize {
        let _ = self.tx.send(protocol::Message::ConfluxRunEnd(
            protocol::ConfluxRunEndEvent {
                manager_ptr: manager as u64,
            },
        ));
        unsafe { EndlessManagerDestructor.call(manager) }
    }
}
