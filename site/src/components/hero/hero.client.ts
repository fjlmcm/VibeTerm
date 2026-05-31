// Hero 客户端动效:
//   1) braille spinner(呼应产品 OSC 标题 spinner)
//   2) 终端窗口 3D tilt —— 鼠标实时跟随翻转(强烈),仅鼠标设备 + 非 reduced-motion
const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

// ── braille spinner ──
const FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const spinners = Array.from(
  document.querySelectorAll<HTMLElement>('[data-title-spinner],[data-term-spinner]'),
);
if (spinners.length && !reduce) {
  let fi = 0;
  setInterval(() => {
    fi = (fi + 1) % FRAMES.length;
    for (const s of spinners) s.textContent = FRAMES[fi];
  }, 95);
}

// ── 终端窗口 3D tilt(鼠标跟随,强烈)──
const visual = document.querySelector<HTMLElement>('.hero-visual');
const win = visual?.querySelector<HTMLElement>('.win');
const fine = window.matchMedia('(pointer: fine)').matches;
if (visual && win && !reduce && fine) {
  const MAX_Y = 18; // 左右翻转最大角度(强)
  const MAX_X = 12; // 上下翻转最大角度
  let raf = 0;
  visual.addEventListener('pointermove', (e) => {
    const r = visual.getBoundingClientRect();
    const px = (e.clientX - r.left) / r.width - 0.5; // -0.5..0.5
    const py = (e.clientY - r.top) / r.height - 0.5;
    if (raf) cancelAnimationFrame(raf);
    raf = requestAnimationFrame(() => {
      const ry = (px * MAX_Y * 2).toFixed(2);
      const rx = (-py * MAX_X * 2).toFixed(2);
      win.style.transition = 'transform 120ms ease-out';
      win.style.transform = `rotateY(${ry}deg) rotateX(${rx}deg) scale(1.04)`;
    });
  });
  visual.addEventListener('pointerleave', () => {
    if (raf) cancelAnimationFrame(raf);
    win.style.transition = 'transform 600ms cubic-bezier(0.16, 1, 0.3, 1)';
    win.style.transform = '';
  });
}
