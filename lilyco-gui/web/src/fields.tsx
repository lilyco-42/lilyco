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
  value: Draft;
  onChange: (value: Draft) => void;
  onBusy: (delta: number) => void;
};

export function ArgumentField({ arg, value, onChange, onBusy }: FieldProps) {
  return <Field kind={arg.kind} value={value} label={arg.about || arg.name} onChange={onChange}
    onBusy={onBusy} required={arg.required} />;
}

type Control = {
  kind: ArgKind;
  value: Draft;
  label: string;
  onChange: (value: Draft) => void;
  onBusy: (delta: number) => void;
  required?: boolean;
};

function Field({ kind, value, label, onChange, onBusy, required }: Control) {
  const id = useId();
  if (kind.type === "flag") return <Label htmlFor={id} className="flex min-h-11 items-center gap-3">
    <Checkbox id={id} checked={value === true} onCheckedChange={onChange} />{label}
  </Label>;
  if (kind.type === "list") return <fieldset className="min-w-0">
    <legend className="mb-2 text-sm font-medium">{label}{required && <span aria-hidden="true"> *</span>}</legend>
    <ListField item={kind.item} value={Array.isArray(value) ? value : []} onChange={onChange} onBusy={onBusy} label={label} />
  </fieldset>;
  const text = typeof value === "string" ? value : "";
  return <div className="grid min-w-0 gap-2">
    <Label htmlFor={id} className="block leading-normal">{label}{required && <span aria-hidden="true" className="text-destructive"> *</span>}</Label>
    {kind.type === "path"
      ? <FileInput id={id} value={text} required={required} mustExist={kind.must_exist} onChange={onChange} onBusy={onBusy} />
      : <ScalarField id={id} kind={kind} value={text} required={required} onChange={onChange} />}
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

function ListField({ item, value, label, onChange, onBusy }: {
  item: ArgKind; value: Draft[]; label: string; onChange: (value: Draft[]) => void; onBusy: (delta: number) => void;
}) {
  const t = useText();
  return <div className="grid gap-3">
    {value.map((entry, index) => <div key={index} className="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-start gap-2">
      <Field kind={item} value={entry} label={`${label} ${index + 1}`} onBusy={onBusy}
        onChange={next => onChange(value.map((existing, at) => index === at ? next : existing))} />
      <Button variant="outline" aria-label={t("delete_row")} onClick={() => onChange(value.filter((_, at) => at !== index))}>×</Button>
    </div>)}
    <Button variant="outline" className="justify-self-start"
      onClick={() => onChange([...value, initialValue(item, undefined)])}>{t("add_item")}</Button>
  </div>;
}
