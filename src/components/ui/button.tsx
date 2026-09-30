import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "../../lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-lg text-[13px] font-medium transition-[color,background-color,border-color,filter] outline-none focus-visible:ring-2 focus-visible:ring-ring/60 disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default: "bg-primary text-primary-foreground font-semibold hover:brightness-110",
        secondary: "border border-border bg-muted text-foreground hover:border-primary/60",
        ghost:
          "border border-border bg-transparent text-muted-foreground hover:text-foreground hover:border-primary/60",
        destructive:
          "border border-border bg-transparent text-destructive hover:border-destructive hover:bg-destructive/10",
        dashed:
          "border border-dashed border-border bg-transparent text-primary hover:bg-primary/10",
      },
      size: {
        default: "h-9 px-4",
        sm: "h-7 rounded-md px-2.5 text-xs",
        icon: "h-9 w-9",
      },
    },
    defaultVariants: {
      variant: "secondary",
      size: "default",
    },
  },
);

function Button({
  className,
  variant,
  size,
  asChild = false,
  ...props
}: React.ComponentProps<"button"> & VariantProps<typeof buttonVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot : "button";
  return (
    <Comp
      data-slot="button"
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Button, buttonVariants };
