import { useEffect, useRef, useState } from "react";
import type { Component, Progress, UiRunRequest } from "./contract";
import { cancelRun, request } from "./transport";

type Phase = "idle" | "running" | "done" | "error";
type RunState = {
  phase: Phase;
  percent: number | null;
  log: string[];
  views: Map<string, Component>;
  result?: unknown;
  message?: string;
};

const blank: RunState = {phase: "idle", percent: null, log: [], views: new Map()};
const messageOf = (error: unknown) => error instanceof Error ? error.message : String(error);

function append(log: string[], line: string): string[] {
  return [...log.slice(-499), line];
}

function applyEvent(state: RunState, event: Progress): RunState {
  switch (event.type) {
    case "started": return {...state, log: event.message ? append(state.log, event.message) : state.log};
    case "tick": return {...state, percent: Number.isFinite(event.percent) ? Math.min(1, Math.max(0, event.percent as number)) : null,
      log: event.message ? append(state.log, event.message) : state.log};
    case "log": return {...state, log: append(state.log, `[${event.level}] ${event.message}`)};
    case "telemetry": return {...state, log: append(state.log, `${event.key} = ${JSON.stringify(event.value)}`)};
    case "view": return {...state, views: new Map(state.views).set(event.id, event.component)};
    case "done": return {...state, phase: "done", percent: 1, result: event.result};
    case "error": return {...state, phase: "error", percent: 1, message: event.message};
    default: return state;
  }
}

export function useRun() {
  const [state, setState] = useState(blank);
  const [session, setSession] = useState<string>();
  const [cancelSent, setCancelSent] = useState(false);
  const active = useRef(false);
  const stream = useRef<EventSource>(null);
  const mounted = useRef(true);
  const cancelling = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      stream.current?.close();
    };
  }, []);

  function finish() {
    stream.current?.close();
    stream.current = null;
    active.current = false;
    setSession(undefined);
  }

  async function run(body: UiRunRequest) {
    if (active.current) return;
    active.current = true;
    cancelling.current = false;
    setCancelSent(false);
    setState({...blank, phase: "running"});
    try {
      const response = await request("/run", body);
      if (!mounted.current) return;
      if (typeof response.session_id !== "string" || !response.session_id) throw new Error("Missing session ID");
      setSession(response.session_id);
      const events = new EventSource("/progress/" + encodeURIComponent(response.session_id));
      stream.current = events;
      events.onmessage = ({data}) => {
        if (events !== stream.current) return;
        let event: Progress;
        try {
          event = JSON.parse(data);
          if (!event || typeof event.type !== "string") return;
        } catch { return; }
        setState(previous => applyEvent(previous, event));
        if (event.type === "done" || event.type === "error") finish();
      };
      events.onerror = () => {
        if (events !== stream.current) return;
        setState(previous => ({...previous, phase: "error", percent: 1}));
        finish();
      };
    } catch (error) {
      if (!mounted.current) return;
      setState(previous => ({...previous, phase: "error", message: messageOf(error)}));
      finish();
    }
  }

  async function cancel() {
    const source = stream.current;
    if (!session || !source || cancelling.current) return;
    cancelling.current = true;
    setCancelSent(true);
    try { await cancelRun(session); }
    catch (error) {
      if (stream.current !== source) return;
      setState(previous => ({...previous, log: append(previous.log, messageOf(error))}));
      cancelling.current = false;
      setCancelSent(false);
    }
  }

  return {state, run, cancel, cancellable: Boolean(session) && !cancelSent, cancelSent};
}
