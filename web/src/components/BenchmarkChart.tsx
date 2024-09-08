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

export interface BenchmarkChartProps {
  dataset: BenchmarkImage[];
}

export default function BenchmarkChart(props: BenchmarkChartProps) {
  const { dataset } = props;

  const images = new Set(
    dataset.flatMap((image) => image.times.map((time) => time.type)),
  );
  const data = [];
  for (const image of dataset) {
    const times = Object.fromEntries(
      image.times.map((time: BenchmarkImageTime) => [time.type, time.time]),
    );

    data.push({ image: image.image, ...times });
  }

  const series = [...images].map((key) => ({
    dataKey: key,
    label: key,
  }));

  return (
    <>
      <BarChart
        dataset={data}
        xAxis={[{ scaleType: "band", dataKey: "image" }]}
        series={series}
        width={1000}
        height={500}
      />
    </>
  );
}
