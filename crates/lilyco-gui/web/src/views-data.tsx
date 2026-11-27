import { useState } from "react";
import type { Component } from "./contract";
import { useText } from "./i18n";
import { Button } from "@/ui/button";
import { Table, TableBody, TableCaption, TableCell, TableHead, TableHeader, TableRow } from "@/ui/table";

export function DataTable({ component }: { component: Extract<Component, {kind: "table"}> }) {
  const t = useText();
  const [sort, setSort] = useState<{index: number; asc: boolean}>();
  const rows = component.rows ?? [];
  const visible = sort ? [...rows].sort((a, b) =>
    (a[sort.index] ?? "").localeCompare(b[sort.index] ?? "", undefined, {numeric: true}) * (sort.asc ? 1 : -1)) : rows;
  return <div className="min-w-0 overflow-x-auto" role="region" aria-label={component.caption ?? t("table")} tabIndex={0}>
    <Table data-view-kind="table">
      {component.caption && <TableCaption>{component.caption}</TableCaption>}
      <TableHeader><TableRow>{(component.columns ?? []).map((column, index) =>
        <TableHead key={index} scope="col" aria-sort={sort?.index === index ? sort.asc ? "ascending" : "descending" : undefined}>
          {component.sortable ? <Button size="sm" variant="ghost" onClick={() => setSort({index, asc: sort?.index !== index || !sort.asc})}>
            {column}{sort?.index === index && <span aria-hidden="true">{sort.asc ? "↑" : "↓"}</span>}
          </Button> : column}
        </TableHead>)}</TableRow></TableHeader>
      <TableBody>{visible.map((row, index) => <TableRow key={index}>
        {row.map((cell, at) => <TableCell key={at}>{cell}</TableCell>)}
      </TableRow>)}</TableBody>
    </Table>
  </div>;
}

export function ChartView({component}: {component: Extract<Component, {kind: "chart"}>}) {
  const t = useText();
  const points = component.series ?? [];
  const scale = points.reduce((peak, value) => Math.max(peak, Math.abs(value)), 1);
  if (component.chart_kind === "line" && points.length > 0) {
    const polyline = points.map((value, index) =>
      `${(index / Math.max(1, points.length - 1)) * 96 + 2},${50 - (value / scale) * 44}`).join(" ");
    return <figure className="grid gap-2">
      <svg viewBox="0 0 100 100" preserveAspectRatio="none" role="img" aria-label={t("chart")} className="h-32 w-full text-primary">
        <polyline points={polyline} stroke="currentColor" strokeWidth="2" vectorEffect="non-scaling-stroke" fill="none" />
      </svg>
      <figcaption className="text-sm text-muted-foreground">{points.map((value, index) => `${component.labels?.[index] ?? index + 1}: ${value}`).join(" · ")}</figcaption>
    </figure>;
  }
  return <ul className="grid gap-2" aria-label={t("chart")}>{points.map((value, index) =>
    <li key={index} className="grid gap-1">
      <div className="flex justify-between gap-2 text-sm"><span>{component.labels?.[index] ?? index + 1}</span><span>{value}</span></div>
      <div className="h-1.5 rounded-full bg-primary" aria-hidden="true" style={{width: `${Math.abs(value) / scale * 100}%`}} />
    </li>)}</ul>;
}
