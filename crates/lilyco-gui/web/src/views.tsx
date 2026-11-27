import { Fragment, useId, useState, type ReactNode } from "react";
import type { Component } from "./contract";
import { useText } from "./i18n";
import { DataTable, ChartView } from "./views-data";
import { ViewMenu, ViewOverlay } from "./views-overlays";
import { ViewField } from "./views-fields";
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from "@/ui/accordion";
import { Alert, AlertDescription, AlertTitle } from "@/ui/alert";
import { AspectRatio } from "@/ui/aspect-ratio";
import { Avatar, AvatarFallback, AvatarImage } from "@/ui/avatar";
import { Badge } from "@/ui/badge";
import { Breadcrumb, BreadcrumbItem, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator } from "@/ui/breadcrumb";
import { Button } from "@/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/ui/collapsible";
import { HoverCard, HoverCardContent, HoverCardTrigger } from "@/ui/hover-card";
import { Progress } from "@/ui/progress";
import { ScrollArea } from "@/ui/scroll-area";
import { Separator } from "@/ui/separator";
import { Skeleton } from "@/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/ui/tabs";
import { Toggle } from "@/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/ui/toggle-group";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/ui/tooltip";

export type Actions = {
  canSelect: (command: string) => boolean;
  select: (command: string) => void;
};

export function ViewNode({component, actions}: {component: Component; actions: Actions}) {
  const t = useText();
  const id = useId();
  const render = (items: Component[] | undefined) => <div className="grid min-w-0 gap-3">{items?.map((item, index) =>
    <ViewNode key={index} component={item} actions={actions} />)}</div>;
  if (!component || typeof component !== "object" || typeof component.kind !== "string") return null;
  switch (component.kind) {
    case "card": return <Card size="sm">{(component.title || component.description) && <CardHeader>
      {component.title && <CardTitle>{component.title}</CardTitle>}{component.description && <CardDescription>{component.description}</CardDescription>}
    </CardHeader>}<CardContent>{render(component.children)}</CardContent></Card>;
    case "section": return <section className="grid min-w-0 gap-2">
      {component.title && <h3 className="font-medium">{component.title}</h3>}{render(component.children)}
    </section>;
    case "separator": return <div className="grid gap-1">{component.label && <span className="text-sm">{component.label}</span>}<Separator /></div>;
    case "scroll_area": return <ScrollArea className="max-h-80" style={component.max_height ? {maxHeight: component.max_height} : undefined}>{render(component.children)}</ScrollArea>;
    case "aspect_ratio": return <AspectRatio ratio={component.ratio > 0 ? component.ratio : 1}>{render(component.children)}</AspectRatio>;
    case "collapsible": return <Collapsible defaultOpen={component.open}>
      <CollapsibleTrigger render={<Button variant="outline" />}>{component.title}</CollapsibleTrigger>
      <CollapsibleContent className="pt-2">{render(component.children)}</CollapsibleContent>
    </Collapsible>;
    case "accordion": return <Accordion defaultValue={(component.items ?? []).filter(item => item.open).map(item => item.id)}>
      {component.items?.map(item => <AccordionItem key={item.id} value={item.id}>
        <AccordionTrigger>{item.title}</AccordionTrigger><AccordionContent>{item.body}</AccordionContent>
      </AccordionItem>)}
    </Accordion>;
    case "tabs": return <Tabs key={component.active ?? ""} defaultValue={component.items?.some(item => item.id === component.active)
      ? component.active : component.items?.[0]?.id}>
      <TabsList className="h-auto max-w-full flex-wrap">{component.items?.map(item =>
        <TabsTrigger key={item.id} value={item.id}>{item.label}</TabsTrigger>)}</TabsList>
      {component.items?.map(item => <TabsContent key={item.id} value={item.id}>{render(item.children)}</TabsContent>)}
    </Tabs>;
    case "breadcrumb": return <Breadcrumb aria-label={t("breadcrumb")}><BreadcrumbList>{component.items?.map((item, index) =>
      <Fragment key={index}>{index > 0 && <BreadcrumbSeparator />}<BreadcrumbItem><BreadcrumbPage>{item}</BreadcrumbPage></BreadcrumbItem></Fragment>)}</BreadcrumbList></Breadcrumb>;
    case "pagination": return <p className="text-sm text-muted-foreground">{t("page_of", {page: component.page, total: component.pages})}</p>;
    case "sidebar":
    case "navigation_menu": return <nav aria-label={t("sidebar")} className="flex flex-wrap gap-1">
      {component.items?.map(item => <Badge key={item.id} variant={item.id === component.active ? "default" : "outline"}>{item.label}</Badge>)}
    </nav>;
    case "resizable": return <SplitView ratio={component.ratio ?? 0.5} part={[render(component.start), render(component.end)]} />;
    case "carousel": return <SlideView slides={(component.items ?? []).map(item =>
      <div key={item.title}>{item.title && <h3>{item.title}</h3>}{render(item.children)}</div>)} initial={component.index ?? 0} />;
    case "progress": return <div className="grid gap-2">
      {component.label && <span id={id} className="text-sm">{component.label}</span>}
      <Progress aria-labelledby={component.label ? id : undefined} aria-label={component.label ? undefined : t("progress")}
        value={component.total && component.current != null ? Math.min(100, Math.max(0, component.current / component.total * 100)) : null} />
    </div>;
    case "badge": return <Badge variant={component.tone === "danger" ? "destructive" : "secondary"}>{component.text}</Badge>;
    case "alert": return <Alert variant={component.tone === "danger" ? "destructive" : "default"}>
      <AlertTitle>{component.title}</AlertTitle>{component.description && <AlertDescription>{component.description}</AlertDescription>}
    </Alert>;
    case "toast": return <Alert role="status" variant={component.tone === "danger" ? "destructive" : "default"}><AlertDescription>{component.text}</AlertDescription></Alert>;
    case "skeleton": return <div className="grid gap-2">{Array.from({length: Math.min(50, component.lines ?? 3)}, (_, index) =>
      <Skeleton key={index} className="h-3" />)}</div>;
    case "avatar": return <Avatar>
      {component.src?.startsWith("/") && !component.src.startsWith("//") && <AvatarImage src={component.src} alt={component.name} />}
      <AvatarFallback aria-label={component.name}>{component.name.slice(0, 2)}</AvatarFallback>
    </Avatar>;
    case "key_value": return <dl className="grid gap-2">{component.items?.map((item, index) =>
      <div key={index} className="flex flex-wrap justify-between gap-2 text-sm wrap-anywhere"><dt>{item.key}</dt><dd>{item.value}</dd></div>)}</dl>;
    case "table": return <DataTable component={component} />;
    case "chart": return <ChartView component={component} />;
    case "log": return <pre role="log" aria-label={t("output")} tabIndex={0} className="max-h-60 overflow-auto rounded bg-muted p-3 text-xs whitespace-pre-wrap wrap-anywhere">
      {component.lines?.map(line => `[${line.level}] ${line.text}`).join("\n") || t("no_log")}
    </pre>;
    case "result": return <Alert variant={component.ok ? "default" : "destructive"}>
      <AlertTitle>{component.title}</AlertTitle>
      {component.value != null && <AlertDescription><pre className="whitespace-pre-wrap wrap-anywhere">{JSON.stringify(component.value, null, 2)}</pre></AlertDescription>}
    </Alert>;
    case "button": return <Button variant={component.variant === "primary" ? "default" : component.variant === "danger" ? "destructive" : component.variant}
      disabled={component.disabled || !actions.canSelect(component.action)} onClick={() => actions.select(component.action)}>{component.label}</Button>;
    case "toggle": return <Toggle defaultPressed={component.pressed}>{component.label}</Toggle>;
    case "toggle_group": return <ToggleGroup multiple defaultValue={component.selected} className="flex-wrap">
      {component.options?.map(option => <ToggleGroupItem key={option} value={option}>{option}</ToggleGroupItem>)}
    </ToggleGroup>;
    case "tooltip": return <Tooltip><TooltipTrigger render={<div tabIndex={0} />}>{render(component.children)}</TooltipTrigger><TooltipContent>{component.text}</TooltipContent></Tooltip>;
    case "hover_card": return <HoverCard>
      <HoverCardTrigger render={<Button variant="outline" />}>{component.trigger}</HoverCardTrigger>
      <HoverCardContent><h3 className="font-medium">{component.title}</h3><p>{component.body}</p></HoverCardContent>
    </HoverCard>;
    case "dialog": case "sheet": case "drawer": case "alert_dialog": case "popover":
      return <ViewOverlay component={component} children={component.kind === "alert_dialog" ? undefined : render(component.children)} />;
    case "dropdown_menu": case "context_menu": case "menubar": case "command":
      return <ViewMenu component={component} actions={actions} />;
    case "input": case "textarea": case "select": case "checkbox": case "radio_group": case "switch":
    case "slider": case "combobox": case "input_otp": case "calendar": case "date_picker": case "label":
      return <ViewField component={component} />;
    case "form": return <section className="grid gap-2">{component.submit && <h3 className="font-medium">{component.submit}</h3>}{render(component.children)}</section>;
    default: return <Alert>
      <AlertDescription>{t("unknown_component", {kind: String((component as {kind?: unknown}).kind)})}
        <pre className="whitespace-pre-wrap wrap-anywhere">{JSON.stringify(component, null, 2)}</pre>
      </AlertDescription>
    </Alert>;
  }
}

function SplitView({ratio, part}: { ratio: number; part: [ReactNode, ReactNode] }) {
  const [value, setValue] = useState(Math.round(ratio * 100));
  const t = useText();
  return <div className="grid min-w-0 gap-2">
    <input type="range" aria-label={t("split_ratio")} min={20} max={80} value={value} onChange={event => setValue(event.target.valueAsNumber)} />
    <div className="flex flex-wrap gap-3 [&>*]:grow [&>*]:min-w-44">
      <div style={{flexBasis: `calc(${value}% - 1rem)`}}>{part[0]}</div>
      <div style={{flexBasis: `calc(${100 - value}% - 1rem)`}}>{part[1]}</div>
    </div>
  </div>;
}

function SlideView({slides, initial}: {slides: ReactNode[]; initial: number}) {
  const [index, setIndex] = useState(initial);
  const t = useText();
  if (!slides.length) return null;
  const current = Math.min(Math.max(0, index), slides.length - 1);
  return <div className="grid min-w-0 gap-3">
    {slides[current]}
    <div className="flex flex-wrap items-center gap-2">
      <Button variant="outline" disabled={!current} onClick={() => setIndex(current - 1)}>{t("previous")}</Button>
      <span role="status" className="text-sm">{t("page_of", {page: current + 1, total: slides.length})}</span>
      <Button variant="outline" disabled={current + 1 >= slides.length} onClick={() => setIndex(current + 1)}>{t("next")}</Button>
    </div>
  </div>;
}
