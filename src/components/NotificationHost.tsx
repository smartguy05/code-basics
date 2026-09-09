import { useEffect } from "react";
import {
  notificationTtlMs,
  type AppNotification,
} from "./notificationLogic";

/**
 * The app's notification stack — one global instance, rendered by `App`.
 *
 * A rendering shell: every decision (what earns a notification, what it says,
 * how the stack behaves, how long each kind lives) is in `notificationLogic.ts`,
 * which has tests. What is left here is the markup and one timer.
 *
 * Deliberately **not** a workspace tab signal. `workspaceTabsLogic` can outline
 * a codebase's tab, which is the right surface for a build that broke in a
 * codebase — but a launcher entry's `cwd` may sit outside every open workspace,
 * so a service dying is often nobody's tab. That is the gap this fills.
 */
export function NotificationHost({
  notifications,
  onDismiss,
}: {
  notifications: AppNotification[];
  onDismiss: (id: string) => void;
}) {
  if (notifications.length === 0) return null;

  return (
    // `aria-live="polite"`, not `assertive`: these are things worth knowing,
    // not things worth interrupting a screen reader mid-sentence for.
    <div className="notification-host" role="status" aria-live="polite">
      {notifications.map((notification) => (
        <NotificationRow
          key={notification.id}
          notification={notification}
          onDismiss={onDismiss}
        />
      ))}
    </div>
  );
}

function NotificationRow({
  notification,
  onDismiss,
}: {
  notification: AppNotification;
  onDismiss: (id: string) => void;
}) {
  const ttl = notificationTtlMs(notification.kind);

  useEffect(() => {
    // `null` means "until dismissed" — an error must not remove itself, since
    // the case this whole surface exists for is something ending while the user
    // was looking somewhere else.
    if (ttl === null) return;
    const timer = setTimeout(() => onDismiss(notification.id), ttl);
    return () => clearTimeout(timer);
  }, [ttl, notification.id, onDismiss]);

  return (
    <div className={`notification notification-${notification.kind}`}>
      <div className="notification-body">
        <div className="notification-title">{notification.title}</div>
        {notification.detail && (
          <div className="notification-detail">{notification.detail}</div>
        )}
      </div>
      <button
        className="notification-dismiss"
        onClick={() => onDismiss(notification.id)}
        aria-label="Dismiss"
        title="Dismiss"
      >
        ×
      </button>
    </div>
  );
}
