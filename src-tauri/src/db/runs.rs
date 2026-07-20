use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfluxBuffDelta {
    pub room_index: u32,
    pub buff_ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfluxRoom {
    pub log_id: i64,
    pub room_index: u32,
    pub quest_id: Option<u32>,
    pub primary_target: Option<u32>,
    pub duration: i64,
    pub total_damage: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfluxRun {
    pub id: i64,
    pub start_time: i64,
    pub end_time: Option<i64>,
    pub duration: Option<i64>,
    pub room_count: u32,
    pub completed: Option<bool>,
    pub buffs: Vec<ConfluxBuffDelta>,
    pub rooms: Vec<ConfluxRoom>,
}

pub fn insert_run(conn: &Connection, start_time: i64) -> Result<i64> {
    conn.execute(
        "INSERT INTO runs (start_time, room_count, buffs) VALUES (?, 0, '[]')",
        params![start_time],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn finalize_run(
    conn: &Connection,
    run_id: i64,
    end_time: i64,
    room_count: u32,
    completed: bool,
    buffs: &[ConfluxBuffDelta],
) -> Result<()> {
    let buffs = serde_json::to_string(buffs)?;
    conn.execute(
        "UPDATE runs SET end_time = ?, duration = MAX(? - start_time, 1), room_count = ?, completed = ?, buffs = ? WHERE id = ?",
        params![end_time, end_time, room_count, completed, buffs, run_id],
    )?;
    Ok(())
}

pub fn sweep_orphaned_runs(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM runs WHERE end_time IS NULL AND NOT EXISTS (SELECT 1 FROM logs WHERE logs.run_id = runs.id)",
        [],
    )?;
    conn.execute(
        "UPDATE runs SET end_time = (SELECT MAX(time + duration) FROM logs WHERE logs.run_id = runs.id), duration = MAX((SELECT MAX(time + duration) FROM logs WHERE logs.run_id = runs.id) - start_time, 1), room_count = (SELECT COUNT(*) FROM logs WHERE logs.run_id = runs.id) WHERE end_time IS NULL",
        [],
    )?;
    Ok(())
}

pub fn delete_runs_without_rooms(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM runs WHERE NOT EXISTS (SELECT 1 FROM logs WHERE logs.run_id = runs.id)",
        [],
    )?;
    Ok(())
}

pub fn get_runs(conn: &Connection, per_page: u32, offset: u32) -> Result<Vec<ConfluxRun>> {
    let mut stmt = conn.prepare(
        "SELECT id, start_time, end_time, duration, room_count, completed, buffs FROM runs WHERE EXISTS (SELECT 1 FROM logs WHERE logs.run_id = runs.id) ORDER BY start_time DESC LIMIT ? OFFSET ?",
    )?;
    let rows = stmt
        .query_map(params![per_page, offset], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, u32>(4)?,
                row.get::<_, Option<bool>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    rows.into_iter()
        .map(
            |(id, start_time, end_time, duration, room_count, completed, buffs)| {
                Ok(ConfluxRun {
                    id,
                    start_time,
                    end_time,
                    duration,
                    room_count,
                    completed,
                    buffs: buffs
                        .as_deref()
                        .and_then(|value| serde_json::from_str(value).ok())
                        .unwrap_or_default(),
                    rooms: get_rooms_for_run(conn, id)?,
                })
            },
        )
        .collect()
}

fn get_rooms_for_run(conn: &Connection, run_id: i64) -> Result<Vec<ConfluxRoom>> {
    let mut stmt = conn.prepare(
        "SELECT id, room_index, quest_id, primary_target, duration, COALESCE(total_damage, 0) FROM logs WHERE run_id = ? ORDER BY room_index",
    )?;
    let rooms = stmt
        .query_map(params![run_id], |row| {
            Ok(ConfluxRoom {
                log_id: row.get(0)?,
                room_index: row.get::<_, Option<u32>>(1)?.unwrap_or(0),
                quest_id: row.get(2)?,
                primary_target: row.get(3)?,
                duration: row.get(4)?,
                total_damage: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rooms)
}

pub fn get_runs_count(conn: &Connection) -> Result<i32> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM runs WHERE EXISTS (SELECT 1 FROM logs WHERE logs.run_id = runs.id)",
        [],
        |row| row.get(0),
    )?)
}
