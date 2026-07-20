import { useMeterSettingsStore } from "@/stores/useMeterSettingsStore";
import { AbilityBreakdownColumn, CharacterType, ComputedSkillState } from "@/types";
import { computeOvercapPercentage, getSkillName } from "@/utils";
import { ActionIcon, Tooltip } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import { Info } from "@phosphor-icons/react";
import { useTranslation } from "react-i18next";
import { DamageDetailsModal } from "./DamageDetailsModal";
import { useSkillRow } from "./useSkillRow";

export type SkillRowProps = {
  characterType: CharacterType;
  skill: ComputedSkillState;
  color: string;
  nested?: boolean;
};

export const SkillRow = ({ characterType, skill, color, nested }: SkillRowProps) => {
  const { t } = useTranslation();
  const [detailsOpened, { open: openDetails, close: closeDetails }] = useDisclosure(false);
  const abilityColumns = useMeterSettingsStore((state) => state.ability_breakdown_columns);
  const {
    showFullValues,
    totalDamage,
    totalDamageUnit,
    minDmg,
    minDmgUnit,
    maxDmg,
    maxDmgUnit,
    rawAverageDmg,
    averageDmg,
    averageDmgUnit,
  } = useSkillRow(skill);

  return (
    <>
      <tr className={`skill-row ${nested ? "nested" : ""}`}>
        {nested ? (
          <td className="text-left row-data nested">{getSkillName(characterType, skill)}</td>
        ) : (
          <td className="text-left row-data">{getSkillName(characterType, skill)}</td>
        )}
        <td className="text-center row-data">{skill.hits}</td>
        <td className="text-center row-data">
          {showFullValues ? (
            skill.totalDamage.toLocaleString()
          ) : (
            <>
              {totalDamage}
              <span className="unit font-sm">{totalDamageUnit}</span>
            </>
          )}
        </td>
        <td className="text-center row-data">
          {showFullValues ? (
            skill.minDamage ? (
              skill.minDamage.toLocaleString()
            ) : (
              ""
            )
          ) : (
            <>
              {skill.minDamage && minDmg}
              <span className="unit font-sm">{minDmgUnit}</span>
            </>
          )}
        </td>
        <td className="text-center row-data">
          {showFullValues ? (
            skill.maxDamage ? (
              skill.maxDamage.toLocaleString()
            ) : (
              ""
            )
          ) : (
            <>
              {skill.maxDamage && maxDmg}
              <span className="unit font-sm">{maxDmgUnit}</span>
            </>
          )}
        </td>
        <td className="text-center row-data">
          {showFullValues ? (
            rawAverageDmg.toLocaleString()
          ) : (
            <>
              {averageDmg}
              <span className="unit font-sm">{averageDmgUnit}</span>
            </>
          )}
        </td>
        <td className="text-center row-data">
          {skill.percentage.toFixed(0)}
          <span className="unit font-sm">%</span>
        </td>
        {abilityColumns.map((column) => {
          const value = column === AbilityBreakdownColumn.Overcap ? computeOvercapPercentage(skill) : null;
          return (
            <td key={column} className="text-center row-data">
              {value === null ? "—" : `${value.toFixed(1)}%`}
            </td>
          );
        })}
        <td className="text-center row-data">
          {skill.damageDetails && (
            <Tooltip label={t("ui.damage-details.open")}>
              <ActionIcon size="sm" variant="subtle" aria-label={t("ui.damage-details.open")} onClick={openDetails}>
                <Info size={15} />
              </ActionIcon>
            </Tooltip>
          )}
        </td>
        <div className="damage-bar" style={{ backgroundColor: color, width: `${skill.percentage}%` }} />
      </tr>
      {detailsOpened && (
        <DamageDetailsModal characterType={characterType} skill={skill} opened onClose={closeDetails} />
      )}
    </>
  );
};
