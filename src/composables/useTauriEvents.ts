import { listen } from "@tauri-apps/api/event";
import { onUnmounted } from "vue";

/**
 * Subscribe to a Tauri app-level event for the lifetime of the calling component.
 * The listener is automatically removed when the component unmounts.
 */
export function useTauriEvents(event: string, handler: () => void): void {
  let unlistenFn: (() => void) | null = null;

  // Wrap handler so the event payload is ignored (we only care about the trigger)
  listen(event, () => handler()).then((fn) => {
    unlistenFn = fn;
  });

  onUnmounted(() => {
    unlistenFn?.();
  });
}
