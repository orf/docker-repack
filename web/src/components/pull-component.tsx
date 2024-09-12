import { useEffect, useState } from "react";
import { humanFileSize } from "../utils.ts";

export interface Layer {
  bytes: number;
  digest: string;
}

interface LayerPullState {
  layer: Layer;
  bytes_done: number;
  // percent_done: number;
  done: boolean;
}

function updatePullState(
  bytes_per_tick: number,
  layer_state: LayerPullState,
): LayerPullState {
  const total_bytes = layer_state.layer.bytes;
  if (total_bytes <= layer_state.bytes_done) {
    layer_state.done = true;
    return layer_state;
  }
  // vary bytes_per_tick by up to 25% to simulate network jitter
  bytes_per_tick *= 0.7 + Math.random() * 0.3;
  const new_bytes_done = layer_state.bytes_done + bytes_per_tick;
  let done = false;
  if (new_bytes_done >= layer_state.layer.bytes) {
    done = true;
  }
  // const percent_done = (new_bytes_done / total_bytes) * 100;
  return {
    layer: layer_state.layer,
    bytes_done: new_bytes_done,
    // percent_done,
    done,
  };
}

// 3.11: Pulling from library/python
// 56c9b9253ff9: Downloading  6.589MB/49.59MB
// 364d19f59f69: Downloading  4.668MB/23.59MB
// 843b1d832182: Downloading  8.043MB/64MB
// a348c2a8d946: Waiting
// dd681ddda6db: Waiting
// 2fa7159a8e74: Waiting
// 2d3256a435e2: Waiting
// 8d76c12bea0d: Waiting

function LayerPullComponent({ pull }: { pull: LayerPullState }) {
  const prefix = pull.layer.digest.slice(0, 12);
  if (pull.done) {
    return (
      <>
        {prefix}: Pull complete{"\n"}
      </>
    );
  }
  return (
    <>
      {prefix}: Downloading {humanFileSize(pull.bytes_done).padStart(5)}/
      {humanFileSize(pull.layer.bytes)}
      {"\n"}
    </>
  );
}

export default function PullComponent({
  layers,
  bandwidth,
  max_downloads,
}: {
  layers: Layer[];
  bandwidth: number;
  max_downloads: number;
}) {
  const initialState = layers.map((layer) => ({
    layer,
    bytes_done: 0,
    done: false,
  }));
  const [state, setState] = useState<LayerPullState[]>(initialState);
  const [startTime, setStartTime] = useState(Date.now());

  const elapsed = Date.now() - startTime;

  useEffect(() => {
    if (state.every((layer_state) => layer_state.done)) {
      return;
    }
    const interval = setInterval(() => {
      let downloadCount = 0;
      const in_progress_count = state.filter((layer) => !layer.done).length;
      const bytes_per_layer = bandwidth / in_progress_count;
      const newState = state.map((layer) => {
        if (!layer.done && downloadCount < max_downloads) {
          downloadCount += 1;
          return updatePullState(bytes_per_layer, layer);
        }
        return layer;
      });
      setState(newState);
    }, 100);

    // Clean up the interval on component unmount
    return () => clearInterval(interval);
  }, [state, bandwidth, max_downloads]);

  return (
    <>
      <pre>
        {state.map((layer, i) => (
          <LayerPullComponent key={i} pull={layer} />
        ))}
        {"\n"}
        Total Time: {(elapsed / 1000).toFixed(1)} seconds{"\n"}
      </pre>
    </>
  );
}
