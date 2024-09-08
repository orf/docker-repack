import benchmark_data from "./data/benchmarks.json";
import groupBy from "lodash.groupby";
import { Octokit } from "@octokit/rest";

export interface BenchmarkImageTime {
  image: string;
  // type: "original" | "zstd" | "25MB" | "50MB" | "100MB" | "200MB";
  type: string;
  time: number;
}

export interface BenchmarkImage {
  name: string;
  times_faster: number;
  times: BenchmarkImageTime[];
}

export interface BenchmarkData {
  images: BenchmarkImage[];
}

const octokit = new Octokit();

export async function parseBenchmarkData(): Promise<BenchmarkData> {
    const resp = await octokit.actions.listArtifactsForRepo({
        owner: "orf",
        repo: "docker-repack"
    });
    throw new Error(JSON.stringify(resp.data.artifacts));

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

      const fastest = Math.min(...times.map((time) => time.time));
      const original = times.find((time) => time.type === "original")!.time;

      const percentage_faster = Number((original / fastest).toFixed(1));

      return {
        name: image,
        times,
        times_faster: percentage_faster,
      };
    },
  );
  return {
    images: parsed,
  };
}
