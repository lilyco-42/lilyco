import { useId } from "react";
import { Calendar } from "@/ui/calendar";
import { Checkbox } from "@/ui/checkbox";
import { Input } from "@/ui/input";
import { InputOTP, InputOTPGroup, InputOTPSlot } from "@/ui/input-otp";
import { Label } from "@/ui/label";
import { NativeSelect, NativeSelectOption } from "@/ui/native-select";
import { RadioGroup, RadioGroupItem } from "@/ui/radio-group";
import { Slider } from "@/ui/slider";
import { Switch } from "@/ui/switch";
import { Textarea } from "@/ui/textarea";
import type { Component } from "./contract";

type Field = Extract<Component, {kind: "input" | "textarea" | "select" | "checkbox" | "radio_group" | "switch" |
  "slider" | "combobox" | "input_otp" | "calendar" | "date_picker" | "label"}>;

function date(value: string | null | undefined): Date | undefined {
  if (!value || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return undefined;
  const parsed = new Date(value + "T00:00:00");
  return Number.isFinite(parsed.getTime()) ? parsed : undefined;
}

export function ViewField({component}: {component: Field}) {
  const fallbackId = useId();
  if (component.kind === "label") return <Label htmlFor={component.html_for ?? undefined}>{component.text}</Label>;
  if (component.kind === "calendar") {
    const marked = (component.marked ?? []).flatMap(day => date(day) ?? []);
    return <Calendar mode="single" defaultMonth={date(component.month + "-01")}
      selected={date(component.selected)} modifiers={{marked}} modifiersClassNames={{marked: "bg-accent"}} />;
  }
  const id = component.name || fallbackId;
  const label = component.label || component.name;
  return <div className="grid min-w-0 gap-2">
    <Label htmlFor={id}>{label}</Label>
    <Widget key={JSON.stringify(component)} component={component} id={id} label={label} />
  </div>;
}

function Widget({component: item, id, label}: {
  component: Exclude<Field, {kind: "calendar" | "label"}>; id: string; label: string;
}) {
  switch (item.kind) {
    case "input": return <Input id={id} defaultValue={item.value} placeholder={item.placeholder ?? undefined} />;
    case "textarea": return <Textarea id={id} defaultValue={item.value} rows={Math.min(30, item.rows ?? 3)} />;
    case "select": return <NativeSelect id={id} defaultValue={item.value} className="w-full">
      {item.options?.map(option => <NativeSelectOption key={option} value={option}>{option}</NativeSelectOption>)}
    </NativeSelect>;
    case "checkbox": return <Checkbox id={id} defaultChecked={item.checked} />;
    case "switch": return <Switch id={id} defaultChecked={item.checked} />;
    case "radio_group": return <RadioGroup id={id} aria-label={label} defaultValue={item.selected ?? undefined} className="gap-2">
      {item.options?.map((option, index) => <Label key={option} htmlFor={id + "-" + index} className="flex min-h-11 gap-2">
        <RadioGroupItem id={id + "-" + index} value={option} />{option}</Label>)}
    </RadioGroup>;
    case "slider": return <Slider id={id} aria-label={label} min={item.min} max={item.max} defaultValue={item.value} />;
    case "combobox": return <>
      <Input id={id} list={id + "-list"} defaultValue={item.value} />
      <datalist id={id + "-list"}>{item.options?.map(option => <option key={option} value={option} />)}</datalist>
    </>;
    case "input_otp": {
      const length = Math.max(1, Math.min(12, item.length ?? 6));
      return <InputOTP id={id} maxLength={length} defaultValue={item.value}>
        <InputOTPGroup className="max-w-full flex-wrap">{Array.from({length}, (_, index) => <InputOTPSlot key={index} index={index} />)}</InputOTPGroup>
      </InputOTP>;
    }
    case "date_picker": return <Input id={id} type="date" defaultValue={item.value ?? undefined} />;
  }
}
