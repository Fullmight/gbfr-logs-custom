import type { ConfluxRun, ConfluxSearchResult } from "@/types";
import {
  epochToLocalTime,
  humanizeNumbers,
  millisecondsToElapsedFormat,
  translateEnemyTypeId,
  translateQuestId,
} from "@/utils";
import { Box, Collapse, Group, Pagination, Table, Text, UnstyledButton } from "@mantine/core";
import { CaretDown, CaretRight } from "@phosphor-icons/react";
import { invoke } from "@tauri-apps/api";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";

const EMPTY_RESULT: ConfluxSearchResult = { runs: [], page: 1, pageCount: 0, runCount: 0 };

export const ConfluxIndexPage = () => {
  const [result, setResult] = useState(EMPTY_RESULT);
  const [page, setPage] = useState(1);
  const load = useCallback(async () => {
    setResult(await invoke<ConfluxSearchResult>("fetch_conflux_runs", { page }));
  }, [page]);

  useEffect(() => {
    void load();
    const unlisten = listen("conflux-run-saved", () => void load());
    return () => void unlisten.then((dispose) => dispose());
  }, [load]);

  return (
    <Box>
      <Text mb="sm">{result.runCount === 1 ? "1 Conflux run" : `${result.runCount} Conflux runs`}</Text>
      {result.runs.length === 0 ? (
        <Text c="dimmed">No Conflux runs saved.</Text>
      ) : (
        <Table striped highlightOnHover>
          <Table.Thead>
            <Table.Tr>
              <Table.Th />
              <Table.Th>Date</Table.Th>
              <Table.Th>Duration</Table.Th>
              <Table.Th>Rooms</Table.Th>
              <Table.Th>Outcome</Table.Th>
              <Table.Th>Total damage</Table.Th>
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {result.runs.map((run) => (
              <RunRow key={run.id} run={run} />
            ))}
          </Table.Tbody>
        </Table>
      )}
      {result.pageCount > 1 && <Pagination mt="sm" total={result.pageCount} value={page} onChange={setPage} />}
    </Box>
  );
};

const RunRow = ({ run }: { run: ConfluxRun }) => {
  const [open, setOpen] = useState(false);
  const totalDamage = run.rooms.reduce((sum, room) => sum + room.totalDamage, 0);
  const [damage, unit] = humanizeNumbers(totalDamage);
  const outcome = run.completed === true ? "Completed" : run.completed === false ? "Abandoned" : "Unknown";

  return (
    <>
      <Table.Tr>
        <Table.Td>
          <UnstyledButton aria-label={open ? "Collapse run" : "Expand run"} onClick={() => setOpen(!open)}>
            {open ? <CaretDown size={16} /> : <CaretRight size={16} />}
          </UnstyledButton>
        </Table.Td>
        <Table.Td>{epochToLocalTime(run.startTime)}</Table.Td>
        <Table.Td>{run.duration == null ? "—" : millisecondsToElapsedFormat(run.duration)}</Table.Td>
        <Table.Td>{run.roomCount}</Table.Td>
        <Table.Td>{outcome}</Table.Td>
        <Table.Td>
          {damage}
          {unit}
        </Table.Td>
      </Table.Tr>
      <Table.Tr>
        <Table.Td colSpan={6} p={0}>
          <Collapse in={open}>
            <Box p="sm">
              <Table withTableBorder>
                <Table.Thead>
                  <Table.Tr>
                    <Table.Th>Room</Table.Th>
                    <Table.Th>Quest</Table.Th>
                    <Table.Th>Target</Table.Th>
                    <Table.Th>Duration</Table.Th>
                    <Table.Th>Damage</Table.Th>
                    <Table.Th>Buffs acquired</Table.Th>
                    <Table.Th>Log</Table.Th>
                  </Table.Tr>
                </Table.Thead>
                <Table.Tbody>
                  {run.rooms.map((room) => {
                    const buffs = run.buffs.find((delta) => delta.roomIndex === room.roomIndex)?.buffIds ?? [];
                    return (
                      <Table.Tr key={room.logId}>
                        <Table.Td>{room.roomIndex + 1}</Table.Td>
                        <Table.Td>{room.questId == null ? "Unknown" : translateQuestId(room.questId)}</Table.Td>
                        <Table.Td>
                          {room.primaryTarget == null ? "Unknown" : translateEnemyTypeId(room.primaryTarget)}
                        </Table.Td>
                        <Table.Td>{millisecondsToElapsedFormat(room.duration)}</Table.Td>
                        <Table.Td>{room.totalDamage.toLocaleString()}</Table.Td>
                        <Table.Td>{buffs.length ? buffs.map((id) => `Unknown buff #${id}`).join(", ") : "—"}</Table.Td>
                        <Table.Td>
                          <Link to={`/logs/${room.logId}`}>View</Link>
                        </Table.Td>
                      </Table.Tr>
                    );
                  })}
                </Table.Tbody>
              </Table>
              <Group mt="xs">
                <Text size="xs" c="dimmed">
                  Unknown buff IDs are retained for later name backfilling.
                </Text>
              </Group>
            </Box>
          </Collapse>
        </Table.Td>
      </Table.Tr>
    </>
  );
};
