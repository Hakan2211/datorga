import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Link } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { AlertTriangle, Check, Plus, ShieldCheck, Sparkles, X } from "lucide-react";
import type { EntityKind } from "@/bindings/EntityKind";
import type { FactEdit } from "@/bindings/FactEdit";
import type { NoteFact } from "@/bindings/NoteFact";
import type { SectionDef } from "@/bindings/SectionDef";
import { SentLink } from "@/components/autopilot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  api,
  errorMessage,
  q,
  type ExtractPreview,
  type ExtractProgress,
} from "@/lib/api";
import { cn } from "@/lib/utils";
import { HelpLink } from "@/components/help-link";
import { HELP } from "@/lib/help";

const KINDS: EntityKind[] = ["person", "ort", "arbeitgeber", "datum"];
const TOKEN = /(\[(?:PERSON|ORT|ARBEITGEBER|DATUM)_\d+\])/;

/** `mm:ss` from the start of the recording, like the transcript. */
export function clock(ms: number): string {
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const mm = String(Math.floor((s % 3600) / 60)).padStart(2, "0");
  const ss = String(s % 60).padStart(2, "0");
  return h ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

/** Scrolls the transcript sidebar to a cited segment and flashes it. */
function showSegment(segmentId: string) {
  const el = document.querySelector<HTMLElement>(
    `[data-segment-id="${segmentId}"]`,
  );
  if (!el) return;
  el.scrollIntoView({ block: "center", behavior: "smooth" });
  el.classList.add("ring-2", "ring-primary");
  setTimeout(() => el.classList.remove("ring-2", "ring-primary"), 1600);
}

type Draft = { checked: boolean; text: string; section: string; tags: string[] };

/** A Bericht section as a short chip label ("2 Symptomatik und psychischer…"). */
export function tagLabel(s: SectionDef): string {
  const h = s.heading.trim();
  const short = h.length > 30 ? `${h.slice(0, 28).trimEnd()}…` : h;
  return s.number ? `${s.number} ${short}` : short;
}

/**
 * AI-proposed facts for a Sitzungsnotiz. Nothing reaches the note until the therapist picks
 * facts and applies them; each fact links back to the transcript lines it cites.
 */
export function FactDraftCard({
  reportId,
  patientId,
  sessionId,
  sections,
  onApply,
}: {
  reportId: string;
  patientId: string;
  /** The note's session: the autopilot may have proposed these facts ("Was wurde gesendet?"). */
  sessionId: string | null;
  sections: SectionDef[];
  onApply: (edits: FactEdit[]) => Promise<void>;
}) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const llm = useQuery(q.llm());
  const facts = useQuery(q.noteFacts(reportId));
  // Bericht sections facts can be tagged with (none without a treatment case).
  const tagSections = useQuery(q.factTagSections(reportId)).data ?? [];
  const [previewOpen, setPreviewOpen] = useState(false);
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const [applying, setApplying] = useState(false);

  const proposed = useMemo(
    () => facts.data?.filter((f) => f.status === "proposed") ?? [],
    [facts.data],
  );
  const applied = facts.data?.filter((f) => f.status === "applied") ?? [];
  // New proposals start selected, as the model filed them.
  useEffect(() => {
    setDrafts((prev) => {
      const next: Record<string, Draft> = {};
      for (const f of proposed)
        next[f.id] = prev[f.id] ?? {
          checked: true,
          text: f.text,
          section: f.section_key,
          tags: f.bericht ?? [],
        };
      return next;
    });
  }, [proposed]);

  const setFacts = (list: NoteFact[]) =>
    qc.setQueryData(q.noteFacts(reportId).queryKey, list);
  const reject = useMutation({
    mutationFn: (ids: string[]) => api.rejectFacts(reportId, ids),
    onSuccess: setFacts,
    onError: (e) => toast.error(errorMessage(e)),
  });

  const chosen = proposed.filter(
    (f) => drafts[f.id]?.checked && drafts[f.id].text.trim(),
  );
  const apply = async () => {
    setApplying(true);
    try {
      await onApply(
        chosen.map((f) => ({
          id: f.id,
          section_key: drafts[f.id].section,
          text: drafts[f.id].text,
          ...(tagSections.length ? { bericht: drafts[f.id].tags } : {}),
        })),
      );
      void qc.invalidateQueries({ queryKey: ["noteFacts", reportId] });
      toast.success(t("facts.applied", { count: chosen.length }));
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setApplying(false);
    }
  };

  const active = llm.data?.config.active;
  const heading = (key: string) =>
    sections.find((s) => s.key === key)?.heading ?? key;
  const tagHeading = (key: string) => {
    const s = tagSections.find((d) => d.key === key);
    return s ? tagLabel(s) : key;
  };

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-3">
        <CardTitle className="flex items-center gap-2 text-base">
          <Sparkles className="size-4 text-primary" /> {t("facts.title")} <HelpLink slug={HELP.note} />
        </CardTitle>
        <span className="ml-auto" />
        {sessionId && <SentLink sessionId={sessionId} step="extract" />}
        <Button
          size="sm"
          variant={proposed.length ? "outline" : "default"}
          onClick={() => setPreviewOpen(true)}
          disabled={!active}
        >
          {proposed.length || applied.length
            ? t("facts.again")
            : t("facts.propose")}
        </Button>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        {!active ? (
          <p className="text-muted-foreground">
            {t("facts.noProvider")}{" "}
            <Link to="/settings" className="underline">
              {t("facts.settingsLink")}
            </Link>
          </p>
        ) : proposed.length === 0 ? (
          <p className="text-muted-foreground">
            {applied.length ? t("facts.noneOpen") : t("facts.intro")}
          </p>
        ) : (
          <>
            <p className="text-muted-foreground">{t("facts.reviewHint")}</p>
            {sections
              .filter((s) => proposed.some((f) => f.section_key === s.key))
              .map((s) => (
                <div key={s.key} className="space-y-1.5">
                  <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
                    {s.heading}
                  </h3>
                  <ul className="space-y-1.5">
                    {proposed
                      .filter((f) => f.section_key === s.key)
                      .map((f) => (
                        <FactRow
                          key={f.id}
                          fact={f}
                          draft={
                            drafts[f.id] ?? {
                              checked: true,
                              text: f.text,
                              section: f.section_key,
                              tags: f.bericht ?? [],
                            }
                          }
                          sections={sections}
                          tagSections={tagSections}
                          onChange={(d) =>
                            setDrafts((all) => ({ ...all, [f.id]: d }))
                          }
                          onReject={() => reject.mutate([f.id])}
                        />
                      ))}
                  </ul>
                </div>
              ))}
            <div className="flex items-center gap-3 pt-1">
              <Button
                size="sm"
                onClick={() => void apply()}
                disabled={applying || chosen.length === 0}
              >
                <Check /> {t("facts.apply", { count: chosen.length })}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => reject.mutate(proposed.map((f) => f.id))}
                disabled={reject.isPending || applying}
              >
                {t("facts.rejectAll")}
              </Button>
            </div>
          </>
        )}
        {applied.length > 0 && (
          <details className="text-muted-foreground">
            <summary className="cursor-pointer text-xs">
              {t("facts.appliedList", { count: applied.length })}
            </summary>
            <ul className="mt-1.5 space-y-1">
              {applied.map((f) => (
                <li
                  key={f.id}
                  className="flex flex-wrap items-baseline gap-x-2 text-[13px]"
                >
                  <span className="text-foreground">{f.text}</span>
                  <span className="text-xs">· {heading(f.section_key)}</span>
                  {f.bericht && f.bericht.length > 0 && (
                    <span className="text-xs">
                      · {t("facts.tagsShort")}:{" "}
                      {f.bericht.map(tagHeading).join(", ")}
                    </span>
                  )}
                  <Citations fact={f} />
                </li>
              ))}
            </ul>
          </details>
        )}
      </CardContent>
      {previewOpen && (
        <RedactionDialog
          reportId={reportId}
          patientId={patientId}
          previewKey={["extractPreview", reportId]}
          loadPreview={() => api.extractPreview(reportId)}
          progressEvent="extract-progress"
          labels={{
            send: t("facts.preview.send"),
            runLocal: t("facts.preview.runLocal"),
            running: t("facts.progress.running"),
            part: (done, total) => t("facts.progress.chunk", { done, total }),
            parts: (count) => t("facts.preview.chunks", { count }),
          }}
          onRun={async (digest, requestId) => {
            const r = await api.extractRun(reportId, digest, requestId);
            qc.setQueryData(q.noteFacts(reportId).queryKey, r.facts);
            const open = r.facts.filter((f) => f.status === "proposed").length;
            if (r.cancelled) {
              if (r.chunks === 0) toast.info(t("facts.cancelledNothing"));
              else
                toast.success(
                  t("facts.cancelledPartial", {
                    done: r.chunks,
                    total: r.total,
                    count: open,
                  }),
                );
              return;
            }
            toast.success(
              t("facts.done", { count: open }) +
                (r.dropped
                  ? ` ${t("facts.dropped", { count: r.dropped })}`
                  : ""),
            );
          }}
          onClose={() => setPreviewOpen(false)}
        />
      )}
    </Card>
  );
}

function FactRow({
  fact,
  draft,
  sections,
  tagSections,
  onChange,
  onReject,
}: {
  fact: NoteFact;
  draft: Draft;
  sections: SectionDef[];
  tagSections: SectionDef[];
  onChange: (d: Draft) => void;
  onReject: () => void;
}) {
  const { t } = useTranslation();
  const missing = tagSections.filter((s) => !draft.tags.includes(s.key));
  return (
    <li
      className={cn(
        "flex items-start gap-2 rounded-md border px-2 py-1.5",
        !draft.checked && "opacity-60",
      )}
    >
      <Checkbox
        className="mt-2"
        checked={draft.checked}
        onCheckedChange={(v) => onChange({ ...draft, checked: v === true })}
        aria-label={t("facts.select")}
      />
      <div className="flex-1 space-y-1">
        <Textarea
          value={draft.text}
          onChange={(e) => onChange({ ...draft, text: e.target.value })}
          rows={1}
          className="min-h-0 resize-none border-0 px-1 py-1 shadow-none focus-visible:ring-1 field-sizing-content"
        />
        <div className="flex flex-wrap items-center gap-2">
          <Citations fact={fact} />
          <Select
            value={draft.section}
            onValueChange={(section) => onChange({ ...draft, section })}
          >
            <SelectTrigger
              size="sm"
              className="ml-auto h-7 w-auto text-xs"
              aria-label={t("facts.section")}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {sections.map((s) => (
                <SelectItem key={s.key} value={s.key}>
                  {s.heading}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        {tagSections.length > 0 && (
          <div
            className="flex flex-wrap items-center gap-1"
            data-testid="fact-tags"
          >
            <span className="text-xs text-muted-foreground">
              {t("facts.tags")}:
            </span>
            {draft.tags.length === 0 && (
              <span className="text-xs text-muted-foreground italic">
                {t("facts.noTags")}
              </span>
            )}
            {draft.tags.map((key) => {
              const s = tagSections.find((d) => d.key === key);
              return (
                <span
                  key={key}
                  title={s?.heading}
                  className="flex items-center gap-0.5 rounded-full bg-primary/10 py-0.5 pr-0.5 pl-2 text-[11px] text-primary"
                >
                  {s ? tagLabel(s) : key}
                  <button
                    type="button"
                    className="rounded-full p-0.5 hover:bg-primary/20"
                    onClick={() =>
                      onChange({
                        ...draft,
                        tags: draft.tags.filter((k) => k !== key),
                      })
                    }
                    aria-label={t("facts.removeTag", {
                      section: s?.heading ?? key,
                    })}
                  >
                    <X className="size-3" />
                  </button>
                </span>
              );
            })}
            {missing.length > 0 && (
              <Select
                value=""
                onValueChange={(key) =>
                  onChange({ ...draft, tags: [...draft.tags, key] })
                }
              >
                <SelectTrigger
                  size="sm"
                  className="h-6 w-auto gap-1 border-dashed px-2 text-[11px]"
                  aria-label={t("facts.addTagLabel")}
                >
                  <Plus className="size-3" /> {t("facts.addTag")}
                </SelectTrigger>
                <SelectContent position="popper">
                  {missing.map((s) => (
                    <SelectItem key={s.key} value={s.key}>
                      {s.number ? `${s.number}. ` : ""}
                      {s.heading}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </div>
        )}
      </div>
      <Button
        variant="ghost"
        size="icon"
        className="size-7"
        onClick={onReject}
        title={t("facts.reject")}
        aria-label={t("facts.reject")}
      >
        <X className="size-3.5" />
      </Button>
    </li>
  );
}

function Citations({ fact }: { fact: NoteFact }) {
  const { t } = useTranslation();
  return (
    <span className="flex flex-wrap gap-1">
      {fact.citations.map((c) => (
        <Tooltip key={c.segment_id}>
          <TooltipTrigger asChild>
            <button
              type="button"
              onClick={() => showSegment(c.segment_id)}
              className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground tabular-nums hover:bg-primary/15 hover:text-foreground"
              aria-label={t("facts.citation", { time: clock(c.start_ms) })}
            >
              {clock(c.start_ms)}
            </button>
          </TooltipTrigger>
          <TooltipContent className="max-w-sm">{c.text}</TooltipContent>
        </Tooltip>
      ))}
    </span>
  );
}

export type RedactionLabels = {
  send: string;
  runLocal: string;
  running: string;
  /** Progress while request `done` of `total` runs. */
  part: (done: number, total: number) => string;
  /** "in N Teilen" next to the target. */
  parts: (count: number) => string;
};

/**
 * Shows exactly what would be sent, lets the therapist add missed terms, then runs the request.
 * Shared by fact extraction (Sitzungsnotiz) and drafting (Bericht). `onRun` gets the digest of
 * the text shown, so the backend can refuse if it changed since, and the request id that
 * "Abbrechen" and the streamed text refer to.
 */
export function RedactionDialog({
  reportId,
  patientId,
  previewKey,
  loadPreview,
  progressEvent,
  labels,
  onRun,
  onClose,
  children,
  canRun = true,
}: {
  reportId: string;
  patientId: string;
  previewKey: readonly unknown[];
  loadPreview: () => Promise<ExtractPreview>;
  progressEvent: "extract-progress" | "draft-progress" | "summary-progress" | "gaps-progress";
  labels: RedactionLabels;
  onRun: (digest: string, requestId: string) => Promise<void>;
  onClose: () => void;
  /** Extra controls above the preview (e.g. an instruction). */
  children?: ReactNode;
  canRun?: boolean;
}) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const preview = useQuery({
    queryKey: previewKey,
    queryFn: loadPreview,
    gcTime: 0,
  });
  const [term, setTerm] = useState("");
  const [kind, setKind] = useState<EntityKind>("person");
  const [progress, setProgress] = useState<ExtractProgress | null>(null);
  const [requestId, setRequestId] = useState<string | null>(null);
  const [received, setReceived] = useState(0);
  const [cancelling, setCancelling] = useState(false);

  const refresh = () => void qc.invalidateQueries({ queryKey: previewKey });
  const add = useMutation({
    mutationFn: () => api.addPseudoTerm(patientId, kind, term),
    onSuccess: () => {
      setTerm("");
      refresh();
    },
    onError: (e) => toast.error(errorMessage(e)),
  });
  const remove = useMutation({
    mutationFn: (id: string) => api.removePseudoTerm(id),
    onSuccess: refresh,
    onError: (e) => toast.error(errorMessage(e)),
  });
  const run = useMutation({
    mutationFn: (v: { digest: string; id: string }) => onRun(v.digest, v.id),
    onSuccess: onClose,
    onError: (e) => {
      setProgress(null);
      setRequestId(null);
      setCancelling(false);
      toast.error(errorMessage(e));
    },
  });
  const start = (digest: string) => {
    const id = crypto.randomUUID();
    setRequestId(id);
    setReceived(0);
    setCancelling(false);
    run.mutate({ digest, id });
  };
  const cancel = () => {
    if (!requestId) return;
    setCancelling(true);
    api.llmCancel(requestId).catch((e) => toast.error(errorMessage(e)));
  };

  useEffect(() => {
    if (!requestId) return;
    const off = api.onLlmDelta(requestId, (text) =>
      setReceived((n) => n + text.length),
    );
    return () => void off.then((f) => f());
  }, [requestId]);

  useEffect(() => {
    const off = listen<ExtractProgress>(progressEvent, (e) => {
      if (e.payload.report_id === reportId) setProgress(e.payload);
    });
    return () => void off.then((f) => f());
  }, [reportId, progressEvent]);

  const data = preview.data;
  const local = data?.provider?.id === "local";
  const busy = run.isPending;

  return (
    <Dialog open onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent className="max-h-[90vh] grid-rows-[auto_minmax(0,1fr)_auto] sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldCheck className="size-5 text-primary" />{" "}
            {t("facts.preview.title")}
          </DialogTitle>
          <DialogDescription>
            {local ? t("facts.preview.introLocal") : t("facts.preview.intro")}
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 space-y-4 overflow-y-auto pr-1 text-sm">
          {children}
          {preview.isError && (
            <p className="text-destructive">{errorMessage(preview.error)}</p>
          )}
          {data && (
            <>
              <p className="flex flex-wrap items-center gap-2">
                <span className="text-muted-foreground">
                  {t("facts.preview.target")}
                </span>
                <Badge variant="secondary">
                  {data.provider?.label ?? t("facts.preview.noProvider")}
                </Badge>
                {data.model && (
                  <span className="font-mono text-xs">{data.model}</span>
                )}
                {data.messages.length > 1 && (
                  <span className="text-muted-foreground">
                    · {labels.parts(data.messages.length)}
                  </span>
                )}
              </p>
              {data.provider?.us_intermediary && (
                <p className="flex items-start gap-2 rounded-md bg-warning/15 px-3 py-2 text-[13px]">
                  <AlertTriangle className="mt-0.5 size-4 shrink-0" />{" "}
                  {t(`settings.llm.warning.${data.provider.id}`)}
                </p>
              )}

              <section className="space-y-1.5">
                <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
                  {t("facts.preview.tokens")}
                </h3>
                {data.tokens.length === 0 ? (
                  <p className="text-muted-foreground">
                    {t("facts.preview.noTokens")}
                  </p>
                ) : (
                  <ul className="flex flex-wrap gap-1.5">
                    {data.tokens.map((tk) => (
                      <li
                        key={tk.token}
                        className="rounded bg-muted px-2 py-0.5 text-[13px]"
                      >
                        <span className="font-mono text-primary">
                          {tk.token}
                        </span>{" "}
                        = {tk.original}
                      </li>
                    ))}
                  </ul>
                )}
              </section>

              <section className="space-y-1.5">
                <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
                  {t("facts.preview.outgoing")}
                </h3>
                <p className="text-[13px] text-muted-foreground">
                  {t("facts.preview.selectHint")}
                </p>
                <div
                  className="max-h-72 space-y-3 overflow-y-auto rounded-md border bg-muted/30 p-3 font-mono text-[12.5px] leading-relaxed whitespace-pre-wrap"
                  onMouseUp={() => {
                    const sel =
                      document.getSelection()?.toString().trim() ?? "";
                    if (sel && sel.length < 80 && !TOKEN.test(sel))
                      setTerm(sel);
                  }}
                  data-testid="outgoing"
                >
                  {data.messages.map((m, i) => (
                    <div key={i}>
                      {m.split(TOKEN).map((part, j) =>
                        j % 2 === 1 ? (
                          <mark
                            key={j}
                            className="rounded bg-primary/15 px-0.5 text-primary"
                          >
                            {part}
                          </mark>
                        ) : (
                          part
                        ),
                      )}
                    </div>
                  ))}
                </div>
              </section>

              <section className="space-y-2">
                <h3 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
                  {t("facts.preview.terms")}
                </h3>
                <p className="text-[13px] text-muted-foreground">
                  {t("facts.preview.termsHint")}
                </p>
                <form
                  className="flex gap-2"
                  onSubmit={(e) => {
                    e.preventDefault();
                    if (term.trim()) add.mutate();
                  }}
                >
                  <Input
                    value={term}
                    onChange={(e) => setTerm(e.target.value)}
                    placeholder={t("facts.preview.termPlaceholder")}
                    className="flex-1"
                  />
                  <Select
                    value={kind}
                    onValueChange={(v) => setKind(v as EntityKind)}
                  >
                    <SelectTrigger className="w-40">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {KINDS.map((k) => (
                        <SelectItem key={k} value={k}>
                          {t(`facts.kind.${k}`)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Button
                    type="submit"
                    variant="outline"
                    disabled={!term.trim() || add.isPending || busy}
                  >
                    {t("facts.preview.addTerm")}
                  </Button>
                </form>
                {data.terms.length > 0 && (
                  <ul className="flex flex-wrap gap-1.5">
                    {data.terms.map((tm) => (
                      <li
                        key={tm.id}
                        className="flex items-center gap-1 rounded-full border py-0.5 pr-1 pl-2.5 text-[13px]"
                      >
                        {tm.text}{" "}
                        <span className="text-xs text-muted-foreground">
                          · {t(`facts.kind.${tm.kind}`)}
                        </span>
                        <button
                          type="button"
                          className="rounded-full p-0.5 hover:bg-muted"
                          onClick={() => remove.mutate(tm.id)}
                          disabled={busy}
                          aria-label={t("facts.preview.removeTerm", {
                            term: tm.text,
                          })}
                        >
                          <X className="size-3" />
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </section>
            </>
          )}
        </div>

        <DialogFooter className="items-center gap-3">
          {busy && (
            <span
              className="mr-auto text-sm text-muted-foreground"
              role="status"
            >
              {progress?.stage === "loading"
                ? t("facts.progress.loading")
                : progress && progress.total > 1
                  ? labels.part(
                      Math.min(progress.done + 1, progress.total),
                      progress.total,
                    )
                  : labels.running}
              {received > 0 &&
                ` · ${t("facts.progress.received", { count: received })}`}
            </span>
          )}
          <Button
            variant="ghost"
            onClick={busy ? cancel : onClose}
            disabled={busy && cancelling}
          >
            {busy && cancelling
              ? t("facts.progress.cancelling")
              : t("common.cancel")}
          </Button>
          <Button
            onClick={() => data && start(data.digest)}
            disabled={
              !canRun ||
              !data?.provider ||
              !data.model ||
              preview.isFetching ||
              busy ||
              add.isPending
            }
          >
            <Sparkles /> {local ? labels.runLocal : labels.send}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
