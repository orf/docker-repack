import groupBy from "lodash.groupby";
import { Octokit } from "@octokit/rest";
import axios, { isCancel, AxiosError } from "axios";
import AdmZip from "adm-zip";

export interface BenchmarkImageTime {
  image: string;
  // type: "original" | "zstd" | "25MB" | "50MB" | "100MB" | "200MB";
  type: string;
  time: number;
}

export interface BenchmarkImage {
  name: string;
  times_faster: number;
  fastest_type: string;
  times: BenchmarkImageTime[];
}

export interface BenchmarkData {
  images: BenchmarkImage[];
}

export async function parseBenchmarkData(): Promise<BenchmarkData> {
  const octokit = new Octokit({ auth: process.env.GITHUB_TOKEN });

  const owner = "orf";
  const repo = "docker-repack";
  const resp = await octokit.actions.listArtifactsForRepo({
    owner,
    repo,
    name: "results",
  });
  const artifact = resp.data.artifacts[0];
  const artifact_response = await octokit.actions.downloadArtifact({
    owner,
    repo,
    artifact_id: artifact.id,
    archive_format: "zip",
  });
  const artifact_data = await axios.get(artifact_response.url, {
    responseType: "arraybuffer",
  });
  const zipfile: Buffer = Buffer.from(artifact_data.data);
  const zip = new AdmZip(zipfile);
  const results = zip.getEntry("results.json");
  if (results == null) {
    throw new Error("results.json not found in zip");
  }
  const benchmark_data: any = JSON.parse(results.getData().toString("utf-8")!);

  const image_times: BenchmarkImageTime[] = benchmark_data.results.map(
    (res: { parameters: { image: string; type: string }; mean: number }) => {
      return {
        image: res.parameters.image,
        type: res.parameters.type,
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

      const sorted_by_speed = times
        .filter((time) => time.type !== "original")
        .sort((a, b) => a.time - b.time);

      const fastest = sorted_by_speed[0];
      const original = times.find((time) => time.type === "original")!.time;

      const percentage_faster = Number((original / fastest.time).toFixed(1));

      return {
        name: image,
        times,
        fastest_type: fastest.type,
        times_faster: percentage_faster,
      };
    },
  );
  return {
    images: parsed,
  };
}
