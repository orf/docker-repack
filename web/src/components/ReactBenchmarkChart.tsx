import * as React from "react";
import { BarChart } from "@mui/x-charts/BarChart";
import type {
  BenchmarkData,
  BenchmarkImage,
  BenchmarkImageTime,
} from "../benchmark_parser.ts";

import "@fontsource/roboto/300.css";
import "@fontsource/roboto/400.css";
import "@fontsource/roboto/500.css";
import "@fontsource/roboto/700.css";
import { useState } from "react";

export interface BenchmarkChartProps {
  image: BenchmarkImage;
}

export default function ReactBenchmarkChart(props: BenchmarkChartProps) {
  const { image } = props;

  const [showAll, setShowAll] = useState(false);

  const allImageTypes = new Set(image.times.map((time) => time.type));
  const data = [];
  const times = Object.fromEntries(
    image.times.map((time: BenchmarkImageTime) => [time.type, time.time]),
  );
  data.push({ image: image.name, ...times });

  if (!showAll) {
    allImageTypes.clear();
    allImageTypes.add("original");
    allImageTypes.add(image.fastest_type);
  }

  const series = [...allImageTypes].map((key) => ({
    dataKey: key,
    label: key,
  }));

  return (
    <>
      <button onClick={() => setShowAll(!showAll)}>Show All</button>
      <BarChart
        dataset={data}
        xAxis={[{ scaleType: "band", dataKey: "image" }]}
        series={series}
      />
    </>
  );
}
