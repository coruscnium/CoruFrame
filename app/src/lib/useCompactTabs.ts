import { onMounted, onUnmounted, ref, watch, type Ref } from "vue";
import { textScale } from "../stores/ui";

export function useCompactTabs(element: Ref<HTMLElement | null>) {
  const compact = ref(false);
  let frame = 0;
  let observer: ResizeObserver | null = null;

  function check() {
    if (!element.value) return;
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      const target = element.value;
      if (!target) return;
      // Equal-width tab cells fill the row, so measuring them as laid out
      // would always report overflow. `tabs-measuring` drops the flex sizing
      // and the collapsed labels for one synchronous read, giving the cells'
      // natural width without ever flipping the visible compact state.
      target.classList.add("tabs-measuring");
      const required = Array.from(
        target.querySelectorAll<HTMLElement>('[role="tab"]'),
      ).reduce((sum, tab) => sum + tab.getBoundingClientRect().width, 0);
      target.classList.remove("tabs-measuring");
      compact.value = required > target.clientWidth - 24;
    });
  }

  onMounted(() => {
    check();
    observer = new ResizeObserver(check);
    if (element.value) observer.observe(element.value);
    void document.fonts?.ready.then(check);
  });

  onUnmounted(() => {
    cancelAnimationFrame(frame);
    observer?.disconnect();
  });

  watch(textScale, check);

  return compact;
}
