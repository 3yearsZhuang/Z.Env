import * as React from "react";

import { cn } from "../../lib/utils";

const fieldClasses =
  "w-full rounded-lg border border-input bg-card px-3 py-2 text-[13px] text-foreground outline-none transition-colors placeholder:text-muted-foreground/70 focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-ring/40 disabled:cursor-not-allowed disabled:opacity-50";

function Input({
  className,
  ...props
}: React.ComponentProps<"input">) {
  return <input data-slot="input" className={cn(fieldClasses, className)} {...props} />;
}

function Textarea({
  className,
  ...props
}: React.ComponentProps<"textarea">) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(fieldClasses, "font-mono leading-relaxed", className)}
      {...props}
    />
  );
}

export { Input, Textarea };
