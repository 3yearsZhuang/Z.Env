import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "../../lib/utils";

const badgeVariants = cva(
  "inline-flex items-center rounded-full px-2 py-0.5 text-[10.5px] font-medium leading-4",
  {
    variants: {
      variant: {
        default: "border border-border bg-muted text-muted-foreground",
        primary: "border border-primary/40 bg-primary/10 text-primary",
        success: "border border-success/40 bg-success/10 text-success",
        destructive: "border border-destructive/40 bg-destructive/10 text-destructive",
        warning: "border border-warning/40 bg-warning/10 text-warning",
      },
    },
    defaultVariants: { variant: "default" },
  },
);

function Badge({
  className,
  variant,
  ...props
}: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants>) {
  return (
    <span data-slot="badge" className={cn(badgeVariants({ variant, className }))} {...props} />
  );
}

export { Badge, badgeVariants };
