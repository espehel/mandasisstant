import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Status =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "recording" }
  | { kind: "transcribing" }
  | { kind: "done"; transcript: string }
  | { kind: "error"; message: string };

export default function App() {
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  const isRecording = status.kind === "recording";
  const isBusy = status.kind === "loading" || status.kind === "transcribing";

  async function startRecording() {
    setStatus({ kind: "loading" });
    try {
      await invoke("start_listening");
      setStatus({ kind: "recording" });
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
    }
  }

  async function stopRecording() {
    setStatus({ kind: "transcribing" });
    try {
      const transcript = await invoke<string>("stop_listening");
      setStatus({ kind: "done", transcript });
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
    }
  }

  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-8 p-8">
      <button
        type="button"
        className={`btn btn-xl btn-wide ${isRecording ? "btn-error" : "btn-primary"}`}
        disabled={isBusy}
        onClick={isRecording ? stopRecording : startRecording}
      >
        {isRecording ? "Stop" : "Record"}
      </button>
      <section aria-live="polite" className="min-h-24 w-full max-w-2xl text-center">
        <StatusMessage status={status} />
      </section>
    </main>
  );
}

function StatusMessage({ status }: { status: Status }) {
  switch (status.kind) {
    case "idle":
      return <p className="text-base-content/60">Press Record and speak English or Mandarin.</p>;
    case "loading":
      return (
        <p className="flex items-center justify-center gap-2 text-base-content/60">
          <span className="loading loading-spinner loading-sm" aria-hidden="true" />
          Loading speech model…
        </p>
      );
    case "recording":
      return (
        <p className="flex items-center justify-center gap-2">
          <span className="status status-error animate-pulse" aria-hidden="true" />
          Listening… Press Stop when you're done.
        </p>
      );
    case "transcribing":
      return (
        <p className="flex items-center justify-center gap-2 text-base-content/60">
          <span className="loading loading-spinner loading-sm" aria-hidden="true" />
          Transcribing…
        </p>
      );
    case "done":
      return status.transcript ? (
        <p className="text-2xl leading-relaxed">{status.transcript}</p>
      ) : (
        <p className="text-base-content/60">No speech recognized.</p>
      );
    case "error":
      return (
        <div role="alert" className="alert alert-error alert-soft">
          {status.message}
        </div>
      );
  }
}
