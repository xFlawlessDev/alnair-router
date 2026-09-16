import { createApp, h, nextTick, ref } from "vue";
import { describe, expect, it, vi } from "vitest";

import ConfirmDialog from "./ConfirmDialog.vue";

/**
 * Regression guard: the confirm action must fire while the parent's pending
 * item is still set. Reka's AlertDialogAction closes the dialog as part of its
 * own click handler, which historically cleared the parent state before the
 * confirm handler ran, making deletes silently no-op.
 */
describe("ConfirmDialog", () => {
  it("emits confirm with the parent state intact", async () => {
    const pending = ref<{ id: string } | null>({ id: "alias-1" });
    const confirmedWith = vi.fn();

    const container = document.createElement("div");
    document.body.appendChild(container);

    const app = createApp({
      setup() {
        return () =>
          h(ConfirmDialog, {
            open: pending.value !== null,
            title: "Delete alias?",
            description: "This cannot be undone.",
            "onUpdate:open": (value: boolean) => {
              pending.value = value ? pending.value : null;
            },
            onConfirm: () => confirmedWith(pending.value?.id),
          });
      },
    });
    app.mount(container);
    await nextTick();

    const action = [...document.querySelectorAll("button")].find((button) =>
      button.textContent?.includes("Delete"),
    );
    expect(action, "confirm button should render").toBeTruthy();

    action!.click();
    await nextTick();

    expect(confirmedWith).toHaveBeenCalledWith("alias-1");

    app.unmount();
    container.remove();
  });
});
