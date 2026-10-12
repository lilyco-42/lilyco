import { useEffect, useRef, useState } from 'react';
import { visionRequest, type VisionJob, type VisionParams, type VisionTool } from './vision-tools';

export function useVisionWorkspace() {
  const [original, setOriginal] = useState<File | null>(null);
  const [rebuilt, setRebuilt] = useState<File | null>(null);
  const [preview, setPreview] = useState('');
  const [session, setSession] = useState<string | null>(null);
  const sessionRef = useRef<string | null>(null);
  const [job, setJob] = useState<VisionJob | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  const [capabilities, setCapabilities] = useState<{engine: boolean; html_browser: boolean} | null>(null);
  const generation = useRef(0);
  useEffect(() => { visionRequest<typeof capabilities>('capabilities').then(setCapabilities).catch(e => setError(String(e.message))); }, []);
  useEffect(() => {
    if (!original || !original.type.startsWith('image/')) { setPreview(''); return; }
    const url = URL.createObjectURL(original);
    setPreview(url);
    return () => URL.revokeObjectURL(url);
  }, [original]);
  useEffect(() => {
    if (!job || !['queued', 'running'].includes(job.state)) return;
    let active = true;
    const poll = async () => {
      try { const value = await visionRequest<VisionJob>(`jobs/${job.id}`); if (active) setJob(value); }
      catch (e) { if (active) { setError((e as Error).message); setJob(null); } }
    };
    const timer = setInterval(poll, 350);
    return () => { active = false; clearInterval(timer); };
  }, [job?.id, job?.state]);
  const busy = pending || !!job && ['queued', 'running'].includes(job.state);

  async function discard() {
    generation.current++;
    if (sessionRef.current) await visionRequest(`workspaces/${sessionRef.current}`, {method: 'DELETE'});
    sessionRef.current = null; setSession(null); setJob(null);
  }
  async function select(file: File | null, second = false) {
    if (busy) return;
    setPending(true); setError('');
    try { await discard(); second ? setRebuilt(file) : (setOriginal(file), setRebuilt(null)); }
    catch (e) { setError((e as Error).message); }
    finally { setPending(false); }
  }
  async function clear() {
    setPending(true); setError('');
    try { await discard(); setOriginal(null); setRebuilt(null); }
    catch (e) { setError((e as Error).message); }
    finally { setPending(false); }
  }
  async function run(tool: VisionTool, params: VisionParams) {
    if (busy || !original) return;
    setPending(true); setError(''); setJob(null);
    const version = generation.current;
    try {
      let id = session;
      if (!id) {
        const body = new FormData(); body.append('original', original);
        if (rebuilt) body.append('rebuilt', rebuilt);
        const uploaded = await visionRequest<{id: string}>('workspaces', {method: 'POST', body});
        id = uploaded.id; sessionRef.current = id; setSession(id);
      }
      const next = await visionRequest<VisionJob>('jobs', {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({session: id, tool, params})});
      if (version === generation.current) setJob(next);
    } catch (e) { setError((e as Error).message); }
    finally { setPending(false); }
  }
  async function cancel() {
    if (!job) return;
    try { setJob(await visionRequest<VisionJob>(`jobs/${job.id}/cancel`, {method: 'POST'})); }
    catch (e) { setError((e as Error).message); }
  }
  return {original, rebuilt, preview, job, pending, busy, error, capabilities, select, clear, run, cancel, resetResult: () => setJob(null)};
}
