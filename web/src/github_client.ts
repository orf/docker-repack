import { Octokit } from "@octokit/rest";
import axios from "axios";
import AdmZip from "adm-zip";

export const githubClient = new Octokit({ auth: process.env.GITHUB_TOKEN });

export async function getArtifact(name: string): Promise<AdmZip> {
  const owner = "orf";
  const repo = "docker-repack";
  const resp = await githubClient.actions.listArtifactsForRepo({
    owner,
    repo,
    name,
  });
  const artifact = resp.data.artifacts[0];
  const artifact_response = await githubClient.actions.downloadArtifact({
    owner,
    repo,
    artifact_id: artifact.id,
    archive_format: "zip",
  });
  const artifact_data = await axios.get(artifact_response.url, {
    responseType: "arraybuffer",
  });
  const zipfile: Buffer = Buffer.from(artifact_data.data);
  return new AdmZip(zipfile);
}
