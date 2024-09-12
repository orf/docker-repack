export function formatDuration(
  seconds: number,
  short: boolean = false,
): string {
  const ms = seconds * 1000;
  if (ms < 1000) {
    return `${ms.toFixed(0)} ms`;
  }
  const time = {
    day: Math.floor(ms / 86400000),
    hour: Math.floor(ms / 3600000) % 24,
    minute: Math.floor(ms / 60000) % 60,
    second: Math.floor(ms / 1000) % 60,
    m: Math.floor(ms) % 1000,
  };
  if (short) {
    return Object.entries(time)
      .filter((val) => val[1] !== 0)
      .map(([key, val]) => `${val}${key[0]}`)
      .join(" ");
  }
  return Object.entries(time)
    .filter((val) => val[1] !== 0)
    .map(([key, val]) => `${val} ${key}${val !== 1 ? "s" : ""}`)
    .join(", ");
}

export function humanFileSize(size: number): string {
  const i = size == 0 ? 0 : Math.floor(Math.log(size) / Math.log(1024));
  return (
    +(size / Math.pow(1024, i)).toFixed(2) + ["B", "kB", "MB", "GB", "TB"][i]
  );
}
