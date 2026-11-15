import { useEffect, useState } from "react";
import { HostError, hostStatus, resetHostSession } from "../analysis/host";

type Connection = { readonly state: "checking" | "ready" } | { readonly state: "unavailable"; readonly message: string };

/** Check API availability without sending query data or testing a provider key. */
export function useHostConnection() {
  const [attempt, setAttempt] = useState(0);
  const [connection, setConnection] = useState<Connection>({ state: "checking" });
  useEffect(() => {
    let current = true;
    void hostStatus().then(
      () => {
        if (current) setConnection({ state: "ready" });
      },
      (error: unknown) => {
        if (!current) return;
        const message = error instanceof HostError ? error.message : "The analysis server could not be reached. Check your sign-in and connection.";
        setConnection({ state: "unavailable", message });
      },
    );
    return () => {
      current = false;
    };
  }, [attempt]);
  const retry = (): void => {
    resetHostSession();
    setConnection({ state: "checking" });
    setAttempt((value) => value + 1);
  };
  return { connection, retry };
}
