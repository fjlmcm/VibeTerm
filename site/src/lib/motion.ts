// 全站动效编排:Lenis 平滑滚动 + GSAP ScrollTrigger 驱动 reveal。
// 完全尊重 prefers-reduced-motion(降级为立即可见、原生滚动)。
import Lenis from 'lenis';
import { gsap } from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';

const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
const reveals = Array.from(document.querySelectorAll<HTMLElement>('[data-reveal]'));

if (reduce) {
  // 降级:全部立即可见,原生滚动,不启动任何库
  for (const el of reveals) el.classList.add('is-in');
} else {
  document.documentElement.classList.add('motion-js');
  gsap.registerPlugin(ScrollTrigger);

  // ── Lenis 平滑滚动 ──
  const lenis = new Lenis({ duration: 1.1, smoothWheel: true });
  lenis.on('scroll', ScrollTrigger.update);
  const raf = (time: number) => {
    lenis.raf(time);
    requestAnimationFrame(raf);
  };
  requestAnimationFrame(raf);

  // 锚点交给 Lenis(带固定导航偏移)
  for (const a of Array.from(document.querySelectorAll<HTMLAnchorElement>('a[href^="#"]'))) {
    a.addEventListener('click', (e) => {
      const href = a.getAttribute('href');
      if (!href || href.length < 2) return;
      const target = document.querySelector<HTMLElement>(href);
      if (target) {
        e.preventDefault();
        lenis.scrollTo(target, { offset: -64 });
      }
    });
  }

  // ── reveal:首屏元素载入即播,屏下元素滚动触发 ──
  const vh = window.innerHeight;
  for (const el of reveals) {
    const delay = Math.min(parseFloat(el.dataset.revealDelay || '0') / 1000 || 0, 0.32);
    const inView = el.closest('#top') !== null || el.getBoundingClientRect().top < vh;
    const base = {
      opacity: 1,
      y: 0,
      duration: 0.8,
      delay,
      ease: 'power3.out',
      onComplete: () => gsap.set(el, { clearProps: 'willChange' }),
    };
    if (inView) {
      gsap.fromTo(el, { opacity: 0, y: 26 }, base);
    } else {
      gsap.fromTo(
        el,
        { opacity: 0, y: 26 },
        { ...base, scrollTrigger: { trigger: el, start: 'top 87%', once: true } },
      );
    }
  }

  // 字体/布局稳定后校正触发点
  requestAnimationFrame(() => ScrollTrigger.refresh());
  window.addEventListener('load', () => ScrollTrigger.refresh());
}
