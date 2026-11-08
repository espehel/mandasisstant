import { useEffect, useRef } from "react";
import type { Session } from "./sessions";

export type Mic =
  | { kind: "paused" }
  | { kind: "starting" }
  | { kind: "listening" }
  | { kind: "stopping" }
  | { kind: "failed"; message: string };

type SessionScreenProps = {
  session: Session;
  mic: Mic;
  onPlay: () => void;
  onPause: () => void;
};

export default function SessionScreen({ session, mic, onPlay, onPause }: SessionScreenProps) {
  const isListening = mic.kind === "listening";
  const isBusy = mic.kind === "starting" || mic.kind === "stopping";
  const transcriptEnd = useRef<HTMLDivElement>(null);
  const transcriptCount = session.transcripts.length;

  // Keep the newest transcript in view as segments arrive.
  useEffect(() => {
    transcriptEnd.current?.scrollIntoView({ block: "end" });
  }, [transcriptCount]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <main className="flex-1 overflow-y-auto rounded-box bg-base-100">
        <div className="mx-auto max-w-2xl px-6 py-8">
          <div aria-live="polite">
            {transcriptCount > 0 ? (
              <ol className="space-y-4">
                {session.transcripts.map((transcript, index) => (
                  // Transcripts are only ever appended, so an index is a stable key.
                  <li key={index} className="text-2xl leading-relaxed">
                    {transcript}
                  </li>
                ))}
              </ol>
            ) : (
              <p className="text-center text-base-content/60">
                Speak English or Mandarin. Each sentence appears here after a short pause.
              </p>
            )}
          </div>
          {mic.kind === "failed" && (
            <div role="alert" className="alert alert-error alert-soft mt-6">
              {mic.message}
            </div>
          )}
          <div ref={transcriptEnd} />
        </div>
      </main>

      {/* Sits on the window frame, like the header; with the frame's bottom gutter it's as tall. */}
      <footer className="grid h-14 shrink-0 grid-cols-[1fr_auto_1fr] items-center gap-4 px-4 pt-2">
        <div role="status" className="text-sm">
          <MicStatus mic={mic} />
        </div>
        <button
          type="button"
          className="btn btn-circle btn-primary btn-lg"
          aria-label={isListening ? "Pause" : "Play"}
          disabled={isBusy}
          onClick={isListening ? onPause : onPlay}
        >
          {isListening ? <PauseIcon /> : <PlayIcon />}
        </button>
      </footer>
    </div>
  );
}

function MicStatus({ mic }: { mic: Mic }) {
  switch (mic.kind) {
    case "starting":
      return (
        <span className="flex items-center gap-2 text-base-content/60">
          <span className="loading loading-spinner loading-xs" aria-hidden="true" />
          Starting…
        </span>
      );
    case "listening":
      return (
        <span className="flex items-center gap-2">
          <span className="status status-error animate-pulse" aria-hidden="true" />
          Listening
        </span>
      );
    case "stopping":
      return (
        <span className="flex items-center gap-2 text-base-content/60">
          <span className="loading loading-spinner loading-xs" aria-hidden="true" />
          Finishing…
        </span>
      );
    case "paused":
    case "failed":
      return <span className="text-base-content/60">Paused</span>;
  }
}

function PlayIcon() {
  return (
    <svg viewBox="0 0 24 24" className="size-6" fill="currentColor" aria-hidden="true">
      <path d="M8 5.5v13a1 1 0 0 0 1.53.85l10.4-6.5a1 1 0 0 0 0-1.7L9.53 4.65A1 1 0 0 0 8 5.5Z" />
    </svg>
  );
}

function PauseIcon() {
  return (
    <svg viewBox="0 0 24 24" className="size-6" fill="currentColor" aria-hidden="true">
      <rect x="6" y="5" width="4" height="14" rx="1" />
      <rect x="14" y="5" width="4" height="14" rx="1" />
    </svg>
  );
}
