import benchmark_data from "./data/benchmarks.json";
import groupBy from "lodash.groupby";

export interface BenchmarkImageTime {
  image: string;
  // type: "original" | "zstd" | "25MB" | "50MB" | "100MB" | "200MB";
  type: string;
  time: number;
}

export interface BenchmarkImage {
  image: string;
  times: BenchmarkImageTime[];
}

export interface BenchmarkData {
  images: BenchmarkImage[];
}

export function parseBenchmarkData(): BenchmarkData {
  const image_times: BenchmarkImageTime[] = benchmark_data.results.map(
    (res) => {
      return {
        image: res.parameters.image,
        type: res.parameters.suffix,
        time: res.mean,
      };
    },
  );
  const mapped = groupBy(image_times, (time) => time.image);
  const parsed: BenchmarkImage[] = Object.entries(mapped).map(
    ([image, times]) => {
      if (times === undefined) {
        throw new Error("times is undefined");
      }
      return {
        image,
        times,
      };
    },
  );
  return {
    images: parsed,
  };
}
