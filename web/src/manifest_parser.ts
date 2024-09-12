import { getArtifact } from "./github_client.ts";

export interface Layer {
  digest: string;
  size: number;
}

export async function getManifests(): Promise<{ [key: string]: Layer[] }> {
  const zip = await getArtifact("image-manifests");
  const entries = zip.getEntries().map((entry) => {
    const parsed = JSON.parse(entry.getData().toString());
    const layers: Layer[] = parsed.LayersData.map(
      (layer: { Digest: string; Size: number }) => {
        return {
          digest: layer.Digest,
          size: layer.Size,
        };
      },
    );
    const name = entry.entryName.replace("manifest-", "").replace(".json", "");
    return [name, layers];
  });
  return Object.fromEntries(entries);
}
