// @ts-ignore
import humanizeDuration from "humanize-duration";

const shortEnglishHumanizer = humanizeDuration.humanizer({
  language: "shortEn",
  languages: {
    shortEn: {
      y: () => "y",
      mo: () => "mo",
      w: () => "w",
      d: () => "d",
      h: () => "h",
      m: () => "m",
      s: () => "s",
      ms: () => "ms",
    },
  },
});

export function formatDuration(
  seconds: number,
  short: boolean = false,
): string {
  const args = { units: ["m", "s"], round: true };
  if (short) {
    return shortEnglishHumanizer(seconds * 1000, args);
  }
  return humanizeDuration(seconds * 1000, args);
}

export function humanFileSize(size: number): string {
  const i = size == 0 ? 0 : Math.floor(Math.log(size) / Math.log(1024));
  return (
    +(size / Math.pow(1024, i)).toFixed(2) + ["B", "kB", "MB", "GB", "TB"][i]
  );
}
