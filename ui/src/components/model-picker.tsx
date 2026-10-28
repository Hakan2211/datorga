// OpenRouter model choice for therapists who don't know models: three tiers with one tested
// model each, a list of the other tested models, and what was actually billed so far.
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { CheckCircle2, Star } from "lucide-react";
import type { CloudModel } from "@/bindings/CloudModel";
import type { Tier } from "@/bindings/Tier";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { q } from "@/lib/api";
import { cn } from "@/lib/utils";

const TIERS: Tier[] = ["guenstig", "ausgewogen", "beste"];

/** Small amounts read better in cents: "0,4 ct", from 1 $ on "1,20 $". */
export function money(usd: number): string {
  if (usd < 1)
    return `${(usd * 100).toLocaleString("de-DE", { maximumFractionDigits: usd < 0.1 ? 1 : 0 })} ct`;
  return `${usd.toLocaleString("de-DE", { minimumFractionDigits: 2, maximumFractionDigits: 2 })} $`;
}

function Stars({ n }: { n: number }) {
  const { t } = useTranslation();
  return (
    <span
      className="inline-flex"
      aria-label={t("settings.llm.picker.quality", { n })}
      title={t("settings.llm.picker.quality", { n })}
    >
      {[1, 2, 3, 4, 5].map((i) => (
        <Star
          key={i}
          className={cn(
            "size-3.5",
            i <= n ? "fill-primary text-primary" : "text-muted-foreground/40",
          )}
        />
      ))}
    </span>
  );
}

export function ModelPicker({
  value,
  onSelect,
}: {
  value: string | null;
  onSelect: (id: string) => void;
}) {
  const { t } = useTranslation();
  const catalog = useQuery(q.llmCatalog());
  const spend = useQuery(q.llmSpend());
  if (!catalog.data) return null;
  const { models, recommended } = catalog.data;
  const byTier = (tier: Tier) => models.find((m) => m.tier === tier);
  const others = models.filter((m) => !m.tier);
  const custom = value && !models.some((m) => m.id === value);

  return (
    <div className="space-y-3">
      <div className="grid gap-2 sm:grid-cols-3">
        {TIERS.map((tier) => {
          const m = byTier(tier);
          if (!m) return null;
          const selected = value === m.id;
          return (
            <button
              key={tier}
              type="button"
              onClick={() => onSelect(m.id)}
              aria-pressed={selected}
              className={cn(
                "flex flex-col gap-1.5 rounded-md border bg-card p-3 text-left transition-colors hover:border-ring",
                selected && "border-primary ring-1 ring-primary",
              )}
            >
              <span className="flex items-center justify-between gap-2">
                <span className="font-medium">
                  {t(`settings.llm.picker.tier.${tier}`)}
                </span>
                {selected ? (
                  <CheckCircle2 className="size-4 text-primary" />
                ) : (
                  tier === recommended && (
                    <Badge
                      variant="secondary"
                      className="bg-success/15 font-normal text-success"
                    >
                      {t("settings.llm.picker.recommended")}
                    </Badge>
                  )
                )}
              </span>
              <span className="text-xs text-muted-foreground">
                {t(`settings.llm.picker.tierHint.${tier}`)}
              </span>
              <span className="text-xs">
                {m.label} · {m.vendor}
              </span>
              <Stars n={m.quality} />
              <span className="text-xs text-muted-foreground">
                {t("settings.llm.picker.perNote", { cost: money(m.cost_note) })}
                <br />
                {t("settings.llm.picker.perBericht", {
                  cost: money(m.cost_bericht),
                })}
              </span>
              <span className="text-sm font-medium">
                {t("settings.llm.picker.perMonth", {
                  cost: money(m.cost_month),
                })}
              </span>
            </button>
          );
        })}
      </div>

      <details className="rounded-md border px-3 py-2">
        <summary className="cursor-pointer text-sm">
          {t("settings.llm.picker.others")}
        </summary>
        <ul className="mt-2 divide-y">
          {others.map((m) => (
            <OtherRow
              key={m.id}
              m={m}
              selected={value === m.id}
              onSelect={onSelect}
            />
          ))}
        </ul>
      </details>

      {custom && (
        <p className="text-xs text-muted-foreground">
          {t("settings.llm.picker.custom", { model: value })}
        </p>
      )}
      <p className="text-xs text-muted-foreground">
        {t("settings.llm.picker.footnote")}
      </p>
      {spend.data && spend.data.total.requests > 0 && (
        <p className="text-xs">
          {t("settings.llm.picker.spent", {
            month: money(spend.data.month.usd),
            requests: spend.data.month.requests,
            total: money(spend.data.total.usd),
          })}
        </p>
      )}
    </div>
  );
}

function OtherRow({
  m,
  selected,
  onSelect,
}: {
  m: CloudModel;
  selected: boolean;
  onSelect: (id: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <li className="flex items-center gap-3 py-2 text-sm">
      <span className="min-w-0 flex-1">
        {m.label}{" "}
        <span className="text-xs text-muted-foreground">· {m.vendor}</span>
      </span>
      <Stars n={m.quality} />
      <span className="w-24 text-right text-xs text-muted-foreground">
        {t("settings.llm.picker.perMonthShort", { cost: money(m.cost_month) })}
      </span>
      <Button
        size="sm"
        variant={selected ? "secondary" : "outline"}
        disabled={selected}
        onClick={() => onSelect(m.id)}
      >
        {selected
          ? t("settings.llm.picker.chosen")
          : t("settings.llm.picker.choose")}
      </Button>
    </li>
  );
}
