import { useEffect, useRef, useState } from 'react';
import { mediaRequest, type MediaJob, type MediaParams, type MediaSource, type MediaTool } from './media-api';

export function useMediaWorkspace() {
  const [source, setSource] = useState<MediaSource | null>(null);
  const sourceRef = useRef<MediaSource | null>(null);
  const [job, setJob] = useState<MediaJob | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  const [ready, setReady] = useState<boolean | null>(null);
  const version = useRef(0);
  const busy = pending || !!job && ['queued', 'running'].includes(job.state);
  useEffect(() => {
    let active = true;
    mediaRequest<{engine: boolean}>('capabilities').then(value => {if (active) setReady(value.engine);})
      .catch(error => {if (active) {setReady(false); setError(error.message);}});
    return () => {active = false;};
  }, []);
  const polling = job && ['queued', 'running'].includes(job.state) ? job.id : null;
  useEffect(() => {
    if (!polling) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const value = await mediaRequest<MediaJob>(`jobs/${polling}`);
        if (active) {
          setJob(previous => previous?.id !== value.id || !['queued', 'running'].includes(previous.state) ? previous : value);
          setError('');
        }
      } catch (error) {if (active) setError((error as Error).message + ' · 正在重连，任务状态尚未确认');}
      if (active) timer = setTimeout(poll, 450);
    }
    timer = setTimeout(poll, 200);
    return () => {active = false; clearTimeout(timer);};
  }, [polling]);

  async function discard() {
    version.current++;
    if (sourceRef.current) await mediaRequest(`workspaces/${sourceRef.current.id}`, {method: 'DELETE'});
    sourceRef.current = null; setSource(null); setJob(null);
  }
  async function select(file: File | null) {
    if (!file || busy) return;
    setPending(true); setError('');
    try {
      await discard();
      if (file.size > 256 * 1024 * 1024) throw new Error('单个视频最多 256 MB');
      const body = new FormData(); body.append('original', file);
      const value = await mediaRequest<MediaSource>('workspaces', {method: 'POST', body});
      sourceRef.current = value; setSource(value);
    } catch (error) {setError((error as Error).message);}
    finally {setPending(false);}
  }
  async function clear() {
    if (pending) return;
    setPending(true); setError('');
    try {await discard();} catch (error) {setError((error as Error).message);}
    finally {setPending(false);}
  }
  async function run(tool: MediaTool, params: MediaParams) {
    if (busy || !source) return;
    const generation = version.current;
    setPending(true); setError(''); setJob(null);
    try {
      const next = await mediaRequest<MediaJob>('jobs', {method: 'POST', headers: {'Content-Type': 'application/json'},
        body: JSON.stringify({session: source.id, tool, params})});
      if (generation === version.current) setJob(next);
    } catch (error) {setError((error as Error).message);}
    finally {setPending(false);}
  }
  async function cancel() {
    if (!job || pending) return;
    setPending(true);
    try {setJob(await mediaRequest<MediaJob>(`jobs/${job.id}/cancel`, {method:'POST'}));}
    catch (error) {setError((error as Error).message);}
    finally {setPending(false);}
  }
  return {source, job, pending, busy, ready, error, select, clear, run, cancel,
    resetResult: () => {if (!busy) setJob(null);}};
}
