import { useEffect, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import AppHeader from "./AppHeader";
import SessionScreen, { type Mic } from "./SessionScreen";
import WelcomeScreen from "./WelcomeScreen";
import { loadSessions, saveSessions, type Session } from "./sessions";

export default function App() {
  const [sessions, setSessions] = useState(loadSessions);
  const [openSessionId, setOpenSessionId] = useState<string | null>(null);
  // Always refers to the open session: leaving a session closes the microphone first.
  const [mic, setMic] = useState<Mic>({ kind: "paused" });
  const [saveError, setSaveError] = useState<string | null>(null);
  const openSession = sessions.find((session) => session.id === openSessionId);

  useEffect(() => {
    try {
      saveSessions(sessions);
      setSaveError(null);
    } catch (error) {
      setSaveError(`Couldn't save sessions: ${String(error)}`);
    }
  }, [sessions]);

  async function listen(sessionId: string) {
    setMic({ kind: "starting" });
    // Appends by id, so the trailing transcript lands in its session even after leaving it.
    const onTranscript = new Channel<string>((transcript) => {
      setSessions((current) =>
        current.map((session) =>
          session.id === sessionId
            ? { ...session, transcripts: [...session.transcripts, transcript] }
            : session,
        ),
      );
    });
    try {
      await invoke("start_listening", { onTranscript });
      setMic({ kind: "listening" });
    } catch (error) {
      setMic({ kind: "failed", message: String(error) });
    }
  }

  /** Returns whether the microphone closed without an error. */
  async function pause() {
    setMic({ kind: "stopping" });
    try {
      await invoke("stop_listening");
      setMic({ kind: "paused" });
      return true;
    } catch (error) {
      setMic({ kind: "failed", message: String(error) });
      return false;
    }
  }

  async function startSession() {
    const session: Session = { id: crypto.randomUUID(), startedAt: Date.now(), transcripts: [] };
    setSessions((current) => [session, ...current]);
    setOpenSessionId(session.id);
    await listen(session.id);
  }

  async function leaveSession() {
    // Stay if stopping fails, so its error remains visible.
    if (mic.kind === "listening" && !(await pause())) {
      return;
    }
    setMic({ kind: "paused" });
    setOpenSessionId(null);
  }

  return (
    // The window frame: the header and the gutters around each screen's sheets.
    <div className="flex h-screen flex-col bg-base-200 px-2 pb-2">
      <AppHeader
        session={openSession}
        homeDisabled={mic.kind === "starting" || mic.kind === "stopping"}
        onHome={leaveSession}
      />
      {openSession ? (
        <SessionScreen
          session={openSession}
          mic={mic}
          onPlay={() => listen(openSession.id)}
          onPause={pause}
        />
      ) : (
        <WelcomeScreen sessions={sessions} onStart={startSession} onOpen={setOpenSessionId} />
      )}
      {saveError && (
        <div className="toast toast-top toast-center">
          <div role="alert" className="alert alert-error">
            {saveError}
          </div>
        </div>
      )}
    </div>
  );
}
