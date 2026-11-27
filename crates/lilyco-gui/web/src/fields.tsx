import { useId } from "react";
import type { ArgKind, ArgSchema } from "./contract";
import { initialValue, type Draft } from "./arguments";
import { useText } from "./i18n";
import { FileInput } from "./files";
import { Button } from "@/ui/button";
import { Checkbox } from "@/ui/checkbox";
import { Input } from "@/ui/input";
import { Label } from "@/ui/label";
import { NativeSelect, NativeSelectOption } from "@/ui/native-select";

type FieldProps = {
  arg: ArgSchema;
  label: string;
  value: Draft;
  onChange: (update: (value: Draft) => Draft) => void;
  onBusy: (delta: number) => void;
  uploading: boolean;
};

export function ArgumentField({ arg, label, value, onChange, onBusy, uploading }: FieldProps) {
  const wide = arg.kind.type === "path" || arg.kind.type === "list";
  return <div className={wide ? "min-w-0 sm:col-span-2" : "min-w-0"}>
    <Field kind={arg.kind} value={value} label={label} onChange={onChange}
      onBusy={onBusy} required={arg.required} uploading={uploading} />
  </div>;
}

type Control = {
  kind: ArgKind;
  value: Draft;
  label: string;
  onChange: FieldProps["onChange"];
  onBusy: FieldProps["onBusy"];
  required?: boolean;
  uploading: boolean;
};

function Field({ kind, value, label, onChange, onBusy, required, uploading }: Control) {
  const id = useId();
  const setValue = (next: Draft) => onChange(() => next);
  if (kind.type === "flag") return <Label htmlFor={id} className="flex min-h-11 min-w-0 items-center gap-3 wrap-anywhere">
    <Checkbox id={id} checked={value === true} onCheckedChange={setValue} />{label}
  </Label>;
  if (kind.type === "list") return <fieldset className="min-w-0">
    <legend className="mb-2 text-sm font-medium wrap-anywhere">{label}{required && <span aria-hidden="true"> *</span>}</legend>
    <ListField item={kind.item} value={Array.isArray(value) ? value : []} onChange={onChange}
      onBusy={onBusy} label={label} uploading={uploading} />
  </fieldset>;
  const text = typeof value === "string" ? value : "";
  return <div className="grid min-w-0 gap-2">
    <Label htmlFor={id} className="block leading-normal wrap-anywhere">{label}{required && <span aria-hidden="true" className="text-destructive"> *</span>}</Label>
    {kind.type === "path"
      ? <FileInput id={id} value={text} required={required} mustExist={kind.must_exist} onChange={setValue} onBusy={onBusy} />
      : <ScalarField id={id} kind={kind} value={text} required={required} onChange={setValue} />}
  </div>;
}

function ScalarField({ id, kind, value, required, onChange }: {
  id: string; kind: Exclude<ArgKind, {type: "list" | "path" | "flag"}>;
  value: string; required?: boolean; onChange: (value: string) => void;
}) {
  const t = useText();
  if (kind.type === "enum") return <NativeSelect id={id} value={value}
    required={required} className="h-11 w-full" onChange={event => onChange(event.target.value)}>
    <NativeSelectOption value="">{t("choose_option")}</NativeSelectOption>
    {kind.values.map(option => <NativeSelectOption key={option} value={option}>{option}</NativeSelectOption>)}
  </NativeSelect>;
  const min = kind.type === "number" ? kind.min ?? undefined : undefined;
  const max = kind.type === "number" ? kind.max ?? undefined : undefined;
  const bounds = min !== undefined && max !== undefined ? `${min} – ${max}` :
    min !== undefined ? `≥ ${min}` : max !== undefined ? `≤ ${max}` : "";
  return <>
    <Input id={id} className="h-11" value={value} required={required}
      type={kind.type === "number" ? "number" : "text"}
      min={min} max={max} step={kind.type === "number" ? "any" : undefined}
      aria-describedby={bounds ? id + "-bounds" : undefined}
      onChange={event => onChange(event.target.value)} />
    {bounds && <div id={id + "-bounds"} className="text-xs text-muted-foreground">{t("bounds", {range: bounds})}</div>}
  </>;
}

function ListField({ item, value, label, onChange, onBusy, uploading }: {
  item: ArgKind; value: Draft[]; label: string;
  onChange: FieldProps["onChange"]; onBusy: FieldProps["onBusy"]; uploading: boolean;
}) {
  const t = useText();
  function changeRows(update: (rows: Draft[]) => Draft[]) {
    onChange(previous => update(Array.isArray(previous) ? previous : []));
  }
  return <div className="grid gap-3">
    {value.map((entry, index) => <div key={index} className="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-start gap-2">
      <Field kind={item} value={entry} label={`${label} ${index + 1}`} onBusy={onBusy} uploading={uploading}
        onChange={update => changeRows(rows => rows.map((existing, at) => index === at ? update(existing) : existing))} />
      <Button variant="outline" disabled={uploading} aria-label={t("delete_row")}
        onClick={() => changeRows(rows => rows.filter((_, at) => at !== index))}>×</Button>
    </div>)}
    <Button variant="outline" className="justify-self-start" disabled={uploading}
      onClick={() => changeRows(rows => [...rows, initialValue(item, undefined)])}>{t("add_item")}</Button>
  </div>;
}
