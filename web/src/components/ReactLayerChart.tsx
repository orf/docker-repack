import { BarChart } from "@mui/x-charts/BarChart";
import type { Layer } from "../manifest_parser.ts";
import { humanFileSize } from "../utils.ts";

const valueFormatter = (value: number | null) => humanFileSize(value!);

const chartSetting = {
  xAxis: [
    {
      label: "Size",
      valueFormatter,
    },
  ],
};

export default function LayerChart({ layers }: { layers: Layer[] }) {
  const dataset = layers.map((layer, idx) => ({
    size: layer.size,
    layer: `#${idx + 1}`,
  }));
  return (
    <BarChart
      dataset={dataset}
      yAxis={[{ scaleType: "band", dataKey: "layer" }]}
      series={[{ dataKey: "size", label: "Layer Size", valueFormatter }]}
      layout="horizontal"
      slotProps={{ legend: { hidden: true } }}
      {...chartSetting}
    />
  );
}
