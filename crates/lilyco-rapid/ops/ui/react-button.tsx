import type { ButtonHTMLAttributes } from "react";
import { cn } from "@/lib/utils";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "default" | "secondary" | "ghost" | "destructive";
  size?: "default" | "sm" | "icon";
}

export function Button({ className, variant, size, ...props }: ButtonProps) {
  return <button className={cn("rapid-button", variant, size, className)} {...props} />;
}
