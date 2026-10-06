import { useEffect, useRef, useState } from "react";

/**
 * A `blob:` URL for an evidence image (phase 5, ruling 3). The URL is made from the loaded Blob,
 * revoked when the key changes or the component unmounts, and a load that resolves after either
 * is revoked at once instead of leaking.
 */
export function useImageUrl(load: (() => Promise<Blob>) | null, key: string | null): { url: string | null; error: boolean } {
  const [state, setState] = useState<{ url: string | null; error: boolean }>({ url: null, error: false });
  const loadRef = useRef(load);
  loadRef.current = load;
  useEffect(() => {
    const current = loadRef.current;
    setState({ url: null, error: false });
    if (current === null || key === null) return undefined;
    let live = true;
    let made: string | null = null;
    current().then((blob) => {
      const url = URL.createObjectURL(blob);
      if (!live) { URL.revokeObjectURL(url); return; }
      made = url;
      setState({ url, error: false });
    }, () => { if (live) setState({ url: null, error: true }); });
    return () => {
      live = false;
      if (made !== null) URL.revokeObjectURL(made);
    };
  }, [key]);
  return state;
}
