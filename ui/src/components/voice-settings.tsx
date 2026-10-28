// Settings → "Stimme": the OpenAI key (voice features only, with region) and read-aloud
// (provider, model, voice, sample). Read-aloud is off by default.
import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AlertTriangle,
  CheckCircle2,
  ExternalLink,
  KeyRound,
  Loader2,
  PlugZap,
  Volume2,
} from "lucide-react";
import type { TtsConfig } from "@/bindings/TtsConfig";
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
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  api,
  errorMessage,
  OPENAI_EU_BASE_URL,
  q,
  type LlmOverview,
  type TtsOverview,
} from "@/lib/api";

const TTS_PROVIDERS = ["openrouter", "openai", "custom"] as const;

export function VoiceCard() {
  const { t } = useTranslation();
  const llm = useQuery(q.llm());
  const tts = useQuery(q.tts());

  return (
    <Card className="max-w-2xl" data-testid="voice-card">
      <CardHeader>
        <CardTitle className="text-base">{t("settings.voice.title")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4 text-sm">
        {llm.data && <OpenaiKey data={llm.data} />}
        {tts.data && <ReadAloud data={tts.data} />}
      </CardContent>
    </Card>
  );
}

function OpenaiKey({ data }: { data: LlmOverview }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const info = data.providers.find((p) => p.id === "openai")!;
  const current = data.config.providers.openai ?? {
    model: null,
    base_url: null,
  };
  const eu = current.base_url === OPENAI_EU_BASE_URL;
  const [key, setKey] = useState("");
  const refresh = () => void qc.invalidateQueries({ queryKey: ["llm"] });

  const storeKey = useMutation({
    mutationFn: (k: string | null) => api.setLlmKey("openai", k),
    onSuccess: (_, k) => {
      setKey("");
      refresh();
      toast.success(
        k ? t("settings.llm.keySaved") : t("settings.llm.keyRemoved"),
      );
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
  const region = useMutation({
    mutationFn: (toEu: boolean) =>
      api.setLlmConfig({
        ...data.config,
        providers: {
          ...data.config.providers,
          openai: { ...current, base_url: toEu ? OPENAI_EU_BASE_URL : null },
        },
      }),
    onSuccess: refresh,
    onError: (e) => toast.error(errorMessage(e)),
  });
  const check = useMutation({
    mutationFn: () => api.llmModels("openai"),
    onSuccess: (list) =>
      toast.success(t("settings.voice.checkOk", { count: list.length })),
    onError: (e) => toast.error(errorMessage(e)),
  });

  return (
    <section className="space-y-3 rounded-md border p-3" data-testid="openai-key">
      <p className="font-medium">{t("settings.voice.openai")}</p>
      <p className="text-[13px] text-muted-foreground">
        {t("settings.voice.openaiIntro")}
      </p>
      <p className="flex gap-2 rounded-md border border-warning/40 bg-warning/10 p-3 text-[13px]">
        <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" />
        <span>{t("settings.voice.openaiWarning")}</span>
      </p>
      <div className="space-y-1.5">
        <Label htmlFor="openai-key" className="flex items-center gap-2">
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
            id="openai-key"
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
          {t("settings.llm.keyHint")}{" "}
          <button
            type="button"
            className="inline-flex items-center gap-1 underline"
            onClick={() => void openUrl(info.key_url!)}
          >
            {t("settings.llm.keyCreate")} <ExternalLink className="size-3" />
          </button>
        </p>
      </div>
      <div className="flex items-end gap-2">
        <div className="space-y-1.5">
          <Label>{t("settings.voice.region")}</Label>
          <Select
            value={eu ? "eu" : "us"}
            onValueChange={(v) => region.mutate(v === "eu")}
          >
            <SelectTrigger className="w-80">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(["us", "eu"] as const).map((r) => (
                <SelectItem key={r} value={r}>
                  {t(`settings.voice.regions.${r}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <Button
          variant="outline"
          disabled={!info.has_key || check.isPending}
          onClick={() => check.mutate()}
        >
          {check.isPending ? (
            <Loader2 className="size-4 animate-spin" />
          ) : (
            <PlugZap className="size-4" />
          )}
          {t("settings.voice.check")}
        </Button>
      </div>
    </section>
  );
}

function ReadAloud({ data }: { data: TtsOverview }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const config = data.config;
  const [model, setModel] = useState(config.model ?? "");
  const [voice, setVoice] = useState(config.voice ?? "");
  const audio = useRef<HTMLAudioElement | null>(null);
  useEffect(() => () => audio.current?.pause(), []);

  const save = useMutation({
    mutationFn: (c: TtsConfig) => api.setTtsConfig(c),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["llm", "tts"] }),
    onError: (e) => toast.error(errorMessage(e)),
  });
  const set = (patch: Partial<TtsConfig>) => save.mutate({ ...config, ...patch });
  const sample = useMutation({
    mutationFn: api.ttsSample,
    onSuccess: (buf) => {
      audio.current?.pause();
      const url = URL.createObjectURL(new Blob([buf], { type: "audio/mpeg" }));
      const el = new Audio(url);
      el.onended = () => URL.revokeObjectURL(url);
      audio.current = el;
      el.play().catch((e) => toast.error(errorMessage(e)));
    },
    onError: (e) => toast.error(errorMessage(e)),
    // The audit log gains a `llm.tts` entry.
    onSettled: () => void qc.invalidateQueries({ queryKey: ["llm", "spend"] }),
  });
  const custom = config.provider === "custom";
  const defaultModel = data.default_models[config.provider];

  return (
    <section className="space-y-3 rounded-md border p-3" data-testid="read-aloud">
      <p className="font-medium">{t("settings.voice.readAloud")}</p>
      <p className="text-[13px] text-muted-foreground">
        {t("settings.voice.readAloudIntro")}
      </p>
      <label className="flex items-center gap-2">
        <Checkbox
          checked={config.enabled}
          onCheckedChange={(v) => set({ enabled: v === true })}
        />
        {t("settings.voice.enabled")}
      </label>
      <div className="flex flex-wrap gap-3">
        <div className="space-y-1.5">
          <Label>{t("settings.voice.provider")}</Label>
          <Select
            value={config.provider}
            onValueChange={(v) => {
              // Model and voice ids differ per provider.
              setModel("");
              setVoice("");
              set({
                provider: v as TtsConfig["provider"],
                model: null,
                voice: null,
              });
            }}
          >
            <SelectTrigger className="w-72">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {TTS_PROVIDERS.map((p) => (
                <SelectItem key={p} value={p}>
                  {t(`settings.voice.providers.${p}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="min-w-56 flex-1 space-y-1.5">
          <Label htmlFor="tts-model">{t("settings.voice.model")}</Label>
          <Input
            id="tts-model"
            value={model}
            placeholder={
              defaultModel
                ? t("settings.voice.modelDefault", { model: defaultModel })
                : t("settings.stt.modelPlaceholder")
            }
            onChange={(e) => setModel(e.target.value)}
            onBlur={() =>
              model.trim() !== (config.model ?? "") &&
              set({ model: model.trim() || null })
            }
          />
        </div>
      </div>
      <div className="flex items-end gap-2">
        <div className="space-y-1.5">
          <Label>{t("settings.voice.voice")}</Label>
          {custom ? (
            <Input
              className="w-72"
              value={voice}
              placeholder={t("settings.voice.voicePlaceholder")}
              onChange={(e) => setVoice(e.target.value)}
              onBlur={() =>
                voice.trim() !== (config.voice ?? "") &&
                set({ voice: voice.trim() || null })
              }
            />
          ) : (
            <Select
              value={config.voice ?? data.voices[0].id}
              onValueChange={(v) => set({ voice: v })}
            >
              <SelectTrigger className="w-72">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {data.voices.map((v) => (
                  <SelectItem key={v.id} value={v.id}>
                    {v.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        </div>
        <Button
          variant="outline"
          disabled={!!data.problem || sample.isPending || save.isPending}
          onClick={() => sample.mutate()}
        >
          {sample.isPending ? (
            <Loader2 className="size-4 animate-spin" />
          ) : (
            <Volume2 className="size-4" />
          )}
          {sample.isPending
            ? t("settings.voice.sampleRunning")
            : t("settings.voice.sample")}
        </Button>
      </div>
      {data.problem && (
        <p className="flex items-center gap-2 text-destructive">
          <AlertTriangle className="size-4" /> {data.problem}
        </p>
      )}
    </section>
  );
}
