import { useEffect, useRef, useState } from "react";
import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap, lineNumbers } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { editorColors, languageFor } from "../../components/language";
import { lineToPos } from "../../components/searchLogic";
import { diagramBody, inDocumentFrame } from "./frontMatterLogic";
import { onEditorFontSizeChange } from "../../editorFontSize";
import * as api from "../../ipc/api";
import type { ValidationError } from "../../ipc/types";

/**
 * A CodeMirror editor over one stored diagram file.
 *
 * The same shape as `FileEditor` — one editor, built once, Ctrl+S to save, a
 * dot in the owning view while there are unsaved changes — with one thing
 * added that a plain file does not have: the diagram is checked as it is
 * typed, and what the check says is shown beside the text.
 *
 * # Validation is not a gate on saving, and that is deliberate
 *
 * `arch_write_diagram` writes the file and *then* validates it, returning the
 * problem rather than raising it: `Ok(null)` is saved and valid, `Ok(error)` is
 * **saved and broken**, and only a rejection means nothing was written. This
 * component was built against that contract and the contract was read, not
 * assumed — `src-tauri/src/commands/architecture.rs` calls `store::write`
 * before `mermaid::validate` and its module documentation argues the order at
 * length. So a resolved `ValidationError` here clears the dirty flag exactly
 * as a clean save does, and is reported as a problem *with the saved diagram*,
 * never as a failed save.
 *
 * Refusing the save would be the reflex and it is wrong for a reason specific
 * to Mermaid: source passes through invalid states on the way to every valid
 * one — an arrow typed before its target node exists is invalid and is also
 * how every diagram gets drawn. An editor that would not save those is an
 * editor a person cannot use while they are still working, one crash or one
 * closed window away from losing the drawing. Losing someone's work to protect
 * them from their own half-finished edit is the worse failure.
 *
 * The live check (this component's own `arch_validate` calls) stores nothing
 * and is a diagnostic only.
 *
 * # Every check here is run over the body, never the file
 *
 * A stored diagram opens with a front matter block, and `mermaid::validate`
 * takes the post-front-matter body — `store::parse` is what separates the two,
 * and validating the whole file makes the validator read `---` as a diagram
 * type and refuse it. That is not a hypothetical: it is `DiagramType` on line
 * 1 for *every* file this app has ever written, including the copy "Save a
 * copy…" has just made, while Mermaid strips the block and draws the picture
 * perfectly well one pane above. {@link diagramBody} shows the executed
 * output; it also argues why the stripping is here and not in the command.
 *
 * This applies to the save's verdict too, and that is the one surprise in this
 * file. `arch_write_diagram` validates the exact bytes it was handed, and what
 * it is handed is the file — so its `ValidationError` carries the same
 * artifact. The save therefore takes the file's *own* frame back by re-running
 * the body check over the text that reached the disk. Nothing is dropped: the
 * user is still told about a diagram that was saved broken, and now the
 * message is about the diagram rather than about its front matter.
 */
export interface DiagramEditorProps {
  /**
   * File name including `.md`, as `arch_list_diagrams` reports it.
   *
   * The editor is keyed by this in the owning view, so it never rebinds: a
   * different diagram is a different editor, with its own undo history.
   */
  name: string;
  /** The file exactly as it is on disk, front matter and fence included. */
  initialText: string;
  /** Raised on every transition, so the view can show the unsaved dot. */
  onDirtyChange: (dirty: boolean) => void;
  /**
   * The text that reached the disk.
   *
   * The view re-renders the canvas from it and re-lists the diagrams, because
   * a save can *move* the file: editing a derived diagram promotes it out of
   * the regenerated directory, so the path the list held is no longer right.
   */
  onSaved: (text: string) => void;
}

/**
 * Check one diagram the way the renderer sees it: the body, with the answer
 * put back into the frame of the whole file so a line number still points at
 * the line the user is looking at.
 */
async function checkDiagram(text: string): Promise<ValidationError | null> {
  const { body, offset } = diagramBody(text);
  return inDocumentFrame(await api.archValidate(body), offset);
}

export function DiagramEditor({
  name,
  initialText,
  onDirtyChange,
  onSaved,
}: DiagramEditorProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);

  /** What the last check said about the text — saved or merely typed. */
  const [problem, setProblem] = useState<ValidationError | null>(null);
  /** Set only when the *write* was refused: then nothing is on disk. */
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  /** Cleared by the next edit, so it can never describe text that has moved on. */
  const [savedAt, setSavedAt] = useState<string | null>(null);

  // Read through refs so a changed callback cannot tear the editor down.
  const handlers = useRef({ onDirtyChange, onSaved });
  handlers.current = { onDirtyChange, onSaved };
  const dirty = useRef(false);
  const checkTimer = useRef<number | null>(null);

  useEffect(() => {
    if (!hostRef.current) return;

    const setDirty = (next: boolean) => {
      if (dirty.current === next) return;
      dirty.current = next;
      handlers.current.onDirtyChange(next);
    };

    /**
     * Check the text without storing it, after the typing has paused.
     *
     * Debounced because every keystroke passes through states nobody wants
     * named: a message that appears and vanishes on the way to the closing
     * bracket is noise, and the one that matters is the one still there when
     * the user stops. The result is dropped if the document has moved on since
     * the call was made — an answer about text nobody is looking at any more
     * would be a message pointing at a line that has since shifted.
     */
    const check = (view: EditorView) => {
      if (checkTimer.current !== null) window.clearTimeout(checkTimer.current);
      checkTimer.current = window.setTimeout(() => {
        const asked = view.state.doc.toString();
        checkDiagram(asked)
          .then((result) => {
            if (viewRef.current?.state.doc.toString() !== asked) return;
            setProblem(result);
          })
          .catch((e) => {
            // The check itself failing says nothing about the diagram, so it
            // must not be shown as if it did.
            setSaveError(api.errorMessage(e));
          });
      }, 400);
    };

    const save = (view: EditorView) => {
      const text = view.state.doc.toString();
      setSaving(true);
      api
        .archWriteDiagram(name, text)
        .then(() => {
          // Saved either way — see this module's documentation.
          setDirty(false);
          setSaveError(null);
          setSavedAt(new Date().toLocaleTimeString());
          handlers.current.onSaved(text);

          // The write's own verdict is deliberately not used: it validated the
          // whole file, front matter and all, so it reports `DiagramType` on
          // line 1 for every stored diagram. The same question is asked again
          // about the same text, in the frame the renderer works in. A check
          // that cannot be answered leaves the last one standing rather than
          // announcing something about the diagram it did not learn — and the
          // result is dropped if the buffer has moved on since.
          void checkDiagram(text)
            .then((result) => {
              if (viewRef.current?.state.doc.toString() !== text) return;
              setProblem(result);
            })
            .catch(() => {});
        })
        .catch((e) => setSaveError(api.errorMessage(e)))
        .finally(() => setSaving(false));
      return true;
    };

    const extensions: Extension[] = [
      lineNumbers(),
      history(),
      search({ top: true }),
      highlightSelectionMatches(),
      keymap.of([
        { key: "Mod-s", run: save },
        ...searchKeymap,
        indentWithTab,
        ...defaultKeymap,
        ...historyKeymap,
      ]),
      EditorView.updateListener.of((update) => {
        if (!update.docChanged) return;
        setDirty(true);
        setSavedAt(null);
        check(update.view);
      }),
      EditorView.theme({
        "&": { height: "100%" },
        ".cm-scroller": { overflow: "auto" },
      }),
      // The stored form is Markdown — front matter, prose, one fenced Mermaid
      // block — so the file's own extension picks the mode, and the app's one
      // theme colours it. A second theme for one editor would drift from the
      // other two the first time either changed.
      ...languageFor("diagram.md"),
      ...editorColors,
    ];

    const view = new EditorView({
      state: EditorState.create({ doc: initialText, extensions }),
      parent: hostRef.current,
    });
    viewRef.current = view;

    // What is already on disk may already be broken — an agent wrote some of
    // these files — so the first check happens before anyone types.
    void checkDiagram(initialText)
      .then((result) => {
        if (viewRef.current !== view) return;
        setProblem(result);
      })
      .catch(() => {
        /* An unanswered check is not a problem with the diagram. */
      });

    return () => {
      if (checkTimer.current !== null) window.clearTimeout(checkTimer.current);
      view.destroy();
      viewRef.current = null;
      // The editor is going away with its buffer; whatever it held is no longer
      // this view's unsaved state.
      if (dirty.current) {
        dirty.current = false;
        handlers.current.onDirtyChange(false);
      }
    };
    // `initialText` is the document this editor opens on and deliberately does
    // not re-run it: a re-read while someone is typing would discard the edit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [name]);

  // CodeMirror caches character metrics; a CSS font-size change needs saying
  // out loud (see `editorFontSize.ts`).
  useEffect(() => onEditorFontSizeChange(() => viewRef.current?.requestMeasure()), []);

  /** Put the cursor on the line the validator named. */
  const goToProblem = () => {
    const view = viewRef.current;
    if (!view || !problem) return;
    const line = view.state.doc.line(lineToPos(view.state.doc.lines, problem.line));
    view.dispatch({
      selection: { anchor: line.from },
      effects: EditorView.scrollIntoView(line.from, { y: "center" }),
    });
    view.focus();
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", minHeight: 0 }}>
      <div
        className="toolbar"
        style={{ borderBottom: "1px solid var(--border)", borderTop: "1px solid var(--border)" }}
      >
        <span className="mono" style={{ fontSize: 12 }}>
          {name}
        </span>
        {saving && <span className="spinner" />}
        <span style={{ flex: 1 }} />
        {savedAt !== null && (
          <span className="faint" style={{ fontSize: 11 }}>
            Saved at {savedAt}
          </span>
        )}
        <span className="faint" style={{ fontSize: 11 }}>
          Ctrl+S to save · a diagram that does not render is saved anyway
        </span>
      </div>

      {/* Not `.error`: the file is on disk. This describes the diagram, not the
          save, and calling it an error would tell the user their work was lost
          when it was not. */}
      {problem !== null && (
        <div className="warning" style={{ margin: "8px 8px 0" }}>
          <strong>This diagram will not render as it stands.</strong>{" "}
          <button
            onClick={goToProblem}
            style={{ padding: "0 6px" }}
            title="Put the cursor on the line this refers to"
          >
            Line {problem.line}
          </button>{" "}
          <span className="muted mono" style={{ fontSize: 11 }}>
            {problem.rule}
          </span>
          <div>{problem.detail}</div>
          <div className="faint" style={{ fontSize: 11, marginTop: 4 }}>
            Saving is not blocked by this. Mermaid passes through states like
            this on the way to every finished drawing, so an editor that refused
            them would be one you could not use while drawing.
          </div>
        </div>
      )}

      {/* The one case where nothing reached the disk. */}
      {saveError !== null && (
        <div className="error" style={{ margin: "8px 8px 0" }}>
          <strong>Not saved.</strong> {saveError}
        </div>
      )}

      <div className="editor-area" style={{ flex: 1, minHeight: 0 }}>
        <div className="editor-host" ref={hostRef} />
      </div>
    </div>
  );
}
