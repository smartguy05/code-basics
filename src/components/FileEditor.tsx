import { useEffect, useRef, useState } from "react";
import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap, lineNumbers } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { editorColors, languageFor } from "./language";
import * as api from "../ipc/api";

/**
 * A plain CodeMirror editor over one workspace file.
 *
 * Loads the file once on mount and saves with Ctrl+S (`Mod-s`). Kept mounted
 * while hidden — like the console sessions — so undo history, scroll position
 * and unsaved changes survive switching between file tabs.
 */
export function FileEditor({
  path,
  onDirtyChange,
}: {
  /** Workspace-relative path. A FileEditor is keyed by it and never rebinds. */
  path: string;
  onDirtyChange: (dirty: boolean) => void;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Read through refs so the editor is not torn down when they change.
  const handlers = useRef({ onDirtyChange });
  handlers.current = { onDirtyChange };
  const dirty = useRef(false);

  useEffect(() => {
    let cancelled = false;

    async function build() {
      let content: string;
      try {
        content = await api.fsReadFile(path);
      } catch (e) {
        if (!cancelled) setError(api.errorMessage(e));
        return;
      }
      if (cancelled || !hostRef.current) return;

      const setDirty = (next: boolean) => {
        if (dirty.current === next) return;
        dirty.current = next;
        handlers.current.onDirtyChange(next);
      };

      const save = (view: EditorView) => {
        const text = view.state.doc.toString();
        api
          .fsWriteFile(path, text)
          .then(() => {
            setDirty(false);
            setError(null);
          })
          .catch((e) => setError(api.errorMessage(e)));
        return true;
      };

      const extensions: Extension[] = [
        lineNumbers(),
        history(),
        keymap.of([
          { key: "Mod-s", run: save },
          indentWithTab,
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) setDirty(true);
        }),
        EditorView.theme({
          "&": { height: "100%" },
          ".cm-scroller": { overflow: "auto" },
        }),
        ...languageFor(path),
        ...editorColors,
      ];

      try {
        const view = new EditorView({
          state: EditorState.create({ doc: content, extensions }),
          parent: hostRef.current,
        });
        viewRef.current = view;
        setError(null);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    }

    void build();
    return () => {
      cancelled = true;
      viewRef.current?.destroy();
      viewRef.current = null;
    };
  }, [path]);

  if (error) {
    return (
      <div className="error" style={{ whiteSpace: "pre-wrap" }}>
        {error}
      </div>
    );
  }

  return <div className="editor-host" ref={hostRef} />;
}
