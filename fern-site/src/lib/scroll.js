const reduced = () =>
  typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;

/** 进场：进入视口一次，加 .in。动效偏好关闭时直接就位。 */
export function reveal(node, { threshold = 0.18, delay = 0 } = {}) {
  node.classList.add('reveal');
  if (delay) node.style.transitionDelay = delay + 'ms';
  if (reduced()) {
    node.classList.add('in');
    return {};
  }
  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (e.isIntersecting) {
          node.classList.add('in');
          io.unobserve(node);
        }
      }
    },
    { threshold, rootMargin: '0px 0px -8% 0px' }
  );
  io.observe(node);
  return { destroy: () => io.disconnect() };
}

/** 元素穿过视口的进度 0–1，交给回调。start/end 用来裁剪有效区间。 */
export function track(node, options) {
  let { onprogress, start = 0, end = 1 } = options;
  let frame = 0;

  function measure() {
    frame = 0;
    const r = node.getBoundingClientRect();
    const vh = window.innerHeight;
    const raw = (vh - r.top) / (vh + r.height);
    const p = Math.min(1, Math.max(0, (raw - start) / (end - start)));
    onprogress(p);
  }
  function schedule() {
    if (!frame) frame = requestAnimationFrame(measure);
  }

  measure();
  window.addEventListener('scroll', schedule, { passive: true });
  window.addEventListener('resize', schedule);
  return {
    update(next) {
      ({ onprogress, start = 0, end = 1 } = next);
      schedule();
    },
    destroy() {
      if (frame) cancelAnimationFrame(frame);
      window.removeEventListener('scroll', schedule);
      window.removeEventListener('resize', schedule);
    }
  };
}

/** 进入视口时触发一次，用于起动打字这类一次性演示。 */
export function once(node, run) {
  if (reduced()) {
    run(true);
    return {};
  }
  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (e.isIntersecting) {
          run(false);
          io.unobserve(node);
        }
      }
    },
    { threshold: 0.5 }
  );
  io.observe(node);
  return { destroy: () => io.disconnect() };
}
