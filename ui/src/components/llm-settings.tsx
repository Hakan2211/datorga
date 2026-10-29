// Settings → "KI-Entwürfe": provider, API key (OS keychain), endpoint, model, connection test;
// for the local provider: hardware, recommendation and model downloads instead.
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { openUrl } from "@tauri-apps/plugin-opener";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  CheckCircle2,
  Cpu,
  ExternalLink,
  KeyRound,
  Loader2,
  PlugZap,
  RefreshCw,
  ShieldCheck,
  Sparkles,
} from "lucide-react";
import type { LlmConfig } from "@/bindings/LlmConfig";
import type { ProviderId } from "@/bindings/ProviderId";
import type { RemoteModel } from "@/bindings/RemoteModel";
import type { Fit } from "@/bindings/Fit";
import type { Hardware } from "@/bindings/Hardware";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ModelPicker } from "@/components/model-picker";
import { ModelRow } from "@/components/model-row";
import { KeyGuideButton } from "@/components/key-guide";
import { HelpLink } from "@/components/help-link";
import { HELP } from "@/lib/help";
import { api, errorMessage, q, type LlmOverview } from "@/lib/api";

const OFF = "off";

export function LlmCard() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const llm = useQuery(q.llm());
  const save = useMutation({
    mutationFn: (config: LlmConfig) => api.setLlmConfig(config),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["llm"] }),
    onError: (e) => toast.error(errorMessage(e)),
  });
  const data = llm.data;
  const active = data?.config.active ?? null;

  return (
    <Card className="max-w-2xl" data-tour="llm-settings">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          {t("settings.llm.title")} <HelpLink slug={HELP.ai} />
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-4 text-sm">
        <p className="flex gap-2 text-muted-foreground">
          <ShieldCheck className="mt-0.5 size-4 shrink-0 text-success" />{" "}
          {t("settings.llm.intro")}
        </p>
        {data && (
          <div className="space-y-1.5">
            <Label>{t("settings.llm.provider")}</Label>
            <Select
              value={active ?? OFF}
              onValueChange={(v) =>
                save.mutate({
                  ...data.config,
                  active: v === OFF ? null : (v as ProviderId),
                })
              }
            >
              <SelectTrigger className="w-80">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={OFF}>{t("settings.llm.off")}</SelectItem>
                {data.providers.filter((p) => p.drafting).map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    {p.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}
        {data && active === "local" && (
          <LocalForm data={data} onSave={(c) => save.mutate(c)} />
        )}
        {data && active && active !== "local" && (
          <ProviderForm
            key={active}
            data={data}
            id={active}
            onSave={(c) => save.mutate(c)}
          />
        )}
      </CardContent>
    </Card>
  );
}

function ProviderForm({
  data,
  id,
  onSave,
}: {
  data: LlmOverview;
  id: ProviderId;
  onSave: (c: LlmConfig) => void;
}) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const info = data.providers.find((p) => p.id === id)!;
  const current = data.config.providers[id] ?? { model: null, base_url: null };
  const [key, setKey] = useState("");
  const [baseUrl, setBaseUrl] = useState(current.base_url ?? "");
  const [model, setModel] = useState(current.model ?? "");
  const [models, setModels] = useState<RemoteModel[]>([]);
  const refresh = () => void qc.invalidateQueries({ queryKey: ["llm"] });

  const update = (patch: Partial<typeof current>) =>
    onSave({
      ...data.config,
      providers: { ...data.config.providers, [id]: { ...current, ...patch } },
    });

  const storeKey = useMutation({
    mutationFn: (k: string | null) => api.setLlmKey(id, k),
    onSuccess: (_, k) => {
      setKey("");
      refresh();
      void qc.invalidateQueries({ queryKey: ["balance"] });
      toast.success(
        k ? t("settings.llm.keySaved") : t("settings.llm.keyRemoved"),
      );
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
  const load = useMutation({
    mutationFn: () => api.llmModels(id),
    onSuccess: (list) => {
      setModels(list);
      toast.success(t("settings.llm.modelsLoaded", { count: list.length }));
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
  const test = useMutation({
    mutationFn: () => api.testLlm(id),
    onSuccess: (r) =>
      toast.success(
        t("settings.llm.testOk", { model: r.model, ms: r.latency_ms }),
      ),
    onError: (e) => toast.error(errorMessage(e)),
  });
  const ready = (!info.needs_key || info.has_key) && !!current.model;

  return (
    <div className="space-y-4">
      {info.us_intermediary && (
        <p className="flex gap-2 rounded-md border border-warning/40 bg-warning/10 p-3">
          <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" />
          <span>{t(`settings.llm.warning.${id}`)}</span>
        </p>
      )}

      <div className="space-y-1.5">
        <Label htmlFor="llm-key" className="flex items-center gap-2">
          <KeyRound className="size-4" /> {t("settings.llm.key")}
          {info.has_key && (
            <Badge
              variant="secondary"
              className="bg-success/15 font-normal text-success"
            >
              <CheckCircle2 className="size-3" /> {t("settings.llm.keyStored")}
            </Badge>
          )}
        </Label>
        <div className="flex gap-2">
          <Input
            id="llm-key"
            type="password"
            autoComplete="off"
            value={key}
            placeholder={info.has_key ? "••••••••" : "sk-…"}
            onChange={(e) => setKey(e.target.value)}
          />
          <Button
            variant="outline"
            disabled={!key.trim() || storeKey.isPending}
            onClick={() => storeKey.mutate(key)}
          >
            {t("common.save")}
          </Button>
          {info.has_key && (
            <Button
              variant="ghost"
              disabled={storeKey.isPending}
              onClick={() => storeKey.mutate(null)}
            >
              {t("settings.llm.keyRemove")}
            </Button>
          )}
        </div>
        <p className="text-xs text-muted-foreground">
          {t("settings.llm.keyHint")}
          {info.key_url && (
            <>
              {" "}
              <button
                type="button"
                className="inline-flex items-center gap-1 underline"
                onClick={() => void openUrl(info.key_url!)}
              >
                {t("settings.llm.keyCreate")}{" "}
                <ExternalLink className="size-3" />
              </button>
            </>
          )}
          {(id === "openrouter" || id === "requesty") && (
            <>
              {" · "}
              <KeyGuideButton provider={id} />
            </>
          )}
        </p>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="llm-base">{t("settings.llm.baseUrl")}</Label>
        <div className="flex gap-2">
          <Input
            id="llm-base"
            value={baseUrl}
            placeholder={info.default_base_url || "https://…/v1"}
            onChange={(e) => setBaseUrl(e.target.value)}
          />
          <Button
            variant="outline"
            disabled={baseUrl === (current.base_url ?? "")}
            onClick={() => update({ base_url: baseUrl.trim() || null })}
          >
            {t("common.save")}
          </Button>
        </div>
        {info.default_base_url && (
          <p className="text-xs text-muted-foreground">
            {t("settings.llm.baseUrlHint")}
          </p>
        )}
      </div>

      {id === "openrouter" && (
        <div className="space-y-1.5">
          <Label>{t("settings.llm.model")}</Label>
          <ModelPicker
            value={current.model}
            onSelect={(m) => {
              setModel(m);
              update({ model: m });
            }}
          />
        </div>
      )}

      <ModelField advanced={id === "openrouter"}>
        <div className="space-y-1.5">
          <Label htmlFor="llm-model">
            {t(
              id === "openrouter"
                ? "settings.llm.modelOwn"
                : "settings.llm.model",
            )}
          </Label>
          <div className="flex gap-2">
            <Input
              id="llm-model"
              list="llm-models"
              value={model}
              placeholder={t("settings.llm.modelPlaceholder")}
              onChange={(e) => setModel(e.target.value)}
            />
            <datalist id="llm-models">
              {models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name ?? m.id}
                  {m.context_length
                    ? ` · ${Math.round(m.context_length / 1000)}k`
                    : ""}
                </option>
              ))}
            </datalist>
            <Button
              variant="outline"
              disabled={model.trim() === (current.model ?? "")}
              onClick={() => update({ model: model.trim() || null })}
            >
              {t("common.save")}
            </Button>
            <Button
              variant="ghost"
              disabled={load.isPending || (info.needs_key && !info.has_key)}
              onClick={() => load.mutate()}
              aria-label={t("settings.llm.loadModels")}
              title={t("settings.llm.loadModels")}
            >
              <RefreshCw className={load.isPending ? "animate-spin" : ""} />
            </Button>
          </div>
        </div>
      </ModelField>

      <Button
        variant="outline"
        disabled={!ready || test.isPending}
        onClick={() => test.mutate()}
      >
        <PlugZap />{" "}
        {test.isPending ? t("settings.llm.testing") : t("settings.llm.test")}
      </Button>
    </div>
  );
}

/** On OpenRouter the free model field is for experts: folded away under "Erweitert". */
function ModelField({
  advanced,
  children,
}: {
  advanced: boolean;
  children: React.ReactNode;
}) {
  const { t } = useTranslation();
  if (!advanced) return <>{children}</>;
  return (
    <details className="rounded-md border px-3 py-2">
      <summary className="cursor-pointer text-sm">
        {t("settings.llm.advanced")}
      </summary>
      <div className="mt-2">{children}</div>
    </details>
  );
}

const FIT_CLASS: Record<Fit, string> = {
  gpu: "bg-success/15 text-success",
  partial: "bg-success/10 text-foreground",
  cpu: "bg-warning/15 text-foreground",
  too_big: "bg-destructive/10 text-destructive",
};

const gib = (n: number) => Math.round(n / 2 ** 30);

function hardwareLine(
  t: (k: string, o?: Record<string, unknown>) => string,
  hw: Hardware,
): string {
  const parts = [t("settings.local.ram", { gb: gib(hw.ram_bytes) })];
  for (const g of hw.gpus) {
    parts.push(
      g.vram_bytes >= 2 ** 30
        ? t("settings.local.gpu", { name: g.name, gb: gib(g.vram_bytes) })
        : g.name,
    );
  }
  parts.push(t("settings.local.threads", { count: hw.cpu_threads }));
  return parts.join(" · ");
}

/** Provider "local": the machine, the recommended model, downloads and the server state. */
function LocalForm({
  data,
  onSave,
}: {
  data: LlmOverview;
  onSave: (c: LlmConfig) => void;
}) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const refresh = () => void qc.invalidateQueries({ queryKey: ["local"] });
  const test = useMutation({
    mutationFn: () => api.testLlm("local"),
    onSuccess: (r) =>
      toast.success(t("settings.local.testOk", { ms: r.latency_ms })),
    onError: (e) => toast.error(errorMessage(e)),
    onSettled: refresh,
  });
  const local = useQuery(q.local(test.isPending));
  const act = useMutation({
    mutationFn: (f: () => Promise<void>) => f(),
    onSuccess: refresh,
    onError: (e) => toast.error(errorMessage(e)),
  });
  const chosen = data.config.providers.local?.model ?? null;
  const choose = (id: string) =>
    onSave({
      ...data.config,
      providers: {
        ...data.config.providers,
        local: { model: id, base_url: null },
      },
    });
  const importFile = async (id: string) => {
    const path = await open({
      multiple: false,
      filters: [{ name: "GGUF", extensions: ["gguf"] }],
    });
    if (typeof path === "string")
      act.mutate(() => api.importLocalModel(id, path));
  };
  const o = local.data;
  if (!o) return null;
  const rec = o.recommendation;
  const recLabel = o.models.find((m) => m.id === rec.model)?.label;
  const chosenInstalled = o.models.some((m) => m.id === chosen && m.installed);

  return (
    <div className="space-y-4">
      {!o.engine && (
        <p className="flex items-center gap-2 text-destructive">
          <AlertTriangle className="size-4" />{" "}
          {t("settings.local.engineMissing")}
        </p>
      )}
      <p>{t("settings.local.intro")}</p>
      <p className="flex gap-2 text-muted-foreground">
        <Cpu className="mt-0.5 size-4 shrink-0" />
        <span>
          {t("settings.local.machine")}: {hardwareLine(t, o.hardware)}
        </span>
      </p>
      <p
        className={
          "flex gap-2 rounded-md border p-3 " +
          (!rec.model || rec.experimental
            ? "border-warning/40 bg-warning/10"
            : "border-success/40 bg-success/10")
        }
      >
        {rec.model ? (
          <Sparkles className="mt-0.5 size-4 shrink-0 text-success" />
        ) : (
          <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" />
        )}
        <span>
          {rec.model
            ? t("settings.local.recommended", { model: recLabel })
            : t("settings.local.noneFits")}
          {rec.experimental && <> {t("settings.local.experimentalHint")}</>}
        </span>
      </p>

      <ul className="divide-y rounded-md border">
        {o.models.map((m) => (
          <ModelRow
            key={m.id}
            title={m.label}
            bytes={m.bytes}
            description={t(`settings.local.models.${m.id}`)}
            badges={
              <>
                {m.id === rec.model && (
                  <Badge variant="secondary" className="font-normal">
                    {t("settings.local.recommendedBadge")}
                  </Badge>
                )}
                <Badge
                  variant="secondary"
                  className={"font-normal " + FIT_CLASS[m.fit]}
                >
                  {t(`settings.local.fit.${m.fit}`)}
                </Badge>
                {m.experimental && (
                  <Badge variant="outline" className="font-normal">
                    {t("settings.local.experimental")}
                  </Badge>
                )}
              </>
            }
            installed={m.installed}
            transfer={m.transfer}
            active={chosen === m.id}
            busy={act.isPending}
            onUse={() => choose(m.id)}
            onDownload={() => act.mutate(() => api.downloadLocalModel(m.id))}
            onImport={() => void importFile(m.id)}
            onDelete={() => act.mutate(() => api.deleteLocalModel(m.id))}
          />
        ))}
      </ul>

      <div className="flex flex-wrap items-center gap-3">
        <Button
          variant="outline"
          disabled={!o.engine || !chosenInstalled || test.isPending}
          onClick={() => test.mutate()}
        >
          <PlugZap />{" "}
          {test.isPending
            ? t("settings.local.testing")
            : t("settings.local.test")}
        </Button>
        {o.loading ? (
          <span className="flex items-center gap-2 text-muted-foreground">
            <Loader2 className="size-4 animate-spin" />{" "}
            {t("settings.local.loading")}
          </span>
        ) : o.loaded ? (
          <>
            <span className="text-muted-foreground">
              {t("settings.local.loaded", {
                model:
                  o.models.find((m) => m.id === o.loaded)?.label ?? o.loaded,
              })}
            </span>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => act.mutate(api.unloadLocal)}
              disabled={act.isPending}
            >
              {t("settings.local.unload")}
            </Button>
          </>
        ) : null}
      </div>
      {o.engine && (
        <p className="text-xs text-muted-foreground">
          {t("settings.local.engine", { engine: o.engine })}
        </p>
      )}
    </div>
  );
}
