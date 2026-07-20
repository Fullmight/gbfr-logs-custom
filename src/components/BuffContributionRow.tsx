import { useMeterSettingsStore } from "@/stores/useMeterSettingsStore";
import { BuffContributionState } from "@/types";
import { humanizeNumbers } from "@/utils";
import { useTranslation } from "react-i18next";

const formatStatusName = (statusName: string) =>
  statusName
    .replace(/^Status/, "")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .trim();

export const BuffContributionRow = ({
  buff,
  playerTotalDamage,
  color,
}: {
  buff: BuffContributionState;
  playerTotalDamage: number;
  color: string;
}) => {
  const { t } = useTranslation();
  const abilityColumns = useMeterSettingsStore((state) => state.ability_breakdown_columns);
  const [damage, damageUnit] = humanizeNumbers(buff.contributedDamage);
  const percentage = playerTotalDamage > 0 ? (buff.contributedDamage / playerTotalDamage) * 100 : 0;
  const attributionTooltip = t("ui.buff-contribution.tooltip", {
    rawName: buff.statusName,
    category: buff.category,
  });

  return (
    <tr className="skill-row buff-row" title={attributionTooltip}>
      <td className="text-left row-data">{formatStatusName(buff.statusName) || t("ui.buff-contribution.unknown")}</td>
      <td className="text-center row-data">{buff.activeHits}</td>
      <td className="text-center row-data">
        ≈{damage}
        <span className="unit font-sm">{damageUnit}</span>
      </td>
      <td className="text-center row-data" />
      <td className="text-center row-data" />
      <td className="text-center row-data" />
      <td className="text-center row-data">
        {percentage.toFixed(1)}
        <span className="unit font-sm">%</span>
      </td>
      {abilityColumns.map((column) => (
        <td key={column} className="text-center row-data">
          —
        </td>
      ))}
      <td className="text-center row-data">{t("ui.buff-contribution.included")}</td>
      <div className="damage-bar" style={{ backgroundColor: color, width: `${Math.min(percentage, 100)}%` }} />
    </tr>
  );
};
