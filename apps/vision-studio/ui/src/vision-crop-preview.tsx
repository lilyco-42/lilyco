import { useRef, useState, type PointerEvent } from 'react';
import { Button } from './components/ui/button';

type Props = {src: string; region: string; disabled: boolean; onRegion: (region: string) => void};

export function VisionCropPreview({src, region, disabled, onRegion}: Props) {
  const [size, setSize] = useState<[number, number] | null>(null);
  const [expanded, setExpanded] = useState(false);
  const anchor = useRef<[number, number] | null>(null);
  const values = region.split(',').map(Number);
  const rectangle = values.length === 4 && values.every(Number.isFinite) ? values : null;
  function point(event: PointerEvent<SVGSVGElement>): [number, number] {
    const matrix = event.currentTarget.getScreenCTM();
    const value = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix!.inverse());
    return [Math.max(0, Math.min(size![0], Math.floor(value.x))), Math.max(0, Math.min(size![1], Math.floor(value.y)))];
  }
  function begin(event: PointerEvent<SVGSVGElement>) {
    if (disabled || !size || event.button !== 0) return;
    event.preventDefault();
    const [x, y] = point(event);
    anchor.current = [Math.min(x, size[0] - 1), Math.min(y, size[1] - 1)];
    event.currentTarget.setPointerCapture(event.pointerId);
  }
  function move(event: PointerEvent<SVGSVGElement>) {
    if (!anchor.current || disabled || !size) return;
    const [x, y] = point(event);
    const [startX, startY] = anchor.current;
    if (x !== startX && y !== startY) onRegion([Math.min(x, startX), Math.min(y, startY), Math.max(x, startX), Math.max(y, startY)].join(','));
  }
  function end(event: PointerEvent<SVGSVGElement>) {
    if (!anchor.current) return;
    move(event); anchor.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
  }
  return <div className="studio-checker rounded-lg border p-3">
    <img id="originalPreview" className="sr-only" src={src} alt="" onLoad={e => setSize([e.currentTarget.naturalWidth, e.currentTarget.naturalHeight])}/>
    {size && <svg id="cropStage" role="img" aria-label="拖动选择裁剪区域，也可填写像素坐标" width={size[0]} height={size[1]} viewBox={`0 0 ${size[0]} ${size[1]}`} className={`${expanded ? 'max-h-[70vh]' : 'vision-crop-compact'} h-auto w-full touch-none select-none cursor-crosshair`} onPointerDown={begin} onPointerMove={move} onPointerUp={end} onPointerCancel={end}>
      <image href={src} width={size[0]} height={size[1]}/>
      {rectangle && <rect x={Math.min(rectangle[0], rectangle[2])} y={Math.min(rectangle[1], rectangle[3])} width={Math.abs(rectangle[2] - rectangle[0])} height={Math.abs(rectangle[3] - rectangle[1])} className="fill-primary/15 stroke-primary" strokeWidth="2" vectorEffect="non-scaling-stroke"/>}
    </svg>}
    <div className="mt-2 flex items-center justify-between gap-2"><p className="min-w-0 text-xs text-muted-foreground">拖动框选，也可填写右侧像素坐标。</p><Button variant="ghost" size="xs" disabled={disabled} onClick={() => setExpanded(!expanded)}>{expanded ? '收起原图' : '放大原图'}</Button></div>
  </div>;
}
