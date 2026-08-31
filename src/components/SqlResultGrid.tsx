import type { CSSProperties } from "react";
import type { SqlColumn, SqlRowCap, SqlValue } from "../ipc/types";
import { capNotice, cellRender, columnWidths } from "../views/sqlLogic";
import type { StoppedNote } from "../views/sqlViewLogic";

/**
 * One result set, rendered as a fixed-column list.
 *
 * There is no table component in this codebase and no virtualization library
 * available, so this is built on the same idiom as `RunningPanel`: plain `div`s
 * with classes, one per row. The backend's 1000-row cap is what bounds the DOM,
 * which is why the cap is a *reported* fact below rather than something this
 * component applies.
 *
 * **This component decides nothing about a cell.** Every branch on
 * `SqlValue.kind` lives in `sqlLogic.cellRender`, for the reason the type's own
 * doc gives: a null cell, an empty string and a truncated string are three
 * different answers, and so are `unsupported` and `unavailable`. A grid that
 * drew any two of them the same would have told the reader something untrue
 * about their data — so the mapping is in one tested place, not scattered
 * through JSX.
 *
 * Two rules are visible in the layout and are not cosmetic:
 *
 *  - The **cap notice is the last row of the grid**, not a toast. A toast
 *    scrolls away and the user then believes they saw every row. It is repeated
 *    in the header strip so it is also visible before scrolling.
 *  - A **stop gets the same treatment**, for the same reason and one more: the
 *    backend reports a stopped statement as an ordinary completion with no cap,
 *    so without this the header would read `300 rows` with nothing anywhere in
 *    the grid saying reading ended early. The wording is `stoppedNote`'s, not
 *    this component's.
 *  - Wide content scrolls inside `sql-grid-scroll`. The page body must never
 *    scroll sideways.
 */
export interface SqlResultGridProps {
  columns: SqlColumn[];
  /** Rows exactly as delivered — every `rows` event concatenated, nothing else. */
  rows: SqlValue[][];
  /**
   * Null means **no cap was applied** — which is not the same as "every row is
   * here". A user-stopped statement completes with no cap and a short result,
   * so `stopped` is the other half of that question and both must be read
   * together. Its presence *is* the truncation report, and `reason` says
   * whether the row limit or the byte budget bit.
   */
  rowCap: SqlRowCap | null;
  /**
   * The stop verdict for this result set, from `sqlViewLogic.stoppedNote`, or
   * `null` when the run was not cut short. Required rather than optional: a
   * caller that forgets it would render a partial answer as a complete one,
   * which is the exact failure this prop exists to prevent.
   */
  stopped: StoppedNote | null;
  /** `0` (ran, matched nothing) and `null` (no count to report) are opposite facts. */
  rowsAffected: number | null;
  /** Null while the statement is still running — not zero. */
  elapsedMs: number | null;
  /** What to call this result set, e.g. `Statement 2`. */
  title?: string;
  /** The statement is still streaming rows into `rows`. */
  running?: boolean;
}

export function SqlResultGrid({
  columns,
  rows,
  rowCap,
  stopped,
  rowsAffected,
  elapsedMs,
  title,
  running = false,
}: SqlResultGridProps) {
  const cap = capNotice({ rowCap });

  // Track sizing comes from the tested `columnWidths`, which measures the
  // *rendered* cell (a NULL marker is wider than nothing) over a bounded sample
  // and clamps to a floor and a ceiling. Nothing about widths is decided here.
  const widths = columnWidths(columns, rows);
  // `columnWidths` clamps to `MAX_COL_CHARS`, so the clamped number is the track
  // *width* and not its minimum. It was `minmax(${chars}ch, max-content)`, which
  // made the ceiling a floor: `max-content` is unbounded, so one 4096-character
  // cell (the backend's text cap) grew its column to tens of thousands of pixels
  // and pushed every other column off-screen — and `.sql-grid-cell`'s ellipsis
  // could never fire, because the track always grew to fit the text.
  const tableStyle: CSSProperties = {
    gridTemplateColumns: widths.map((chars) => `${chars}ch`).join(" "),
  };

  return (
    <div className="sql-grid">
      {/* Modelled on ObjectTree's CaptureHeader: what was read, and what the
          read could not promise. Both halves are permanent, never a tooltip. */}
      <div className="sql-grid-header">
        <div className="sql-grid-header-row">
          {title !== undefined && <span className="sql-grid-title">{title}</span>}
          <span className="badge">
            {rows.length} {rows.length === 1 ? "row" : "rows"}
          </span>
          <span className="sql-grid-meta">
            {rowsAffected === null
              ? "rows affected: not reported"
              : `${rowsAffected} affected`}
          </span>
          <span className="sql-grid-meta">
            {elapsedMs === null ? (running ? "running…" : "elapsed: not reported") : `${elapsedMs} ms`}
          </span>
          {stopped !== null && <span className="sql-grid-stopped-note">{stopped.header}</span>}
        </div>

        {cap !== null && (
          <div className="sql-grid-cap-banner">
            <span className="sql-warn-icon">⚠</span> {cap}
          </div>
        )}

        {stopped !== null && (
          <div className="sql-grid-stopped-banner">
            <span className="sql-warn-icon">⚠</span> {stopped.row}
          </div>
        )}
      </div>

      <div className="sql-grid-scroll">
        <div className="sql-grid-table" style={tableStyle} role="table">
          <div className="sql-grid-row sql-grid-head" role="row">
            {columns.map((column, index) => (
              <div
                className="sql-grid-cell sql-grid-th"
                role="columnheader"
                key={`${index}:${column.name}`}
                title={column.typeName ?? "type not reported"}
              >
                <span className="sql-grid-col-name">{column.name}</span>
                {/* Null means *not reported*, never "it has no type" — so the
                    absence is stated rather than left blank. */}
                <span className="sql-grid-col-type">{column.typeName ?? "type not reported"}</span>
              </div>
            ))}
          </div>

          {rows.map((row, rowIndex) => (
            <div className="sql-grid-row" role="row" key={rowIndex}>
              {columns.map((column, columnIndex) => {
                const value = row[columnIndex];
                if (value === undefined) {
                  // The row is shorter than the column list. That is a defect
                  // in what arrived, not an empty cell, and is said as such
                  // rather than drawn as one.
                  return (
                    <div
                      className="sql-grid-cell sql-missing"
                      role="cell"
                      key={columnIndex}
                      title={`No cell was delivered for column "${column.name}" in this row.`}
                    >
                      no cell
                    </div>
                  );
                }
                const cell = cellRender(value);
                return (
                  <div
                    className={`sql-grid-cell ${cell.className}`}
                    role="cell"
                    key={columnIndex}
                    title={cell.title ?? undefined}
                  >
                    {cell.text}
                  </div>
                );
              })}
            </div>
          ))}

          {rows.length === 0 && columns.length > 0 && (
            <div className="sql-grid-row sql-grid-empty-row" role="row">
              <div className="sql-grid-cell sql-grid-span" role="cell">
                {running ? "No rows yet." : "No rows."}
              </div>
            </div>
          )}

          {/* The cap, as the final row. Anyone who scrolls to the bottom of the
              data reads it there; anyone who does not has already read the
              banner above. */}
          {cap !== null && (
            <div className="sql-grid-row sql-grid-cap-row" role="row">
              <div className="sql-grid-cell sql-grid-span" role="cell">
                {cap}
              </div>
            </div>
          )}

          {/* The stop, as the true final row — after the cap, because it is the
              verdict on the whole run rather than on this result's size. */}
          {stopped !== null && (
            <div className="sql-grid-row sql-grid-stopped-row" role="row">
              <div className="sql-grid-cell sql-grid-span" role="cell">
                {stopped.row}
              </div>
            </div>
          )}
        </div>
      </div>

      {columns.length === 0 && (
        <div className="sql-grid-no-columns">
          {rowsAffected === null
            ? "This statement returned no columns, and reported no row count."
            : `This statement returned no columns. ${rowsAffected} rows affected.`}
        </div>
      )}
    </div>
  );
}
