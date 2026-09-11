import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "../ipc/api";
import { useDockEntry } from "./DockContext";
import { dockId } from "./dockLogic";
import { useFocusEntry, useFocusOffset } from "./focusOrderContext";
import {
  clampPanelPosition,
  loadPanelLayout,
  savePanelLayout,
  type PanelLayout,
  type PanelSize,
} from "./reviewLayoutLogic";
import { REDIS_LAYOUT_KEY } from "./redisPanelLogic";
import {
  candidateBlockedReason,
  connectionLabel,
  discoveredToSaved,
  mergeScan,
  ttlLabel,
} from "./redisLogic";
import type {
  RedisConnectionView,
  RedisDiscovery,
  RedisKeyInfo,
  RedisValue,
} from "../ipc/types";

/**
 * The Redis plugin panel: manage connections (discovered from appsettings/
 * secrets or typed), browse the keyspace, and view/edit values. A floating panel
 * on the `reviewLayoutLogic` recipe, like `SqlPanel`. Every decision it makes is
 * in the tested `redisLogic`/`redisPanelLogic`; this is the rendering shell.
 *
 * The two agent-consent toggles (expose, allow writes) each move through their
 * own backend verb, so a save never raises consent. The panel itself may always
 * write — `allowWrites` gates only the MCP path.
 */
export function RedisPanel({
  root,
  workspaceName,
  onClose,
  restoreRequest,
}: {
  root: string;
  workspaceName: string;
  onClose: () => void;
  restoreRequest: number;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const [minimized, setMinimized] = useState(false);
  useEffect(() => setMinimized(false), [restoreRequest]);

  const focusKey = dockId(root, "redis");
  const raise = useFocusEntry(focusKey);
  const offset = useFocusOffset(focusKey);
  const restore = useCallback(() => setMinimized(false), []);
  useEffect(() => {
    if (!minimized) raise();
  }, [minimized, raise]);
  useDockEntry(
    minimized
      ? { id: focusKey, scope: root, label: "Redis", order: 0, status: workspaceName, onRestore: restore }
      : null,
  );

  const [pos, setPos] = useState<PanelLayout | undefined>(() => {
    const saved = loadPanelLayout(localStorage, REDIS_LAYOUT_KEY);
    return saved.left !== undefined && saved.top !== undefined ? saved : undefined;
  });
  const [size] = useState<PanelSize | undefined>(() => {
    const saved = loadPanelLayout(localStorage, REDIS_LAYOUT_KEY);
    return saved.width !== undefined && saved.height !== undefined
      ? { width: saved.width, height: saved.height }
      : undefined;
  });

  // --- data state ---
  const [connections, setConnections] = useState<RedisConnectionView[]>([]);
  const [discovery, setDiscovery] = useState<RedisDiscovery | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [match, setMatch] = useState("");
  const [keys, setKeys] = useState<RedisKeyInfo[]>([]);
  const [cursor, setCursor] = useState<string>("0");
  const [complete, setComplete] = useState(true);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [value, setValue] = useState<RedisValue | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refreshConnections = useCallback(async () => {
    try {
      setConnections(await api.redisListConnections());
    } catch (e) {
      setError(String(e));
    }
  }, []);
  useEffect(() => {
    void refreshConnections();
  }, [refreshConnections]);

  const run = async (f: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const scanFrom = (from: string) =>
    run(async () => {
      const page = await api.redisScanKeys(selected!, match || null, from, 300);
      setKeys((prev) => (from === "0" ? page.keys : mergeScan(prev, page)));
      setCursor(page.cursor);
      setComplete(page.complete);
    });

  const selectConnection = (id: string) => {
    setSelected(id);
    setKeys([]);
    setCursor("0");
    setComplete(true);
    setSelectedKey(null);
    setValue(null);
  };

  const openKey = (key: string) =>
    run(async () => {
      setSelectedKey(key);
      setValue(await api.redisGetKey(selected!, key));
    });

  const reloadValue = () => {
    if (selected && selectedKey) void openKey(selectedKey);
  };

  const onHeaderPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button, input, textarea, select")) return;
    const panel = panelRef.current;
    if (!panel) return;
    const rect = panel.getBoundingClientRect();
    const grabX = e.clientX - rect.left;
    const grabY = e.clientY - rect.top;
    const header = e.currentTarget;
    header.setPointerCapture(e.pointerId);
    let latest: PanelLayout = { left: rect.left, top: rect.top };
    let moved = false;
    const onMove = (ev: PointerEvent) => {
      moved = true;
      const s = { width: panel.offsetWidth, height: panel.offsetHeight };
      latest = clampPanelPosition(
        { left: ev.clientX - grabX, top: ev.clientY - grabY },
        s,
        { width: window.innerWidth, height: window.innerHeight },
      );
      setPos(latest);
    };
    const onUp = () => {
      header.releasePointerCapture(e.pointerId);
      header.removeEventListener("pointermove", onMove);
      header.removeEventListener("pointerup", onUp);
      if (moved) savePanelLayout(localStorage, latest, REDIS_LAYOUT_KEY);
    };
    header.addEventListener("pointermove", onMove);
    header.addEventListener("pointerup", onUp);
  };

  const activeConn = connections.find((c) => c.id === selected) ?? null;

  return (
    <div
      className="review-panel redis-panel"
      hidden={minimized}
      ref={panelRef}
      onPointerDownCapture={raise}
      style={
        {
          ...(pos ? { left: pos.left, top: pos.top, right: "auto", bottom: "auto" } : {}),
          ...(size ? { width: size.width, height: size.height } : {}),
          "--cb-stack": offset,
        } as React.CSSProperties
      }
    >
      <div className="review-header" onPointerDown={onHeaderPointerDown}>
        <strong>Redis</strong>
        <span className="faint" style={{ fontSize: 12 }}>
          {workspaceName}
        </span>
        <span style={{ flex: 1 }} />
        <button onClick={() => setMinimized(true)} title="Minimize">
          —
        </button>
        <button onClick={onClose} title="Close">
          ✕
        </button>
      </div>

      <div className="redis-body">
        <div className="redis-connections">
          <div className="redis-section-head">
            <span>Connections</span>
            <button
              disabled={busy}
              onClick={() =>
                void run(async () => setDiscovery(await api.redisDiscover(root)))
              }
              title="Scan appsettings, user secrets and .env for Redis connections"
            >
              Discover
            </button>
          </div>
          <ul className="redis-conn-list">
            {connections.map((c) => (
              <li
                key={c.id}
                className={c.id === selected ? "active" : ""}
                onClick={() => selectConnection(c.id)}
              >
                <span className="redis-conn-name">{connectionLabel(c)}</span>
                <label title="Expose to agents over MCP (read)">
                  <input
                    type="checkbox"
                    checked={c.exposeToAgents}
                    onClick={(e) => e.stopPropagation()}
                    onChange={(e) =>
                      void run(async () =>
                        setConnections(await api.redisSetExposeToAgents(c.id, e.target.checked)),
                      )
                    }
                  />
                  expose
                </label>
                <label title="Allow agents to write (needs expose too)">
                  <input
                    type="checkbox"
                    checked={c.allowWrites}
                    onClick={(e) => e.stopPropagation()}
                    onChange={(e) =>
                      void run(async () =>
                        setConnections(await api.redisSetAllowWrites(c.id, e.target.checked)),
                      )
                    }
                  />
                  writes
                </label>
                <button
                  title="Delete this saved connection"
                  onClick={(e) => {
                    e.stopPropagation();
                    void run(async () => setConnections(await api.redisDeleteConnection(c.id)));
                  }}
                >
                  ✕
                </button>
              </li>
            ))}
          </ul>

          {discovery && (
            <div className="redis-discovered">
              <div className="redis-section-head">
                <span>Discovered</span>
              </div>
              {discovery.candidates.length === 0 && <p className="faint">None found.</p>}
              <ul className="redis-conn-list">
                {discovery.candidates.map((cand) => {
                  const blocked = candidateBlockedReason(cand);
                  return (
                    <li key={cand.id}>
                      <span className="redis-conn-name" title={cand.origin}>
                        {cand.name}
                      </span>
                      {blocked ? (
                        <span className="faint" title={blocked}>
                          unresolved
                        </span>
                      ) : (
                        <button
                          onClick={() =>
                            void run(async () =>
                              setConnections(
                                await api.redisSaveConnection(
                                  discoveredToSaved(cand, root, Date.now()),
                                ),
                              ),
                            )
                          }
                        >
                          Add
                        </button>
                      )}
                    </li>
                  );
                })}
              </ul>
              {discovery.warnings.map((w, i) => (
                <p key={i} className="warning">
                  {w}
                </p>
              ))}
            </div>
          )}

          {activeConn && (
            <div className="redis-keys">
              <div className="redis-scan">
                <input
                  placeholder="match (e.g. user:*)"
                  value={match}
                  onChange={(e) => setMatch(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && void scanFrom("0")}
                />
                <button disabled={busy} onClick={() => void scanFrom("0")}>
                  Scan
                </button>
              </div>
              <ul className="redis-key-list">
                {keys.map((k) => (
                  <li
                    key={k.key}
                    className={k.key === selectedKey ? "active" : ""}
                    onClick={() => void openKey(k.key)}
                  >
                    <span className={`redis-type redis-type-${k.type}`}>{k.type}</span>
                    <span className="redis-key-name">{k.key}</span>
                    <span className="faint">{ttlLabel(k.ttlMs)}</span>
                  </li>
                ))}
              </ul>
              {!complete && (
                <button disabled={busy} onClick={() => void scanFrom(cursor)}>
                  Load more
                </button>
              )}
            </div>
          )}
        </div>

        <div className="redis-value">
          {error && <div className="browser-error">{error}</div>}
          {selected && selectedKey && value ? (
            <RedisValueEditor
              connectionId={selected}
              keyName={selectedKey}
              value={value}
              onChanged={reloadValue}
              onDeleted={() => {
                setSelectedKey(null);
                setValue(null);
                void scanFrom("0");
              }}
              setError={setError}
            />
          ) : (
            <p className="faint">Select a connection, scan for keys, then pick one to view.</p>
          )}
        </div>
      </div>
    </div>
  );
}

/** The type-aware value view and editor for one key. */
function RedisValueEditor({
  connectionId,
  keyName,
  value,
  onChanged,
  onDeleted,
  setError,
}: {
  connectionId: string;
  keyName: string;
  value: RedisValue;
  onChanged: () => void;
  onDeleted: () => void;
  setError: (e: string | null) => void;
}) {
  const [draft, setDraft] = useState("");
  const [field, setField] = useState("");

  const act = async (f: () => Promise<void>) => {
    setError(null);
    try {
      await f();
      onChanged();
    } catch (e) {
      setError(String(e));
    }
  };

  const header = (
    <div className="redis-value-head">
      <code>{keyName}</code>
      <span className={`redis-type redis-type-${value.kind === "zSet" ? "zset" : value.kind}`}>
        {value.kind}
      </span>
      <span style={{ flex: 1 }} />
      <button
        title="Delete this key"
        onClick={() => void act(async () => {
          await api.redisDeleteKey(connectionId, keyName);
          onDeleted();
        })}
      >
        Delete key
      </button>
    </div>
  );

  if (value.kind === "none") {
    return (
      <div>
        {header}
        <p className="faint">This key no longer exists.</p>
      </div>
    );
  }

  return (
    <div>
      {header}
      {"truncated" in value && value.truncated && (
        <p className="warning">This value is capped; not all of it is shown.</p>
      )}

      {value.kind === "string" && (
        <div className="redis-string-edit">
          <textarea
            defaultValue={value.text}
            onChange={(e) => setDraft(e.target.value)}
            rows={8}
          />
          <button
            onClick={() =>
              void act(() =>
                api.redisSetString(connectionId, keyName, draft || value.text, null),
              )
            }
          >
            Save
          </button>
        </div>
      )}

      {value.kind === "list" && (
        <div>
          <ol className="redis-elems">
            {value.items.map((v, i) => (
              <li key={i}>{v}</li>
            ))}
          </ol>
          <div className="redis-add">
            <input placeholder="value" value={draft} onChange={(e) => setDraft(e.target.value)} />
            <button
              onClick={() => void act(() => api.redisListPush(connectionId, keyName, draft, false))}
            >
              Push (back)
            </button>
            <button
              onClick={() => void act(() => api.redisListPush(connectionId, keyName, draft, true))}
            >
              Push (front)
            </button>
          </div>
        </div>
      )}

      {value.kind === "set" && (
        <div>
          <ul className="redis-elems">
            {value.members.map((m, i) => (
              <li key={i}>
                {m}
                <button onClick={() => void act(() => api.redisSetRemove(connectionId, keyName, m))}>
                  ✕
                </button>
              </li>
            ))}
          </ul>
          <div className="redis-add">
            <input placeholder="member" value={draft} onChange={(e) => setDraft(e.target.value)} />
            <button onClick={() => void act(() => api.redisSetAdd(connectionId, keyName, draft))}>
              Add
            </button>
          </div>
        </div>
      )}

      {value.kind === "zSet" && (
        <div>
          <ul className="redis-elems">
            {value.entries.map((e, i) => (
              <li key={i}>
                {e.member} <span className="faint">({e.score})</span>
                <button onClick={() => void act(() => api.redisZsetRemove(connectionId, keyName, e.member))}>
                  ✕
                </button>
              </li>
            ))}
          </ul>
          <div className="redis-add">
            <input placeholder="member" value={draft} onChange={(e) => setDraft(e.target.value)} />
            <input placeholder="score" value={field} onChange={(e) => setField(e.target.value)} />
            <button
              onClick={() =>
                void act(() =>
                  api.redisZsetAdd(connectionId, keyName, draft, Number(field) || 0),
                )
              }
            >
              Add
            </button>
          </div>
        </div>
      )}

      {value.kind === "hash" && (
        <div>
          <ul className="redis-elems">
            {value.fields.map((f, i) => (
              <li key={i}>
                <strong>{f.field}</strong> = {f.value}
                <button onClick={() => void act(() => api.redisHashDelete(connectionId, keyName, f.field))}>
                  ✕
                </button>
              </li>
            ))}
          </ul>
          <div className="redis-add">
            <input placeholder="field" value={field} onChange={(e) => setField(e.target.value)} />
            <input placeholder="value" value={draft} onChange={(e) => setDraft(e.target.value)} />
            <button onClick={() => void act(() => api.redisHashSet(connectionId, keyName, field, draft))}>
              Set
            </button>
          </div>
        </div>
      )}

      {value.kind === "stream" && (
        <div>
          <ul className="redis-elems">
            {value.entries.map((e) => (
              <li key={e.id}>
                <code>{e.id}</code>: {e.fields.map((f) => `${f.field}=${f.value}`).join(", ")}
              </li>
            ))}
          </ul>
          <div className="redis-add">
            <input
              placeholder='fields as field=value,field2=value2'
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
            />
            <button
              onClick={() =>
                void act(() =>
                  api.redisStreamAdd(connectionId, keyName, null, parseFields(draft)),
                )
              }
            >
              XADD
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

/** Parse `a=1,b=2` into field pairs for XADD. */
function parseFields(text: string): [string, string][] {
  return text
    .split(",")
    .map((p) => p.trim())
    .filter(Boolean)
    .map((p) => {
      const i = p.indexOf("=");
      return i < 0 ? ([p, ""] as [string, string]) : ([p.slice(0, i), p.slice(i + 1)] as [string, string]);
    });
}
