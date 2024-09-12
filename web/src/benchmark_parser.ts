import groupBy from "lodash.groupby";
import axios from "axios";
import AdmZip from "adm-zip";
// @ts-ignore
import sources from "../../benchmark/sources.yaml";
import { getArtifact, githubClient } from "./github_client.ts";
import { getManifests, type Layer } from "./manifest_parser.ts";

const destinationToUpstream = Object.fromEntries(
  // @ts-ignore
  sources.upstream_images.map(({ upstream_image, destination }) => [
    destination,
    upstream_image,
  ]),
);

export interface BenchmarkImageTime {
  image: string;
  // type: "original" | "zstd" | "25MB" | "50MB" | "100MB" | "200MB";
  type: string;
  time: number;
  total_size: number;
  layers: Layer[];
}

export interface BenchmarkImage {
  name: string;
  name_slug: string;
  times_faster: number;
  times_smaller: number;
  fastest_type: string;
  times: BenchmarkImageTime[];
}

export interface BenchmarkData {
  images: BenchmarkImage[];
}

export async function parseBenchmarkData(): Promise<BenchmarkData> {
  const manifests = await getManifests();

  const zip = await getArtifact("benchmark-results");
  const results = zip.getEntry("results.json");
  if (results == null) {
    throw new Error("results.json not found in zip");
  }
  const benchmark_data: any = JSON.parse(results.getData().toString("utf-8")!);

  const image_times: BenchmarkImageTime[] = benchmark_data.results
    .map(
      (res: { parameters: { image: string; type: string }; mean: number }) => {
        const manifestKey = `${res.parameters.image}-${res.parameters.type}`;
        const manifest = manifests[manifestKey];
        if (manifest === undefined) {
          return null;
        }
        return {
          image: res.parameters.image,
          type: res.parameters.type,
          time: res.mean,
          total_size: manifest.reduce((acc, layer) => acc + layer.size, 0),
          layers: manifest,
        };
      },
    )
    .filter((x: BenchmarkImageTime | null) => x !== null);
  const mapped = groupBy(image_times, (time) => time.image);
  const parsed: BenchmarkImage[] = Object.entries(mapped)
    .map(([image, times]) => {
      if (times === undefined) {
        throw new Error("times is undefined");
      }

      const sorted_by_speed = times
        .filter((time) => time.type !== "original")
        .sort((a, b) => a.time - b.time);

      const fastest = sorted_by_speed[0];
      const original = times.find((time) => time.type === "original")!;

      const percentage_faster = Number(
        (original.time / fastest.time).toFixed(1),
      );

      const percentage_smaller = Number(
        (original.total_size / fastest.total_size).toFixed(1),
      );

      const sorted_times = [
        original,
        ...times.filter((time) => time.type !== "original"),
      ];

      if (destinationToUpstream[image] === undefined) {
        return null;
      }

      return {
        name: destinationToUpstream[image],
        name_slug: image.replaceAll(".", "-"),
        times: sorted_times,
        fastest_type: fastest.type,
        times_smaller: percentage_smaller,
        times_faster: percentage_faster,
      };
    })
    .filter((x) => x !== null);
  return {
    images: parsed,
  };
}
