import * as React from "react";
import { BarChart } from "@mui/x-charts/BarChart";
import type {
  BenchmarkImage,
  BenchmarkImageTime,
} from "../benchmark_parser.ts";

import "@fontsource/roboto/300.css";
import "@fontsource/roboto/400.css";
import "@fontsource/roboto/500.css";
import "@fontsource/roboto/700.css";

import { useState } from "react";
import { formatDuration, humanFileSize } from "../utils.ts";

export interface BenchmarkChartProps {
  image: BenchmarkImage;
  prop: "time" | "total_size";
}

export default function ReactBenchmarkChart(props: BenchmarkChartProps) {
  const { image, prop } = props;

  const [showAll, setShowAll] = useState(false);

  const allImageTypes = new Set(image.times.map((time) => time.type));
  const data = [];
  const values = Object.fromEntries(
    image.times.map((time: BenchmarkImageTime) => [time.type, time[prop]]),
  );
  data.push({ image: image.name, ...values });

  if (!showAll) {
    allImageTypes.clear();
    allImageTypes.add("original");
    allImageTypes.add(image.fastest_type);
  }

  const formatter = prop === "time" ? formatDuration : humanFileSize;

  const series = [...allImageTypes].map((key) => ({
    dataKey: key,
    label: key,
    valueFormatter: (value: number | null) => {
      if (value === null) {
        return "N/A";
      }
      return formatter(value, true);
    },
  }));

  return (
    <div className={"h-full"}>
      <div className={"h-5/6"}>
        <BarChart
          margin={{ left: 75 }}
          dataset={data}
          yAxis={[
            { valueFormatter: (value: number) => formatter(value, true) },
          ]}
          xAxis={[{ scaleType: "band", dataKey: "image" }]}
          series={series}
        />
      </div>
      <div className={"h-fit text-center"}>
        <button onClick={() => setShowAll(!showAll)} className={"align-top"}>
          {showAll
            ? "Show only fastest result"
            : `Show all ${image.times.length} results`}
        </button>
      </div>
    </div>
  );
}
