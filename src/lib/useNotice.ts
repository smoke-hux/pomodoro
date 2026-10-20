import { useCallback, useEffect, useRef, useState } from "react";

/** One replaceable announcement, with no timer left behind after unmount. */
export function useNotice() {
  const [notice, setNotice] = useState("");
  const timeout = useRef<number | null>(null);
  const showNotice = useCallback((message: string) => {
    setNotice(message);
    if (timeout.current !== null) window.clearTimeout(timeout.current);
    timeout.current = window.setTimeout(() => {
      timeout.current = null;
      setNotice("");
    }, 3_000);
  }, []);

  useEffect(
    () => () => {
      if (timeout.current !== null) window.clearTimeout(timeout.current);
    },
    [],
  );

  return { notice, showNotice };
}
