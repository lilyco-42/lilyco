import type { ReactNode } from "react";
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle, AlertDialogTrigger } from "@/ui/alert-dialog";
import { Button } from "@/ui/button";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/ui/command";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from "@/ui/context-menu";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from "@/ui/dialog";
import { Drawer, DrawerContent, DrawerHeader, DrawerTitle, DrawerTrigger } from "@/ui/drawer";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/ui/dropdown-menu";
import { Menubar, MenubarContent, MenubarItem, MenubarMenu, MenubarTrigger } from "@/ui/menubar";
import { Popover, PopoverContent, PopoverTrigger } from "@/ui/popover";
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetTrigger } from "@/ui/sheet";
import type { Component } from "./contract";
import { useText } from "./i18n";
import type { Actions } from "./views";

type Overlay = Extract<Component, {kind: "dialog" | "sheet" | "drawer" | "alert_dialog" | "popover"}>;
export function ViewOverlay({component: item, children}: {component: Overlay; children: ReactNode}) {
  switch (item.kind) {
    case "dialog": return <Dialog defaultOpen={item.open} key={String(item.open)}>
      <DialogTrigger render={<Button variant="outline" />}>{item.title}</DialogTrigger>
      <DialogContent className="max-h-[85dvh] overflow-auto">
        <DialogHeader><DialogTitle>{item.title}</DialogTitle>{item.description && <DialogDescription>{item.description}</DialogDescription>}</DialogHeader>{children}
      </DialogContent>
    </Dialog>;
    case "sheet": return <Sheet defaultOpen={item.open} key={String(item.open)}>
      <SheetTrigger render={<Button variant="outline" />}>{item.title}</SheetTrigger>
      <SheetContent side={item.side} className="overflow-auto">
        <SheetHeader><SheetTitle>{item.title}</SheetTitle></SheetHeader><div className="px-4">{children}</div>
      </SheetContent>
    </Sheet>;
    case "drawer": return <Drawer defaultOpen={item.open} key={String(item.open)}>
      <DrawerTrigger render={<Button variant="outline" />}>{item.title}</DrawerTrigger>
      <DrawerContent><DrawerHeader><DrawerTitle>{item.title}</DrawerTitle></DrawerHeader>
        <div className="max-h-[65dvh] overflow-auto p-4">{children}</div></DrawerContent>
    </Drawer>;
    case "alert_dialog": return <AlertDialog defaultOpen={item.open} key={String(item.open)}>
      <AlertDialogTrigger render={<Button variant="outline" />}>{item.title}</AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader><AlertDialogTitle>{item.title}</AlertDialogTitle>
          {item.description && <AlertDialogDescription>{item.description}</AlertDialogDescription>}</AlertDialogHeader>
        <AlertDialogFooter><AlertDialogCancel>{item.cancel}</AlertDialogCancel><AlertDialogAction>{item.confirm}</AlertDialogAction></AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>;
    case "popover": return <Popover defaultOpen={item.open} key={String(item.open)}>
      <PopoverTrigger render={<Button variant="outline" />}>{item.trigger}</PopoverTrigger>
      <PopoverContent className="max-h-[65dvh] overflow-auto">{children}</PopoverContent>
    </Popover>;
  }
}

type Menu = Extract<Component, {kind: "dropdown_menu" | "context_menu" | "menubar" | "command"}>;
export function ViewMenu({component: item, actions}: {component: Menu; actions: Actions}) {
  const t = useText();
  switch (item.kind) {
    case "dropdown_menu": return <DropdownMenu>
      <DropdownMenuTrigger render={<Button variant="outline" />}>{item.trigger}</DropdownMenuTrigger>
      <DropdownMenuContent>{item.items?.map(entry => <DropdownMenuItem key={entry.id}
        disabled={!entry.action || !actions.canSelect(entry.action)}
        variant={entry.danger ? "destructive" : "default"}
        onClick={() => { if (entry.action) actions.select(entry.action); }}>{entry.label}</DropdownMenuItem>)}</DropdownMenuContent>
    </DropdownMenu>;
    case "context_menu": return <ContextMenu>
      <ContextMenuTrigger render={<Button variant="outline" aria-label={t("view_action")} />}>{t("view_action")}</ContextMenuTrigger>
      <ContextMenuContent>{item.items?.map(entry => <ContextMenuItem key={entry.id}
        disabled={!entry.action || !actions.canSelect(entry.action)}
        variant={entry.danger ? "destructive" : "default"}
        onClick={() => { if (entry.action) actions.select(entry.action); }}>{entry.label}</ContextMenuItem>)}</ContextMenuContent>
    </ContextMenu>;
    case "menubar": return <Menubar className="h-auto flex-wrap">{item.menus?.map((group, index) =>
      <MenubarMenu key={index}><MenubarTrigger>{group.label}</MenubarTrigger><MenubarContent>
        {group.items?.map(entry => <MenubarItem key={entry.id}
          disabled={!entry.action || !actions.canSelect(entry.action)}
          variant={entry.danger ? "destructive" : "default"}
          onClick={() => { if (entry.action) actions.select(entry.action); }}>{entry.label}</MenubarItem>)}
      </MenubarContent></MenubarMenu>)}</Menubar>;
    case "command": return <Command className="border">
      <CommandInput defaultValue={item.query ?? undefined} placeholder={t("search_commands")} />
      <CommandList><CommandEmpty>{t("no_commands")}</CommandEmpty><CommandGroup>
        {item.items?.map((entry, index) => <CommandItem key={index} value={entry.label}
          disabled={!actions.canSelect(entry.action)} onSelect={() => actions.select(entry.action)}>
          {entry.label}{entry.hint && <span className="ml-auto text-xs text-muted-foreground">{entry.hint}</span>}
        </CommandItem>)}
      </CommandGroup></CommandList>
    </Command>;
  }
}
